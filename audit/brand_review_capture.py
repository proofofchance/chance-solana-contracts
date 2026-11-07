#!/usr/bin/env python3
"""Capture and independently check retained V2 Solana giveaway state.

All program accounts are fetched in one finalized getProgramAccounts bank. Dense
participant indexes and retained task/evidence/candidate records detect omissions.
The selected RPC remains trusted for authentic bank/consensus evidence. A state
snapshot cannot certify historical signer execution, reveal preimages or transfers;
those limitations are reported rather than inferred from mutable counters.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys

from common import (Incomplete, Mismatch, Reader, Rpc, LOADER, account_data, b58encode,
                    check, decode, json_default, pda, pubkey)
import brand_review_reference as ref

IDL_PATH = Path(__file__).parent / 'schemas/giveaways-brand-review-v1.idl.json'


class Decoder:
    def __init__(self, idl=None):
        self.idl = idl or json.loads(IDL_PATH.read_text())
        self.types = {t['name']: t['type'] for t in self.idl['types']}
        self.accounts = {bytes(t['discriminator']): t['name'] for t in self.idl['accounts']}

    def value(self, reader, kind):
        if kind == 'pubkey':
            return b58encode(reader.take(32))
        if kind in ('string', 'bytes'):
            size = reader.integer(4)
            if size > 32768:
                raise Incomplete('IDL value exceeds capture resource limit')
            data = reader.take(size)
            return data.decode('utf-8', 'strict') if kind == 'string' else data
        if isinstance(kind, str):
            return reader.value(kind)
        if 'array' in kind:
            element, count = kind['array']
            if element == 'u8':
                return reader.take(count)
            return [self.value(reader, element) for _ in range(count)]
        if 'vec' in kind:
            count = reader.integer(4)
            if count > 4096:
                raise Incomplete('IDL vector exceeds capture resource limit')
            return [self.value(reader, kind['vec']) for _ in range(count)]
        if 'defined' in kind:
            name = kind['defined']['name'] if isinstance(kind['defined'], dict) else kind['defined']
            definition = self.types[name]
            if definition['kind'] != 'struct':
                raise Incomplete('Unsupported V2 IDL type')
            return {f['name']: self.value(reader, f['type']) for f in definition['fields']}
        raise Incomplete('Unsupported V2 IDL field')

    def account(self, raw):
        name = self.accounts.get(raw[:8])
        if name is None:
            raise Incomplete('Unknown program-owned account discriminator')
        reader = Reader(raw[8:])
        value = self.value(reader, {'defined': {'name': name}})
        check(not any(reader.data[reader.offset:]), 'Nonzero unrecognized account tail')
        return name, value


def _raw(account):
    return account_data(account)


def verify_snapshot(bundle, manifest=None, *, isolated=False):
    """Verify a complete raw snapshot; fixture mode never asserts network identity."""
    program, instance = bundle['program'], bundle['instance']
    accounts = bundle['accounts']
    decoder = Decoder()
    if manifest is None and not isolated:
        raise Incomplete('A caller-selected code/source/network manifest is required')
    if manifest:
        check(manifest['program'] == program and manifest['instance'] == instance,
              'Manifest instance/program differs')
        check(manifest['idlSha256'] == hashlib.sha256(IDL_PATH.read_bytes()).hexdigest(),
              'IDL differs from selected source manifest')
        if not isolated:
            check(bundle['genesisHash'] == manifest['genesisHash'], 'Genesis binding differs')
    decoded = {}
    check(len(accounts) <= 200000, 'Snapshot exceeds account capture bound')
    for key, account in accounts.items():
        if account['owner'] == program and not account.get('executable', False):
            decoded[key] = decoder.account(_raw(account))

    def get(key, name):
        if key not in decoded:
            raise Incomplete('Missing retained ' + name + ' account')
        actual, value = decoded[key]
        check(actual == name, 'Account discriminator differs for ' + name)
        return value

    g = get(instance, 'Giveaway')
    check(1 <= g['task_count'] <= 32 and g['registered_count'] <= 4096
          and g['approved_count'] <= 1000 and g['consumed_count'] <= g['base_pool_count']
          <= g['registered_count'] and g['status'] in range(5), 'Release count/status bounds differ')
    config = get(g['config'], 'Config')
    check(pda(program, b'config') == g['config'], 'Noncanonical config PDA')
    check(pda(program, b'giveaway', pubkey(g['config']), g['id'].to_bytes(8, 'little')) == instance,
          'Noncanonical giveaway PDA')
    check(g['brand'] == g['creator'] and g['host'] == config['authority']
          and g['fee_bps'] == config['fee_bps'] and g['network_domain'] == config['network_domain'],
          'Frozen authority/fee/network binding differs')
    rulebook = get(g['rulebook'], 'Rulebook')
    check(pda(program, b'rules', pubkey(g['creator']), rulebook['nonce'].to_bytes(8, 'little'))
          == g['rulebook'], 'Noncanonical rulebook PDA')
    check(rulebook['creator'] == g['creator'] and rulebook['instance'] == instance
          and rulebook['sealed'] and rulebook['appended'] == rulebook['task_count']
          == g['task_count'], 'Incomplete or substituted taskbook')
    profile = ref.HashProfile('solana', pubkey(instance).hex())
    tasks = []
    for i in range(g['task_count']):
        task = get(pda(program, b'task', pubkey(g['rulebook']), i.to_bytes(2, 'little')), 'Task')
        record = ref.Task(**task['input'])
        check(task['rulebook'] == g['rulebook'] and record.task_id == i
              and task['hash'] == profile.task_hash(record), 'Task identity/hash differs')
        tasks.append(record)
    check(profile.task_root(tasks) == rulebook['root'] == g['task_root'], 'Task root differs')
    check(sum(name == 'Task' and value['rulebook'] == g['rulebook'] for name, value in decoded.values())
          == g['task_count'], 'Unexpected retained task record')
    terms = g['terms']
    terms_bytes = b''.join(ref.uint(terms[name], size) for name, size in
                          [('funding', 8), ('winners', 4), ('entry_start', 8), ('entry_end', 8),
                           ('upload_end', 8), ('remediation_end', 8), ('opening_cutoff', 8),
                           ('absolute_deadline', 8), ('review_seconds', 4),
                           ('finalization_reserve_seconds', 4)])
    rules = ref.sha(b'GIVEAWAY_CREATION_V2' + pubkey(program) + pubkey(g['config'])
                    + pubkey(config['registry_program']) + pubkey(config['registry_config'])
                    + pubkey(config['authority']) + config['network_domain']
                    + ref.uint(config['fee_bps'], 2) + pubkey(g['creator']) + pubkey(g['rulebook'])
                    + ref.uint(rulebook['nonce'], 8) + rulebook['root']
                    + ref.uint(g['task_count'], 2) + ref.uint(g['id'], 8) + terms_bytes)
    check(rules == g['rules_hash'], 'Checked creation commitment differs')
    if manifest:
        check(manifest['rulesHash'] == rules.hex(), 'Manifest rules hash differs')

    people = [(key, value) for key, (name, value) in decoded.items()
              if name == 'Participant' and value['giveaway'] == instance]
    indexed = sorted((value['index'], key, value) for key, value in people
                     if value['index'] != 2**64 - 1)
    check([i for i, _, _ in indexed] == list(range(g['registered_count'])),
          'Registered participant enumeration is incomplete or duplicated')
    entries, evidence_count = [], 0
    for key, participant in people:
        check(pda(program, b'participant', pubkey(instance), pubkey(participant['wallet'])) == key,
              'Noncanonical participant PDA')
        evidence, total = [], 0
        for task_id in range(g['task_count']):
            count = participant['versions'][task_id]
            check(count <= 4, 'Evidence version exceeds release cap')
            for version in range(1, count + 1):
                e = get(pda(program, b'evidence', pubkey(key), ref.uint(task_id, 2), bytes([version])), 'Evidence')
                check(e['participant'] == key and e['task_id'] == task_id and e['version'] == version,
                      'Evidence identity differs')
                ref.text_bytes(e['text'], tasks[task_id].max_evidence_bytes)
                check(e['hash'] == profile.evidence_hash(pubkey(participant['wallet']).hex(),
                                                       task_id, version, e['text']), 'Evidence hash differs')
                check(e['published_at'] < terms['entry_end'], 'Evidence published after cutoff')
                evidence_count += 1
                if version == count:
                    check(len(e['text'].encode()) == participant['lengths'][task_id], 'Evidence length differs')
                    total += len(e['text'].encode())
                    evidence.append((version, e['text']))
        check(total == participant['current_bytes'], 'Evidence byte accounting differs')
        if participant['sealed']:
            check(len(evidence) == g['task_count'] and participant['sealed_revision'] == participant['revision'],
                  'Current entry is incomplete or stale')
            check(profile.entry_root(pubkey(participant['wallet']).hex(), evidence,
                                     participant['revision'], participant['commitment'])
                  == participant['evidence_root'], 'Sealed evidence root differs')
        if participant['index'] != 2**64 - 1:
            entries.append(ref.Entry(pubkey(participant['wallet']).hex(), sealed=participant['sealed'],
                                     index=participant['index'],
                                     digest=participant['contribution'] if participant['revealed'] else None))
    check(sum(p['sealed'] for _, p in people) == g['sealed_count'], 'Complete entry count differs')
    check(sum(p['attested'] for _, p in people) == g['attested_count']
          and sum(p['revealed'] for _, p in people) == g['included_count'], 'Reveal coverage differs')
    owners = {key for key, _ in people}
    check(sum(name == 'Evidence' and value['participant'] in owners for name, value in decoded.values())
          == evidence_count, 'Unexpected retained evidence version')
    aggregate = bytearray(32)
    for _, p in people:
        if p['revealed']:
            for i, value in enumerate(p['contribution']):
                aggregate[i] ^= value
    check(bytes(aggregate) == g['aggregate'], 'Reveal aggregate differs')
    entries.sort(key=lambda item: item.index)
    included = [e for e in entries if e.sealed and e.digest is not None]
    ranked = []
    if g['status'] >= 2 and g['base_pool_count'] == len(included):
        pool, seed = profile.pool_and_seed(entries, terms['winners'])
        check(pool == g['pool_hash'] and seed == g['seed'], 'Frozen pool/seed differs')
        ranked = sorted(included, key=lambda e: profile.candidate_key(seed, e.wallet))
    elif g['status'] in (2, 3):
        raise Mismatch('Frozen candidate pool count differs')
    candidates = sorted((value['ordinal'], key, value) for key, (name, value) in decoded.items()
                        if name == 'Candidate' and value['giveaway'] == instance)
    check([ordinal for ordinal, _, _ in candidates] ==
          list(range(g['consumed_count'] + int(g['pending']))), 'Candidate history is incomplete')
    approved, terminal = [], 0
    participants = {value['wallet']: value for _, value in people}
    for ordinal, key, candidate in candidates:
        check(pda(program, b'candidate', pubkey(instance), ref.uint(ordinal, 8)) == key,
              'Noncanonical candidate PDA')
        check(ordinal < len(ranked) and candidate['wallet'] == b58encode(bytes.fromhex(ranked[ordinal].wallet))
              and candidate['key'] == profile.candidate_key(g['seed'], ranked[ordinal].wallet),
              'Candidate skipped the next deterministic rank')
        p = participants[candidate['wallet']]
        check(candidate['entry_revision'] == p['sealed_revision'] and candidate['evidence_root'] == p['evidence_root']
              and candidate['slot'] == len(approved), 'Candidate evidence/slot binding differs')
        check(candidate['opened_at'] < terms['opening_cutoff']
              and candidate['deadline'] == candidate['opened_at'] + terms['review_seconds'],
              'Candidate review window differs')
        verdict = candidate['verdict']
        if verdict in (1, 2):
            check(candidate['decided_at'] < candidate['deadline'], 'Late brand decision')
            check((verdict == 1 and candidate['reason'] == 0) or
                  (verdict == 2 and 1 <= candidate['reason'] <= 3), 'Invalid decision reason')
            terminal += 1
        else:
            check(verdict == 0 and ordinal == len(candidates) - 1 and g['pending'],
                  'Unresolved candidate precedes a later decision')
        if verdict == 1:
            slot_key = pda(program, b'winner', pubkey(instance), ref.uint(len(approved), 4))
            slot = get(slot_key, 'WinnerSlot')
            check(slot['giveaway'] == instance and slot['slot'] == len(approved)
                  and slot['wallet'] == candidate['wallet'] and slot['ordinal'] == ordinal
                  and slot['revision'] == candidate['entry_revision']
                  and slot['evidence_root'] == candidate['evidence_root'] and slot['approved'],
                  'Accepted winner slot differs from brand decision')
            approved.append({'wallet': pubkey(slot['wallet']).hex(), 'ordinal': ordinal,
                             'evidence_root': slot['evidence_root'].hex(), 'revision': slot['revision'],
                             'paid': slot['paid']})
    check(terminal == g['consumed_count'] and len(approved) == g['approved_count'], 'Final decision counts differ')
    check((not g['pending'] and g['pending_deadline'] == 0)
          or (g['pending'] and candidates[-1][2]['deadline'] == g['pending_deadline']),
          'Global pending review deadline differs')
    check(sum(name == 'WinnerSlot' and value['giveaway'] == instance and value['approved']
              for name, value in decoded.values()) == len(approved), 'Unexpected approved winner slot')
    accepted_root = profile.accepted_root(approved)
    check(accepted_root == g['accepted_root'], 'Accepted winner commitment differs')
    if g['status'] == 3:
        check(not g['pending'] and approved and (len(approved) == terms['winners']
              or terminal == len(ranked)), 'Outcome locked before slots/full exhaustion')
        fee = terms['funding'] * g['fee_bps'] // 10000
        per = (terms['funding'] - fee) // len(approved)
        remainder = terms['funding'] - fee - per * len(approved)
        check((g['fee'], g['per_winner'], g['remainder']) == (fee, per, remainder), 'Pooled accounting differs')
        check(profile.outcome_hash(rules, g['seed'], accepted_root, len(approved), per, fee,
                                   remainder, terminal) == g['outcome_hash'], 'Final outcome commitment differs')
    check(sum(w['paid'] for w in approved) == g['paid_winners'], 'Paid slot count differs')
    vault_key = pda(program, b'vault', pubkey(instance))
    vault = get(vault_key, 'Vault')
    check(vault['giveaway'] == instance and vault_key == g['vault'], 'Vault binding differs')
    liability = terms['funding']
    if g['status'] == 4:
        check(g['fee'] == g['per_winner'] == g['remainder'] == g['paid_winners'] == 0,
              'Refund outcome contains vested payment state')
        liability = 0
    elif g['status'] == 3:
        liability = (len(approved) - g['paid_winners']) * g['per_winner']
        liability += 0 if g['fee_paid'] else g['fee']
        liability += 0 if g['remainder_paid'] else g['remainder']
    check(accounts[vault_key]['lamports'] >= liability, 'Escrow is insolvent')
    _verify_registry(bundle, config, g, rulebook, manifest, isolated)
    return {'schema': 'giveaway-v2-state-capture-report', 'status': 'state_consistent',
            'qualification': 'isolated_actual_runtime_state' if isolated else 'finalized_rpc_state',
            'instance': instance, 'slot': bundle['slot'], 'tasks': len(tasks), 'evidenceVersions': evidence_count,
            'registered': g['registered_count'], 'basePool': g['base_pool_count'],
            'candidates': len(candidates), 'approved': len(approved), 'liability': liability,
            'limitations': ['RPC consensus/authenticity is trusted; no light-client proof.',
                            'Retained state proves process consistency, not external task truth.',
                            'Historical signer execution, reveal preimages and recipient transfers require transaction replay.',
                            'An isolated runtime fixture is not evidence of a deployed network.']}


def _verify_registry(bundle, config, g, rulebook, manifest, isolated):
    accounts, registry = bundle['accounts'], config['registry_program']
    entry = decode('registry', 'Instance', account_data(accounts[bundle['registryEntry']], registry))
    check(entry['instance'] == pubkey(bundle['instance']) and entry['creator'] == pubkey(g['creator'])
          and entry['owner_program'] == pubkey(bundle['program']) and entry['rules_hash'] == g['rules_hash'],
          'Canonical registry instance binding differs')
    cfg = decode('registry', 'Config', account_data(accounts[config['registry_config']], registry))
    release_key = b58encode(entry['release'])
    release = decode('registry', 'Release', account_data(accounts[release_key], registry))
    check(pda(registry, b'instance', pubkey(config['registry_config']), ref.uint(entry['sequence'], 8))
          == bundle['registryEntry'] and release['program'] == pubkey(bundle['program'])
          and release['domain'] == 2 and release['registry'] == pubkey(config['registry_config']),
          'Registry PDA/release differs')
    check(pda(registry, b'release', pubkey(config['registry_config']), pubkey(bundle['program'])) == release_key
          and 1 <= entry['sequence'] <= cfg['instance_count']
          and 1 <= release['sequence'] <= cfg['release_count']
          and release['registered_slot'] <= entry['created_slot'] <= bundle['slot'],
          'Registry sequence or creation chronology differs')
    if isolated:
        check(bundle.get('provenance', '').startswith('isolated'), 'Fixture provenance must be explicitly isolated')
        if manifest:
            check(bundle['programElfSha256'] == manifest['programElfSha256'], 'Fixture ELF identity differs')
        return
    check(pda(registry, b'registry', cfg['authority']) == config['registry_config'], 'Registry config PDA differs')
    for program in (bundle['program'], registry):
        descriptor = account_data(accounts[program], LOADER)
        data_key = pda(LOADER, pubkey(program))
        data = account_data(accounts[data_key], LOADER)
        check(accounts[program]['executable'] and descriptor[:4] == ref.uint(2, 4)
              and descriptor[4:36] == pubkey(data_key) and data[:4] == ref.uint(3, 4)
              and len(data) > 45 and data[12] == 0
              and int.from_bytes(data[4:12], 'little') <= entry['created_slot'],
              'Executable is mutable, unbound, or replaced after creation')
        actual = hashlib.sha256(data[45:].rstrip(b'\0')).hexdigest()
        check(actual == manifest['executables'][program], 'Executable differs from selected manifest')
        if program == bundle['program']:
            check(actual == release['executable_hash'].hex(), 'Release executable commitment differs')
            source = json.dumps(manifest['source'], sort_keys=True, separators=(',', ':')).encode()
            check(hashlib.sha256(source).digest() == release['source_hash'], 'Public source commitment differs')


def capture(rpc, manifest):
    check(rpc.request('getGenesisHash', []) == manifest['genesisHash'], 'RPC genesis differs')
    options = {'encoding': 'base64', 'commitment': 'finalized', 'withContext': True}
    response = rpc.request('getProgramAccounts', [manifest['program'], options])
    slot = response['context']['slot']
    rows = response['value']
    if len(rows) > 200000:
        raise Incomplete('Program account enumeration exceeds capture bound')
    accounts = {row['pubkey']: row['account'] for row in rows}
    check(len(accounts) == len(rows), 'Duplicate program account')
    missing = [key for key in manifest['externalAccounts'] if key not in accounts]
    for first in range(0, len(missing), 100):
        batch = missing[first:first + 100]
        data = rpc.request('getMultipleAccounts', [batch, {'encoding': 'base64', 'commitment': 'finalized',
                                                        'minContextSlot': slot}])
        check(data['context']['slot'] == slot, 'Accounts came from different finalized banks; retry capture')
        check(len(data['value']) == len(batch), 'Incomplete external account batch')
        accounts.update(zip(batch, data['value']))
    block = rpc.request('getBlock', [slot, {'commitment': 'finalized', 'transactionDetails': 'none', 'rewards': False}])
    if not block or not block.get('blockhash'):
        raise Incomplete('Finalized checkpoint block unavailable')
    bundle = {'program': manifest['program'], 'instance': manifest['instance'], 'registryEntry': manifest['registryEntry'],
              'genesisHash': manifest['genesisHash'], 'slot': slot, 'blockhash': block['blockhash'], 'accounts': accounts}
    report = verify_snapshot(bundle, manifest)
    last = rpc.request('getBlock', [slot, {'commitment': 'finalized', 'transactionDetails': 'none', 'rewards': False}])
    check(last and last['blockhash'] == block['blockhash'], 'Checkpoint changed during capture')
    return bundle, report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--snapshot', type=Path)
    parser.add_argument('--isolated-runtime-fixture', action='store_true')
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text()) if args.manifest else None
    if args.snapshot:
        result = verify_snapshot(json.loads(args.snapshot.read_text()), manifest,
                                 isolated=args.isolated_runtime_fixture)
    else:
        if manifest is None or args.isolated_runtime_fixture:
            raise Incomplete('RPC capture requires a source/code/network manifest')
        endpoint = os.environ.get('CHANCE_SOLANA_RPC_URL')
        if not endpoint:
            raise Incomplete('Set CHANCE_SOLANA_RPC_URL; its value is never printed')
        _, result = capture(Rpc(endpoint), manifest)
    print(json.dumps(result, default=json_default, indent=2))
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (Mismatch, ref.Invalid) as error:
        print(json.dumps({'status': 'contradictory', 'error': str(error)})); sys.exit(1)
    except (Incomplete, KeyError, ValueError, TypeError, OSError) as error:
        print(json.dumps({'status': 'incomplete', 'error': str(error)})); sys.exit(2)
