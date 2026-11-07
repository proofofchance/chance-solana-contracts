#!/usr/bin/env python3
"""Promote the allowlisted private source into a checkout of the public repository.

`--check` lists the files that differ and exits 1 when any do. Without it, the
differing files are copied into, or deleted from, the public checkout's working
tree. Committing and pushing the public repository happens separately, as
Plexer44.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

PRIVATE = Path(__file__).resolve().parents[1]
PUBLIC_REMOTES = (
    'git@github.com:proofofchance/chance-solana-contracts.git',
    'https://github.com/proofofchance/chance-solana-contracts.git',
    'https://github.com/proofofchance/chance-solana-contracts',
)
FILES = ('Cargo.lock', 'rust-toolchain.toml')
ROOTS = ('programs/', 'crates/', '.cargo/', 'audit/', 'tools/')


def git(repo, *args):
    return subprocess.check_output(['git', '-C', str(repo), *args], text=True)


def tracked(repo):
    names = git(repo, 'ls-files', '-z').split('\0')
    return {n for n in names if n in FILES or n.startswith(ROOTS)}


def same(a, b):
    if a.is_symlink() or b.is_symlink():
        return a.is_symlink() and b.is_symlink() and os.readlink(a) == os.readlink(b)
    return b.is_file() and a.read_bytes() == b.read_bytes()


def differing(private, public):
    mine, theirs = tracked(private), tracked(public)
    return sorted(n for n in mine | theirs if n not in mine or not same(private / n, public / n))


def promote(private, public):
    if git(public, 'remote', 'get-url', 'origin').strip() not in PUBLIC_REMOTES:
        raise ValueError('The public checkout must track proofofchance/chance-solana-contracts')
    if git(public, 'status', '--porcelain'):
        raise ValueError('The public checkout must be clean before promotion')
    if git(private, 'status', '--porcelain', '--', *FILES, *ROOTS):
        raise ValueError('Commit the private source before promoting it')
    mine = tracked(private)
    changed = differing(private, public)
    for name in changed:
        source, target = private / name, public / name
        if target.is_symlink() or target.exists():
            target.unlink()
        if name not in mine:
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_symlink():
            target.symlink_to(os.readlink(source))
        else:
            shutil.copy2(source, target)
    return changed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--public', required=True, type=Path, help='checkout of the public repository')
    parser.add_argument('--check', action='store_true', help='only list the differing files')
    args = parser.parse_args()
    public = args.public.resolve()
    changed = differing(PRIVATE, public) if args.check else promote(PRIVATE, public)
    print(json.dumps({'changed': changed, 'copied': not args.check}))
    if args.check and changed:
        sys.exit(1)


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as e:
        print(e, file=sys.stderr)
        sys.exit(1)
