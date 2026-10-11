import importlib.util
import json
from pathlib import Path
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


if __name__ == '__main__':
    unittest.main()
