"""Check file selection and review behavior in isolated Git repositories."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import quality


class RepositoryFilesTest(unittest.TestCase):
    """Keep local and CI check scopes consistent as files change."""

    def setUp(self):
        # Step #1: Isolate file selection from the developer checkout.
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        original = quality.ROOT
        self.addCleanup(setattr, quality, "ROOT", original)
        quality.ROOT = Path(self.directory.name)
        quality.run(["git", "init", "-q", "-b", "main"])
        self.write(".gitignore", "ignored/\n")
        self.write("src/tracked.rs")
        self.write("src/deleted.rs")
        # Step #2: Track a baseline, then add deleted, new, and ignored cases.
        quality.run(["git", "add", "."])
        quality.run(["git", "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "Initial"])
        (quality.ROOT / "src/deleted.rs").unlink()
        self.write("src/new file.rs")
        self.write("src-extra/outside.rs")
        self.write("ignored/artifact.rs")

    def write(self, name, contents=""):
        path = quality.ROOT / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)

    def test_scope_includes_new_files_but_excludes_deleted_and_neighboring_files(self):
        self.assertEqual(quality.repo_files("src"), ["src/new file.rs", "src/tracked.rs"])

    def test_ignored_artifacts_never_enter_the_root_scope(self):
        self.assertNotIn("ignored/artifact.rs", quality.repo_files("."))
        self.assertIn("src-extra/outside.rs", quality.repo_files("."))

    def test_review_includes_untracked_file_content(self):
        self.write("src/new file.rs", "unique review content\n")
        (quality.ROOT / "src/skills").symlink_to("../ignored", target_is_directory=True)
        script = "import quality; quality.ROOT = __import__('pathlib').Path('.'); quality.review('src')"
        result = subprocess.run(
            ["python3", "-c", script], cwd=quality.ROOT,
            env={**os.environ, "PYTHONPATH": str(Path(quality.__file__).parent)},
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True,
        )
        self.assertIn(b"+unique review content", result.stdout)
        self.assertIn(b"bazel test //src/...", result.stdout)
        self.assertIn(b"new file mode 120000", result.stdout)
        self.assertIn(b"+../ignored", result.stdout)
        self.assertNotIn(b"error:", result.stderr)


if __name__ == "__main__":
    unittest.main()
