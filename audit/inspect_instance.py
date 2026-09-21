#!/usr/bin/env python3
"""Inspect Solana release, ownership, immutable executable and public build evidence."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys

from common import (LOADER, ZERO, Incomplete, Mismatch, Rpc, account_data, b58encode,
                    check, decode, json_default, pda, pubkey)


def inspect(rpc, genesis, registry, entry_address, build=None):
    check(rpc.request('getGenesisHash', []) == genesis, 'RPC genesis differs from selected network')
    pubkey(registry)
    pubkey(entry_address)
    options = {'encoding':'base64', 'commitment':'finalized'}
    first = rpc.request('getAccountInfo', [entry_address, options])
    entry = decode('registry', 'Instance', account_data(first['value'], registry))
    cfg_address, rel_address = b58encode(entry['registry']), b58encode(entry['release'])
    program, instance = b58encode(entry['owner_program']), b58encode(entry['instance'])
    target_data, registry_data = pda(LOADER, pubkey(program)), pda(LOADER, pubkey(registry))
    vault = pda(program,b'vault',pubkey(instance))
    winners = pda(program,b'winners_root_v2',pubkey(instance))
    addresses = [entry_address, cfg_address, rel_address, program, target_data, registry, registry_data, instance, vault, winners]
    batch = rpc.request('getMultipleAccounts', [addresses, {**options, 'minContextSlot': first['context']['slot']}])
    if len(batch['value']) != len(addresses):
        raise Incomplete('Incomplete account batch')
    accounts = dict(zip(addresses, batch['value']))
    # All related mutable accounts are read from this single finalized bank.
    entry = decode('registry', 'Instance', account_data(accounts[entry_address], registry))
    check(b58encode(entry['registry']) == cfg_address and b58encode(entry['release']) == rel_address
          and b58encode(entry['owner_program']) == program and b58encode(entry['instance']) == instance,
          'Inventory identity changed during collection')
    cfg = decode('registry', 'Config', account_data(accounts[cfg_address], registry))
    release = decode('registry', 'Release', account_data(accounts[rel_address], registry))
    check(cfg['schema'] == 1, 'Unknown registry schema')
    check(pda(registry, b'registry', cfg['authority']) == cfg_address, 'Invalid registry config PDA')
    check(pda(registry, b'instance', pubkey(cfg_address), entry['sequence'].to_bytes(8,'little')) == entry_address, 'Invalid inventory PDA')
    check(pda(registry, b'release', pubkey(cfg_address), pubkey(program)) == rel_address, 'Invalid release PDA')
    check(release['registry'] == pubkey(cfg_address) and release['program'] == pubkey(program), 'Release identity mismatch')
    check(1 <= release['sequence'] <= cfg['release_count'] and 1 <= entry['sequence'] <= cfg['instance_count'], 'Inventory exceeds registry counts')
    check(release['status'] in [1, 2, 3, 4], 'Unknown release lifecycle status')
    check(entry['created_slot'] >= release['registered_slot'] and entry['created_slot'] <= batch['context']['slot'], 'Inconsistent creation slot')
    domain = {1:'daily', 2:'giveaway'}.get(release['domain'])
    if domain is None:
        raise Incomplete('Unsupported release domain')
    state = decode(domain, 'Lottery' if domain == 'daily' else 'Giveaway', account_data(accounts[instance], program))
    check(state['account_version'] == 2, 'Unsupported instance account version')
    check(pda(program, b'lottery' if domain == 'daily' else b'giveaway', pubkey(state['config']), state['id'].to_bytes(8,'little')) == instance, 'Invalid event PDA')
    check(pda(program, b'vault', pubkey(instance)) == state['vault'], 'Invalid vault PDA')
    ledger = None
    if domain=='giveaway' and state['winners_computed']:
        ledger = decode('giveaway','WinnersLedger',account_data(accounts[winners],program))
        check(ledger['giveaway']==instance and ledger['protocol_version']==2,'Winner ledger binding mismatch')
    if accounts[vault] is not None:
        account_data(accounts[vault],program)
    escrow={'address':vault,'balance':accounts[vault]['lamports'] if accounts[vault] else 0}
    binaries = {}
    for label, owner_program, data_address in [('instance', program, target_data), ('registry', registry, registry_data)]:
        descriptor = account_data(accounts[owner_program], LOADER)
        raw = account_data(accounts[data_address], LOADER)
        check(accounts[owner_program]['executable'] and descriptor[:4] == (2).to_bytes(4,'little') and descriptor[4:36] == pubkey(data_address), 'Unsupported executable descriptor')
        check(not accounts[data_address]['executable'] and raw[:4] == (3).to_bytes(4,'little') and len(raw) > 45, 'Invalid ProgramData')
        check(raw[12] in [0,1], 'Invalid loader authority option')
        authority = None if raw[12] == 0 else b58encode(raw[13:45])
        binaries[label] = {'program':owner_program, 'programData':data_address,
            'deploymentSlot':int.from_bytes(raw[4:12],'little'), 'upgradeAuthority':authority,
            'executableSha256':hashlib.sha256(raw[45:].rstrip(b'\0')).hexdigest()}
        check(authority is None, label+' program is mutable; fixed rules cannot be established')
        check(binaries[label]['deploymentSlot']<=entry['created_slot'], label+' executable was replaced after instance creation')
    check(binaries['instance']['executableSha256'] == release['executable_hash'].hex(), 'Executable differs from release commitment')
    build_result = {'status':'unavailable', 'reason':'Provide a separately reproduced public build manifest and ELF'}
    if build:
        manifest_path, elf_path = build
        manifest = json.loads(manifest_path.read_text())
        check(manifest.get('schema') == 1 and manifest.get('library') == {'daily':'daily_lottery', 'giveaway':'giveaways'}[domain], 'Wrong build manifest schema/library')
        reference = manifest['source']
        check(isinstance(reference['commit'],str) and len(reference['commit']) == 40 and all(c in '0123456789abcdef' for c in reference['commit']), 'Source commit must be a full SHA')
        check(reference['repository'].startswith('https://github.com/proofofchance/'), 'Unexpected public source repository')
        # Source reference is JSON with sorted keys, compact separators and UTF-8.
        committed = hashlib.sha256(json.dumps(reference, sort_keys=True, separators=(',',':')).encode()).hexdigest()
        check(committed == release['source_hash'].hex(), 'Public source reference differs from registry commitment')
        binary_hash = hashlib.sha256(elf_path.read_bytes().rstrip(b'\0')).hexdigest()
        check(binary_hash == manifest['executableSha256'] == binaries['instance']['executableSha256'], 'Reproduced ELF does not match deployment')
        build_result = {'status':'match', 'source':reference, 'executableSha256':binary_hash,
                       'qualification':'Binary equality; public fetch/build provenance and security require separate review'}
    check(pubkey(state['authority' if domain=='daily' else 'creator'])==entry['creator'], 'Creator differs from canonical inventory')
    slot = batch['context']['slot']
    if cfg['record_count']>100_000:
        raise Incomplete('Registry history exceeds audit resource limit')
    lifecycle = []
    for first_sequence in range(1,cfg['record_count']+1,100):
        sequences=list(range(first_sequence,min(first_sequence+100,cfg['record_count']+1)))
        record_addresses=[pda(registry,b'record',pubkey(cfg_address),sequence.to_bytes(8,'little')) for sequence in sequences]
        records=rpc.request('getMultipleAccounts',[record_addresses,{**options,'minContextSlot':slot}])
        if len(records['value'])!=len(sequences): raise Incomplete('Registry history is incomplete')
        for sequence,account in zip(sequences,records['value']):
            record=decode('registry','Record',account_data(account,registry))
            check(record['sequence']==sequence and record['registry']==pubkey(cfg_address) and record['slot']<=slot,'Registry history identity or slot mismatch')
            if record['release']==pubkey(rel_address): lifecycle.append(record)
    block = rpc.request('getBlock', [slot, {'commitment':'finalized', 'transactionDetails':'none', 'rewards':False}])
    if not block or not block.get('blockhash'):
        raise Incomplete('Finalized bank block is unavailable')
    return {'schema':1, 'network':{'genesisHash':genesis}, 'checkpoint':{'slot':slot,'blockhash':block['blockhash'],'commitment':'finalized'},
        'registry':registry, 'inventoryAddress':entry_address, 'instance':instance,'domain':domain,
        'escrow':escrow,'winnersLedger':ledger,'releaseHistory':lifecycle, 'inventory':entry, 'release':release, 'config':cfg, 'state':state, 'programs':binaries,
        'ownershipAndRelease':'pass', 'buildMatch':build_result, 'eventReplay':'not_run', 'securityReview':'not_run',
        'trustAssumptions':['Authentic and complete finalized RPC evidence', 'Caller-selected registry trust root'],
        'limitations':['Source/build match is separate from security review and event replay.',
                       'Retired release status does not remove historical payment rights.',
                       'No deployment or authority mutation is performed.']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--genesis-hash', required=True)
    parser.add_argument('--registry', required=True)
    parser.add_argument('--inventory', required=True)
    parser.add_argument('--build-manifest', type=Path)
    parser.add_argument('--elf', type=Path)
    args = parser.parse_args()
    if bool(args.build_manifest) != bool(args.elf):
        raise Incomplete('Supply both the public build manifest and reproduced ELF')
    endpoint = os.environ.get('CHANCE_SOLANA_RPC_URL')
    if not endpoint:
        raise Incomplete('Set CHANCE_SOLANA_RPC_URL')
    report = inspect(Rpc(endpoint), args.genesis_hash, args.registry, args.inventory,
                     (args.build_manifest, args.elf) if args.elf else None)
    print(json.dumps(report, default=json_default, indent=2))
    return 2  # Discovery/build equality is not an overall security or replay pass.


if __name__ == '__main__':
    try:
        sys.exit(main())
    except Mismatch as error:
        print(json.dumps({'ownershipAndRelease':'fail','error':str(error)}))
        sys.exit(1)
    except (Incomplete, KeyError, ValueError, OSError) as error:
        print(json.dumps({'ownershipAndRelease':'incomplete','error':str(error)}))
        sys.exit(2)
