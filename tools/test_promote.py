import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('promote', Path(__file__).with_name('promote.py'))
promote = importlib.util.module_from_spec(spec)
spec.loader.exec_module(promote)


def git(repo, *args):
    subprocess.run(['git', '-C', str(repo), *args], check=True, capture_output=True)


def repository(root, files, remote=None):
    root.mkdir()
    git(root, 'init', '-q')
    for name, body in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body)
    if remote:
        git(root, 'remote', 'add', 'origin', remote)
    commit(root)
    return root


def commit(root):
    git(root, 'add', '-A')
    git(root, '-c', 'user.name=t', '-c', 'user.email=t@example.com', 'commit', '-q', '--allow-empty', '-m', 'c')


class PromoteTest(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        base = Path(self.scratch.name)
        self.private = repository(base / 'private', {
            'Cargo.lock': 'lock\n',
            'programs/registry/src/lib.rs': 'new\n',
            'tools/idl-export/main.rs': 'idl\n',
            'operations/src/lib.rs': 'private only\n',
            '.github/workflows/ci.yml': 'private only\n',
        })
        os.symlink('lib.rs', self.private / 'programs/registry/src/link.rs')
        commit(self.private)
        self.public = repository(base / 'public', {
            'Cargo.lock': 'lock\n',
            'programs/registry/src/lib.rs': 'old\n',
            'crates/removed.rs': 'gone privately\n',
            'README.md': 'public only\n',
        }, remote='https://github.com/proofofchance/chance-solana-contracts.git')

    def tearDown(self):
        self.scratch.cleanup()

    def test_only_allowlisted_differences_are_reported(self):
        self.assertEqual(promote.differing(self.private, self.public), [
            'crates/removed.rs',
            'programs/registry/src/lib.rs',
            'programs/registry/src/link.rs',
            'tools/idl-export/main.rs',
        ])

    def test_promotion_makes_the_allowlisted_trees_equal(self):
        changed = promote.promote(self.private, self.public)
        self.assertEqual(len(changed), 4)
        commit(self.public)
        self.assertEqual(promote.differing(self.private, self.public), [])
        self.assertEqual((self.public / 'programs/registry/src/lib.rs').read_text(), 'new\n')
        self.assertEqual(os.readlink(self.public / 'programs/registry/src/link.rs'), 'lib.rs')
        self.assertFalse((self.public / 'crates/removed.rs').exists())
        self.assertFalse((self.public / 'operations').exists())
        self.assertFalse((self.public / '.github').exists())
        self.assertEqual((self.public / 'README.md').read_text(), 'public only\n')

    def test_promotion_refuses_a_dirty_public_checkout(self):
        (self.public / 'README.md').write_text('edited\n')
        with self.assertRaisesRegex(ValueError, 'clean'):
            promote.promote(self.private, self.public)

    def test_promotion_refuses_uncommitted_private_source(self):
        (self.private / 'programs/registry/src/lib.rs').write_text('uncommitted\n')
        with self.assertRaisesRegex(ValueError, 'Commit'):
            promote.promote(self.private, self.public)

    def test_promotion_refuses_another_public_repository(self):
        git(self.public, 'remote', 'set-url', 'origin', 'https://github.com/someone/else.git')
        with self.assertRaisesRegex(ValueError, 'must track'):
            promote.promote(self.private, self.public)


if __name__ == '__main__':
    unittest.main()
