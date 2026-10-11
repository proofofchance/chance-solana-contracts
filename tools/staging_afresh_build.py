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
import stat
import subprocess
import sys
import tarfile
import tempfile

IMAGE = 'solanafoundation/solana-verifiable-build@sha256:ff3b148fb6adc3025c46ac38f132f473ccbdc4391f253234d98aa6519aec07f8'
GENESIS = 'EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG'
INPUT_PATH = 'releases/staging-afresh-v1.inputs'
KEYS = ('schema', 'environment', 'genesis_hash', 'registry_program',
        'daily_lottery_program', 'giveaways_program', 'initialization_authority')
PROGRAMS = {'chance_registry': 'registry', 'daily_lottery': 'daily-lottery-v1', 'giveaways': 'giveaways-v1'}
ROOTS = ('programs', 'crates', '.cargo', 'Cargo.lock', 'rust-toolchain.toml', 'tools/idl-export')
MAX_TARGET_ENTRIES = 200000


def _identity(metadata):
    return metadata.st_dev, metadata.st_ino


class _Scratch:
    """Mint paths internally; ownership repair never accepts a checkout path."""
    def __init__(self, parent):
        self.path = Path(tempfile.mkdtemp(prefix='.afresh-reproduce-', dir=parent)).resolve()
        self.descriptor = os.open(self.path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        self.overlays = []

    def __enter__(self):
        return self

    def check(self):
        metadata = self.path.lstat()
        if (_identity(metadata) != _identity(os.fstat(self.descriptor))
                or not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid()
                or metadata.st_mode & 0o077):
            raise ValueError('Owned build scratch identity changed')

    def bind(self, name):
        self.check()
        if name not in ('private', 'public') or any(overlay.path.name == name for overlay in self.overlays):
            raise ValueError('Only the two minted source overlays can be bound')
        overlay = _Overlay(self, name)
        self.overlays.append(overlay)
        return overlay

    def __exit__(self, exception_type, exception, traceback):
        try:
            self.check()
            for overlay in self.overlays:
                overlay.reclaim_target()
            shutil.rmtree(self.path)
        except Exception:
            if exception is None:
                raise
            # Preserve the actual verifier/IDL failure. Unsafe scratch remains
            # unadmitted for inspection rather than broadening a cleanup path.
            print('Scratch cleanup failed; original build failure preserved.', file=sys.stderr)
        finally:
            for overlay in self.overlays:
                os.close(overlay.descriptor)
            os.close(self.descriptor)
        return False


class _Overlay:
    def __init__(self, scratch, name):
        self.scratch, self.path = scratch, scratch.path / name
        self.descriptor = os.open(self.path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)

    def check(self):
        self.scratch.check()
        metadata = self.path.lstat()
        if (_identity(metadata) != _identity(os.fstat(self.descriptor))
                or not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid()):
            raise ValueError('Minted source overlay identity changed')

    @staticmethod
    def _inventory(descriptor):
        rows = []
        def visit(directory):
            rows.append(os.fstat(directory))
            if len(rows) > MAX_TARGET_ENTRIES:
                raise ValueError('Scratch target inventory exceeds its bound')
            with os.scandir(directory) as entries:
                for entry in entries:
                    metadata = entry.stat(follow_symlinks=False)
                    if stat.S_ISDIR(metadata.st_mode):
                        child = os.open(entry.name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=directory)
                        try:
                            if _identity(os.fstat(child)) != _identity(metadata):
                                raise ValueError('Scratch target entry changed')
                            visit(child)
                        finally:
                            os.close(child)
                    else:
                        rows.append(metadata)
                    if len(rows) > MAX_TARGET_ENTRIES:
                        raise ValueError('Scratch target inventory exceeds its bound')
        visit(descriptor)
        return [(stat.S_IFMT(row.st_mode), row.st_dev, row.st_ino, row.st_nlink, row.st_uid, row.st_gid) for row in rows]

    @staticmethod
    def _validate_inventory(rows, root_identity):
        if not rows or len(rows) > MAX_TARGET_ENTRIES:
            raise ValueError('Scratch target inventory exceeds its bound')
        if rows[0][0] != stat.S_IFDIR or rows[0][1:3] != root_identity:
            raise ValueError('Scratch target inventory root differs from its pinned directory')
        counts = {}
        for kind, device, inode, links, uid, gid in rows:
            if device != root_identity[0]:
                raise ValueError('Scratch target crosses a filesystem boundary')
            if kind not in (stat.S_IFDIR, stat.S_IFREG, stat.S_IFLNK):
                raise ValueError('Scratch target contains a special file')
            # chown -h changes the symlink inode itself. An externally linked
            # symlink therefore has the same ownership escape as a regular file.
            if kind in (stat.S_IFREG, stat.S_IFLNK):
                key = device, inode
                count, previous_links = counts.get(key, (0, links))
                if links != previous_links:
                    raise ValueError('Scratch hardlink inventory changed')
                counts[key] = count + 1, links
        if any(count != links for count, links in counts.values()):
            raise ValueError('Scratch target contains a hardlink outside its admitted tree')
        return any(uid != os.getuid() or gid != os.getgid() for _, _, _, _, uid, gid in rows)

    def _docker(self, descriptor, *, inspect=False):
        # Both commands are fixed executables/arguments. Only a minted target is
        # mounted; no checkout, credential, Docker socket or parent is exposed.
        self.check()
        target = self.path / 'target'
        metadata = target.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or _identity(metadata) != _identity(os.fstat(descriptor)):
            raise ValueError('Scratch build target identity changed')
        if ',' in str(target) or any(ord(character) < 32 for character in str(target)):
            raise ValueError('Scratch mount path contains unsupported separators')
        command = ['docker', 'run', '--rm', '--platform', 'linux/amd64', '--user', '0:0',
                   '--network', 'none', '--read-only', '--cap-drop', 'ALL',
                   '--security-opt', 'no-new-privileges']
        capabilities = ['DAC_READ_SEARCH'] if inspect else ['CHOWN', 'DAC_OVERRIDE', 'FOWNER']
        for capability in capabilities:
            command.extend(['--cap-add', capability])
        command.extend(['--mount', f'type=bind,src={target},dst=/owned-target' + (',readonly' if inspect else ''),
                        '--entrypoint', '/usr/bin/find' if inspect else '/usr/bin/chown', IMAGE])
        if inspect:
            command.extend(['-P', '/owned-target', '-printf', r'%y %D %i %n %U %G\0'])
        else:
            uid, gid = os.getuid(), os.getgid()
            if not all(type(value) is int and 0 <= value < 2**32 - 1 for value in (uid, gid)):
                raise ValueError('Invalid numeric scratch owner')
            command.extend(['-R', '-h', '-P', '--', f'{uid}:{gid}', '/owned-target'])
        result = subprocess.run(command, check=True, capture_output=True, timeout=120)
        if not inspect:
            return None
        if len(result.stdout) > 32 * 1024**2:
            raise ValueError('Scratch target inventory exceeds its bound')
        types = {'d': stat.S_IFDIR, 'f': stat.S_IFREG, 'l': stat.S_IFLNK}
        rows = []
        for raw in result.stdout.split(b'\0')[:-1]:
            fields = raw.decode('ascii').split(' ')
            if len(fields) != 6 or fields[0] not in types or not all(field.isdecimal() for field in fields[1:]):
                raise ValueError('Malformed no-follow scratch target inventory')
            rows.append((types[fields[0]], *(int(field) for field in fields[1:])))
        if not result.stdout.endswith(b'\0'):
            raise ValueError('Incomplete scratch target inventory')
        return rows

    def reclaim_target(self):
        self.check()
        target = self.path / 'target'
        try:
            metadata = target.lstat()
        except FileNotFoundError:
            return
        if not stat.S_ISDIR(metadata.st_mode):
            raise ValueError('Scratch build target must be a real directory')
        # O_PATH pins even a root-owned mode-0700 directory on Linux. On macOS
        # Docker Desktop bind outputs are mapped to the host owner's identity.
        flags = getattr(os, 'O_PATH', os.O_RDONLY) | os.O_DIRECTORY | os.O_NOFOLLOW
        descriptor = os.open('target', flags, dir_fd=self.descriptor)
        try:
            if _identity(os.fstat(descriptor)) != _identity(metadata):
                raise ValueError('Scratch build target identity changed')
            try:
                readable = os.open('target', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=self.descriptor)
                try:
                    if _identity(os.fstat(readable)) != _identity(os.fstat(descriptor)):
                        raise ValueError('Scratch build target identity changed')
                    rows = self._inventory(readable)
                finally:
                    os.close(readable)
            except PermissionError:
                rows = self._docker(descriptor, inspect=True)
                if not rows or rows[0][0] != stat.S_IFDIR or rows[0][1:3] != _identity(os.fstat(descriptor)):
                    raise ValueError('Mounted scratch target identity differs')
            if self._validate_inventory(rows, _identity(os.fstat(descriptor))):
                self.check()
                if _identity(target.lstat()) != _identity(os.fstat(descriptor)):
                    raise ValueError('Scratch build target identity changed')
                self._docker(descriptor)
                self.check()
                if _identity(target.lstat()) != _identity(os.fstat(descriptor)):
                    raise ValueError('Scratch build target identity changed')
                readable = os.open('target', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=self.descriptor)
                try:
                    if _identity(os.fstat(readable)) != _identity(os.fstat(descriptor)):
                        raise ValueError('Scratch build target identity changed')
                    if self._validate_inventory(self._inventory(readable), _identity(os.fstat(descriptor))):
                        raise ValueError('Scratch target ownership repair did not complete')
                finally:
                    os.close(readable)
        finally:
            os.close(descriptor)


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


def build_overlay(source, verifier='solana-verify', *, overlay):
    if source != overlay.path:
        raise ValueError('Builder requires its minted source overlay')
    overlay.check()
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(('CARGO_', 'RUST_', 'ANCHOR_', 'CHANCE_'))
                   and key not in ('RUSTFLAGS', 'RUSTUP_TOOLCHAIN')}
    environment['DOCKER_DEFAULT_PLATFORM'] = 'linux/amd64'
    for library in PROGRAMS:
        try:
            subprocess.run([verifier, 'build', '--library-name', library, '--base-image', IMAGE,
                            str(source), '--', '-Znext-lockfile-bump', '--features', 'staging-afresh-v1'],
                           cwd=source, env=environment, check=True)
        except BaseException:
            try:
                overlay.reclaim_target()
            except Exception:
                print('Scratch ownership repair failed; original verifier failure preserved.', file=sys.stderr)
            raise
        else:
            overlay.reclaim_target()
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


def reproduce(private_source, public_source, input_file, output, *, verifier='solana-verify', build=None):
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
    with tempfile.TemporaryDirectory(prefix='.afresh-artifacts-', dir=output.parent) as publication:
        result = Path(publication)
        with _Scratch(output.parent) as scratch:
            first, second = scratch.path / 'private', scratch.path / 'public'
            source_snapshot(private_source, first, input_bytes)
            private_overlay = scratch.bind('private')
            source_snapshot(public_source, second, input_bytes)
            public_overlay = scratch.bind('public')
            for source, overlay in [(first, private_overlay), (second, public_overlay)]:
                if build is None:
                    build_overlay(source, verifier, overlay=overlay)
                else:
                    build(source, verifier)
                overlay.check()
                retain_only_build_outputs(source)
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
        # Scratch cleanup must succeed before the admission record is published.
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
