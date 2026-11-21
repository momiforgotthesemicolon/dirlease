class Dirlease < Formula
  desc "Session-scoped temporary directories with durable scheduled cleanup"
  homepage "https://github.com/momiforgotthesemicolon/dirlease"
  url "https://github.com/momiforgotthesemicolon/dirlease/releases/download/v0.0.2-alpha/dirlease-0.0.2-alpha-source.tar.gz"
  version "0.0.2-alpha"
  sha256 "e794fca7730e99063f1d036457169ddd974557d0ce8c33fc38199f90efcd0ff5"
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
    assert_equal "dirlease 0.0.2-alpha", shell_output("#{bin}/dirlease --version").strip
    assert_match "DIRLEASE_SESSION", shell_output("#{bin}/dirlease --help")
  end
end
