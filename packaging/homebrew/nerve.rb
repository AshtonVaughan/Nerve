class Nerve < Formula
  desc "Real-time computer-use execution runtime for AI agents"
  homepage "https://github.com/ashtonvaughan/nerve"
  url "https://github.com/ashtonvaughan/nerve/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  license "Apache-2.0"
  head "https://github.com/ashtonvaughan/nerve.git", branch: "main"

  depends_on "rust" => :build

  def install
    cd "core" do
      system "cargo", "install", *std_cargo_args(path: "crates/nerve-cli")
    end
  end

  service do
    run [opt_bin/"nerve", "start"]
    keep_alive true
    log_path var/"log/nerve.log"
    error_log_path var/"log/nerve.log"
  end

  test do
    assert_match "Nerve", shell_output("#{bin}/nerve --help")
  end
end
