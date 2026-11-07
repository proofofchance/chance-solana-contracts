#!/usr/bin/env python3
"""Build and execute the explicit V1 brand-review schema, without deployment."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
INPUTS = ('programs', 'crates', '.cargo', 'Cargo.lock', 'rust-toolchain.toml')
OUTPUT = ROOT / 'target/development/giveaways-brand-review-v1'


def fingerprint(root=ROOT):
    paths = subprocess.check_output(
        ['git', '-C', str(root), 'ls-files', '-z', '--cached', '--others',
         '--exclude-standard', '--', *INPUTS]).decode().split('\0')
    return {p: hashlib.sha256((root / p).read_bytes()).hexdigest()
            for p in sorted(set(paths) - {''})}


def verify(root=ROOT, output=OUTPUT):
    record = json.loads((output / 'development-build.json').read_text())
    elf = output / 'giveaways.so'
    if (record.get('schema') != 1 or record.get('features') != ['brand-review']
            or record.get('inputs') != fingerprint(root)
            or record.get('elfFileSha256') != hashlib.sha256(elf.read_bytes()).hexdigest()):
        raise ValueError('Development ELF/source binding changed; rebuild before runtime testing')
    return elf


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['build', 'runtime', 'verify'])
    args = parser.parse_args()
    if args.action == 'build':
        before = fingerprint()
        OUTPUT.mkdir(parents=True, exist_ok=True)
        manifest = OUTPUT / 'development-build.json'
        manifest.unlink(missing_ok=True)
        subprocess.run(['cargo', 'build-sbf', '--manifest-path',
                        'programs/giveaways-v1/Cargo.toml', '--sbf-out-dir', str(OUTPUT),
                        '--', '--locked', '--features', 'brand-review'], cwd=ROOT, check=True)
        if before != fingerprint():
            raise ValueError('Source changed during the development build')
        record = {'schema': 1, 'features': ['brand-review'], 'inputs': before,
                  'status': 'native development only; no release or deployment approval',
                  'elfFileSha256': hashlib.sha256((OUTPUT / 'giveaways.so').read_bytes()).hexdigest()}
        manifest.write_text(json.dumps(record, sort_keys=True, indent=2) + '\n')
    else:
        elf = verify()
        if args.action == 'runtime':
            subprocess.run(['cargo', 'test', '--locked', '--manifest-path',
                            'programs/registry/Cargo.toml', '--test', 'giveaway_brand_review',
                            '--', '--ignored'], cwd=ROOT, check=True,
                           env=dict(os.environ, CHANCE_GIVEAWAY_BRAND_REVIEW_ELF=str(elf)))


if __name__ == '__main__':
    main()
