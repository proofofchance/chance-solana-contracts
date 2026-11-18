from datetime import UTC, datetime
import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

import scan as watch
import subprocess


NOW = datetime(2026, 11, 18, 9, tzinfo=UTC)


class DependencyWatchTests(unittest.TestCase):
    def test_cooling_major_leaves_mature_patch_visible(self):
        pin = watch.Pin('npm', 'example', '1.4.0', 'app')
        releases = [watch.Release('2.0.0', '2026-11-15T09:00:00Z'),
                    watch.Release('1.4.1', '2026-11-14T09:00:00Z'),
                    watch.Release('3.0.0-beta.1', '2026-10-01T09:00:00Z')]
        result = watch.classify(pin, releases, NOW)
        self.assertEqual(result['latest_stable']['version'], '2.0.0')
        self.assertEqual(result['latest_stable']['state'], 'cooling')
        self.assertEqual(result['eligible']['version'], '1.4.1')

    def test_followup_patch_does_not_restart_major_hold(self):
        pin = watch.Pin('cargo', 'example', '1.0.0', 'app')
        releases = [watch.Release('2.0.0', '2026-11-01T09:00:00Z'),
                    watch.Release('2.0.1', '2026-11-15T09:00:00Z')]
        result = watch.classify(pin, releases, NOW)
        self.assertEqual(result['eligible']['version'], '2.0.1')
        self.assertEqual(result['eligible']['eligible_at'], NOW.isoformat())

    def test_fresh_minor_on_new_major_retains_minor_hold(self):
        pin = watch.Pin('npm', 'example', '1.5.0', 'app')
        releases = [watch.Release('2.0.0', '2026-10-01T09:00:00Z'),
                    watch.Release('2.3.0', '2026-11-14T09:00:00Z')]
        result = watch.classify(pin, releases, NOW)
        self.assertEqual(result['latest_stable']['eligible_at'], '2026-11-21T09:00:00+00:00')
        self.assertEqual(result['eligible']['version'], '2.0.0')

    def test_advisory_batch_is_bound_to_exact_public_versions(self):
        pins = [watch.Pin('npm', 'example', '1.5.0', 'app/package.json'),
                watch.Pin('npm', 'example', '1.5.0', 'another/file')]
        response = b'{"results":[{"vulns":[{"id":"TEST-123","modified":"2026-11-14T09:00:00Z"}]}]}'
        with patch.object(watch.Registry, 'http', return_value=response) as fetch:
            count, findings = watch.advisories(pins)
            self.assertEqual(count, 1)
            self.assertEqual(findings[0]['id'], 'TEST-123')
            self.assertEqual(fetch.call_args.args[1], {'queries': [
                {'package': {'ecosystem': 'npm', 'name': 'example'}, 'version': '1.5.0'}
            ]})
        with patch.object(watch.Registry, 'http', return_value=b'{"results":[]}'):
            with self.assertRaises(ValueError):
                watch.advisories(pins)

    def test_unknown_age_requires_review(self):
        pin = watch.Pin('cargo', 'example', '0.2.0', 'app')
        result = watch.classify(pin, [watch.Release('0.3.0', None)], NOW)
        self.assertEqual(result['latest_stable']['type'], 'major')
        self.assertEqual(result['state'], 'verify-publish-date')
        self.assertIsNone(result['eligible'])

    def test_current_moved_action_tag_reports_sha_even_if_newer_exists(self):
        pin = watch.Pin('github-tag', 'owner/action', 'v1.0.0', 'workflow', 'a' * 40)
        result = watch.classify(pin, [watch.Release('v1.0.0', None, 'b' * 40),
                                     watch.Release('v2.0.0', None, 'c' * 40)], NOW)
        self.assertEqual(result['state'], 'pin-mismatch')
        self.assertEqual(result['expected_commit'], 'b' * 40)

    def test_registry_caches_metadata_and_never_substitutes_stale_for_failure(self):
        with TemporaryDirectory() as tmp:
            r = watch.Registry(Path(tmp), NOW)
            self.assertEqual(r.cached('npm', 'example', lambda: ['1.2.3']), ['1.2.3'])
            r = watch.Registry(Path(tmp), NOW.replace(hour=10))
            def fail():
                raise OSError('network failed')
            self.assertEqual(r.cached('npm', 'example', fail), ['1.2.3'])
            r = watch.Registry(Path(tmp), NOW.replace(hour=22))
            with self.assertRaises(OSError):
                r.cached('npm', 'example', fail)

    def test_cargo_direct_lock_edge_does_not_report_transitives_or_another_version(self):
        lock = '''
[[package]]
name = "app"
version = "0.1.0"
dependencies = ["dep 1.1.0"]
[[package]]
name = "dep"
version = "1.1.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
dependencies = ["old"]
[[package]]
name = "dep"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
[[package]]
name = "old"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
'''
        self.assertEqual(list(watch.cargo_pins(lock, 'app')), [watch.Pin('cargo', 'dep', '1.1.0', 'app')])

    def test_cargo_registry_ignores_yanked_and_prerelease_versions(self):
        rows = [
            {'vers': '1.0.0', 'pubtime': '2026-11-10T12:00:00Z', 'yanked': False},
            {'vers': '1.1.0', 'pubtime': '2026-11-11T12:00:00Z', 'yanked': True},
            {'vers': '1.2.0-rc.1', 'pubtime': '2026-11-13T12:00:00Z', 'yanked': False},
        ]
        body = '\n'.join(json.dumps(r) for r in rows).encode()
        with patch.object(watch.Registry, 'http', return_value=body):
            result = watch.Registry(Path('/unused'), NOW).lookup('cargo', 'example', set())
        self.assertEqual([r['version'] for r in result], ['1.0.0'])

    def test_npm_uses_the_same_latest_channel_with_mixed_installed_versions(self):
        packument = {'time': {'1.0.0': '2026-11-01T09:00:00Z',
                              '2.0.0': '2026-11-05T09:00:00Z'},
                      'versions': {'1.0.0': {}, '2.0.0': {}}}
        r = watch.Registry(Path('/unused'), NOW)
        with patch.object(watch.Registry, 'http', side_effect=[b'{"version":"1.0.0"}']):
            current = r.lookup('npm', 'example', {'1.0.0'})
        with patch.object(watch.Registry, 'http', side_effect=[b'{"version":"1.0.0"}',
                                                              json.dumps(packument).encode()]):
            mixed = r.lookup('npm', 'example', {'0.9.0', '1.0.0'})
        self.assertEqual([row['version'] for row in current], ['1.0.0'])
        self.assertEqual([row['version'] for row in mixed], ['1.0.0'])

    def test_missing_major_origin_is_reported_unverified(self):
        pin = watch.Pin('github', 'owner/tool', '1.0.0', 'workflow')
        result = watch.classify(pin, [watch.Release('2.1.2', '2026-09-01T09:00:00Z')], NOW)
        self.assertEqual(result['state'], 'verify-release-origin')
        self.assertIsNone(result['eligible'])

    def test_malformed_npm_manifest_and_excluded_secret_directory(self):
        with self.assertRaises(ValueError):
            list(watch.npm_pins('{"dependencies":[]}', 'package.json'))
        self.assertFalse(watch.selected_file('.secrets/example/package.json'))

    def test_reads_only_the_named_commit_and_reports_source_bound_pin_mismatch(self):
        with TemporaryDirectory() as tmp:
            repo = Path(tmp)
            def git(*args):
                return subprocess.check_output(['git', '-C', tmp, *args], stderr=subprocess.DEVNULL).decode().strip()
            git('init')
            (repo / 'scripts').mkdir()
            (repo / 'package.json').write_text('{"dependencies":{"wrangler":"4.164.0"}}')
            (repo / 'scripts/guard.mjs').write_text("""
export function qualifyWranglerVersion(manifestVersion, installedVersion) {
  check(manifestVersion === '4.128.0' && installedVersion === manifestVersion, 'unqualified_wrangler')
}
""")
            git('add', '.')
            git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-m', 'Fixture')
            head = git('rev-parse', 'HEAD')
            (repo / 'package.json').write_text('{"dependencies":{"wrangler":"4.128.0"}}')
            pins = watch.inventory(repo, head)
            self.assertIn(watch.Pin('npm', 'wrangler', '4.164.0', 'package.json'), pins)
            self.assertIn(watch.Pin('npm-guard', 'wrangler', '4.128.0', 'scripts/guard.mjs'), pins)
            with patch.object(watch.Registry, 'metadata', return_value=[
                {'version': '4.164.0', 'published': None}
            ]), patch.object(watch, 'advisories', return_value=(1, [])):
                report = watch.evaluate(pins, {'fixture': head}, repo / 'report.json', NOW)
            self.assertEqual(report['updates'][0]['state'], 'tool-pin-mismatch')
            self.assertEqual(report['updates'][0]['manifest_version'], '4.164.0')
            with self.assertRaises(ValueError):
                watch.inventory(repo, 'HEAD')

    def test_detects_stale_publisher_pin_separately_from_npm_manifest(self):
        manifest = '{"devDependencies": {"wrangler": "4.164.0"}}'
        script = '''
  const directory = resolve("wrangler/package.json")
  check(read(directory).version === '4.128.0', 'unqualified_wrangler')
'''
        pins = list(watch.npm_pins(manifest, 'package.json'))
        pins += list(watch.file_pins('scripts/guard.mjs', script, 'publisher'))
        self.assertEqual([(p.package, p.version) for p in pins],
                         [('wrangler', '4.164.0'), ('wrangler', '4.128.0')])

    def test_discovers_release_pins_and_actions_without_local_fixture_dependencies(self):
        body = '''
      - uses: actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa # v7.0.1
      - uses: ./.github/actions/setup
      - uses: owner/action@main
      - run: curl https://github.com/nats-io/nats-server/releases/download/v2.15.3/active.tar.gz
      - run: curl https://github.com/nats-io/nats-server/releases/download/v2.15.2/predecessor.tar.gz
'''
        pins = list(watch.file_pins('.github/workflows/ci.yml', body, 'ci.yml'))
        self.assertEqual({p.version for p in pins}, {'v7.0.1', 'moving:main', 'v2.15.3', 'v2.15.2'})
        self.assertFalse(watch.selected_file('crates/vendor/dependency/Cargo.lock'))


if __name__ == '__main__':
    unittest.main()
