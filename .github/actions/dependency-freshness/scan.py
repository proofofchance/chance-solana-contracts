#!/usr/bin/env python3
"""Check committed dependency pins against stable releases and public advisories."""
from __future__ import annotations

import argparse
from configparser import ConfigParser, Error as ConfigError
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
import gzip
import hashlib
from html import escape
from http.client import HTTPException
import json
import os
from pathlib import Path, PurePosixPath
import re
import ssl
import subprocess
import tempfile
import tomllib
from urllib.parse import quote
from urllib.request import Request, urlopen


BUFFER = {'major': 14, 'minor': 7, 'patch': 3, 'commit': 14}
SEMVER = re.compile(r'^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:\+[\w.-]+)?$')
OBJECT_ID = re.compile(r'^[0-9a-f]{40}$')
NPM_NAME = re.compile(r'^(?:@[a-z0-9._-]+/)?[a-z0-9._-]+$')
REPO_NAME = re.compile(r'^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$')
ARCHIVE_ONLY = {'vendor', 'fixtures', 'imported', 'snapshots', 'node_modules', '.claude',
                '.secrets', '.local'}
NPM_SECTIONS = ('dependencies', 'devDependencies', 'optionalDependencies')
OSV_ECOSYSTEMS = {'npm': 'npm', 'cargo': 'crates.io', 'pypi': 'PyPI'}
CACHE_TTL = timedelta(hours=12)


def semver(value):
    match = SEMVER.fullmatch(value) if isinstance(value, str) else None
    return tuple(map(int, match.groups())) if match else None


def timestamp(value):
    try:
        parsed = datetime.fromisoformat(value)
        return parsed.astimezone(UTC) if parsed.tzinfo else None
    except (TypeError, ValueError):
        return None


@dataclass(frozen=True, order=True)
class Pin:
    provider: str
    package: str
    version: str
    location: str
    commit: str = ''


@dataclass(frozen=True)
class Release:
    version: str
    published: str | None
    commit: str = ''


def classify(pin, releases, now, buffers=BUFFER):
    current = semver(pin.version)
    stable = [r for r in releases if semver(r.version) is not None]
    if not stable:
        raise ValueError('no stable release in source metadata')
    stable.sort(key=lambda release: semver(release.version), reverse=True)
    if current is None and not pin.commit:
        raise ValueError('unrecognized current version')
    if current is None and pin.commit:
        newest = stable[0]
        if newest.commit == pin.commit:
            return None
        choices = [(newest, 'commit')]
    else:
        choices = []
        for release in stable:
            version = semver(release.version)
            if version > current:
                bump = 'major' if version[0] != current[0] else 'minor' if version[1] != current[1] else 'patch'
                # A minor bump on a 0.x API can be breaking.
                if current[0] == 0 and bump == 'minor':
                    bump = 'major'
                choices.append((release, bump))
        if pin.commit:
            same = next((r for r in stable if semver(r.version) == current and r.commit), None)
            if same and same.commit and same.commit != pin.commit:
                return {'state': 'pin-mismatch', 'version': same.version, 'expected_commit': same.commit}

    def candidate(release, bump):
        date = timestamp(release.published)
        if not date:
            return {'version': release.version, 'type': bump, 'state': 'verify-publish-date'}
        version = semver(release.version)
        eligible = date + timedelta(days=buffers['patch' if bump != 'commit' else 'commit'])

        origins = []
        if bump == 'major':
            start = (version[0], 0, 0) if version[0] else (0, version[1], 0)
            origins.append((start, buffers['major']))
        if bump == 'minor' or (bump == 'major' and version[0] and version[1]):
            origins.append(((*version[:2], 0), buffers['minor']))
        for baseline, days in origins:
            origin = next((timestamp(r.published) for r in stable if semver(r.version) == baseline), None)
            if origin is None:
                return {'version': release.version, 'type': bump, 'state': 'verify-release-origin',
                        'published': date.isoformat()}
            eligible = max(eligible, origin + timedelta(days=days))
        return {'version': release.version, 'type': bump, 'published': date.isoformat(),
                'eligible_at': eligible.isoformat(), 'state': 'eligible' if eligible <= now else 'cooling'}

    if not choices:
        return None
    candidates = [candidate(release, bump) for release, bump in choices]
    # Show the newest eligible release as well as the newest published stable one.
    # A fresh major never hides a mature patch on the previous supported line.
    return {'state': candidates[0]['state'], 'latest_stable': candidates[0],
            'eligible': next((c for c in candidates if c['state'] == 'eligible'), None)}


def command(args, repository=None, timeout=40):
    result = subprocess.run(args, cwd=repository, stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, timeout=timeout,
                            env={**os.environ, 'GIT_TERMINAL_PROMPT': '0', 'GH_PROMPT_DISABLED': '1'})
    if result.returncode:
        raise RuntimeError(f'{args[0]} {args[1]} failed')
    return result.stdout


def selected_file(name):
    path = PurePosixPath(name)
    if set(path.parts) & ARCHIVE_ONLY:
        return False
    return (
        path.name in {'package.json', 'Cargo.lock', '.nvmrc', '.python-version', 'rust-toolchain.toml', 'Dockerfile'}
        or path.name in {'requirements.lock', 'requirements.txt', 'foundry.toml'}
        or name.startswith(('.github/workflows/', '.github/actions/')) and path.suffix in {'.yml', '.yaml'}
        or name.startswith('scripts/') and path.suffix == '.mjs'
        or name in {'tools/tla/check.sh', 'tools/dependency-audit/pins.json',
                    'platform/delivery/haproxy/build_haproxy_v1.py',
                    'platform/delivery/haproxy/build_haproxy_tls_v1.py'}
    )


def npm_pins(data, location):
    parsed = json.loads(data)
    if not isinstance(parsed, dict):
        raise ValueError('npm manifest must be an object')
    for field in NPM_SECTIONS:
        dependencies = parsed.get(field, {})
        if not isinstance(dependencies, dict):
            raise ValueError(f'{field} must be an object')
        for name, value in dependencies.items():
            if isinstance(value, str) and value.startswith('npm:'):
                name, _, value = value.removeprefix('npm:').rpartition('@')
            if NPM_NAME.fullmatch(name) and semver(value):
                yield Pin('npm', name, value, location)
    manager = parsed.get('packageManager', '')
    if not isinstance(manager, str):
        raise ValueError('packageManager must be a string')
    name, at, version = manager.rpartition('@')
    if at and name in {'npm', 'pnpm', 'yarn'} and semver(version):
        yield Pin('npm', name, version.split('+')[0], location)


def cargo_pins(data, location):
    lock = tomllib.loads(data)['package']
    registry = defaultdict(list)
    for package in lock:
        if package.get('source') == 'registry+https://github.com/rust-lang/crates.io-index':
            registry[package['name']].append(package['version'])
    for package in lock:
        if package.get('source'):
            continue
        for dependency in package.get('dependencies', []):
            parts = dependency.split()
            name = parts[0]
            if not NPM_NAME.fullmatch(name) or '/' in name:
                continue
            versions = registry[name]
            # Cargo omits the version when the package name is unambiguous.
            for version in versions if len(parts) == 1 else [v for v in versions if v == parts[1]]:
                if semver(version):
                    yield Pin('cargo', name, version, location)


def file_pins(name, data, location):
    short = PurePosixPath(name).name
    if short == 'package.json':
        yield from npm_pins(data, location)
    elif short == 'Cargo.lock':
        yield from cargo_pins(data, location)
    elif short in {'requirements.lock', 'requirements.txt'}:
        for package, version in re.findall(r'(?m)^([\w.-]+)==([0-9][\w.+-]+)', data):
            if semver(version):
                yield Pin('pypi', package, version, location)
    elif short in {'.nvmrc', '.python-version', 'rust-toolchain.toml'}:
        if short == 'rust-toolchain.toml':
            version = tomllib.loads(data).get('toolchain', {}).get('channel', '')
        else:
            version = data.strip()
        provider = {'.nvmrc': 'node', '.python-version': 'python', 'rust-toolchain.toml': 'github'}[short]
        package = {'.nvmrc': 'node', '.python-version': 'python', 'rust-toolchain.toml': 'rust-lang/rust'}[short]
        if semver(version):
            yield Pin(provider, package, version, location)
    elif short == 'foundry.toml':
        version = tomllib.loads(data).get('profile', {}).get('default', {}).get('solc_version')
        if semver(version):
            yield Pin('github', 'ethereum/solidity', version, location)
    elif name.startswith('scripts/') and short.endswith('.mjs'):
        # Tool guards can pin a different version than the installed manifest.
        patterns = (
            r"wrangler[\s\S]{0,300}?\.version\s*===\s*['\x22]([0-9.]+)['\x22]",
            r"qualifyWranglerVersion\([^)]*\)\s*\{[\s\S]{0,300}?manifestVersion\s*===\s*['\x22]([0-9.]+)['\x22]",
            r"(?im)^\s*(?:export\s+)?const\s+\w*wrangler\w*\s*=\s*['\x22]([0-9.]+)['\x22]",
        )
        for pattern in patterns:
            for version in re.findall(pattern, data):
                if semver(version):
                    yield Pin('npm-guard', 'wrangler', version, location)
    elif short == 'Dockerfile':
        for image, digest in re.findall(
            r'(?m)^FROM[ \t]+(?:--platform=[^ \t]+[ \t]+)?([^ \t]+)@sha256:([0-9a-f]{64})', data
        ):
            yield Pin('image', image, 'sha256:' + digest, location)
    elif short == 'pins.json' and name == 'tools/dependency-audit/pins.json':
        parsed = json.loads(data)
        if parsed.get('name') == 'cargo-audit' and semver(parsed.get('version')):
            yield Pin('cargo', 'cargo-audit', parsed['version'], location)
    elif name == 'tools/tla/check.sh':
        for version in re.findall(r'(?m)^VERSION=(\d+\.\d+\.\d+)$', data):
            yield Pin('github', 'tlaplus/tlaplus', version, location)
    elif 'build_haproxy' in short:
        for version in re.findall(r'(?m)^VERSION = "(\d+\.\d+\.\d+)"', data):
            yield Pin('haproxy', 'haproxy', version, location)
    elif name.startswith(('.github/workflows/', '.github/actions/')):
        for spec, tag in re.findall(r'(?m)^\s*(?:-\s*)?uses:\s*([^\s#]+)(?:\s+#\s*(v[\d.]+))?', data):
            reference, at, pin = spec.partition('@')
            parts = reference.split('/')
            package = '/'.join(parts[:2])
            if not at or not REPO_NAME.fullmatch(package):
                continue
            if len(parts) > 2 and OBJECT_ID.fullmatch(pin) and not tag:
                yield Pin('opaque-git', reference, '', location, pin)
            elif OBJECT_ID.fullmatch(pin):
                yield Pin('github-tag', package, tag, location, pin)
            elif semver(pin):
                yield Pin('github-tag', package, pin, location)
            else:
                yield Pin('github-tag', package, 'moving:' + pin, location)
        for tool, version in re.findall(r'(?m)^\s*(python|node)-version:\s*[\x27"]?v?([\d.]+)', data):
            if semver(version):
                yield Pin(tool, tool, version, location)
        downloads = defaultdict(set)
        for package, release in re.findall(r'https://github\.com/([\w.-]+/[\w.-]+)/releases/download/(v[0-9.]+)/', data):
            if semver(release):
                downloads[package].add(release)
        for package, versions in downloads.items():
            for release in sorted(versions):
                yield Pin('github', package, release, location)
        for image in re.findall(r'(?:docker://)?([a-z0-9.-]+/[a-z0-9/._-]+)@sha256:([0-9a-f]{64})', data):
            yield Pin('image', image[0], 'sha256:' + image[1], location)
        if 'foundry-rs/foundry-toolchain@' in data:
            for version in re.findall(r'(?m)^\s*version:\s*(v[\d.]+)\s*$', data):
                if semver(version):
                    yield Pin('github', 'foundry-rs/foundry', version, location)


def inventory(repository, revision):
    if not OBJECT_ID.fullmatch(revision):
        raise ValueError('expected a full commit SHA')
    commit = command(['git', 'rev-parse', '--verify', f'{revision}^{{commit}}'], repository).decode().strip()
    if commit != revision:
        raise ValueError('revision is not the expected commit')
    tree = command(['git', 'ls-tree', '-rz', revision], repository).split(b'\0')
    entries = {}
    for entry in filter(None, tree):
        info, path = entry.decode().split('\t', 1)
        mode, _, oid = info.split()
        entries[path] = mode, oid

    submodules = {}
    if '.gitmodules' in entries and entries['.gitmodules'][0] == '100644':
        definition = command(['git', 'cat-file', 'blob', entries['.gitmodules'][1]], repository).decode()
        config = ConfigParser(interpolation=None)
        config.read_string(definition)
        for section in config.sections():
            if not section.startswith('submodule '):
                continue
            slug = re.fullmatch(r'https://github\.com/([\w.-]+/[\w.-]+?)(?:\.git)?', config[section]['url'])
            if slug:
                submodules[config[section]['path']] = slug[1]

    pins = set()
    for path, (mode, oid) in entries.items():
        if mode == '160000':
            if path in submodules:
                pins.add(Pin('github-tag', submodules[path], '', path, oid))
            else:
                pins.add(Pin('opaque-git', path, '', path, oid))
        elif mode in {'100644', '100755'} and selected_file(path):
            blob = command(['git', 'cat-file', 'blob', oid], repository).decode()
            pins.update(file_pins(path, blob, path))
    return pins


def atomic_json(path, data):
    path.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False) as stream:
        temp = Path(stream.name)
        json.dump(data, stream, sort_keys=True, indent=2)
        stream.write('\n')
    temp.replace(path)


class Registry:
    def __init__(self, cache, now):
        self.cache = cache
        self.now = now

    def cached(self, provider, name, load):
        key = hashlib.sha256(f'v2:{provider}:{name}'.encode()).hexdigest()
        path = self.cache / f'{key}.json'
        try:
            data = json.loads(path.read_text())
            saved = timestamp(data['fetched_at'])
            if saved and timedelta(0) <= self.now - saved < CACHE_TTL:
                return data['payload']
        except (OSError, ValueError, KeyError, TypeError):
            pass
        value = load()
        if not value:
            raise ValueError('upstream returned no usable release metadata')
        atomic_json(path, {'fetched_at': self.now.isoformat(), 'payload': value})
        return value

    @staticmethod
    def http(url, payload=None):
        headers = {'User-Agent': 'dependency-freshness-check', 'Accept-Encoding': 'gzip'}
        if payload is not None:
            headers['Content-Type'] = 'application/json'
        req = Request(url, data=json.dumps(payload).encode() if payload is not None else None,
                      headers=headers)
        ca = Path('/etc/ssl/cert.pem')
        context = ssl.create_default_context(cafile=str(ca) if ca.is_file() else None)
        limit = 64 * 1024 * 1024
        with urlopen(req, context=context, timeout=35) as response:
            if response.headers.get('Content-Encoding') == 'gzip':
                with gzip.GzipFile(fileobj=response) as stream:
                    data = stream.read(limit + 1)
            else:
                data = response.read(limit + 1)
        if len(data) > limit:
            raise ValueError('registry metadata exceeds size limit')
        return data

    @staticmethod
    def github(endpoint):
        return json.loads(command(['gh', 'api', endpoint], timeout=40))

    def metadata(self, provider, name, versions):
        key = f'{name}:{",".join(sorted(versions))}' if provider in {'npm', 'github-tag', 'haproxy'} else name
        cache_family = {'github-tag': 'github-tag-with-current-ref', 'npm': 'npm-latest-channel'}.get(provider, provider)
        return self.cached(cache_family, key, lambda: self.lookup(provider, name, versions))

    def lookup(self, provider, name, pins):
        def rows(values):
            return [r.__dict__ for r in values if semver(r.version)]

        if provider == 'npm':
            encoded = quote(name, safe='')
            url = f'https://registry.npmjs.org/{encoded}'
            latest = json.loads(self.http(url + '/latest'))['version']
            recommended = semver(latest)
            if recommended is None:
                raise ValueError('npm latest tag is not stable')
            if all(semver(pin) >= recommended for pin in pins):
                return rows([Release(latest, None)])
            doc = json.loads(self.http(url))
            return rows(Release(v, published) for v, published in doc['time'].items()
                        if v in doc['versions'] and semver(v) and semver(v) <= recommended
                        and not doc['versions'][v].get('deprecated'))
        if provider == 'cargo':
            suffix = name if len(name) == 1 else f'2/{name}' if len(name) == 2 else (
                f'3/{name[0]}/{name}' if len(name) == 3 else f'{name[:2]}/{name[2:4]}/{name}')
            data = self.http(f'https://index.crates.io/{suffix}')
            records = [json.loads(line) for line in data.splitlines()]
            return rows(Release(r['vers'], r.get('pubtime')) for r in records if not r['yanked'])
        if provider == 'pypi':
            doc = json.loads(self.http(f'https://pypi.org/pypi/{quote(name, safe="")}/json'))
            return rows(Release(v, min(x['upload_time_iso_8601'] for x in files if not x.get('yanked')))
                        for v, files in doc['releases'].items() if any(not x.get('yanked') for x in files))
        if provider == 'node':
            data = json.loads(self.http('https://nodejs.org/dist/index.json'))
            return rows(Release(r['version'], r['date'] + 'T00:00:00Z') for r in data)
        if provider == 'python':
            data = self.http('https://www.python.org/downloads/').decode()
            # The official release page includes prerelease rows. Use only exact stable labels.
            from html import unescape
            found = re.findall(r'Python (\d+\.\d+\.\d+)</a>\s*</span>\s*<span class="release-date">([^<]+)', data)
            dates = []
            for version, date in found:
                s = unescape(date).strip().replace('.', '').replace('Sept ', 'Sep ')
                fmt = '%b %d, %Y' if len(s.split()[0]) == 3 else '%B %d, %Y'
                dates.append(Release(version, datetime.strptime(s, fmt).replace(tzinfo=UTC).isoformat()))
            return rows(dates)
        if provider == 'haproxy':
            root = 'https://www.haproxy.org/download/'
            index = self.http(root).decode()
            available = {tuple(map(int, (major, minor)))
                         for major, minor in re.findall(r'href="(\d+)\.(\d+)/"', index)}
            if not available:
                raise ValueError('no HAProxy release lines in official index')
            pinned = {semver(version)[:2] for version in pins}
            channels = pinned | set(sorted(available, reverse=True)[:3])
            found = []
            for major, minor in sorted(channels):
                data = self.http(f'{root}{major}.{minor}/src/').decode()
                found.extend(re.findall(
                    r'haproxy-(\d+\.\d+\.\d+)\.tar\.gz</a>\s+(\d{2}-[A-Za-z]{3}-\d{4} \d{2}:\d{2})', data
                ))
            return rows(Release(version, datetime.strptime(date, '%d-%b-%Y %H:%M').replace(tzinfo=UTC).isoformat())
                        for version, date in found)
        releases = self.github(f'repos/{name}/releases?per_page=30')
        stable = [r for r in releases if not r['draft'] and not r['prerelease'] and semver(r['tag_name'])]
        stable.sort(key=lambda r: semver(r['tag_name']), reverse=True)
        def resolve_tag(tag):
            obj = self.github(f'repos/{name}/git/ref/tags/{quote(tag, safe="")}')['object']
            if obj['type'] == 'tag':
                obj = self.github(f'repos/{name}/git/tags/{obj["sha"]}')['object']
            if obj['type'] != 'commit' or not OBJECT_ID.fullmatch(obj['sha']):
                raise ValueError('release tag does not resolve to a commit')
            return obj['sha']

        result = []
        for release in stable:
            tag = release['tag_name']
            verify = provider == 'github-tag' and (release is stable[0] or tag in pins)
            result.append(Release(tag, release['published_at'], resolve_tag(tag) if verify else ''))
        if provider == 'github-tag':
            known = {r.version for r in result}
            # A release list might no longer contain the current pinned tag.
            for tag in pins - known:
                if semver(tag):
                    result.append(Release(tag, None, resolve_tag(tag)))
        return rows(result)


def advisories(pins):
    # Send only public package coordinates. Repo names, source locations and
    # credentials stay local. This query reads advisory data without installing.
    coords = sorted({(OSV_ECOSYSTEMS[p.provider], p.package, p.version)
                     for p in pins if p.provider in OSV_ECOSYSTEMS and semver(p.version)})
    findings = []
    for start in range(0, len(coords), 75):
        batch = coords[start:start + 75]
        queries = [{'package': {'ecosystem': ecosystem, 'name': name}, 'version': version}
                   for ecosystem, name, version in batch]
        document = json.loads(Registry.http('https://api.osv.dev/v1/querybatch', {'queries': queries}))
        results = document['results']
        if len(results) != len(queries) or any(r.get('next_page_token') for r in results):
            raise ValueError('incomplete advisory response')
        for (ecosystem, name, version), result in zip(batch, results, strict=True):
            for entry in result.get('vulns', []):
                identifier = entry['id']
                if not re.fullmatch(r'[A-Za-z0-9_.-]+', identifier):
                    raise ValueError('malformed advisory identifier')
                findings.append({'ecosystem': ecosystem, 'package': name, 'version': version,
                                 'id': identifier, 'modified': entry['modified'],
                                 'url': f'https://osv.dev/vulnerability/{identifier}'})
    return len(coords), findings


def evaluate(pins, sources, output, as_of):
    grouped = defaultdict(list)
    upgrades, manual, public_packages = [], [], []
    root_versions = {
        pin.package: pin.version for pin in pins
        if pin.provider == 'npm' and pin.location == 'package.json'
    }
    for pin in sorted(pins):
        if pin.version.startswith('moving:'):
            upgrades.append({'pin': pin.__dict__, 'state': 'unpinned-ref'})
        elif pin.provider in {'image', 'opaque-git'}:
            manual.append(pin.__dict__)
        elif pin.provider == 'npm-guard':
            expected = root_versions.get(pin.package)
            if expected != pin.version:
                upgrades.append({'pin': pin.__dict__, 'state': 'tool-pin-mismatch',
                                 'manifest_version': expected})
        else:
            grouped[pin.provider, pin.package].append(pin)

    registry = Registry(output.parent / 'metadata', as_of)
    releases_by_package, errors = {}, []
    with ThreadPoolExecutor(max_workers=6) as pool:
        lookups = {key: pool.submit(registry.metadata, *key, {p.version for p in members})
                   for key, members in grouped.items()}
        for key, job in lookups.items():
            try:
                releases = [Release(**r) for r in job.result()]
                releases_by_package[':'.join(key)] = [r.__dict__ for r in releases]
                for pin in grouped[key]:
                    if pin.provider in OSV_ECOSYSTEMS:
                        public_packages.append(pin)
                    if result := classify(pin, releases, as_of):
                        upgrades.append({'pin': pin.__dict__, **result})
            except (OSError, EOFError, HTTPException, KeyError, TypeError, ValueError, RuntimeError,
                    subprocess.TimeoutExpired) as error:
                errors.append(f'{key[0]}/{key[1]}: {type(error).__name__}')
    advisory_count, advisory_findings = 0, []
    try:
        advisory_count, advisory_findings = advisories(public_packages)
    except (OSError, EOFError, HTTPException, KeyError, TypeError, ValueError) as error:
        errors.append(f'OSV public advisory lookup: {type(error).__name__}')
    return {
        'schema': 1, 'observed_at': as_of.isoformat(), 'minimum_age_days': BUFFER,
        'sources': sources, 'pins': [pin.__dict__ for pin in sorted(pins)],
        'release_evidence': releases_by_package, 'updates': upgrades, 'manual_imports': manual,
        'public_package_advisories': advisory_findings,
        'advisory_coordinates_checked': advisory_count, 'incomplete': errors,
    }


def summary(report):
    updates = [r for r in report['updates'] if r.get('eligible') or r['state'] in {
        'pin-mismatch', 'tool-pin-mismatch', 'unpinned-ref'
    }]
    lines = [f"{len(report['pins'])} pins checked, {len(updates)} actionable locations, "
             f"{len(report['public_package_advisories'])} direct advisory findings, "
             f"{len(report['incomplete'])} incomplete lookups.",
             f"{len(report['manual_imports'])} imports require manual source review."]
    for row in updates[:30]:
        pin = row['pin']
        target = row.get('eligible', {}).get('version') if row.get('eligible') else row['state']
        lines.append(f"{pin['location']}: {pin['package']} {pin['version'] or pin['commit']} → {target}")
    for advisory in report['public_package_advisories'][:20]:
        lines.append(f"{advisory['package']} {advisory['version']}: {advisory['url']}")
    lines.extend(report['incomplete'])
    lines.append('See the JSON report for all findings, source pins, and release evidence.')
    return '\n'.join(lines)


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument('--repository', type=Path, required=True)
    cli.add_argument('--revision', required=True)
    cli.add_argument('--label', required=True)
    cli.add_argument('--output', type=Path, required=True)
    args = cli.parse_args()
    now = datetime.now(UTC)
    try:
        pins = inventory(args.repository.resolve(strict=True), args.revision)
        report = evaluate(pins, {args.label: args.revision}, args.output, now)
    except (RuntimeError, OSError, ValueError, KeyError, TypeError, ConfigError, subprocess.TimeoutExpired) as error:
        report = {'schema': 1, 'observed_at': now.isoformat(), 'sources': {args.label: args.revision},
                  'pins': [], 'updates': [], 'manual_imports': [], 'public_package_advisories': [],
                  'incomplete': [f'source inventory: {type(error).__name__}']}
    atomic_json(args.output, report)
    message = summary(report)
    print(message)
    if filename := os.environ.get('GITHUB_STEP_SUMMARY'):
        with Path(filename).open('a') as stream:
            stream.write('## Dependency freshness\n\n<pre>' + escape(message) + '</pre>\n')
    return 2 if report['incomplete'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
