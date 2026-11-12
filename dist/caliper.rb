# Homebrew formula template for Caliper.
#
# This is a starting point for a tap (e.g. homebrew-caliper). Fill in `url`
# and `sha256` for a tagged release tarball, then `brew install --build-from-source`
# against the tap. Caliper needs only a Rust toolchain to build (the kernel is
# dependency-free; the `ai` command additionally shells out to `curl` + `jq` at
# runtime, both already present on macOS).
class Caliper < Formula
  desc "Terminal-native, AI-assisted parametric CAD toolkit"
  homepage "https://github.com/calipercad/caliper"
  url "https://github.com/calipercad/caliper/archive/refs/tags/v1.8.0.tar.gz"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  license "Apache-2.0"
  head "https://github.com/calipercad/caliper.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--path", "crates/cad-cli", "--root", prefix
  end

  test do
    assert_match "Caliper", shell_output("#{bin}/cad version")
  end
end
