class Dirlease < Formula
  desc "Session-scoped temporary directories with durable scheduled cleanup"
  homepage "https://github.com/momiforgotthesemicolon/dirlease"
  url "https://github.com/momiforgotthesemicolon/dirlease/releases/download/v0.0.3-alpha/dirlease-0.0.3-alpha-source.tar.gz"
  version "0.0.3-alpha"
  sha256 "36df2a7d6576caca9ad4747eae3db670f35d0a8e5b890e67e046204e3c9cc6f8"
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
    assert_equal "dirlease 0.0.3-alpha", shell_output("#{bin}/dirlease --version").strip
    assert_match "DIRLEASE_SESSION", shell_output("#{bin}/dirlease --help")
  end
end
