#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
output_file="$(mktemp)"
trap 'rm -rf "$tmp_dir" "$output_file"' EXIT

artifact_fixture="$tmp_dir/artifacts"
host_target="aarch64-apple-darwin"
host_archive="logbrew-cli-${host_target}.tar.xz"

crate_version="$(
  cargo metadata --no-deps --format-version=1 |
    jq -r '.packages[] | select(.name == "logbrew-cli").version'
)"

if [[ -z "$crate_version" || "$crate_version" == "null" ]]; then
  printf 'could not read logbrew-cli version from Cargo metadata\n' >&2
  exit 1
fi

write_formula_fixture() {
  cat >"$artifact_fixture/logbrew.rb" <<RUBY
class Logbrew < Formula
  desc "Public command-line interface for LogBrew."
  homepage "https://logbrew.co"
  version "${crate_version}"
  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/LogBrewCo/cli/releases/download/v${crate_version}/logbrew-cli-aarch64-apple-darwin.tar.xz"
    end
    if Hardware::CPU.intel?
      url "https://github.com/LogBrewCo/cli/releases/download/v${crate_version}/logbrew-cli-x86_64-apple-darwin.tar.xz"
    end
  end
  if OS.linux?
    if Hardware::CPU.arm?
      url "https://github.com/LogBrewCo/cli/releases/download/v${crate_version}/logbrew-cli-aarch64-unknown-linux-gnu.tar.xz"
    end
    if Hardware::CPU.intel?
      url "https://github.com/LogBrewCo/cli/releases/download/v${crate_version}/logbrew-cli-x86_64-unknown-linux-gnu.tar.xz"
    end
  end
  license "MIT"

  BINARY_ALIASES = {
    "aarch64-apple-darwin": {},
    "aarch64-unknown-linux-gnu": {},
    "x86_64-apple-darwin": {},
    "x86_64-unknown-linux-gnu": {}
  }

  def target_triple
    cpu = Hardware::CPU.arm? ? "aarch64" : "x86_64"
    os = OS.mac? ? "apple-darwin" : "unknown-linux-gnu"

    "#{cpu}-#{os}"
  end
end
RUBY
}

make_fixture() {
  rm -rf "$artifact_fixture"
  mkdir -p "$artifact_fixture"
  printf 'native archive fixture\n' >"$artifact_fixture/$host_archive"
  write_formula_fixture
}

run_check() {
  LOGBREW_DIST_HOMEBREW_ARTIFACTS_DIR="$artifact_fixture" \
    LOGBREW_DIST_HOMEBREW_TARGET="$host_target" \
    bash scripts/test-dist-homebrew-formula-smoke.sh "v${crate_version}" >"$output_file" 2>&1
}

expect_failure() {
  local expected_line="$1"

  : >"$output_file"
  if run_check; then
    printf 'expected dist Homebrew formula smoke to fail\n' >&2
    exit 1
  fi

  if ! grep -Fq "$expected_line" "$output_file"; then
    printf 'expected dist Homebrew formula smoke output to contain: %s\n' "$expected_line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
}

make_fixture
: >"$output_file"
if ! run_check; then
  printf 'expected dist Homebrew formula fixture to pass\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Dist Homebrew formula smoke passed for ${host_target}." "$output_file"; then
  printf 'expected dist Homebrew formula smoke success output\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

make_fixture
rm "$artifact_fixture/$host_archive"
expect_failure "Dist Homebrew formula smoke failed: missing generated artifact ${host_archive}"

make_fixture
sed -i.bak 's/version "'"${crate_version}"'"/version "0.0.0"/' "$artifact_fixture/logbrew.rb"
expect_failure "Dist Homebrew formula smoke failed: formula version must be ${crate_version}"

make_fixture
sed -i.bak '/logbrew-cli-aarch64-apple-darwin.tar.xz/d' "$artifact_fixture/logbrew.rb"
expect_failure "Dist Homebrew formula smoke failed: target aarch64-apple-darwin URL must be https://github.com/LogBrewCo/cli/releases/download/v${crate_version}/logbrew-cli-aarch64-apple-darwin.tar.xz"

make_fixture
sed -i.bak '/"aarch64-apple-darwin":/d' "$artifact_fixture/logbrew.rb"
expect_failure "Dist Homebrew formula smoke failed: BINARY_ALIASES must include aarch64-apple-darwin"

printf 'Dist Homebrew formula smoke self-test passed.\n'
