import copy
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('runtime_gate', Path(__file__).with_name('runtime_advisory_gate.py'))
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)
REPO = Path(__file__).resolve().parents[1]


class RuntimeBaseline(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name)
        self.baseline = gate.load(REPO / gate.BASELINE)
        self.report = gate.load(REPO / 'tools/fixtures/runtime-audit-0.23.json')
        for name in [*self.baseline['context_sha256'], gate.LOCK]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(REPO / name, path)
        self.lockfile = self.root / gate.LOCK

    def tearDown(self):
        self.scratch.cleanup()

    def check(self, report=None, status=1, baseline=None, lock=None):
        gate.validate(self.root, lock or self.lockfile, baseline or self.baseline,
                      self.report if report is None else report, status)

    def test_exact_complete_report_is_accepted(self):
        self.check()
        self.assertEqual(self.report['vulnerabilities']['count'], 3)
        self.assertEqual(sum(map(len, self.report['warnings'].values())), 6)
        self.assertEqual(self.report['settings']['ignore'], [])

    def test_other_lock_is_refused_even_with_identical_bytes(self):
        path = self.root / 'operations/Cargo.lock'
        path.parent.mkdir()
        shutil.copy2(self.lockfile, path)
        with self.assertRaisesRegex(gate.PolicyError, 'restricted'):
            self.check(lock=path)

    def test_lock_change_is_rejected(self):
        self.lockfile.write_bytes(self.lockfile.read_bytes() + b'\n')
        with self.assertRaisesRegex(gate.PolicyError, 'Lock changed'):
            self.check()

    def test_new_or_changed_call_path_is_rejected(self):
        path = self.root / 'programs/daily-lottery-v1/tests/full_flow.rs'
        path.write_bytes(path.read_bytes() + b'\n// changed use of signing helper\n')
        with self.assertRaisesRegex(gate.PolicyError, 'context changed'):
            self.check()
        shutil.copy2(REPO / path.relative_to(self.root), path)
        path.with_name('new_path.rs').write_text('new source')
        with self.assertRaisesRegex(gate.PolicyError, 'context changed'):
            self.check()

    def test_feature_changes_are_rejected(self):
        path = self.root / 'tests/runtime/Cargo.toml'
        path.write_bytes(path.read_bytes() + b'\n# features changed\n')
        with self.assertRaisesRegex(gate.PolicyError, 'context changed'):
            self.check()

    def test_new_finding_is_rejected(self):
        report = copy.deepcopy(self.report)
        entry = copy.deepcopy(report['vulnerabilities']['list'][0])
        entry['advisory']['id'] = 'RUSTSEC-2099-9999'
        report['vulnerabilities']['list'].append(entry)
        report['vulnerabilities']['count'] += 1
        with self.assertRaises(gate.PolicyError):
            self.check(report)

    def test_changed_version_checksum_category_and_detail_are_rejected(self):
        for area, key, value in [('package', 'version', '0.5.6'), ('package', 'checksum', '0' * 64),
                                 ('advisory', 'categories', ['code-execution']), ('advisory', 'description', 'Changed exposure')]:
            report = copy.deepcopy(self.report)
            report['vulnerabilities']['list'][0][area][key] = value
            with self.assertRaises(gate.PolicyError):
                self.check(report)

    def test_removed_findings_require_review(self):
        report = copy.deepcopy(self.report)
        report['warnings']['unsound'] = []
        with self.assertRaises(gate.PolicyError):
            self.check(report)

    def test_ignore_and_narrowed_settings_are_rejected(self):
        for key, value in [('ignore', ['RUSTSEC-2022-0093']), ('severity', 'high'), ('target_arch', ['x86_64']),
                           ('target_os', ['linux']), ('informational_warnings', ['unmaintained'])]:
            report = copy.deepcopy(self.report)
            report['settings'][key] = value
            with self.assertRaises(gate.PolicyError):
                self.check(report)

    def test_scanner_errors_are_distinct_from_known_vulnerability_exit(self):
        for status in [0, 2, -9]:
            with self.assertRaises(gate.ScannerError):
                self.check(status=status)
        with self.assertRaises(gate.ScannerError):
            self.check(report={'error': 'database unavailable'})

    def test_widened_scope_and_pending_approval_are_rejected(self):
        baseline = copy.deepcopy(self.baseline)
        baseline['lockfile'] = 'operations/Cargo.lock'
        with self.assertRaises(gate.PolicyError):
            self.check(baseline=baseline)
        baseline = copy.deepcopy(self.baseline)
        baseline['approval']['status'] = 'pending'
        with self.assertRaises(gate.PolicyError):
            self.check(baseline=baseline)

    def test_cli_reports_scanner_failure_without_parsing_partial_json(self):
        self.assertEqual(gate.main(['--report', '/nonexistent/partial.json', '--raw-status', '2']), 2)

    def test_duplicate_json_is_rejected(self):
        with self.assertRaises(gate.ScannerError):
            json.loads('{"ignore":[],"ignore":["hidden"]}', object_pairs_hook=gate.unique)


if __name__ == '__main__':
    unittest.main()
