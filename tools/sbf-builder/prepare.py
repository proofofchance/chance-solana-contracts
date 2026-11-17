#!/usr/bin/env python3
"""Prepare a checksum-verified, credential-free offline Docker context."""
import argparse
import hashlib
import json
import os
import shutil
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent
INDEX = 'index.crates.io-1949cf8c6b5b557f'


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def index_path(name):
    if len(name) == 1:
        return name
    if len(name) == 2:
        return '2/' + name
    if len(name) == 3:
        return '3/' + name[0] + '/' + name
    return name[:2] + '/' + name[2:4] + '/' + name


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--cargo-home', type=Path, required=True)
    parser.add_argument('--source', type=Path, required=True, help='contract checkout with the final program lockfiles')
    parser.add_argument('--output', type=Path, required=True, help='new temporary directory outside the checkout')
    args = parser.parse_args()
    source, output = args.source.resolve(), args.output.resolve()
    if output.exists() or output.is_relative_to(source) or output.is_relative_to(ROOT):
        parser.error('Use a new context directory outside the source checkout')
    pins = json.loads((ROOT / 'pins.json').read_text())
    if digest(ROOT / 'Cargo.lock') != pins['toolLockSha256']:
        parser.error('The reviewed tooling lock does not match pins.json')
    assets = []
    for key in ['cargoBuildSbf', 'platformTools']:
        pin = pins[key]
        path = args.assets / pin['file']
        if not path.is_file() or path.is_symlink() or digest(path) != pin['sha256']:
            parser.error('Missing or unverified official asset: ' + pin['file'])
        assets.append((path, Path('inputs') / pin['file']))
    locks = [ROOT / 'Cargo.lock'] + [source / 'programs' / name / 'Cargo.lock' for name in ['registry', 'daily-lottery-v1', 'giveaways-v1']]
    packages = {}
    for lock in locks:
        for package in tomllib.loads(lock.read_text())['package']:
            if 'source' not in package:
                continue
            if package['source'] != 'registry+https://github.com/rust-lang/crates.io-index':
                parser.error('Only the canonical crates.io registry is supported')
            packages[(package['name'], package['version'])] = package['checksum']
    files = list(assets)
    metadata = {}
    for (name, version), expected in sorted(packages.items()):
        filename = f'{name}-{version}.crate'
        archives = args.cargo_home.glob('registry/cache/index.crates.io-*/' + filename)
        archive = next((p for p in archives if p.is_file() and not p.is_symlink() and digest(p) == expected), None)
        if archive is None:
            parser.error('Fetch the locked canonical archive first: ' + filename)
        files.append((archive, Path('cargo-home/registry/cache') / INDEX / filename))
        rel = index_path(name)
        for path in args.cargo_home.glob('registry/index/index.crates.io-*/.cache/' + rel):
            entries = [json.loads(item) for item in path.read_bytes().split(b'\0') if item.startswith(b'{')]
            if any(item['vers'] == version and item['cksum'] == expected for item in entries):
                metadata.setdefault(name, {})[version] = next(item for item in entries if item['vers'] == version and item['cksum'] == expected)
                break
        else:
            parser.error('Fetch exact official registry metadata first: ' + filename)
    # Validate every input before creating a context. No credentials/config/src
    # trees are copied from CARGO_HOME; Cargo freshly checks/unpacks each archive.
    for filename in ['Dockerfile', 'Cargo.lock', 'pins.json']:
        files.append((ROOT / filename, Path(filename)))
    output.mkdir(parents=True)
    for path, relative in files:
        target = output / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
    for name, versions in metadata.items():
        target = output / 'cargo-home/registry/index' / INDEX / '.cache' / index_path(name)
        target.parent.mkdir(parents=True, exist_ok=True)
        data = b'\x03\x02\0\0\0etag: "verified-locked-canonical-records"\0'
        for version, entry in sorted(versions.items()):
            data += version.encode() + b'\0' + json.dumps(entry, separators=(',', ':')).encode() + b'\0'
        target.write_bytes(data)
    config = output / 'cargo-home/registry/index' / INDEX / 'config.json' 
    config.write_text(json.dumps({'dl': 'https://static.crates.io/crates', 'api': 'https://crates.io'}) + '\n')
    for path in sorted(output.rglob('*'), reverse=True):
        os.utime(path, (0, 0), follow_symlinks=False)
    os.utime(output, (0, 0))
    print(json.dumps({'context': str(output), 'registryPackages': len(packages), 'credentialsCopied': False, 'qualification': 'prepared inputs only'}))


if __name__ == '__main__':
    main()
