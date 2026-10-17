# Homebrew formula template for cad-assist.
#
# This is a starting point for a tap (e.g. homebrew-cad-assist). Fill in `url`
# and `sha256` for a tagged release tarball, then `brew install --build-from-source`
# against the tap. cad-assist needs only a Rust toolchain to build (the kernel is
# dependency-free; the `ai` command additionally shells out to `curl` + `jq` at
# runtime, both already present on macOS).
class CadAssist < Formula
  desc "Terminal-native, AI-assisted parametric CAD toolkit"
  homepage "https://github.com/cad-assist/cad-assist"
  url "https://github.com/cad-assist/cad-assist/archive/refs/tags/v1.0.0.tar.gz"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  license "MIT"
  head "https://github.com/cad-assist/cad-assist.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--path", "crates/cad-cli", "--root", prefix
  end

  test do
    assert_match "cad-assist", shell_output("#{bin}/cad version")
  end
end
