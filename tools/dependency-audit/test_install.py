"""Security boundary tests; all fixtures remain inside temporary directories."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

import install

PREFIX = 'cargo-audit-0.23.0'


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def archive(self, members):
        path = self.root / 'fixture.crate'
        with tarfile.open(path, 'w:gz') as bundle:
            for name, kind, payload in members:
                info = tarfile.TarInfo(name)
                if kind == 'file':
                    data = payload.encode()
                    info.size = len(data)
                    bundle.addfile(info, io.BytesIO(data))
                else:
                    info.type = kind
                    info.linkname = payload
                    bundle.addfile(info)
        return path

    def fixture_recipe(self):
        recipe = self.root / 'recipe'
        recipe.mkdir()
        shutil.copyfile(install.RECIPE / 'install.py', recipe / 'install.py')
        reviewed = b'version = 4\n'
        original = b'version = 3\n'
        (recipe / 'Cargo.lock').write_bytes(reviewed)
        archive = self.archive([
            (PREFIX + '/Cargo.toml', 'file', '[package]\nname="cargo-audit"\nversion="0.23.0"\n'),
            (PREFIX + '/Cargo.lock', 'file', original.decode()),
        ])
        pins = json.loads((install.RECIPE / 'pins.json').read_text())
        pins.update(archive_sha256=install.sha256(archive),
                    upstream_lock_sha256=hashlib.sha256(original).hexdigest(),
                    reviewed_lock_sha256=hashlib.sha256(reviewed).hexdigest())
        (recipe / 'pins.json').write_text(json.dumps(pins))
        (self.root / 'db').mkdir()
        args = argparse.Namespace(work_dir=str(self.root / 'work'),
            install_root=str(self.root / 'installed'), cargo_home=None, target_dir=None,
            cargo='cargo', archive=str(archive), advisory_db=str(self.root / 'db'), offline=True)
        return recipe, archive, args

    def test_plain_files_extract(self):
        archive = self.archive([(PREFIX + '/src/main.rs', 'file', 'fn main() {}')])
        extracted = install.safe_extract(archive, self.root / 'out', PREFIX)
        self.assertEqual((extracted / 'src/main.rs').read_text(), 'fn main() {}')

    def test_traversal_rejected_before_any_extraction(self):
        for name in ['../escape', '/absolute', PREFIX + '/../../escape', PREFIX + '/x/../escape',
                     PREFIX + '\\escape', PREFIX + '//escape']:
            with self.subTest(name=name):
                archive = self.archive([(PREFIX + '/safe', 'file', 'safe'), (name, 'file', 'bad')])
                with self.assertRaises(install.InstallError):
                    install.safe_extract(archive, self.root / 'out', PREFIX)
                self.assertFalse((self.root / 'out').exists())

    def test_links_and_special_files_rejected(self):
        for kind in [tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE, tarfile.CHRTYPE]:
            with self.subTest(kind=kind):
                archive = self.archive([(PREFIX + '/bad', kind, '../../escape')])
                with self.assertRaises(install.InstallError):
                    install.safe_extract(archive, self.root / 'out', PREFIX)
                self.assertFalse((self.root / 'out').exists())

    def test_duplicate_members_rejected(self):
        archive = self.archive([(PREFIX + '/same', 'file', 'a'), (PREFIX + '/same', 'file', 'b')])
        with self.assertRaises(install.InstallError):
            install.safe_extract(archive, self.root / 'out', PREFIX)
        self.assertFalse((self.root / 'out').exists())

    def test_existing_destination_is_preserved(self):
        archive = self.archive([(PREFIX + '/file', 'file', 'new')])
        destination = self.root / 'out'
        destination.mkdir()
        (destination / 'keep').write_text('existing')
        with self.assertRaises(install.InstallError):
            install.safe_extract(archive, destination, PREFIX)
        self.assertEqual(list(destination.iterdir()), [destination / 'keep'])

    def test_tampered_lock_fails_before_network_or_tools(self):
        recipe, archive, args = self.fixture_recipe()
        (recipe / 'Cargo.lock').write_text('tampered')
        with mock.patch.object(install.urllib.request, 'urlopen') as network, \
             mock.patch.object(install.subprocess, 'check_output') as tool:
            with self.assertRaisesRegex(install.InstallError, 'Reviewed Cargo.lock checksum'):
                install.install(args, recipe)
            network.assert_not_called()
            tool.assert_not_called()
        self.assertFalse(Path(args.work_dir).exists())
        self.assertFalse(Path(args.install_root).exists())

    def test_tampered_archive_fails_before_extraction_or_tools(self):
        recipe, archive, args = self.fixture_recipe()
        archive.write_bytes(archive.read_bytes() + b'tampered')
        with mock.patch.object(install.subprocess, 'check_output') as tool:
            with self.assertRaisesRegex(install.InstallError, 'archive checksum'):
                install.install(args, recipe)
            tool.assert_not_called()
        self.assertFalse(Path(args.work_dir).exists())

    def test_offline_mode_never_downloads_missing_inputs(self):
        recipe, archive, args = self.fixture_recipe()
        args.archive = None
        with mock.patch.object(install.urllib.request, 'urlopen') as network:
            with self.assertRaisesRegex(install.InstallError, 'Offline qualification requires'):
                install.install(args, recipe)
            network.assert_not_called()
        self.assertFalse(Path(args.work_dir).exists())

    def test_disk_floor_stops_only_spawned_build(self):
        processes = []
        original = subprocess.Popen
        def start(*args, **kwargs):
            process = original(*args, **kwargs)
            processes.append(process)
            return process
        with mock.patch.object(install, 'disk_floor', side_effect=[None, install.InstallError('floor')]), \
             mock.patch.object(install.subprocess, 'Popen', side_effect=start):
            with self.assertRaisesRegex(install.InstallError, 'floor'):
                install.run_build([sys.executable, '-c', 'import time; time.sleep(120)'],
                                  self.root, dict(os.environ), self.root / 'build.log')
        self.assertEqual(len(processes), 1)
        self.assertIsNotNone(processes[0].poll())

    def fake_build(self, command, cwd, env, log):
        target = Path(env['CARGO_TARGET_DIR']) / 'release/cargo-audit'
        target.parent.mkdir(parents=True)
        target.write_bytes(b'fixture binary')
        log.write_text('mocked build')

    def test_nonzero_self_audit_never_installs(self):
        recipe, archive, args = self.fixture_recipe()
        for findings in [dict(vulnerabilities={'count': 1}, warnings={}, settings={'ignore': []}),
                         dict(vulnerabilities={'count': 0}, warnings={'unsound': [{}]}, settings={'ignore': []}),
                         dict(vulnerabilities={'count': 0}, warnings={}, settings={'ignore': ['RUSTSEC-TEST']})]:
            with self.subTest(findings=findings):
                if Path(args.work_dir).exists():
                    shutil.rmtree(args.work_dir)
                audit = subprocess.CompletedProcess([], 0, json.dumps(findings), '')
                with mock.patch.object(install, 'run_build', side_effect=self.fake_build), \
                     mock.patch.object(install.subprocess, 'check_output', side_effect=['cargo 1.100.0 (fixture)', 'cargo-audit 0.23.0']), \
                     mock.patch.object(install.subprocess, 'run', return_value=audit), \
                     mock.patch.object(install, 'disk_floor'):
                    with self.assertRaisesRegex(install.InstallError, 'self-audit is not clean'):
                        install.install(args, recipe)
                self.assertFalse(Path(args.install_root).exists())


if __name__ == '__main__':
    unittest.main()
