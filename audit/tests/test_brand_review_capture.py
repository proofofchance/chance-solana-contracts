import base64
import copy
import importlib.util
import json
import os
from pathlib import Path
import sys
import unittest

AUDIT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(AUDIT))
spec = importlib.util.spec_from_file_location('solana_brand_review_capture', AUDIT / 'brand_review_capture.py')
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)


def encode_value(decoder, kind, value):
    if kind == 'pubkey':
        return c.pubkey(value)
    if kind in ('string', 'bytes'):
        raw = value.encode() if kind == 'string' else value
        return len(raw).to_bytes(4, 'little') + raw
    if isinstance(kind, str):
        if kind == 'bool':
            return bytes([int(value)])
        return value.to_bytes(int(kind[1:]) // 8, 'little', signed=kind.startswith('i'))
    if 'array' in kind:
        subtype, count = kind['array']
        return bytes(value) if subtype == 'u8' else b''.join(encode_value(decoder, subtype, v) for v in value)
    if 'vec' in kind:
        return len(value).to_bytes(4, 'little') + b''.join(encode_value(decoder, kind['vec'], v) for v in value)
    name = kind['defined']['name']
    return b''.join(encode_value(decoder, f['type'], value[f['name']])
                    for f in decoder.types[name]['fields'])


class DecoderTests(unittest.TestCase):
    def test_unknown_and_truncated_accounts_fail_closed(self):
        decoder = c.Decoder()
        with self.assertRaises(c.Incomplete):
            decoder.account(bytes(16))
        discriminator = next(key for key, value in decoder.accounts.items() if value == 'Candidate')
        with self.assertRaises(c.Incomplete):
            decoder.account(discriminator + bytes(3))

    def test_rpc_manifest_is_required(self):
        with self.assertRaises(c.Incomplete):
            c.verify_snapshot({'program': 'x', 'instance': 'y', 'accounts': {}})

    def test_mixed_finalized_banks_fail_before_state_validation(self):
        class Rpc:
            def request(self, method, arguments):
                if method == 'getGenesisHash':
                    return 'genesis'
                if method == 'getProgramAccounts':
                    return {'context': {'slot': 7}, 'value': []}
                if method == 'getMultipleAccounts':
                    return {'context': {'slot': 8}, 'value': [None]}
                raise AssertionError(method)
        with self.assertRaises(c.Mismatch):
            c.capture(Rpc(), {'genesisHash': 'genesis', 'program': 'p', 'externalAccounts': ['a']})


class ActualRuntimeSnapshot(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if 'CHANCE_GIVEAWAY_BRAND_REVIEW_CAPTURE' not in os.environ:
            raise unittest.SkipTest('Export actual ELF state and explicitly set CHANCE_GIVEAWAY_BRAND_REVIEW_CAPTURE')
        path = Path(os.environ['CHANCE_GIVEAWAY_BRAND_REVIEW_CAPTURE'])
        if not path.exists():
            if 'CHANCE_GIVEAWAY_BRAND_REVIEW_CAPTURE' in os.environ:
                raise AssertionError('Explicit CHANCE_GIVEAWAY_BRAND_REVIEW_CAPTURE fixture is absent: ' + str(path))
            raise unittest.SkipTest('Export an actual ELF fixture with CHANCE_GIVEAWAY_BRAND_REVIEW_CAPTURE')
        cls.snapshot = json.loads(path.read_text())
        cls.decoder = c.Decoder()

    def key(self, name):
        for key, account in self.snapshot['accounts'].items():
            if account['owner'] == self.snapshot['program'] and not account.get('executable', False):
                kind, value = self.decoder.account(c.account_data(account))
                if kind == name:
                    return key
        raise AssertionError(name)

    def mutate(self, name, field, value):
        bundle = copy.deepcopy(self.snapshot)
        key = self.key(name)
        raw = c.account_data(bundle['accounts'][key])
        _, state = self.decoder.account(raw)
        state[field] = value(state[field]) if callable(value) else value
        data = raw[:8] + encode_value(self.decoder, {'defined': {'name': name}}, state)
        bundle['accounts'][key]['data'][0] = base64.b64encode(data + bytes(len(raw) - len(data))).decode()
        return bundle

    def test_actual_loaded_elf_snapshot_passes(self):
        report = c.verify_snapshot(self.snapshot, isolated=True)
        self.assertEqual(report['qualification'], 'isolated_actual_runtime_state')
        self.assertEqual((report['tasks'], report['registered'], report['candidates'], report['approved']), (2, 4, 4, 2))
        self.assertEqual(report['liability'], 0)

    def test_retained_record_omissions_are_detected(self):
        for name in ('Task', 'Evidence', 'Candidate', 'Participant', 'WinnerSlot'):
            bundle = copy.deepcopy(self.snapshot)
            del bundle['accounts'][self.key(name)]
            with self.subTest(name=name), self.assertRaises((c.Incomplete, c.Mismatch)):
                c.verify_snapshot(bundle, isolated=True)

    def test_owner_and_nonzero_tail_tampering_are_detected(self):
        for change in ('owner', 'tail'):
            bundle = copy.deepcopy(self.snapshot)
            key = self.key('Task')
            if change == 'owner':
                bundle['accounts'][key]['owner'] = bundle['registry']
            else:
                data = c.account_data(bundle['accounts'][key]) + b'\1'
                bundle['accounts'][key]['data'][0] = base64.b64encode(data).decode()
            with self.subTest(change=change), self.assertRaises((c.Incomplete, c.Mismatch)):
                c.verify_snapshot(bundle, isolated=True)

    def test_hash_rank_slot_and_accounting_tampering_are_detected(self):
        for name, field, change in [
            ('Evidence', 'text', lambda x: x + '!'),
            ('Participant', 'evidence_root', bytes.fromhex('11' * 32)),
            ('Participant', 'index', 1234),
            ('Candidate', 'key', bytes(64)),
            ('Candidate', 'entry_revision', lambda x: x + 1),
            ('Candidate', 'slot', 99),
            ('Giveaway', 'per_winner', lambda x: x + 1),
            ('Giveaway', 'fee', lambda x: x + 1),
            ('Giveaway', 'paid_winners', 0),
            ('Giveaway', 'registered_count', 2**64 - 1),
            ('Giveaway', 'aggregate', bytes(32)),
            ('Giveaway', 'pending_deadline', 99),
            ('Giveaway', 'outcome_hash', bytes(32)),
            ('WinnerSlot', 'wallet', '11111111111111111111111111111111'),
        ]:
            with self.subTest(name=name, field=field), self.assertRaises((c.Incomplete, c.Mismatch, c.ref.Invalid)):
                c.verify_snapshot(self.mutate(name, field, change), isolated=True)

    def test_missing_network_code_manifest_never_becomes_a_live_pass(self):
        with self.assertRaises(c.Incomplete):
            c.verify_snapshot(self.snapshot)


if __name__ == '__main__':
    unittest.main()
