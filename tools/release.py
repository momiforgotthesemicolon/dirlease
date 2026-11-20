#!/usr/bin/env python3
"""Package committed source and the host Bazel release binary without credentials."""
import argparse
import gzip
import hashlib
import io
from pathlib import Path
import platform
import re
import subprocess
import tarfile

REPOSITORY = "momiforgotthesemicolon/dirlease"


def git(root, *arguments):
    return subprocess.check_output(["git", "-C", str(root), *arguments])


def version(root):
    manifest = (root / "Cargo.toml").read_text()
    package = re.search(r"(?ms)^\[package\]\n(.*?)(?=^\[|\Z)", manifest).group(1)
    value = re.search(r'^version = "([0-9]+\.[0-9]+\.[0-9]+)"$', package, re.M)
    if value is None:
        raise ValueError("release version must be major.minor.patch")
    value = value.group(1)
    cli = (root / "src/main.rs").read_text()
    lock = (root / "Cargo.lock").read_text()
    locked = re.search(r'(?m)^name = "dirlease"\nversion = "([^"\n]+)"$', lock)
    if f'println!("dirlease {value}")' not in cli or locked is None or locked.group(1) != value:
        raise ValueError("Cargo.toml, Cargo.lock and CLI versions must agree")
    return value


def formula(value, url, checksum):
    # Step #1: Describe the source build, optional service, and state-free smoke test.
    return f'''class Dirlease < Formula
  desc "Session-scoped temporary directories with durable scheduled cleanup"
  homepage "https://github.com/{REPOSITORY}"
  url "{url}"
  version "{value}"
  sha256 "{checksum}"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  service do
    run [opt_bin/"dirlease", "serve"]
    keep_alive true
  end

  test do
    assert_equal "dirlease {value}", shell_output("#{{bin}}/dirlease --version").strip
    assert_match "DIRLEASE_SESSION", shell_output("#{{bin}}/dirlease --help")
  end
end
'''


def archive(path, members):
    with path.open("xb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as tar:
                for name, content, mode in members:
                    info = tarfile.TarInfo(name)
                    info.size = len(content)
                    info.mode = mode
                    tar.addfile(info, io.BytesIO(content))


def package(root, binary, output, tag):
    # Step #1: Bind the version and executable to a clean committed source tree.
    value = version(root)
    if tag != f"v{value}":
        raise ValueError("tag must match Cargo.toml version")
    if git(root, "status", "--porcelain", "--untracked-files=normal").strip():
        raise ValueError("commit all changes before packaging")
    if subprocess.check_output([str(binary), "--version"], text=True).strip() != f"dirlease {value}":
        raise ValueError("binary version does not match source")
    system = platform.system().lower()
    machine = platform.machine().lower()
    if system not in ("darwin", "linux") or machine not in ("arm64", "aarch64", "x86_64"):
        raise ValueError("unsupported host platform")
    machine = "aarch64" if machine == "arm64" else machine
    output.mkdir(parents=True, exist_ok=False)

    # Step #2: Produce deterministic source and host binary archives.
    prefix = f"dirlease-{value}"
    source = output / f"{prefix}-source.tar.gz"
    committed = git(root, "archive", "--format=tar", "HEAD")
    members = []
    with tarfile.open(fileobj=io.BytesIO(committed)) as tree:
        for entry in tree:
            if entry.isfile():
                members.append((f"{prefix}/{entry.name}", tree.extractfile(entry).read(), entry.mode))
            elif not entry.isdir() and not entry.issym():
                raise ValueError(f"unsupported archive entry: {entry.name}")
    archive(source, sorted(members))
    archive(output / f"{prefix}-{system}-{machine}.tar.gz", [
        (f"{prefix}/dirlease", binary.read_bytes(), 0o755),
        (f"{prefix}/LICENSE", (root / "LICENSE").read_bytes(), 0o644),
        (f"{prefix}/README.md", (root / "README.md").read_bytes(), 0o644),
    ])

    # Step #3: Emit a release formula and checksums without contacting a service.
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    url = f"https://github.com/{REPOSITORY}/releases/download/{tag}/{source.name}"
    (output / "dirlease.rb").write_text(formula(value, url, digest))
    (output / "SOURCE_COMMIT").write_bytes(git(root, "rev-parse", "HEAD"))
    checksums = [f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n" for p in sorted(output.iterdir())]
    (output / "SHA256SUMS").write_text("".join(checksums))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    arguments = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    try:
        package(root, arguments.binary.resolve(), arguments.output.resolve(), arguments.tag)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"release: {error}\n")


if __name__ == "__main__":
    main()
