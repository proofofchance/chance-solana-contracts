#!/usr/bin/env python3
"""Check the exact user-approved isolated test-runtime advisory baseline."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
LOCK = 'tests/runtime/Cargo.lock'
BASELINE = 'audit/runtime-advisory-baseline.json'
DIRECTORIES = ('tests/runtime', 'programs/daily-lottery-v1/src', 'programs/daily-lottery-v1/tests', 'programs/registry/tests', 'crates/registry-wire/src')
FILES = ('.cargo/config.toml', 'rust-toolchain.toml', 'programs/daily-lottery-v1/Cargo.toml', 'programs/daily-lottery-v1/build.rs', 'crates/registry-wire/Cargo.toml', 'crates/staging-build-input.rs', 'crates/non-production-controls-build.rs', 'tools/runtime_advisory_gate.py', 'tools/test_runtime_advisory_gate.py', 'tools/fixtures/runtime-audit-0.23.json')


class PolicyError(RuntimeError):
    pass


class ScannerError(PolicyError):
    pass


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ScannerError('Duplicate JSON key')
        result[key] = value
    return result


def load(path):
    return json.loads(path.read_text(), object_pairs_hook=unique)


def context(root):
    names = set(FILES)
    for name in DIRECTORIES:
        for path in (root / name).rglob('*'):
            rel = path.relative_to(root)
            if {'target', '__pycache__'} & set(rel.parts):
                continue
            if path.is_file() and path.name != 'Cargo.lock' and path.suffix in {'.rs', '.toml', '.json', '.bin'}:
                names.add(rel.as_posix())
    result = {}
    for name in sorted(names):
        path = root / name
        if not path.is_file() or path.is_symlink() or not path.resolve().is_relative_to(root.resolve()):
            raise PolicyError('Missing or indirect context source: ' + name)
        result[name] = digest(path)
    return result


def findings(report, packages):
    if set(report) != {'database', 'lockfile', 'settings', 'vulnerabilities', 'warnings'}:
        raise ScannerError('Missing or malformed scanner report')
    settings = report['settings']
    if (settings.get('ignore') != [] or settings.get('severity') is not None
            or settings.get('target_arch') != [] or settings.get('target_os') != []
            or sorted(settings.get('informational_warnings', [])) != ['notice', 'unmaintained', 'unsound']):
        raise PolicyError('Ignores and narrowed scanner settings are forbidden')
    if report['database'].get('advisory-count', 0) <= 0 or report['lockfile'].get('dependency-count') != len(packages):
        raise ScannerError('Scanner database/package coverage is incomplete')
    vulnerability = report['vulnerabilities']
    if vulnerability['count'] != len(vulnerability['list']) or vulnerability['found'] != bool(vulnerability['list']):
        raise ScannerError('Inconsistent vulnerability report')
    locked = {(p['name'], p['version']): p for p in packages}
    rows = []

    def add(kind, entry):
        advisory, package = entry['advisory'], entry['package']
        selected = locked.get((package['name'], package['version']))
        if (not selected or package.get('source') != 'registry+https://github.com/rust-lang/crates.io-index'
                or package.get('checksum') != selected.get('checksum') or advisory['package'] != package['name']):
            raise PolicyError('Finding does not match the exact locked package')
        informational = None if kind == 'vulnerability' else kind
        if advisory.get('informational') != informational:
            raise PolicyError('Finding classification changed')
        payload = {key: entry.get(key) for key in ['advisory', 'affected', 'versions']}
        rows.append({'kind': kind, 'advisory': advisory['id'], 'package': package['name'], 'version': package['version'],
                     'categories': sorted(advisory['categories']), 'informational': informational,
                     'advisory_sha256': hashlib.sha256(canonical(payload)).hexdigest()})
    for entry in vulnerability['list']:
        add('vulnerability', entry)
    for kind, entries in report['warnings'].items():
        if kind not in {'unmaintained', 'unsound', 'notice'} or not isinstance(entries, list):
            raise PolicyError('Unreviewed warning category')
        for entry in entries:
            add(kind, entry)
    rows.sort(key=lambda r: (r['kind'], r['advisory'], r['package'], r['version']))
    if len({canonical(row) for row in rows}) != len(rows):
        raise ScannerError('Duplicate findings')
    return rows


def validate(root, lockfile, baseline, report, raw_status):
    expected = root / LOCK
    if lockfile.resolve() != expected.resolve() or lockfile.is_symlink():
        raise PolicyError('Approval is restricted to ' + LOCK)
    if (baseline.get('schema') != 1 or baseline.get('scope') != 'isolated-litesvm-test-runtime'
            or baseline.get('lockfile') != LOCK or baseline.get('approval', {}).get('status') != 'approved'):
        raise PolicyError('No approval for this scope')
    if digest(expected) != baseline['lock_sha256']:
        raise PolicyError('Lock changed; new review required')
    if (baseline['context_directories'] != list(DIRECTORIES) or baseline['context_files'] != list(FILES)
            or context(root) != baseline['context_sha256']):
        raise PolicyError('Source/features/context changed; new review required')
    if raw_status not in {0, 1}:
        raise ScannerError('Scanner error exit ' + str(raw_status))
    try:
        rows = findings(report, tomllib.loads(expected.read_text())['package'])
    except (KeyError, TypeError, AttributeError, ValueError) as error:
        raise ScannerError('Malformed scanner output') from error
    if bool(report['vulnerabilities']['found']) != (raw_status == 1):
        raise ScannerError('Raw scanner status contradicts its findings')
    if rows != baseline['findings']:
        raise PolicyError('New, removed or changed findings; new review required')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lockfile', type=Path, default=Path(LOCK))
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--raw-status', type=int, required=True)
    args = parser.parse_args(argv)
    try:
        lockfile = args.lockfile if args.lockfile.is_absolute() else ROOT / args.lockfile
        if args.raw_status not in {0, 1}:
            raise ScannerError('Scanner error exit ' + str(args.raw_status))
        baseline = load(ROOT / BASELINE)
        try:
            report = load(args.report)
        except (ValueError, UnicodeError, OSError) as error:
            raise ScannerError('Scanner did not produce complete valid JSON') from error
        validate(ROOT, lockfile, baseline, report, args.raw_status)
        print('Matched the approved runtime baseline; raw findings and scanner status remain visible.')
        return 0
    except ScannerError as error:
        print('Scanner error: ' + str(error), file=sys.stderr)
        return 2
    except (PolicyError, KeyError, TypeError, ValueError, OSError) as error:
        print('Runtime baseline rejected: ' + str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
