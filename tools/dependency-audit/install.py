#!/usr/bin/env python3
"""Build the reviewed cargo-audit source without modifying a global installation."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import signal
import ssl
import subprocess
import tarfile
import time
import tomllib
import urllib.request

FLOOR_BYTES = 4 * 1024**3
MAX_ARCHIVE_BYTES = 2 * 1024**2
RECIPE = Path(__file__).resolve().parent


class InstallError(RuntimeError):
    pass


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024**2), b''):
            digest.update(chunk)
    return digest.hexdigest()


def reviewed_inputs(recipe: Path) -> dict:
    pins = json.loads((recipe / 'pins.json').read_text())
    if pins.get('schema') != 1 or pins.get('name') != 'cargo-audit':
        raise InstallError('Unexpected recipe schema or tool')
    if pins.get('version') != '0.23.0' or pins.get('rust') != '1.100.0':
        raise InstallError('Unexpected reviewed toolchain version')
    expected_url = 'https://static.crates.io/crates/cargo-audit/cargo-audit-0.23.0.crate'
    if pins.get('url') != expected_url:
        raise InstallError('Source must be the exact official cargo-audit archive')
    for field in ['archive_sha256', 'upstream_lock_sha256', 'reviewed_lock_sha256']:
        if not re.fullmatch('[a-f0-9]{64}', pins.get(field, '')):
            raise InstallError('Invalid SHA-256 field: ' + field)
    if sha256(recipe / 'Cargo.lock') != pins['reviewed_lock_sha256']:
        raise InstallError('Reviewed Cargo.lock checksum mismatch')
    return pins


def verified_archive(path: Path, pins: dict) -> bytes:
    if path.stat().st_size > MAX_ARCHIVE_BYTES:
        raise InstallError('Source archive exceeds the size limit')
    payload = path.read_bytes()
    if len(payload) > MAX_ARCHIVE_BYTES or hashlib.sha256(payload).hexdigest() != pins['archive_sha256']:
        raise InstallError('Official source archive checksum mismatch')
    return payload


def safe_extract(archive: Path, destination: Path, prefix: str) -> Path:
    """Validate the complete archive before writing; admit only unique plain files/dirs."""
    with gzip.open(archive, 'rb') as compressed:
        payload = compressed.read(16 * 1024**2 + 1)
    if len(payload) > 16 * 1024**2:
        raise InstallError('Expanded source exceeds the size limit')
    with tarfile.open(fileobj=io.BytesIO(payload), mode='r:') as bundle:
        members = bundle.getmembers()
        if len(members) > 4096:
            raise InstallError('Source archive has too many members')
        seen = set()
        total = 0
        for member in members:
            name = member.name.rstrip('/')
            parts = name.split('/')
            if (not name or '\\' in name or PurePosixPath(name).is_absolute()
                    or any(part in ['', '.', '..'] for part in parts)
                    or parts[0] != prefix or name in seen
                    or not (member.isfile() or member.isdir())):
                raise InstallError('Unsafe or duplicate source archive member: ' + member.name)
            seen.add(name)
            total += member.size
            if total > 16 * 1024**2:
                raise InstallError('Expanded source exceeds the size limit')
        if destination.exists():
            raise InstallError('Extraction destination must not exist')
        destination.mkdir(mode=0o700)
        for member in members:
            target = destination.joinpath(*member.name.rstrip('/').split('/'))
            if member.isdir():
                target.mkdir(mode=0o700, parents=True, exist_ok=True)
            else:
                target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                source = bundle.extractfile(member)
                if source is None:
                    raise InstallError('Missing archive file payload')
                with source, target.open('xb') as output:
                    shutil.copyfileobj(source, output)
                target.chmod(0o600 | (member.mode & 0o111))
    return destination / prefix


def disk_floor(path: Path) -> None:
    if shutil.disk_usage(path).free < FLOOR_BYTES:
        raise InstallError('At least 4 GiB free space is required; no further build writes admitted')


def run_build(command: list[str], cwd: Path, env: dict, log: Path) -> None:
    disk_floor(cwd)
    start = time.monotonic()
    with log.open('w') as output:
        process = subprocess.Popen(command, cwd=cwd, env=env, stdout=output,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        try:
            while process.poll() is None:
                disk_floor(cwd)
                if time.monotonic() - start > 1200:
                    raise InstallError('Isolated build exceeded 20 minutes')
                time.sleep(1)
        except BaseException:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=10)
            raise
    if process.returncode:
        raise InstallError('Locked build failed; inspect ' + str(log))


def advisory_fingerprint(database: Path) -> dict:
    digest = hashlib.sha256()
    files = sorted([*database.glob('crates/**/*.md'), *database.glob('rust/**/*.md')])
    if not files:
        raise InstallError('Self-audit did not leave an inspectable advisory database')
    for path in files:
        digest.update(str(path.relative_to(database)).encode() + b'\0' + bytes.fromhex(sha256(path)))
    return {'path': str(database), 'advisory_files': len(files),
            'advisory_files_sha256': digest.hexdigest()}


def install(args: argparse.Namespace, recipe: Path = RECIPE) -> Path:
    # Reject a changed reviewed input before downloads, directory creation or tool execution.
    pins = reviewed_inputs(recipe)
    archive = Path(args.archive).resolve() if args.archive else None
    if args.offline and (archive is None or not args.advisory_db):
        raise InstallError('Offline qualification requires --archive and --advisory-db')
    if args.advisory_db and not Path(args.advisory_db).is_dir():
        raise InstallError('Explicit advisory database directory does not exist')
    payload = verified_archive(archive, pins) if archive else None
    work = Path(args.work_dir).resolve()
    install_root = Path(args.install_root).resolve()
    if work.exists() or install_root.exists() or work == install_root:
        raise InstallError('Work and install roots must be distinct new paths')
    if work.is_relative_to(install_root) or install_root.is_relative_to(work):
        raise InstallError('Work and install roots must not contain one another')
    cargo_home = Path(args.cargo_home).resolve() if args.cargo_home else work / 'cargo-home'
    target = Path(args.target_dir).resolve() if args.target_dir else work / 'target'
    global_cargo = (Path.home() / '.cargo').resolve()
    paths = [work, install_root, cargo_home, target]
    if (any(path == global_cargo or path.is_relative_to(global_cargo) for path in paths)
            or any(path == recipe.resolve() or path.is_relative_to(recipe.resolve()) for path in paths)):
        raise InstallError('Use isolated paths outside the recipe and global Cargo installation')
    work.parent.mkdir(parents=True, exist_ok=True)
    disk_floor(work.parent)
    work.mkdir(mode=0o700)
    if payload is not None:
        archive = work / 'cargo-audit-0.23.0.crate'
        archive.write_bytes(payload)
    else:
        archive = work / 'cargo-audit-0.23.0.crate'
        context = ssl.create_default_context(
            cafile='/etc/ssl/cert.pem' if Path('/etc/ssl/cert.pem').exists() else None)
        request = urllib.request.Request(pins['url'], headers={
            'User-Agent': 'ProofOfChance-reviewed-cargo-audit/1.0'})
        with urllib.request.urlopen(request, timeout=30, context=context) as response:
            payload = response.read(MAX_ARCHIVE_BYTES + 1)
        if len(payload) > MAX_ARCHIVE_BYTES:
            raise InstallError('Source download exceeds the size limit')
        archive.write_bytes(payload)
        verified_archive(archive, pins)
    source = safe_extract(archive, work / 'source', 'cargo-audit-0.23.0')
    package = tomllib.loads((source / 'Cargo.toml').read_text())['package']
    if package['name'] != pins['name'] or package['version'] != pins['version']:
        raise InstallError('Extracted package differs from the reviewed source')
    if sha256(source / 'Cargo.lock') != pins['upstream_lock_sha256']:
        raise InstallError('Unexpected upstream shipped lockfile')
    shutil.copyfile(recipe / 'Cargo.lock', source / 'Cargo.lock')
    env = dict(os.environ, CARGO_HOME=str(cargo_home), CARGO_TARGET_DIR=str(target),
               CARGO_BUILD_JOBS='2', CARGO_REGISTRIES_CRATES_IO_PROTOCOL='sparse')
    for key in ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER',
                'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTC_WRAPPER',
                'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER']:
        env.pop(key, None)
    if args.offline:
        env['CARGO_NET_OFFLINE'] = 'true'
    cargo = args.cargo
    cargo_version = subprocess.check_output(
        [cargo, '+1.100.0', '--version'], cwd=work, env=env, text=True, timeout=30).strip()
    if not cargo_version.startswith('cargo 1.100.0 '):
        raise InstallError('Rust/Cargo 1.100.0 is required')
    command = [cargo, '+1.100.0', 'build', '--release', '--locked',
               '--manifest-path', str(source / 'Cargo.toml'), '--bin', 'cargo-audit']
    if args.offline:
        command.append('--offline')
    run_build(command, work, env, work / 'build.log')
    if sha256(source / 'Cargo.lock') != pins['reviewed_lock_sha256']:
        raise InstallError('Build changed the reviewed lock')
    binary = target / 'release/cargo-audit'
    version = subprocess.check_output(
        [str(binary), '--version'], cwd=work, env=env, text=True, timeout=30).strip()
    if version != 'cargo-audit 0.23.0':
        raise InstallError('Built scanner reports an unexpected version: ' + version)
    audit_command = [str(binary), 'audit', '--file', str(source / 'Cargo.lock'), '--json']
    database = Path(args.advisory_db).resolve() if args.advisory_db else cargo_home / 'advisory-db'
    if args.advisory_db:
        audit_command.extend(['--db', str(database), '--no-fetch', '--stale'])
    elif args.offline:
        raise InstallError('Offline qualification requires --advisory-db')
    audit = subprocess.run(audit_command, cwd=work, env=env, capture_output=True,
                           text=True, timeout=180)
    (work / 'scanner-self-audit.json').write_text(audit.stdout)
    (work / 'scanner-self-audit.stderr').write_text(audit.stderr)
    raw = json.loads(audit.stdout)
    warnings = {kind: len(rows) for kind, rows in raw.get('warnings', {}).items() if rows}
    if (audit.returncode or raw['vulnerabilities']['count'] or warnings
            or raw['settings']['ignore']):
        raise InstallError('Scanner self-audit is not clean; inspect ' + str(work / 'scanner-self-audit.json'))
    disk_floor(work)
    install_root.mkdir(mode=0o700, parents=True)
    (install_root / 'bin').mkdir(mode=0o755)
    installed = install_root / 'bin/cargo-audit'
    shutil.copyfile(binary, installed)
    installed.chmod(0o755)
    result = {'schema': 1, 'tool': version, 'cargo': cargo_version,
              'archive_sha256': sha256(archive),
              'upstream_lock_sha256': pins['upstream_lock_sha256'],
              'reviewed_lock_sha256': sha256(source / 'Cargo.lock'),
              'installer_sha256': sha256(recipe / 'install.py'),
              'pins_sha256': sha256(recipe / 'pins.json'),
              'binary_sha256': sha256(installed), 'binary': str(installed),
              'build_command': command, 'work_dir': str(work),
              'advisory_database': advisory_fingerprint(database),
              'self_audit': {'vulnerabilities': 0, 'warnings': {}, 'ignored': [],
                             'advisory_count': raw['database']['advisory-count']}}
    (install_root / 'qualification.json').write_text(json.dumps(result, indent=2) + '\n')
    print(str(installed), flush=True)
    return installed


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument('--work-dir', required=True, help='New isolated build directory')
    result.add_argument('--install-root', required=True, help='New explicit installation directory')
    result.add_argument('--cargo-home', help='Explicit isolated cache; defaults inside work directory')
    result.add_argument('--target-dir', help='Explicit isolated build target; defaults inside work directory')
    result.add_argument('--cargo', default='cargo', help='Rustup Cargo shim; no default-toolchain changes')
    result.add_argument('--archive', help='Already downloaded official archive; checksum still required')
    result.add_argument('--advisory-db', help='Existing inspected advisory snapshot for offline self-audit')
    result.add_argument('--offline', action='store_true')
    return result


if __name__ == '__main__':
    try:
        install(parser().parse_args())
    except (InstallError, OSError, ValueError, tarfile.TarError,
            subprocess.SubprocessError) as error:
        raise SystemExit('cargo-audit installation denied: ' + str(error)) from error
