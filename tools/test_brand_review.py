import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('brand_review', Path(__file__).with_name('brand_review.py'))
tool = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tool)


class SourceBinding(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name)
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True, capture_output=True)
        (self.root / 'programs').mkdir()
        (self.root / 'programs/source.rs').write_text('source')
        self.output = self.root / 'target/development'
        self.output.mkdir(parents=True)
        (self.output / 'giveaways.so').write_bytes(b'ELF fixture')
        self.record = {'schema': 1, 'features': ['brand-review'], 'inputs': tool.fingerprint(self.root),
                       'elfFileSha256': hashlib.sha256(b'ELF fixture').hexdigest()}
        self.save()

    def tearDown(self):
        self.scratch.cleanup()

    def save(self):
        (self.output / 'development-build.json').write_text(json.dumps(self.record))

    def test_native_elf_is_accepted_only_for_its_exact_untracked_source_snapshot(self):
        self.assertEqual(tool.verify(self.root, self.output), self.output / 'giveaways.so')
        (self.root / 'programs/source.rs').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'ELF/source binding changed'):
            tool.verify(self.root, self.output)

    def test_modified_elf_and_legacy_features_never_reuse_development_evidence(self):
        (self.output / 'giveaways.so').write_bytes(b'different')
        with self.assertRaises(ValueError):
            tool.verify(self.root, self.output)
        (self.output / 'giveaways.so').write_bytes(b'ELF fixture')
        self.record['features'] = []
        self.save()
        with self.assertRaises(ValueError):
            tool.verify(self.root, self.output)

    def test_additional_build_inputs_invalidate_an_existing_manifest(self):
        (self.root / 'programs/extra.rs').write_text('new source')
        with self.assertRaises(ValueError):
            tool.verify(self.root, self.output)


if __name__ == '__main__':
    unittest.main()
