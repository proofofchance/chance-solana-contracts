"""Exercise the actual Rust build policy without compiling program dependencies."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]


def local_inputs():
    # Public isolated fixtures: program [201;32], registry [202;32], and the
    # public signer derived from the explicitly test-only Ed25519 seed [7;32].
    return ('schema=1\nenvironment=local\n'
            'daily_lottery_program=EahQmXc3rwhY3CH1g3ZgUx8L4vHTNmzpK1xtiQ1RAxq6\n'
            'registry_program=Eechuq54TEzmBHXX7Ltbmpu6K5Vf3qgdsRdYmn41vJ9o\n'
            'initialization_authority=GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB\n')


def staging_inputs():
    rows = dict(line.split('=', 1) for line in local_inputs().splitlines())
    return ('schema=1\nenvironment=staging\ngenesis_hash=EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG\n'
            f'registry_program={rows["registry_program"]}\ndaily_lottery_program={rows["daily_lottery_program"]}\n'
            'giveaways_program=8qbHbw2BbbTHBW1sbeqakYXVKRQM8Ne7pLK7m6CVfeR\n'
            f'initialization_authority={rows["initialization_authority"]}\n')


class ControlBuildTests(unittest.TestCase):
    def test_feature_catalog_is_closed_and_scopes_do_not_enable_powers(self):
        features = tomllib.loads((ROOT / 'programs/daily-lottery-v1/Cargo.toml').read_text())['features']
        self.assertEqual(set(features), {'staging-afresh-v1', 'allow-early-upload', 'allow-service-charge-update', 'no-entrypoint', 'default', 'local-test-controls', 'staging-test-controls', 'production-release'})
        self.assertTrue(all(value == [] for value in features.values()), 'Default/scope features must never enable capabilities transitively')

    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.temp.name)
        cls.binary = cls.root / 'build-policy'
        subprocess.run(['rustc', '--edition=2021', str(ROOT / 'programs/daily-lottery-v1/build.rs'), '-o', str(cls.binary)], check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def invoke(self, features, text=None):
        with tempfile.TemporaryDirectory(dir=self.root) as scratch:
            root = Path(scratch)
            manifest = root / 'programs/daily-lottery-v1'
            manifest.mkdir(parents=True)
            (root / 'releases').mkdir()
            (root / 'out').mkdir()
            (root / 'releases/local-test-controls-v1.inputs').write_text(local_inputs() if text is None else text)
            (root / 'releases/staging-afresh-v1.inputs').write_text(staging_inputs())
            env = {key: value for key, value in os.environ.items() if not key.startswith('CARGO_FEATURE_')}
            env.update(CARGO_MANIFEST_DIR=str(manifest), OUT_DIR=str(root / 'out'))
            env.update({f'CARGO_FEATURE_{feature.upper().replace("-", "_")}': '1' for feature in features})
            result = subprocess.run([str(self.binary)], env=env, text=True, capture_output=True)
            output = root / 'out/test_controls.rs'
            return result, output.read_text() if output.exists() else None

    def test_valid_scopes_and_default_have_exact_capability_identity(self):
        for features in [[], ['production-release'], ['local-test-controls'], ['local-test-controls', 'allow-early-upload'], ['staging-afresh-v1', 'staging-test-controls'], ['staging-afresh-v1', 'staging-test-controls', 'allow-early-upload']]:
            with self.subTest(features=features):
                result, output = self.invoke(features)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output is not None, any('test-controls' in flag for flag in features))
                if output:
                    self.assertIn('TEST_CONTROL_PROGRAM_ID', output)
                    self.assertIn('[201, 201,', output)

    def test_forbidden_combinations_fail_before_artifact_generation(self):
        cases = [['allow-early-upload'], ['local-test-controls', 'staging-test-controls'], ['local-test-controls', 'staging-afresh-v1'], ['staging-test-controls']]
        cases += [['production-release', flag] for flag in ['allow-early-upload', 'local-test-controls', 'staging-test-controls', 'staging-afresh-v1', 'allow-service-charge-update']]
        for features in cases:
            with self.subTest(features=features):
                result, output = self.invoke(features)
                self.assertNotEqual(result.returncode, 0)
                self.assertIsNone(output)

    def test_local_identity_is_strict_and_cannot_be_relabelled(self):
        source = local_inputs()
        for text in [source.replace('environment=local', 'environment=production'), source.replace('schema=1', 'schema=2'), source + 'force=true\n', source.rstrip(), source.replace('daily_lottery_program=', 'program=')]:
            result, output = self.invoke(['local-test-controls', 'allow-early-upload'], text)
            self.assertNotEqual(result.returncode, 0)
            self.assertIsNone(output)

    def test_actual_rustc_cfg_cannot_bypass_cargo_build_guards(self):
        cases = [([], True), (['production-release'], True), (['local-test-controls', 'allow-early-upload'], True), (['staging-afresh-v1', 'staging-test-controls', 'allow-early-upload'], True), (['allow-early-upload'], False), (['production-release', 'local-test-controls', 'allow-early-upload'], False), (['local-test-controls', 'staging-test-controls'], False), (['staging-test-controls', 'allow-early-upload'], False)]
        for features, expected in cases:
            command = ['rustc', '--edition=2021', '--crate-type=lib', '--emit=metadata', str(ROOT / 'programs/daily-lottery-v1/src/test_control_policy.rs'), '-o', str(self.root / 'policy.rmeta')]
            for feature in features:
                command += ['--cfg', f'feature="{feature}"']
            result = subprocess.run(command, capture_output=True)
            self.assertEqual(result.returncode == 0, expected, str(features))


if __name__ == '__main__':
    if sys.argv[1:] == ['--write-fixtures']:
        for name, text in [('local-test-controls-v1.inputs', local_inputs()), ('staging-afresh-v1.inputs', staging_inputs())]:
            target = ROOT / 'releases' / name
            target.parent.mkdir(parents=True, exist_ok=True)
            if target.exists() and target.read_text() != text:
                raise SystemExit('Refusing to overwrite existing operational build inputs with fixtures')
            target.write_text(text)
        print('Public deterministic runtime fixtures written; not a live release')
    else:
        unittest.main()
