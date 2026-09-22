"""Finalized transaction history with program-authenticated JSON event extraction."""
import json
import re
from common import Incomplete, check


def events(logs, program):
    """Discard reverted CPI subtrees; a log prefix alone cannot authenticate an emitter."""
    if logs is None:
        raise Incomplete('Transaction logs are unavailable')
    stack, accepted = [], []
    for line in logs:
        entered = re.fullmatch(r'Program (\w+) invoke \[(\d+)\]', line)
        finished = re.fullmatch(r'Program (\w+) (success|failed:.*)', line)
        if entered:
            if int(entered[2]) != len(stack)+1:
                raise Incomplete('Truncated or inconsistent program invocation trace')
            stack.append([entered[1], []])
        elif finished:
            if not stack or stack[-1][0] != finished[1]:
                raise Incomplete('Program completion lacks its invocation')
            _, buffered = stack.pop()
            if finished[2] == 'success':
                (stack[-1][1] if stack else accepted).extend(buffered)
        elif 'Log truncated' in line:
            raise Incomplete('Transaction event log was truncated')
        elif stack and stack[-1][0] == program:
            prefix = next((prefix for prefix in ['Program log: LOTTERY_EVENT: ', 'Program log: GIVEAWAY_EVENT: '] if line.startswith(prefix)), None)
            if prefix:
                record = json.loads(line[len(prefix):])
                if record.get('version') != '1.0.0':
                    raise Incomplete('Unsupported event schema')
                stack[-1][1].append(record['event'])
    if stack:
        raise Incomplete('Incomplete program invocation trace')
    return accepted


def transactions(rpc, report):
    """Fetch complete containing blocks to establish ordering within a slot.

    Signature pagination and canonical block responses are trusted RPC evidence,
    not a light-client proof. Missing/pruned creation history is rejected by replay.
    """
    address = report['instance']
    checkpoint = report['checkpoint']['slot']
    created = report['inventory']['created_slot']
    signatures, before = {}, None
    for _ in range(100):
        options = {'commitment':'finalized', 'limit':1000}
        if before:
            options['before'] = before
        batch = rpc.request('getSignaturesForAddress', [address, options])
        if not batch:
            break
        for record in batch:
            signature = record['signature']
            if signature in signatures:
                raise Incomplete('Duplicate signature pagination')
            signatures[signature] = record
        if len(batch) < 1000 or batch[-1]['slot'] < created:
            break
        before = batch[-1]['signature']
    else:
        raise Incomplete('Instance history exceeds 100,000 signature resource limit')
    wanted = {signature: record for signature,record in signatures.items() if created <= record['slot'] <= checkpoint and record['err'] is None}
    result = []
    for slot in sorted({record['slot'] for record in wanted.values()}):
        block = rpc.request('getBlock', [slot, {'commitment':'finalized', 'encoding':'json', 'transactionDetails':'full', 'rewards':False, 'maxSupportedTransactionVersion':0}])
        if not block or block.get('transactions') is None:
            raise Incomplete('Historical block/transactions are unavailable')
        found = set()
        for index, record in enumerate(block['transactions']):
            signatures_in_tx = record['transaction']['signatures']
            signature = signatures_in_tx[0]
            meta = record.get('meta')
            if meta is None:
                raise Incomplete('Historical transaction metadata is unavailable')
            message = record['transaction']['message']
            keys = message['accountKeys'] + meta.get('loadedAddresses', {}).get('writable', []) + meta.get('loadedAddresses', {}).get('readonly', [])
            if meta['err'] is None and address in keys and signature not in wanted:
                raise Incomplete('Successful instance transaction omitted from signature history')
            if signature not in wanted:
                continue
            check(wanted[signature]['slot'] == slot and meta['err'] is None, 'Signature history differs from block transaction')
            check(address in keys, 'Transaction does not reference the selected instance')
            found.add(signature)
            result.append({'slot':slot, 'blockhash':block['blockhash'], 'index':index, 'signature':signature,
                           'blockTime':block.get('blockTime'), 'keys':keys, 'record':record})
        if found != {signature for signature,record in wanted.items() if record['slot'] == slot}:
            raise Incomplete('A successful history signature is missing from its block')
    last = rpc.request('getBlock', [checkpoint, {'commitment':'finalized', 'transactionDetails':'none', 'rewards':False}])
    check(last is not None and last['blockhash'] == report['checkpoint']['blockhash'], 'Checkpoint changed during history collection')
    return result
