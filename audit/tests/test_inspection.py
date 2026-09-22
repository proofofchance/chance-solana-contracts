import base64
import copy
import json
import os
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).parents[1]))
from common import Incomplete, Mismatch, Reader, b58decode, b58encode, pda, pubkey
from inspect_instance import inspect


class WireTests(unittest.TestCase):
    def test_base58_zero_prefixes_and_width(self):
        for raw in [bytes(32), bytes(range(32)), bytes([255])*32]:
            self.assertEqual(b58decode(b58encode(raw)), raw)
        self.assertEqual(b58encode(bytes(32)), '1'*32)
        with self.assertRaises(Incomplete):
            pubkey('1')
        with self.assertRaises(Incomplete):
            b58decode('0OIl')

    def test_borsh_rejects_truncation_invalid_bool_and_huge_length(self):
        for kind, data in [('u64', bytes(7)), ('bool', b'\x02'), ('String', bytes([255])*4)]:
            with self.subTest(kind=kind):
                with self.assertRaises(Incomplete):
                    Reader(data).value(kind)

    def test_pda_seed_limits(self):
        with self.assertRaises(Incomplete):
            pda('1'*32, bytes(33))


class FixtureRpc:
    """Real SBF account bytes from an isolated LiteSVM bank; RPC transport is a fixture."""
    genesis = b58encode(bytes([99])*32)

    def __init__(self, data):
        self.fixture = copy.deepcopy(data)
        self.accounts = self.fixture['accounts']
        for account in self.accounts.values():
            account['data'] = [base64.b64encode(bytes.fromhex(account.pop('dataHex'))).decode(), 'base64']

    def request(self, method, params):
        if method == 'getGenesisHash':
            return self.genesis
        if method == 'getSignaturesForAddress':
            return [{'signature': row['transaction']['signatures'][0], 'slot': row['slot'], 'err': None}
                    for row in reversed(self.fixture.get('history', [])) if params[0] in row['transaction']['message']['accountKeys']]
        if method == 'getBlock':
            result = {'blockhash': b58encode(bytes([98])*32), 'blockTime': None}
            if params[1]['transactionDetails'] == 'full':
                rows = copy.deepcopy([row for row in self.fixture.get('history', []) if row['slot'] == params[0]])
                for row in rows:
                    for ix in row['transaction']['message']['instructions']:
                        ix['data'] = b58encode(bytes.fromhex(ix.pop('dataHex')))
                result['transactions'] = rows
            return result
        if method == 'getAccountInfo':
            return {'context': {'slot': self.fixture['slot']}, 'value': self.accounts.get(params[0])}
        if method == 'getMultipleAccounts':
            assert params[1]['commitment'] == 'finalized'
            return {'context': {'slot': self.fixture['slot']}, 'value': [self.accounts.get(address) for address in params[0]]}
        raise AssertionError(method)


@unittest.skipUnless(os.environ.get('CHANCE_AUDIT_FIXTURE'), 'requires isolated SBF runtime account export')
class RuntimeEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.data = json.loads(Path(os.environ['CHANCE_AUDIT_FIXTURE']).read_text())
        self.rpc = FixtureRpc(self.data)

    def inspect(self):
        return inspect(self.rpc, self.rpc.genesis, self.data['registry'], self.data['inventory'])

    def test_actual_program_ownership_pdas_and_binary_match(self):
        report = self.inspect()
        self.assertEqual(report['ownershipAndRelease'], 'pass')
        self.assertEqual(report['release']['status'], 4)  # retired but historical instance remains valid
        self.assertTrue(report['state']['settled'])
        self.assertIsNone(report['programs']['instance']['upgradeAuthority'])
        self.assertEqual(report['buildMatch']['status'], 'unavailable')
        self.assertEqual(report['eventReplay'], 'not_run')

    def test_replay_actual_sbf_selection_and_payment(self):
        from replay_instance import replay
        result = replay(self.rpc, self.inspect())
        self.assertEqual(result['eventReplay'], 'pass')
        self.assertEqual(result['accounting']['fundedPrincipal'], 1_000_000)
        self.assertEqual(result['accounting']['paidPrincipal'], 1_000_000)
        self.assertTrue(result['accounting']['financiallyClosed'])

    def test_signature_omission_from_complete_block_is_rejected(self):
        from replay_instance import replay
        original=self.rpc.request
        def omitted(method,params):
            value=original(method,params)
            if method=='getSignaturesForAddress':
                # Other transactions in the same slot ensure the full block is fetched.
                return value[1:]
            return value
        self.rpc.request=omitted
        with self.assertRaisesRegex(Incomplete, 'omitted from signature history'):
            replay(self.rpc,self.inspect())

    def test_unknown_program_event_cannot_silently_pass(self):
        from replay_instance import replay
        for row in self.rpc.fixture['history']:
            for index, line in enumerate(row['meta']['logMessages']):
                if 'WinnerPaid' in line:
                    row['meta']['logMessages'][index] = line.replace('WinnerPaid','UnrecognizedPayment')
                    with self.assertRaisesRegex(Incomplete, 'Unsupported instance event'):
                        replay(self.rpc, self.inspect())
                    return
        self.fail('Missing payment fixture')

    def test_tampered_payout_balance_fails(self):
        from replay_instance import replay
        for row in self.rpc.fixture['history']:
            if any('WinnerPaid' in line for line in row['meta']['logMessages']):
                row['meta']['postBalances'][0] += 1
                break
        with self.assertRaisesRegex(Mismatch, 'balance delta'):
            replay(self.rpc, self.inspect())

    def test_omitted_participation_fails(self):
        from replay_instance import replay
        self.rpc.fixture['history'] = [row for row in self.rpc.fixture['history'] if not any('ParticipationSubmitted' in line for line in row['meta']['logMessages'])]
        with self.assertRaises((Incomplete, Mismatch, KeyError)):
            replay(self.rpc, self.inspect())

    def test_wrong_network(self):
        with self.assertRaisesRegex(Mismatch, 'genesis'):
            inspect(self.rpc, b58encode(bytes([97])*32), self.data['registry'], self.data['inventory'])

    def test_wrong_inventory_owner(self):
        self.rpc.accounts[self.data['inventory']]['owner'] = '1'*32
        with self.assertRaisesRegex(Mismatch, 'owner'):
            self.inspect()

    def test_changed_executable(self):
        report = self.inspect()
        address = report['programs']['instance']['programData']
        raw = bytearray(base64.b64decode(self.rpc.accounts[address]['data'][0]))
        raw[100] ^= 1
        self.rpc.accounts[address]['data'][0] = base64.b64encode(raw).decode()
        with self.assertRaisesRegex(Mismatch, 'Executable differs'):
            self.inspect()

    def test_retained_authority(self):
        report = self.inspect()
        address = report['programs']['instance']['programData']
        raw = bytearray(base64.b64decode(self.rpc.accounts[address]['data'][0]))
        raw[12] = 1
        raw[13:45] = bytes([4])*32
        self.rpc.accounts[address]['data'][0] = base64.b64encode(raw).decode()
        with self.assertRaisesRegex(Mismatch, 'mutable'):
            self.inspect()


class HistoryTests(unittest.TestCase):
    def test_failed_inner_calls_and_spoofed_prefixes_cannot_supply_events(self):
        from history import events
        program = b58encode(bytes([7])*32)
        other = b58encode(bytes([8])*32)
        event = 'Program log: GIVEAWAY_EVENT: '+json.dumps({'version':'1.0.0','event':{'event_type':'WinnerPaid','data':{'amount':1}}})
        logs = [f'Program {other} invoke [1]', event, f'Program {program} invoke [2]', event,
                f'Program {program} failed: rejected', f'Program {other} success']
        self.assertEqual(events(logs, program), [])
        logs = [f'Program {program} invoke [1]', event, f'Program {program} success']
        self.assertEqual(events(logs, program)[0]['event_type'], 'WinnerPaid')
        with self.assertRaises(Incomplete):
            events(logs[:-1], program)
        with self.assertRaises(Incomplete):
            events(['Log truncated'], program)

@unittest.skipUnless(os.environ.get('CHANCE_DAILY_AUDIT_FIXTURE'), 'requires daily SBF runtime export')
class DailyReplayTests(unittest.TestCase):
    def setUp(self):
        self.data = json.loads(Path(os.environ['CHANCE_DAILY_AUDIT_FIXTURE']).read_text())
        self.rpc = FixtureRpc(self.data)

    def replay(self):
        from replay_instance import replay
        report = inspect(self.rpc, self.rpc.genesis, self.data['registry'], self.data['inventory'])
        return replay(self.rpc, report)

    def test_weighted_draws_and_full_payment(self):
        report = self.replay()
        self.assertEqual(report['eventReplay'], 'pass')
        self.assertEqual(len(report['winners']), 2)
        self.assertEqual(report['accounting']['paidPrincipal'], 6_000_000)
        self.assertTrue(report['accounting']['financiallyClosed'])

    def test_wrong_paid_positions_with_correct_count_are_rejected(self):
        from replay_instance import replay
        report=inspect(self.rpc,self.rpc.genesis,self.data['registry'],self.data['inventory'])
        bits=int.from_bytes(report['state']['paid_winners_bitmap'],'little')
        self.assertNotEqual(bits,0)
        report['state']['paid_winners_bitmap']=(bits << 1).to_bytes(32,'little')
        with self.assertRaisesRegex(Mismatch,'Paid bitmap differs'):
            replay(self.rpc,report)

    def test_changed_attestation_calldata_is_rejected(self):
        for row in self.rpc.fixture['history']:
            if any('AttestationSubmitted' in line for line in row['meta']['logMessages']):
                for ix in row['transaction']['message']['instructions']:
                    raw = bytearray.fromhex(ix['dataHex'])
                    if raw[0] == 15:
                        raw[1] = 1
                        ix['dataHex'] = raw.hex()
                break
        with self.assertRaisesRegex(Mismatch, 'vote differs'):
            self.replay()


if __name__ == '__main__':
    unittest.main()
