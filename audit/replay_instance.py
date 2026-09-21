#!/usr/bin/env python3
"""Reconstruct versioned Solana event history from canonical RPC transactions."""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import sys

from common import Incomplete, Mismatch, Reader, Rpc, ZERO, b58decode, b58encode, check, encode, json_default, pubkey
from history import events, transactions
from inspect_instance import inspect
import reference


def digest(value):
    return bytes.fromhex(value) if isinstance(value,str) else bytes(value)


def original_state(state, domain):
    fixed = {'id','config','vault','created_at_unix','upload_start_unix','upload_deadline_unix','account_version','settlement_deadline_unix'}
    fixed |= {'authority','buy_start_unix','buy_deadline_unix','vault_bump','service_charge_bps','ticket_price_lamports','max_winners_cap'} if domain=='daily' else {'creator','total_payout_lamports','number_of_winners','service_fee_bps','active_start_unix','active_deadline_unix','provider_authority'}
    initial = {}
    for key,value in state.items():
        initial[key] = value if key in fixed else bytes(len(value)) if isinstance(value,bytes) else False if isinstance(value,bool) else 0
    if domain == 'daily':
        initial['selected_number_of_winners'] = 1
        initial['paid_winners_bitmap'] = bytes(32)
    return initial


def instruction(tx, program, instance):
    keys = tx['keys']
    selected = [(i,ix) for i,ix in enumerate(tx['record']['transaction']['message']['instructions']) if keys[ix['programIdIndex']]==program and instance in [keys[index] for index in ix['accounts']]]
    if len(selected)!=1:
        raise Incomplete('Replay requires a single top-level instance instruction per transaction; bundled/CPI attribution is unsupported')
    index, ix = selected[0]
    return index, [keys[n] for n in ix['accounts']], b58decode(ix['data'])


def receipt_authorized(tx, index, authority, message):
    instructions = tx['record']['transaction']['message']['instructions']
    for ix in instructions[:index]:
        if tx['keys'][ix['programIdIndex']] != 'Ed25519SigVerify111111111111111111111111111':
            continue
        raw = b58decode(ix['data'])
        if len(raw)<16 or raw[:2] != b'\x01\x00':
            continue
        offsets = [int.from_bytes(raw[i:i+2],'little') for i in range(2,16,2)]
        sig, sig_ix, key, key_ix, msg, length, msg_ix = offsets
        if (sig_ix,key_ix,msg_ix)!=(65535,65535,65535) or sig+64>len(raw) or key+32>len(raw) or msg+length>len(raw):
            continue
        if raw[key:key+32]==pubkey(authority) and raw[msg:msg+length]==message:
            return True
    return False


def replay(rpc, report, history=None):
    domain, state = report['domain'], report['state']
    instance, program = report['instance'], report['programs']['instance']['program']
    authority = state['authority' if domain=='daily' else 'provider_authority']
    initial = original_state(state, domain)
    initial_hash = hashlib.sha256(encode(domain,'Lottery' if domain=='daily' else 'Giveaway',initial)[8:]).digest()
    check(initial_hash==report['inventory']['rules_hash'], 'Frozen initial account state differs from creation commitment')
    rows = transactions(rpc,report) if history is None else history
    people = {}
    selected, paid, refunds = [],set(),set()
    count_created, vested, refunded, complete = 0,False,False,False
    funded = state['total_payout_lamports'] if domain=='giveaway' else 0
    paid_principal = 0
    unknowns = []
    checks = ['initial_rules_hash']
    computed = None
    for tx in rows:
        records = [record for record in events(tx['record']['meta']['logMessages'],program) if record['data'].get('lottery' if domain=='daily' else 'giveaway')==instance]
        if not records:
            continue
        index, accounts, raw = instruction(tx,program,instance)
        signers = tx['keys'][:tx['record']['transaction']['message']['header']['numRequiredSignatures']]
        direct = raw[:1]==b'\x0f' if domain=='daily' else raw[:8]==hashlib.sha256(b'global:attest_reveal').digest()[:8]
        payments = collections.Counter()
        for record in records:
            name,event = record['event_type'],record['data']
            timestamp = event.get('timestamp',event.get('created_at_unix',state['created_at_unix']))
            check(event.get('lottery_id' if domain=='daily' else 'giveaway_id',state['id'])==state['id'], 'Event instance ID mismatch')
            if name in ('LotteryCreated','GiveawayCreated'):
                count_created+=1
                check(count_created==1 and tx['slot']==report['inventory']['created_slot'], 'Missing or repeated canonical creation')
                for key,value in event.items():
                    if key in initial:
                        check(initial[key]==value,'Creation terms mismatch: '+key)
            elif name in ('TicketsPurchased','ParticipationSubmitted'):
                wallet = event['buyer' if domain=='daily' else 'participant']
                check(wallet in signers,'Participation lacks wallet signature')
                check(state['buy_start_unix' if domain=='daily' else 'active_start_unix']<=timestamp<state['buy_deadline_unix' if domain=='daily' else 'active_deadline_unix'],'Participation outside fixed window')
                commitment = event['proof_of_chance_hash' if domain=='daily' else 'commitment_hash']
                if wallet not in people:
                    check(event['participant_index']==len(people),'Omitted or unordered participant')
                    check(commitment is not None,'First participation lacks commitment')
                    people[wallet]={'wallet':wallet,'index':len(people),'commitment':digest(commitment),'tickets':0,'attested':False,'included':False,'disqualified':False,'digest':ZERO,'vote':0,'attested_at':0,
                                    'account':event['participant' if domain=='daily' else 'participant_account']}
                p=people[wallet]
                if domain=='daily':
                    check(commitment is None or digest(commitment)==p['commitment'],'Ticket commitment changed')
                    p['tickets']+=event['tickets_bought']
                    check(event['amount_paid']==event['tickets_bought']*state['ticket_price_lamports'],'Ticket funding amount mismatch')
                    funded+=event['amount_paid']
                    check(p['tickets']==event['total_tickets_for_participant'] and funded==event['total_funds'],'Ticket accounting mismatch')
                else:
                    check(not p['attested'] and not p['disqualified'],'Ineligible participation update')
                    p['commitment']=digest(commitment)
            elif name=='ParticipantDisqualified':
                p=people[event['participant']]
                check(state['creator'] in signers and not p['disqualified'] and not p['attested'] and timestamp<state['upload_start_unix'],'Invalid exclusion authority or timing')
                p['disqualified']=True
            elif name=='AttestationSubmitted':
                wallet=event['wallet' if domain=='daily' else 'participant']
                p=people[wallet]
                check((wallet in signers or (domain=='giveaway' and not direct)) and not p['attested'] and not p['disqualified'],'Invalid attestation actor or duplicate')
                check(state['upload_start_unix']<=timestamp<state['upload_deadline_unix'],'Attestation outside fixed window')
                vote=event.get('voted_number_of_winners',0)
                check(accounts[3]==wallet, 'Attestation account differs from emitted wallet')
                if domain=='daily':
                    check(raw[0] in [4,15] and int.from_bytes(raw[1:9],'little')==vote, 'Attestation vote differs from instruction')
                if not direct:
                    message=(b'IKIGAI_ATTEST_V2' if domain=='daily' else b'GIVEAWAY_ATTEST_V1')+pubkey(instance)+pubkey(wallet)+p['commitment']
                    if domain=='daily': message+=reference.le(vote)
                    check(receipt_authorized(tx,index,authority,message),'Missing prior provider Ed25519 receipt verification')
                p.update(attested=True,attested_at=timestamp,vote=vote)
            elif name=='RevealsUploaded':
                reader=Reader(raw[1:] if domain=='daily' else raw[8:])
                reveals=[]
                if direct:
                    wallet=accounts[3]
                    if domain=='daily':
                        reader.integer(8)
                        plaintext=reader.value('Vec<u8>')
                    else:
                        plaintext=reader.value('String').encode()+b'\x1f'+reader.value('Vec<u8>')
                    reveals=[(wallet,plaintext)]
                else:
                    check(raw[:1]==b'\x05' if domain=='daily' else raw[:8]==hashlib.sha256(b'global:upload_reveals').digest()[:8],'Unknown reveal instruction')
                    count=reader.integer(4)
                    if count>4096: raise Incomplete('Reveal batch exceeds resource bound')
                    for _ in range(count):
                        who=reader.value('Pubkey')
                        wallet=who if domain=='daily' else next((p['wallet'] for p in people.values() if p['account']==who),None)
                        if wallet is None: raise Incomplete('Reveal refers to an unknown participant account')
                        plaintext=reader.value('Vec<u8>') if domain=='daily' else reader.value('String').encode()+b'\x1f'+reader.value('Vec<u8>')
                        reveals.append((wallet,plaintext))
                    if timestamp<state['upload_deadline_unix']:
                        check(all(p['attested'] for p in people.values() if not p['disqualified']),'Provider revealed before all eligible attestations')
                        check(authority in signers,'Premature publication lacks provider signature')
                check(reader.offset==len(reader.data),'Unrecognized reveal instruction tail')
                for wallet,plaintext in reveals:
                    p=people[wallet]
                    if p['included']: continue
                    check(p['attested'] and not p['disqualified'] and hashlib.sha256(plaintext).digest()==p['commitment'],'Reveal does not match accepted commitment')
                    prefix=b'IKIGAI_RPD_V2_REVEAL' if domain=='daily' else b'GIVEAWAY_REVEAL_V1'
                    p.update(included=True,digest=reference.sha(prefix+pubkey(wallet)+reference.le(len(plaintext),4)+plaintext))
                check(reference.aggregate(people.values())==digest(event['aggregate_hash']),'Reveal aggregate mismatch')
            elif name=='WinnerSelected':
                if domain=='giveaway': check(authority in signers,'Winner selection lacks provider authority')
                check(event['winner_index' if domain=='daily' else 'emission_index']==len(selected),'Winner ordering is incomplete')
                selected.append(event['winner' if domain=='daily' else 'participant'])
            elif name=='FinalizationChunkProcessed' and domain=='giveaway':
                check(authority in signers,'Finalization lacks frozen provider authority')
            elif name=='WinnersComputed':
                check(not vested and not refunded and timestamp<state['settlement_deadline_unix'],'Late or repeated vesting')
                check(all(p['included'] for p in people.values() if p['attested']),'Vesting omits accepted reveals')
                computed=(reference.daily if domain=='daily' else reference.giveaway)(state,list(people.values()))
                check(computed['seed']==digest(event['seed']) and computed['winners']==selected,'Independent selection differs from recorded winners')
                if domain=='giveaway':
                    check(computed['commitment']==digest(event['merkle_root']),'Threshold commitment mismatch')
                vested=True
            elif name=='WinnersFinalized':
                check(vested and computed['commitment']==digest(event['winners_merkle_root']),'Winner commitment mismatch')
            elif name=='WinnerPaid':
                wallet=event['winner'];amount=event['amount' if domain=='daily' else 'amount_lamports']
                check(vested and wallet in selected and wallet not in paid,'Unvested, duplicate or unknown winner payment')
                fee=funded*state['service_charge_bps' if domain=='daily' else 'service_fee_bps']//10000
                check(amount==(funded-fee)//len(selected),'Incorrect winner payout')
                paid.add(wallet);payments[wallet]+=amount;paid_principal+=amount
            elif name=='ServiceFeePaid':
                check(vested and len(paid)==len(selected) and event['authority']==authority,'Fee paid before all winner entitlements')
                amount=event['service_fee']+event['remainder'] if domain=='daily' else event['service_fee_lamports']
                expected=funded-paid_principal if domain=='daily' else funded*state['service_fee_bps']//10000
                check(amount==expected,'Incorrect service fee/remainder')
                payments[authority]+=amount+event.get('vault_rent_reclaimed',0);paid_principal+=amount
            elif name in ('NoWinners','RefundsIssued'):
                check(not vested and not refunded,'Refund attempts to replace vested claims')
                active_end=state['buy_deadline_unix' if domain=='daily' else 'active_deadline_unix']
                low=(len(people)<=1 if domain=='daily' else not any(not p['disqualified'] for p in people.values())) and timestamp>=active_end
                no_attesters=timestamp>=state['upload_deadline_unix'] and not any(p['attested'] for p in people.values())
                missing=any(p['attested'] and not p['included'] for p in people.values())
                check(timestamp>=state['settlement_deadline_unix'] or low or no_attesters or (missing and timestamp>=state['upload_deadline_unix']+1800),'Premature refund')
                refunded=True
            elif name=='CreatorRefunded':
                check(event['creator']==state['creator'] and (refunded or vested),'Unexplained creator refund')
                payments[state['creator']]+=event['amount_lamports']
                # A closed vault can include rent or donations; cap at remaining principal.
                paid_principal+=min(funded-paid_principal,event['amount_lamports'])
            elif name=='RefundClaimed':
                wallet=event['wallet'];amount=event['amount']
                check(refunded and wallet not in refunds and wallet in signers,'Invalid participant refund')
                check(amount==people[wallet]['tickets']*state['ticket_price_lamports'],'Incorrect refund principal')
                refunds.add(wallet);payments[wallet]+=amount;paid_principal+=amount
            elif name=='RefundVaultClosed':
                check(refunded and paid_principal==funded and event['authority']==authority,'Premature refund vault closure')
                payments[authority]+=event['vault_rent_reclaimed'];complete=True
            elif name in ('GiveawaySettled','PayoutsComplete','NoBuyersConcluded'):
                check((vested and len(paid)==len(selected)) or refunded or (name=='NoBuyersConcluded' and not people),'Completion lacks terminal outcome')
                complete=True
            elif name in ('GiveawayUpdated','BuyPhaseBegan','RevealWindowAdjusted'):
                raise Mismatch('Fixed instance rules were changed after creation')
            elif name=='WinnersLocked':
                check(vested and authority in signers and event['final_winners_count']==len(selected),'Invalid winner lock')
            elif name=='UploadPhaseBegan':
                start=event['new_start' if domain=='daily' else 'upload_start_unix']
                deadline=event['new_deadline' if domain=='daily' else 'upload_deadline_unix']
                check(start==state['upload_start_unix'] and deadline==state['upload_deadline_unix'],'Upload phase changed fixed terms')
            elif name=='FinalizationChunkProcessed':
                pass  # Provisional progress; canonical winners are checked at vesting.
            elif name in ('RevealRemediationBegan','RevealRemediationCompleted','SettlementPhaseBegan','WinnersAlgorithmInterlude','WinnersLuckyWords'):
                pass  # Informational summaries; replay derives inputs and outcomes independently.
            elif name=='LotterySettled' and refunded:
                check(not event['winners'] and event['winner_payout']==0,'Refund summary contains winner payouts')
            else:
                raise Incomplete('Unsupported instance event: '+name)
        if payments:
            meta=tx['record']['meta'];keys=tx['keys']
            for wallet,amount in payments.items():
                idx=keys.index(wallet)
                delta=meta['postBalances'][idx]-meta['preBalances'][idx]
                if idx==0: delta+=meta['fee']
                check(delta==amount,'Recipient balance delta differs from payout evidence')
            idx=keys.index(state['vault'])
            check(meta['preBalances'][idx]-meta['postBalances'][idx]==sum(payments.values()),'Vault outflow differs from payout evidence')
    check(count_created==1,'Canonical creation evidence is missing')
    check(len(people)==state['participants_count'],'Participant history is incomplete')
    check(sum(p['attested'] for p in people.values())==state['attested_count'],'Attestation history is incomplete')
    check(sum(p['included'] for p in people.values())==state['provider_uploaded_count'],'Reveal history is incomplete')
    check(reference.aggregate(people.values())==state['poc_aggregate_hash'],'Account aggregate differs from replay')
    check(0<=paid_principal<=funded,'Principal accounting is invalid')
    check(state['settled']==(complete or refunded),'Settlement state disagrees with complete event history')
    if domain=='giveaway':
        check(state['winners_computed']==vested and state['winners_locked']==vested,'Vesting state disagrees with history')
        if vested:
            check(report['winnersLedger']['paid_count']==len(paid),'Paid count differs from history')
            check(report['winnersLedger']['winners_count']==len(selected),'Winner count differs from history')
    else:
        check(state['total_funds']==funded and state['total_tickets']==sum(p['tickets'] for p in people.values()),'Funding state differs from history')
        check(state['winners_count']==(len(selected) if vested else 0),'Winner count differs from history')
        if vested:
            check(state['winners_merkle_root']==computed['commitment'],'State winner commitment differs from history')
            check(sum(byte.bit_count() for byte in state['paid_winners_bitmap'])==len(paid),'Paid bitmap differs from history')
    check(report['escrow']['balance']>=funded-paid_principal,'Vault cannot cover outstanding principal')
    if selected and not vested and not refunded: unknowns.append('Selection is provisional; vesting is not complete')
    checks += ['canonical_creation','participant_inventory','attestation_authorization','plaintext_reveals']
    if vested: checks.append('independent_selection')
    if paid_principal: checks.append('payout_balance_deltas')
    return {'schema':1,'instance':instance,'network':report['network'],'registry':report['registry'],
        'release':report['release'],'releaseHistory':report['releaseHistory'],'programs':report['programs'],'checkpoint':report['checkpoint'],'eventReplay':'incomplete' if unknowns else 'pass',
        'checks':checks,'unknowns':unknowns,'winners':selected if vested else [],'buildMatch':report['buildMatch'],'securityReview':'not_run',
        'accounting':{'fundedPrincipal':funded,'paidPrincipal':paid_principal,'outstandingPrincipal':funded-paid_principal,'eventComplete':complete,'financiallyClosed':complete and paid_principal==funded,'vaultBalance':report['escrow']['balance']},
        'trustAssumptions':report['trustAssumptions'],
        'limitations':['Only single top-level instance instructions are supported; bundled/CPI attribution is explicit unsupported evidence.',
                       'Observed execution replay does not establish manipulation resistance or absence of implementation vulnerabilities.',
                       'Current participant account snapshots and arbitrary surplus transfers require further reconciliation.']}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--registry',required=True);parser.add_argument('--inventory',required=True);parser.add_argument('--genesis-hash',required=True)
    parser.add_argument('--build-manifest',type=Path);parser.add_argument('--elf',type=Path)
    args=parser.parse_args()
    if bool(args.build_manifest)!=bool(args.elf): raise Incomplete('Supply both build manifest and ELF')
    endpoint=os.environ.get('CHANCE_SOLANA_RPC_URL')
    if not endpoint: raise Incomplete('Set CHANCE_SOLANA_RPC_URL')
    rpc=Rpc(endpoint)
    report=inspect(rpc,args.genesis_hash,args.registry,args.inventory,(args.build_manifest,args.elf) if args.elf else None)
    result=replay(rpc,report)
    print(json.dumps(result,default=json_default,indent=2))
    return 0 if result['eventReplay']=='pass' else 2


if __name__=='__main__':
    try: sys.exit(main())
    except Mismatch as error:
        print(json.dumps({'eventReplay':'fail','error':str(error)}));sys.exit(1)
    except (Incomplete,KeyError,ValueError,OSError) as error:
        print(json.dumps({'eventReplay':'incomplete','error':str(error)}));sys.exit(2)
