#!/usr/bin/env python3
"""Export the explicitly supported Borsh account layouts from public Rust sources.

This intentionally rejects unfamiliar types; it is not a general Rust parser.
Run from any directory. CI checks the generated result against the committed file.
"""
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
ACCOUNTS = {
    'registry': {'crates/registry-wire/src/lib.rs': ['Config', 'Release', 'Record', 'Instance', 'BusinessKey']},
    'daily': {f'programs/daily-lottery-v1/src/state/{file}.rs': names for file, names in [
        ('config', ['Config']), ('lottery', ['Lottery']), ('participant', ['Participant']),
        ('finalization_ledger', ['FinalizationLedger', 'WinnerPage']), ('vault', ['Vault'])]},
    'giveaway': {f'programs/giveaways-v1/src/state/{file}.rs': names for file, names in [
        ('config', ['Config']), ('giveaway', ['Giveaway']), ('participant', ['Participant']), ('winners_ledger', ['WinnersLedger'])]},
}
TAGS = dict(Config='CHREG001', Release='CHREL001', Record='CHREC001', Instance='CHINS001', BusinessKey='CHKEY001')


def export():
    output = {'schema': 1, 'encoding': 'borsh-little-endian', 'accounts': {}, 'sourceSha256': {},
              'limitations': ['Giveaway zero-copy FinalizationLedger and native WinnerPage trailing entries require their explicit public layouts.',
                              'This account schema is not an Anchor instruction IDL.']}
    for domain, sources in ACCOUNTS.items():
        output['accounts'][domain] = {}
        for file, names in sources.items():
            raw = (ROOT / file).read_bytes()
            output['sourceSha256'][file] = hashlib.sha256(raw).hexdigest()
            text = re.sub(r'//[^\n]*', '', raw.decode())
            for name in names:
                match = re.search(r'pub struct '+name+r'\s*\{(.*?)\n\}', text, re.S)
                if not match:
                    raise ValueError('Missing struct '+name)
                fields = []
                for field, kind in re.findall(r'pub\s+(\w+):\s*([^,]+),', match[1]):
                    kind = re.sub(r'\s+', '', kind)
                    if kind == 'GiveawayStatus':
                        kind = 'u8'
                    if not re.fullmatch(r'Pubkey|u8|u16|u32|u64|i64|bool|String|Vec<u8>|\[u8;\d+\]', kind):
                        raise ValueError(f'Unsupported exported field {name}.{field}: {kind}')
                    fields.append({'name': field, 'type': kind})
                if not fields:
                    raise ValueError('Empty schema '+name)
                discriminator = TAGS[name].encode() if domain == 'registry' else hashlib.sha256(('account:'+name).encode()).digest()[:8]
                output['accounts'][domain][name] = {'discriminator': discriminator.hex(), 'tagIsField': domain == 'registry', 'fields': fields}
    return output


if __name__ == '__main__':
    (ROOT / 'audit/schemas/accounts-v1.json').write_text(json.dumps(export(), indent=2)+'\n')
