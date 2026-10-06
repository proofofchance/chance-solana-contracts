"""Independent V1 selection calculations over ordered reconstructed participants."""
import hashlib
from common import Incomplete, ZERO, pubkey


def sha(data):
    return hashlib.sha256(data).digest()


def le(value, size=8):
    return value.to_bytes(size, 'little')


def aggregate(participants):
    number = 0
    for participant in participants:
        if participant['included']:
            number ^= int.from_bytes(participant['digest'], 'little')
    return number.to_bytes(32,'little')


def daily(state, participants):
    eligible = [p for p in participants if p['included']]
    if not eligible:
        raise Incomplete('No revealed lottery population')
    votes = {}
    for p in eligible:
        count = p['vote']
        if 1 <= count <= min(state['max_winners_cap'], len(participants)-1):
            weight, first = votes.get(count, (0, p['attested_at']))
            votes[count] = (weight+p['tickets'], min(first, p['attested_at']))
    chosen = min(votes, key=lambda v:(-votes[v][0],votes[v][1],v)) if votes else 1
    count = min(chosen, len(eligible), state['max_winners_cap'])
    pool = ZERO
    for p in eligible:
        pool = sha(b'IKIGAI_RPD_V3_POOL'+pool+le(p['index'])+pubkey(p['wallet'])+le(p['tickets'])+p['digest'])
    seed = sha(b'IKIGAI_RPD_V3_SEED'+le(state['id'])+le(len(eligible))+le(sum(p['tickets'] for p in eligible))+aggregate(participants)+pool)
    remaining, winners, commitment = eligible.copy(), [], ZERO
    for index in range(count):
        weight = sum(p['tickets'] for p in remaining)
        threshold = 2**128 % weight
        for nonce in range(1024):
            sample = int.from_bytes(sha(b'IKIGAI_RPD_V3_DRAW'+seed+le(index)+le(nonce,4))[:16], 'little')
            if sample >= threshold:
                ticket = sample % weight
                break
        else:
            raise Incomplete('Draw exceeds bounded rejection sampling limit')
        for position, p in enumerate(remaining):
            if ticket < p['tickets']:
                selected = remaining.pop(position)
                break
            ticket -= p['tickets']
        winners.append(selected['wallet'])
        commitment = sha(b'IKIGAI_WINNERS_V2'+commitment+le(index,4)+pubkey(selected['wallet'])+le(selected['tickets']))
    return {'winners':winners,'seed':seed,'commitment':commitment,'count':count,'pool':pool}


def giveaway(state, participants):
    eligible = [p for p in participants if p['included'] and p['attested'] and not p['disqualified']]
    if not eligible:
        raise Incomplete('No eligible giveaway population')
    pool = ZERO
    for p in eligible:
        leaf = sha(b'GIVEAWAY_PARTICIPANT_V3'+le(p['index'])+pubkey(p['wallet'])+p['digest'])
        pool = sha(b'GIVEAWAY_POOL_V3'+pool+leaf)
    seed = sha(b'GIVEAWAY_FINALIZE_SEED_V3'+le(state['id'])+le(len(eligible))+aggregate(participants)+pool)
    ranked = sorted((sha(b'GIVEAWAY_RANK_V2'+seed+pubkey(p['wallet']))+pubkey(p['wallet']),p['wallet']) for p in eligible)
    count = min(state['number_of_winners'],len(ranked))
    threshold = ranked[count-1][0]
    chosen = {wallet for _,wallet in ranked[:count]}
    # On-chain emission scans join order after computing the rank threshold.
    winners = [p['wallet'] for p in participants if p['wallet'] in chosen]
    return {'winners':winners,'seed':seed,'threshold':threshold,'count':count,'pool':pool,
            'commitment':sha(b'GIVEAWAY_THRESHOLD_V2'+threshold+le(count,4))}
