"""Release format and rejection checks use isolated committed fixtures."""
import hashlib
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
import release


class ReleaseTest(unittest.TestCase):
    def test_package(self):
        # Step #1: Create an isolated versioned repository and executable.
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "repo"
            root.mkdir()
            (root / "src").mkdir()
            (root / "Cargo.toml").write_text('[package]\nversion = "1.2.3"\n')
            (root / "Cargo.lock").write_text('[[package]]\nname = "dirlease"\nversion = "1.2.3"\n')
            (root / "src/main.rs").write_text('println!("dirlease 1.2.3")')
            for name in ("LICENSE", "README.md"):
                (root / name).write_text(name)
            binary = Path(temporary) / "dirlease"
            binary.write_text('#!/bin/sh\necho "dirlease 1.2.3"\n')
            binary.chmod(0o755)
            release.git(root, "init", "-q")
            release.git(root, "add", ".")
            release.git(root, "-c", "user.name=Test", "-c", "user.email=test@example.com", "commit", "-qm", "fixture")

            # Step #2: Verify deterministic archives, manifest, and formula bindings.
            first, second = Path(temporary) / "first", Path(temporary) / "second"
            release.package(root, binary, first, "v1.2.3")
            release.package(root, binary, second, "v1.2.3")
            for item in first.iterdir():
                self.assertEqual(item.read_bytes(), (second / item.name).read_bytes())
            for line in (first / "SHA256SUMS").read_text().splitlines():
                digest, name = line.split("  ")
                self.assertEqual(digest, hashlib.sha256((first / name).read_bytes()).hexdigest())
            with tarfile.open(first / "dirlease-1.2.3-source.tar.gz") as archive:
                self.assertIn("dirlease-1.2.3/Cargo.lock", archive.getnames())
            self.assertIn("/releases/download/v1.2.3/", (first / "dirlease.rb").read_text())

            # Step #3: Reject wrong tags, occupied outputs, dirty sources and mismatched versions.
            with self.assertRaises(ValueError):
                release.package(root, binary, second, "v9.9.9")
            with self.assertRaises(FileExistsError):
                release.package(root, binary, first, "v1.2.3")
            (root / "untracked").touch()
            with self.assertRaises(ValueError):
                release.package(root, binary, second, "v1.2.3")
            (root / "src/main.rs").write_text('println!("dirlease 9.9.9")')
            with self.assertRaises(ValueError):
                release.version(root)


if __name__ == "__main__":
    unittest.main()
