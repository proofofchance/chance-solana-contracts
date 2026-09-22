#!/usr/bin/env python3
"""Record an explicitly reproduced program binary and its exact public source inputs.

This command does not build, deploy, freeze authority or register a verification claim.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT=Path(__file__).resolve().parents[1]
IMAGE='solanafoundation/solana-verifiable-build@sha256:ff3b148fb6adc3025c46ac38f132f473ccbdc4391f253234d98aa6519aec07f8'
PROGRAMS={'daily_lottery':'daily-lottery-v1','giveaways':'giveaways-v1','chance_registry':'registry'}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository',required=True,help='Canonical public GitHub repository URL')
    parser.add_argument('--commit',required=True,help='Exact checked-out public source commit')
    parser.add_argument('--library',required=True,choices=PROGRAMS)
    parser.add_argument('--elf',required=True,type=Path)
    parser.add_argument('--output',required=True,type=Path)
    args=parser.parse_args()
    if not re.fullmatch(r'https://github.com/proofofchance/[A-Za-z0-9_.-]+',args.repository):
        parser.error('Expected the public Proof of Chance repository URL')
    if not re.fullmatch(r'[0-9a-f]{40}',args.commit):
        parser.error('A full commit SHA is required')
    head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
    if head!=args.commit:
        parser.error('The checked-out commit differs from --commit')
    subprocess.run(['git','diff','--exit-code','HEAD','--','programs','crates','.cargo','rust-toolchain.toml','Cargo.lock'],cwd=ROOT,check=True,capture_output=True)
    untracked=subprocess.check_output(['git','ls-files','--others','--exclude-standard','--','programs','crates','.cargo','rust-toolchain.toml','Cargo.lock'],cwd=ROOT,text=True)
    if untracked.strip():
        parser.error('Untracked build inputs must be committed or removed')
    program=PROGRAMS[args.library]
    lock=f'programs/{program}/Cargo.lock'
    source={'repository':args.repository,'commit':args.commit,'mountRoot':'.','manifest':f'programs/{program}/Cargo.toml',
            'library':args.library,'features':[],'buildImage':IMAGE,'verifierVersion':'0.4.11',
            'cargoArgs':['-Znext-lockfile-bump'],'verifierSuppliesLocked':True,'lockSha256':hashlib.sha256((ROOT/lock).read_bytes()).hexdigest()}
    manifest={'schema':1,'library':args.library,'source':source,
              'sourceReferenceSha256':hashlib.sha256(json.dumps(source,sort_keys=True,separators=(',',':')).encode()).hexdigest(),
              'executableSha256':hashlib.sha256(args.elf.read_bytes().rstrip(b'\0')).hexdigest(),
              'elfFileSha256':hashlib.sha256(args.elf.read_bytes()).hexdigest(),
              'deployment':None,'status':'build record only; no deployed match or security approval'}
    args.output.write_text(json.dumps(manifest,indent=2)+'\n')
    print(json.dumps({'output':str(args.output),'sourceReferenceSha256':manifest['sourceReferenceSha256']}))


if __name__=='__main__':
    main()
