import importlib.util
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('afresh_build', Path(__file__).with_name('staging_afresh_build.py'))
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


def key(number):
    alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
    value = int.from_bytes(bytes([number]) * 32, 'big')
    result = ''
    while value:
        value, remainder = divmod(value, 58)
        result = alphabet[remainder] + result
    return result


def inputs():
    values = ('1', 'staging', build.GENESIS, key(1), key(2), key(3), key(4))
    return ''.join(f'{name}={value}\n' for name, value in zip(build.KEYS, values)).encode()


class FreshBuildTests(unittest.TestCase):
    def test_exact_staging_schema_rejects_production_and_unknown_overrides(self):
        valid = inputs()
        self.assertEqual(build.parse_inputs(valid)['giveaways_program'], key(3))
        for altered in (valid.replace(b'environment=staging', b'environment=production'),
                        valid.replace(build.GENESIS.encode(), b'5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d'),
                        valid + b'force=true\n', valid.replace(b'\n', b'\r\n'), valid.rstrip(b'\n'),
                        valid.replace(key(3).encode(), key(2).encode()),
                        valid.replace(key(3).encode(), b'11111111111111111111111111111111')):
            with self.subTest(altered=altered), self.assertRaises(ValueError):
                build.parse_inputs(altered)

    def test_manifest_binds_fresh_identities_source_and_local_not_ci_provenance(self):
        fingerprints = {'programs/registry/Cargo.lock': 'ab' * 32}
        manifest = build.artifact_manifest('chance_registry', b'\x7fELFcode\0\0', '11' * 20, '22' * 20, fingerprints, inputs())
        self.assertEqual(manifest['source']['releaseInputs']['sha256'], build.digest(inputs()))
        self.assertEqual(manifest['source']['sourceInputsSha256'], build.digest(build.canonical(fingerprints).encode()))
        self.assertIsNone(manifest['reproduction']['publicCiRun'])
        self.assertEqual(manifest['source']['features'], ['staging-afresh-v1'])
        self.assertNotEqual(manifest['elfFileSha256'], manifest['executableSha256'])
        self.assertEqual(manifest['sourceReferenceSha256'], build.digest(build.canonical(manifest['source']).encode()))
        changed = build.artifact_manifest('chance_registry', b'\x7fELFcode\0\0', '11' * 20, '22' * 20, fingerprints, inputs().replace(key(4).encode(), key(5).encode()))
        self.assertNotEqual(changed['sourceReferenceSha256'], manifest['sourceReferenceSha256'])

    def test_private_public_source_mismatch_fails_before_any_builder(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            input_file = directory / 'inputs'; input_file.write_bytes(inputs())
            with patch.object(build, 'source_inputs', side_effect=[{'a': 'first'}, {'a': 'second'}]):
                with self.assertRaisesRegex(ValueError, 'private and public'):
                    build.reproduce(directory / 'private', directory / 'public', input_file, directory / 'output', build=lambda *_: self.fail('builder called'))
            self.assertFalse((directory / 'output').exists())

    def test_existing_output_is_never_rebuilt_or_replaced(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            output = directory / 'output'; output.mkdir(); (output / 'retained').write_bytes(b'original')
            with self.assertRaisesRegex(ValueError, 'already exists'):
                build.reproduce(directory / 'private', directory / 'public', directory / 'absent-inputs', output)
            self.assertEqual((output / 'retained').read_bytes(), b'original')

    def test_two_independent_artifacts_and_idl_are_compared_before_admission(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            input_file = directory / 'inputs'; input_file.write_bytes(inputs())
            fingerprints = {f'programs/{program}/Cargo.lock': 'ab' * 32 for program in build.PROGRAMS.values()}
            def fake_git(source, *arguments):
                return 'https://github.com/proofofchance/chance-solana-contracts.git' if arguments[0] == 'remote' else ('11' if source.name == 'public' else '22') * 20
            def snapshot(source, destination, public_inputs):
                destination.mkdir(); (destination / 'target/deploy').mkdir(parents=True)
            calls = []
            def fake_builder(source, verifier):
                calls.append(source.name)
                for library in build.PROGRAMS:
                    (source / f'target/deploy/{library}.so').write_bytes(b'\x7fELF' + library.encode())
                (source / 'target/deploy/giveaways-staging-afresh-v1.idl.json').write_text(json.dumps({'address': key(3)}))
            with patch.object(build, 'source_inputs', return_value=fingerprints), patch.object(build, 'git', side_effect=fake_git), patch.object(build, 'source_snapshot', side_effect=snapshot), patch.object(build.subprocess, 'check_output', return_value='solana-verify 0.4.11\n'):
                record = build.reproduce(directory / 'private', directory / 'public', input_file, directory / 'output', build=fake_builder)
            self.assertEqual(calls, ['private', 'public'])
            self.assertEqual(record['provenance']['kind'], 'local_private_public_reproduction')
            self.assertEqual((directory / 'output/staging-afresh-v1.inputs').read_bytes(), inputs())
            self.assertTrue((directory / 'output/artifacts.json').exists())
            def mismatched(source, verifier):
                fake_builder(source, verifier)
                if source.name == 'public':
                    (source / 'target/deploy/daily_lottery.so').write_bytes(b'\x7fELFdifferent')
            with patch.object(build, 'source_inputs', return_value=fingerprints), patch.object(build, 'git', side_effect=fake_git), patch.object(build, 'source_snapshot', side_effect=snapshot), patch.object(build.subprocess, 'check_output', return_value='solana-verify 0.4.11\n'):
                with self.assertRaisesRegex(ValueError, 'exact ELF'):
                    build.reproduce(directory / 'private', directory / 'public', input_file, directory / 'mismatch', build=mismatched)
            self.assertFalse((directory / 'mismatch/artifacts.json').exists())

    def test_cleanup_failure_cannot_publish_an_artifact_admission_record(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            input_file = directory / 'inputs'; input_file.write_bytes(inputs())
            fingerprints = {f'programs/{program}/Cargo.lock': 'ab' * 32 for program in build.PROGRAMS.values()}
            def fake_git(source, *arguments):
                return 'https://github.com/proofofchance/chance-solana-contracts.git' if arguments[0] == 'remote' else '11' * 20
            def snapshot(source, destination, public_inputs):
                destination.mkdir(); (destination / 'target/deploy').mkdir(parents=True)
            def builder(source, verifier):
                for library in build.PROGRAMS:
                    (source / f'target/deploy/{library}.so').write_bytes(b'\x7fELF' + library.encode())
                (source / 'target/deploy/giveaways-staging-afresh-v1.idl.json').write_text(json.dumps({'address': key(3)}))
            original = shutil.rmtree
            def failed_cleanup(path, *args, **kwargs):
                if Path(path).name.startswith('.afresh-reproduce-'):
                    raise PermissionError('isolated cleanup failure')
                return original(path, *args, **kwargs)
            with patch.object(build, 'source_inputs', return_value=fingerprints), patch.object(build, 'git', side_effect=fake_git), patch.object(build, 'source_snapshot', side_effect=snapshot), patch.object(build.subprocess, 'check_output', return_value='solana-verify 0.4.11\n'), patch.object(build.shutil, 'rmtree', side_effect=failed_cleanup):
                with self.assertRaisesRegex(PermissionError, 'cleanup failure'):
                    build.reproduce(directory / 'private', directory / 'public', input_file, directory / 'output', build=builder)
            self.assertFalse((directory / 'output').exists())


class ScratchOwnershipTests(unittest.TestCase):
    def test_internal_hardlinks_are_retained_and_external_hardlinks_are_rejected(self):
        with tempfile.TemporaryDirectory() as parent, build._Scratch(parent) as scratch:
            source = scratch.path / 'private'; source.mkdir(); overlay = scratch.bind('private')
            target = source / 'target'; target.mkdir()
            first = target / 'one'; first.write_bytes(b'internal')
            os.link(first, target / 'two')
            overlay.reclaim_target()
            sentinel = Path(parent) / 'outside'; sentinel.write_bytes(b'unchanged')
            before = sentinel.stat(); os.link(sentinel, target / 'outside-link')
            with self.assertRaisesRegex(ValueError, 'hardlink outside'):
                overlay.reclaim_target()
            self.assertEqual((sentinel.stat().st_uid, sentinel.stat().st_gid), (before.st_uid, before.st_gid))
            self.assertEqual(sentinel.read_bytes(), b'unchanged')
            (target / 'outside-link').unlink()
            outside_symlink = Path(parent) / 'outside-symlink'; outside_symlink.symlink_to(sentinel)
            os.link(outside_symlink, target / 'symlink-link', follow_symlinks=False)
            with self.assertRaisesRegex(ValueError, 'hardlink outside'):
                overlay.reclaim_target()
            (target / 'symlink-link').unlink()

    def test_inventory_is_bound_to_the_pinned_target_and_its_filesystem(self):
        root = (stat.S_IFDIR, 1, 2, 2, os.getuid(), os.getgid())
        self.assertFalse(build._Overlay._validate_inventory([root], (1, 2)))
        with self.assertRaisesRegex(ValueError, 'pinned directory'):
            build._Overlay._validate_inventory([root], (1, 3))
        for kind in (stat.S_IFDIR, stat.S_IFREG, stat.S_IFLNK):
            with self.subTest(kind=kind), self.assertRaisesRegex(ValueError, 'filesystem boundary'):
                build._Overlay._validate_inventory([root, (kind, 3, 4, 1, os.getuid(), os.getgid())], (1, 2))

    def test_target_symlink_and_replaced_overlay_cannot_reach_a_checkout(self):
        with tempfile.TemporaryDirectory() as parent, build._Scratch(parent) as scratch:
            source = scratch.path / 'private'; source.mkdir(); overlay = scratch.bind('private')
            outside = Path(parent) / 'checkout'; outside.mkdir(); sentinel = outside / 'source'; sentinel.write_bytes(b'original')
            (source / 'target').symlink_to(outside, target_is_directory=True)
            with self.assertRaisesRegex(ValueError, 'real directory'):
                overlay.reclaim_target()
            (source / 'target').unlink()
            retained = scratch.path / 'original'; source.rename(retained); source.mkdir()
            with self.assertRaisesRegex(ValueError, 'overlay identity'):
                overlay.reclaim_target()
            source.rmdir(); retained.rename(source)
            self.assertEqual(sentinel.read_bytes(), b'original')

    def test_normalization_command_is_fixed_and_mounts_only_the_minted_target(self):
        with tempfile.TemporaryDirectory() as parent, build._Scratch(parent) as scratch:
            source = scratch.path / 'private'; source.mkdir(); overlay = scratch.bind('private')
            target = source / 'target'; target.mkdir()
            metadata = target.stat()
            foreign = [(stat.S_IFDIR, metadata.st_dev, metadata.st_ino, metadata.st_nlink, os.getuid() + 1, os.getgid())]
            owned = [(stat.S_IFDIR, metadata.st_dev, metadata.st_ino, metadata.st_nlink, os.getuid(), os.getgid())]
            with patch.object(overlay, '_inventory', side_effect=[foreign, owned]), patch.object(build.subprocess, 'run') as command:
                overlay.reclaim_target()
            arguments = command.call_args.args[0]
            self.assertIn(build.IMAGE, arguments)
            self.assertIn(f'type=bind,src={target},dst=/owned-target', arguments)
            self.assertEqual(arguments[-6:], ['-R', '-h', '-P', '--', f'{os.getuid()}:{os.getgid()}', '/owned-target'])
            self.assertIn('none', arguments); self.assertIn('--read-only', arguments)
            self.assertIn('ALL', arguments); self.assertNotIn('--privileged', arguments)
            self.assertNotIn(str(Path(parent)), arguments)

    def test_mount_separator_and_non_target_descriptor_are_rejected_before_docker(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory) / 'parent,separator'; parent.mkdir()
            with build._Scratch(parent) as scratch:
                source = scratch.path / 'private'; source.mkdir(); overlay = scratch.bind('private')
                target = source / 'target'; target.mkdir()
                descriptor = os.open(target, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
                try:
                    with patch.object(build.subprocess, 'run') as command:
                        with self.assertRaisesRegex(ValueError, 'identity changed'):
                            overlay._docker(overlay.descriptor)
                        with self.assertRaisesRegex(ValueError, 'unsupported separators'):
                            overlay._docker(descriptor)
                        command.assert_not_called()
                finally:
                    os.close(descriptor)

    def test_verifier_failure_is_preserved_even_if_normalization_also_fails(self):
        with tempfile.TemporaryDirectory() as parent, build._Scratch(parent) as scratch:
            source = scratch.path / 'private'; source.mkdir(); overlay = scratch.bind('private')
            failure = subprocess.CalledProcessError(23, ['isolated-verifier'])
            with patch.object(build.subprocess, 'run', side_effect=failure), patch.object(overlay, 'reclaim_target', side_effect=PermissionError('isolated repair failure')) as repair, contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(subprocess.CalledProcessError) as caught:
                    build.build_overlay(source, 'isolated-verifier', overlay=overlay)
            self.assertIs(caught.exception, failure); repair.assert_called_once()


@unittest.skipUnless(os.environ.get('CHANCE_AFRESH_DOCKER_OWNERSHIP_TEST') == '1', 'Opt-in real Linux Docker ownership regression')
class DockerOwnershipTests(unittest.TestCase):
    def test_real_root_owned_outputs_host_idl_retention_and_failed_cleanup(self):
        if sys.platform != 'linux' or os.getuid() == 0:
            self.skipTest('Requires an unprivileged Linux host; Docker Desktop maps host ownership')
        try:
            subprocess.run(['docker', 'info'], check=True, capture_output=True, timeout=20)
        except (OSError, subprocess.SubprocessError):
            self.skipTest('Docker engine unavailable; root-owned regression was not run')
        with tempfile.TemporaryDirectory() as parent:
            sentinel = Path(parent) / 'outside'; sentinel.write_bytes(b'outside ownership and bytes')
            before = sentinel.stat()
            def create_root_outputs(source):
                # Test fixture only. No source checkout or outside sentinel is mounted.
                script = 'mkdir -p /fixture/target/deploy /fixture/target/cache; printf internal > /fixture/target/cache/one; ln /fixture/target/cache/one /fixture/target/cache/two; chmod 700 /fixture/target /fixture/target/cache'
                subprocess.run(['docker', 'run', '--rm', '--platform', 'linux/amd64', '--user', '0:0', '--network', 'none', '--read-only', '--mount', f'type=bind,src={source},dst=/fixture', '--entrypoint', '/bin/sh', build.IMAGE, '-c', script], check=True, capture_output=True, timeout=120)
                self.assertEqual((source / 'target').stat().st_uid, 0)
            with build._Scratch(parent) as scratch:
                source = scratch.path / 'private'; source.mkdir(); overlay = scratch.bind('private')
                create_root_outputs(source); overlay.reclaim_target()
                (source / 'target/debug').mkdir()
                for library in build.PROGRAMS:
                    (source / f'target/deploy/{library}.so').write_bytes(b'\x7fELFisolated test only')
                (source / 'target/deploy/giveaways-staging-afresh-v1.idl.json').write_bytes(b'{}')
                build.retain_only_build_outputs(source)
                self.assertEqual(len(list((source / 'target/deploy').iterdir())), 4)
            self.assertFalse(scratch.path.exists())
            failure = RuntimeError('isolated verifier failure')
            with self.assertRaises(RuntimeError) as caught:
                with build._Scratch(parent) as failed:
                    source = failed.path / 'private'; source.mkdir(); failed.bind('private')
                    create_root_outputs(source)
                    raise failure
            self.assertIs(caught.exception, failure); self.assertFalse(failed.path.exists())
            self.assertEqual((sentinel.stat().st_uid, sentinel.stat().st_gid), (before.st_uid, before.st_gid))
            self.assertEqual(sentinel.read_bytes(), b'outside ownership and bytes')


if __name__ == '__main__':
    unittest.main()
