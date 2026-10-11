#!/usr/bin/env python3
"""Reproduce fresh V1 ELF bytes from matching private/public source and public inputs.

No deployment, authority change, Git write or source-verification registration.
Both independent source overlays use the same pinned verifier image. The output
explicitly records local private/public reproduction rather than public CI.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile

IMAGE = 'solanafoundation/solana-verifiable-build@sha256:ff3b148fb6adc3025c46ac38f132f473ccbdc4391f253234d98aa6519aec07f8'
GENESIS = 'EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG'
INPUT_PATH = 'releases/staging-afresh-v1.inputs'
KEYS = ('schema', 'environment', 'genesis_hash', 'registry_program',
        'daily_lottery_program', 'giveaways_program', 'initialization_authority')
PROGRAMS = {'chance_registry': 'registry', 'daily_lottery': 'daily-lottery-v1', 'giveaways': 'giveaways-v1'}
ROOTS = ('programs', 'crates', '.cargo', 'Cargo.lock', 'rust-toolchain.toml', 'tools/idl-export')


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False)


def digest(value):
    return hashlib.sha256(value).hexdigest()


def parse_inputs(data):
    if len(data) >= 1024:
        raise ValueError('Staging build inputs are too large')
    text = data.decode('ascii')
    lines = text.splitlines()
    if not text.endswith('\n') or '\r' in text or len(lines) != len(KEYS):
        raise ValueError('Use the exact canonical staging input schema')
    result = {}
    alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
    decoded = set()
    for key, line in zip(KEYS, lines):
        expected, separator, value = line.partition('=')
        if expected != key or separator != '=' or not value or '=' in value:
            raise ValueError('Use ordered canonical staging input fields')
        result[key] = value
        if key in KEYS[3:]:
            if not 32 <= len(value) <= 44 or any(c not in alphabet for c in value):
                raise ValueError('Staging identities must be canonical Solana public keys')
            number = 0
            for character in value:
                number = number * 58 + alphabet.index(character)
            raw = b'\0' * (len(value) - len(value.lstrip('1'))) + number.to_bytes((number.bit_length() + 7) // 8, 'big')
            if len(raw) != 32 or raw == b'\0' * 32 or raw in decoded:
                raise ValueError('Program and bootstrap identities must be distinct nonzero 32-byte keys')
            decoded.add(raw)
    if result['schema'] != '1' or result['environment'] != 'staging' or result['genesis_hash'] != GENESIS:
        raise ValueError('Fresh V1 builds are staging/Solana Devnet only')
    return result


def git(source, *arguments):
    return subprocess.check_output(['git', '-C', str(source), *arguments], text=True).strip()


def source_inputs(source):
    if git(source, 'status', '--porcelain', '--', *ROOTS):
        raise ValueError('Commit contract and IDL build inputs before reproducing artifacts')
    files = git(source, 'ls-files', '--', *ROOTS).splitlines()
    if not files or any('\n' in name or '\r' in name for name in files):
        raise ValueError('Missing canonical contract build inputs')
    return {name: digest((source / name).read_bytes()) for name in files}


def source_snapshot(source, destination, input_bytes):
    # Archive tracked HEAD only: ignored targets, keys and local environment files
    # are never copied to the verifier. Preserve the repository-relative lock link.
    archive = destination.parent / (destination.name + '.tar')
    subprocess.run(['git', '-C', str(source), 'archive', '--format=tar', '--output', str(archive), 'HEAD'], check=True)
    destination.mkdir()
    with tarfile.open(archive) as records:
        records.extractall(destination, filter='data')
    archive.unlink()
    (destination / INPUT_PATH).parent.mkdir(parents=True, exist_ok=True)
    (destination / INPUT_PATH).write_bytes(input_bytes)


def build_overlay(source, verifier='solana-verify'):
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(('CARGO_', 'RUST_', 'ANCHOR_', 'CHANCE_'))
                   and key not in ('RUSTFLAGS', 'RUSTUP_TOOLCHAIN')}
    environment['DOCKER_DEFAULT_PLATFORM'] = 'linux/amd64'
    for library in PROGRAMS:
        subprocess.run([verifier, 'build', '--library-name', library, '--base-image', IMAGE,
                        str(source), '--', '-Znext-lockfile-bump', '--features', 'staging-afresh-v1'],
                       cwd=source, env=environment, check=True)
    subprocess.run(['cargo', 'run', '--locked', '--manifest-path', 'tools/idl-export/Cargo.toml',
                    '--', '--staging-afresh-v1'], cwd=source, env=environment, check=True)


def retain_only_build_outputs(source):
    """Bound scratch disk between independent builds; never called on checkouts."""
    files = [f'{library}.so' for library in PROGRAMS] + ['giveaways-staging-afresh-v1.idl.json']
    retained = {name: (source / 'target/deploy' / name).read_bytes() for name in files}
    target = source / 'target'
    if target.is_symlink():
        raise ValueError('Scratch build target cannot be a symlink')
    shutil.rmtree(target)
    (target / 'deploy').mkdir(parents=True)
    for name, data in retained.items():
        (target / 'deploy' / name).write_bytes(data)


def artifact_manifest(library, elf, public_commit, private_commit, fingerprints, inputs):
    program = PROGRAMS[library]
    source = {'repository': 'https://github.com/proofofchance/chance-solana-contracts',
              'commit': public_commit, 'privateCommit': private_commit, 'mountRoot': '.',
              'manifest': f'programs/{program}/Cargo.toml', 'library': library,
              'features': ['staging-afresh-v1'], 'buildImage': IMAGE, 'verifierVersion': '0.4.11',
              'rustToolchain': '1.89.0', 'cargoArgs': ['-Znext-lockfile-bump', '--features', 'staging-afresh-v1'],
              'verifierSuppliesLocked': True, 'lockSha256': fingerprints[f'programs/{program}/Cargo.lock'],
              'sourceInputsSha256': digest(canonical(fingerprints).encode()),
              'releaseInputs': {'path': INPUT_PATH, 'text': inputs.decode('ascii'), 'sha256': digest(inputs)}}
    return {'schema': 2, 'library': library, 'source': source,
            'sourceReferenceSha256': digest(canonical(source).encode()),
            'executableSha256': digest(elf.rstrip(b'\0')), 'elfFileSha256': digest(elf),
            'reproduction': {'kind': 'local_private_public_reproduction', 'independentTrees': 2,
                             'exactElfBytesEqual': True, 'publicCiRun': None},
            'deployment': None, 'status': 'reproduced build only; no deployed match or security approval'}


def reproduce(private_source, public_source, input_file, output, *, verifier='solana-verify', build=build_overlay):
    private_source, public_source = private_source.resolve(), public_source.resolve()
    if private_source == public_source:
        raise ValueError('Independent private and public checkouts are required')
    if output.exists():
        raise ValueError('Fresh build output already exists; retain it and choose a new epoch output')
    input_bytes = input_file.read_bytes()
    identities = parse_inputs(input_bytes)
    private_fingerprints = source_inputs(private_source)
    public_fingerprints = source_inputs(public_source)
    if private_fingerprints != public_fingerprints:
        raise ValueError('Canonical private and public build inputs differ; promote and qualify source first')
    private_commit, public_commit = (git(source, 'rev-parse', 'HEAD') for source in (private_source, public_source))
    if not all(re.fullmatch('[a-f0-9]{40}', commit) for commit in (private_commit, public_commit)):
        raise ValueError('Exact source commit references are required')
    # Refuse a private tree masquerading as the independent public source checkout.
    public_remote = git(public_source, 'remote', 'get-url', 'origin')
    if public_remote not in ('git@github.com:proofofchance/chance-solana-contracts.git',
                             'https://github.com/proofofchance/chance-solana-contracts.git',
                             'https://github.com/proofofchance/chance-solana-contracts'):
        raise ValueError('The independent source must use the canonical public repository')
    version = subprocess.check_output([verifier, '--version'], text=True).strip()
    if version != 'solana-verify 0.4.11':
        raise ValueError('solana-verify 0.4.11 is required')
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.afresh-reproduce-', dir=output.parent) as temporary:
        temporary = Path(temporary)
        first, second, result = temporary / 'private', temporary / 'public', temporary / 'result'
        source_snapshot(private_source, first, input_bytes)
        source_snapshot(public_source, second, input_bytes)
        build(first, verifier)
        retain_only_build_outputs(first)
        build(second, verifier)
        retain_only_build_outputs(second)
        result.mkdir()
        libraries = {}
        for library in PROGRAMS:
            relative = Path('target/deploy') / f'{library}.so'
            elf = (first / relative).read_bytes()
            if elf[:4] != b'\x7fELF' or elf != (second / relative).read_bytes():
                raise ValueError('Independent source builds do not reproduce exact ELF bytes')
            manifest = artifact_manifest(library, elf, public_commit, private_commit, private_fingerprints, input_bytes)
            (result / f'{library}.so').write_bytes(elf)
            (result / f'{library}.manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
            libraries[library] = {'elf': f'{library}.so', 'manifest': manifest}
        relative_idl = Path('target/deploy/giveaways-staging-afresh-v1.idl.json')
        idl = (first / relative_idl).read_bytes()
        if idl != (second / relative_idl).read_bytes() or json.loads(idl)['address'] != identities['giveaways_program']:
            raise ValueError('Fresh giveaway IDL differs from its compiled program identity')
        (result / relative_idl.name).write_bytes(idl)
        (result / 'staging-afresh-v1.inputs').write_bytes(input_bytes)
        record = {'schema': 2, 'kind': 'staging_afresh_v1_artifacts', 'environment': 'staging',
                  'genesisHash': GENESIS, 'identities': identities, 'inputs': private_fingerprints,
                  'publicCommit': public_commit, 'privateCommit': private_commit,
                  'releaseInputsSha256': digest(input_bytes), 'libraries': libraries,
                  'idl': relative_idl.name, 'idlSha256': digest(idl),
                  'provenance': {'kind': 'local_private_public_reproduction', 'independentTrees': 2,
                                 'exactElfBytesEqual': True, 'publicCiRun': None}}
        (result / 'artifacts.json').write_text(json.dumps(record, indent=2) + '\n')
        # Claim a missing directory exclusively, then publish artifacts.json last.
        # An interrupted publication has no admission record and cannot be used.
        output.mkdir()
        for file in result.iterdir():
            if file.name != 'artifacts.json':
                os.rename(file, output / file.name)
        os.rename(result / 'artifacts.json', output / 'artifacts.json')
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--private-source', required=True, type=Path)
    parser.add_argument('--public-source', required=True, type=Path)
    parser.add_argument('--inputs', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    record = reproduce(args.private_source, args.public_source, args.inputs, args.output.resolve())
    print(json.dumps({'output': str(args.output.resolve()), 'publicCommit': record['publicCommit'],
                      'releaseInputsSha256': record['releaseInputsSha256'], 'deployed': False}))


if __name__ == '__main__':
    main()
