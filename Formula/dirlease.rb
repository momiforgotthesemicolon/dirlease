class Dirlease < Formula
  desc "Session-scoped temporary directories with durable scheduled cleanup"
  homepage "https://github.com/momiforgotthesemicolon/dirlease"
  url "https://codeload.github.com/momiforgotthesemicolon/dirlease/tar.gz/35d16bdcdbbfb84c5e8f137ef7ba308ed73f7e1a"
  version "0.1.0"
  sha256 "d281e2b0d8c45f34c938d55d5ba2ab439cad0d5ffccc444c2fee15e2abda7890"
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
    assert_equal "dirlease 0.1.0", shell_output("#{bin}/dirlease --version").strip
    assert_match "DIRLEASE_SESSION", shell_output("#{bin}/dirlease --help")
  end
end
