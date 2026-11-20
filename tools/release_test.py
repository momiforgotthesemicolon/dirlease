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
        for value in ("1.2.3", "0.0.1-alpha", "1.2.3-beta.1"):
            with self.subTest(version=value):
                self.check_package(value)

    def test_rejects_malformed_versions(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for value in ("1.2", "1.2.3-", "1.2.3-alpha..1", "1.2.3-alpha/1"):
                with self.subTest(version=value):
                    (root / "Cargo.toml").write_text(f'[package]\nversion = "{value}"\n')
                    with self.assertRaisesRegex(ValueError, "release version must be"):
                        release.version(root)

    def check_package(self, value):
        # Step #1: Create an isolated versioned repository and executable.
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "repo"
            root.mkdir()
            (root / "src").mkdir()
            (root / "Cargo.toml").write_text(f'[package]\nversion = "{value}"\n')
            (root / "Cargo.lock").write_text(f'[[package]]\nname = "dirlease"\nversion = "{value}"\n')
            (root / "src/main.rs").write_text(f'println!("dirlease {value}")')
            for name in ("LICENSE", "README.md"):
                (root / name).write_text(name)
            binary = Path(temporary) / "dirlease"
            binary.write_text(f'#!/bin/sh\necho "dirlease {value}"\n')
            binary.chmod(0o755)
            release.git(root, "init", "-q")
            release.git(root, "add", ".")
            release.git(root, "-c", "user.name=Test", "-c", "user.email=test@example.com", "commit", "-qm", "fixture")

            # Step #2: Verify deterministic archives, manifest, and formula bindings.
            first, second = Path(temporary) / "first", Path(temporary) / "second"
            release.package(root, binary, first, f"v{value}")
            release.package(root, binary, second, f"v{value}")
            for item in first.iterdir():
                self.assertEqual(item.read_bytes(), (second / item.name).read_bytes())
            for line in (first / "SHA256SUMS").read_text().splitlines():
                digest, name = line.split("  ")
                self.assertEqual(digest, hashlib.sha256((first / name).read_bytes()).hexdigest())
            with tarfile.open(first / f"dirlease-{value}-source.tar.gz") as archive:
                self.assertIn(f"dirlease-{value}/Cargo.lock", archive.getnames())
            self.assertIn(f"/releases/download/v{value}/", (first / "dirlease.rb").read_text())

            # Step #3: Reject wrong tags, occupied outputs, dirty sources and mismatched versions.
            with self.assertRaises(ValueError):
                release.package(root, binary, second, "v9.9.9")
            with self.assertRaises(FileExistsError):
                release.package(root, binary, first, f"v{value}")
            (root / "untracked").touch()
            with self.assertRaises(ValueError):
                release.package(root, binary, second, f"v{value}")
            (root / "src/main.rs").write_text('println!("dirlease 9.9.9")')
            with self.assertRaises(ValueError):
                release.version(root)


if __name__ == "__main__":
    unittest.main()
