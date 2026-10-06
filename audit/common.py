"""Bounded public Solana wire decoding and read-only JSON-RPC helpers."""
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import ssl
import sys
import urllib.request

ALPHABET = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
LOADER = 'BPFLoaderUpgradeab1e11111111111111111111111'
ZERO = bytes(32)


class Incomplete(Exception):
    pass


class Mismatch(Exception):
    pass


def check(condition, message):
    if not condition:
        raise Mismatch(message)


def b58encode(raw):
    number = int.from_bytes(raw, 'big')
    text = ''
    while number:
        number, digit = divmod(number, 58)
        text = ALPHABET[digit] + text
    return '1' * (len(raw)-len(raw.lstrip(b'\0'))) + text


def b58decode(text):
    if not isinstance(text, str) or len(text) > 2048:
        raise Incomplete('Invalid base58 input')
    number = 0
    for char in text:
        if char not in ALPHABET:
            raise Incomplete('Invalid base58 character')
        number = number*58 + ALPHABET.index(char)
    return bytes(len(text)-len(text.lstrip('1'))) + number.to_bytes((number.bit_length()+7)//8, 'big')


def pubkey(text):
    raw = b58decode(text)
    if len(raw) != 32:
        raise Incomplete('Expected a 32-byte Solana address')
    return raw


def on_curve(raw):
    # Decompress Edwards25519 y, as required by find_program_address.
    prime = 2**255-19
    y = int.from_bytes(raw, 'little') & (2**255-1)
    y %= prime
    d = -121665 * pow(121666, prime-2, prime) % prime
    denominator = (d*y*y+1) % prime
    if denominator == 0:
        return False
    square = (y*y-1) * pow(denominator, prime-2, prime) % prime
    return square == 0 or pow(square, (prime-1)//2, prime) == 1


def pda(program, *seeds):
    if len(seeds) > 15 or any(len(seed) > 32 for seed in seeds):
        raise Incomplete('PDA seeds exceed runtime limits')
    for bump in range(255, -1, -1):
        digest = hashlib.sha256(b''.join(seeds)+bytes([bump])+pubkey(program)+b'ProgramDerivedAddress').digest()
        if not on_curve(digest):
            return b58encode(digest)
    raise Incomplete('No valid PDA bump')


class Reader:
    def __init__(self, data):
        self.data, self.offset = data, 0

    def take(self, count):
        if count < 0 or self.offset+count > len(self.data):
            raise Incomplete('Truncated serialized evidence')
        start = self.offset
        self.offset += count
        return self.data[start:self.offset]

    def integer(self, size, signed=False):
        return int.from_bytes(self.take(size), 'little', signed=signed)

    def value(self, kind):
        if kind == 'Pubkey':
            return b58encode(self.take(32))
        if kind == 'bool':
            value = self.integer(1)
            if value not in (0, 1):
                raise Incomplete('Invalid serialized boolean')
            return bool(value)
        if re.fullmatch(r'[ui](8|16|32|64)', kind):
            return self.integer(int(kind[1:])//8, kind[0] == 'i')
        fixed = re.fullmatch(r'\[u8;(\d+)\]', kind)
        if fixed:
            return self.take(int(fixed[1]))
        if kind in ('String', 'Vec<u8>'):
            count = self.integer(4)
            if count > 1_048_576:
                raise Incomplete('Serialized vector exceeds audit resource limit')
            raw = self.take(count)
            return raw.decode('utf-8') if kind == 'String' else raw
        raise Incomplete('Unsupported Borsh field: '+kind)


def decode(domain, name, raw):
    schema = json.loads((Path(__file__).parent/'schemas/accounts-v1.json').read_text())['accounts'][domain][name]
    if raw[:8].hex() != schema['discriminator']:
        raise Incomplete('Unsupported '+domain+' '+name+' discriminator')
    reader = Reader(raw if schema['tagIsField'] else raw[8:])
    result = {field['name']: reader.value(field['type']) for field in schema['fields']}
    # Account allocations may reserve zero-filled tail capacity beyond Borsh data.
    if name != 'WinnerPage' and any(reader.data[reader.offset:]):
        raise Incomplete('Nonzero unrecognized account tail')
    return result


def account_data(account, owner=None):
    if account is None:
        raise Incomplete('Required account is absent at the checkpoint')
    if owner:
        check(account['owner'] == owner, 'Account owner mismatch')
    encoded = account['data']
    if not isinstance(encoded, list) or encoded[1] != 'base64':
        raise Incomplete('RPC account data must be base64')
    raw = base64.b64decode(encoded[0], validate=True)
    if len(raw) > 10_485_760:
        raise Incomplete('Account exceeds runtime size limit')
    return raw


class Rpc:
    def __init__(self, endpoint):
        if not endpoint.startswith(('http://', 'https://')):
            raise Incomplete('RPC must use HTTP(S)')
        self.endpoint, self.counter = endpoint, 0
        cafile = os.environ.get('SSL_CERT_FILE')
        if cafile is None and sys.platform == 'darwin' and Path('/etc/ssl/cert.pem').exists():
            cafile = '/etc/ssl/cert.pem'
        self.tls = ssl.create_default_context(cafile=cafile)

    def request(self, method, params):
        self.counter += 1
        payload = json.dumps({'jsonrpc':'2.0', 'id':self.counter, 'method':method, 'params':params}).encode()
        try:
            request = urllib.request.Request(self.endpoint, payload, {'Content-Type':'application/json'})
            with urllib.request.urlopen(request, timeout=30, context=self.tls) as response:
                result = json.load(response)
        except Exception:
            raise Incomplete('RPC request failed: '+method) from None
        if result.get('id') != self.counter or result.get('error') or 'result' not in result:
            raise Incomplete('RPC cannot provide '+method+' evidence')
        return result['result']


def json_default(value):
    if isinstance(value, bytes):
        return value.hex()
    raise TypeError(type(value).__name__)


def encode(domain, name, state):
    schema = json.loads((Path(__file__).parent/'schemas/accounts-v1.json').read_text())['accounts'][domain][name]
    result = b'' if schema['tagIsField'] else bytes.fromhex(schema['discriminator'])
    for field in schema['fields']:
        kind, value = field['type'], state[field['name']]
        if kind == 'Pubkey':
            result += pubkey(value)
        elif kind == 'bool':
            result += bytes([int(value)])
        elif re.fullmatch(r'[ui](8|16|32|64)', kind):
            result += value.to_bytes(int(kind[1:])//8, 'little', signed=kind[0]=='i')
        elif kind.startswith('[u8;'):
            expected = int(kind[4:-1])
            if len(value) != expected:
                raise Incomplete('Fixed array width mismatch')
            result += value
        elif kind in ('String','Vec<u8>'):
            raw = value.encode() if kind == 'String' else value
            result += len(raw).to_bytes(4,'little')+raw
        else:
            raise Incomplete('Unsupported field encoding')
    return result
