#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
output_file="$(mktemp)"
trap 'rm -rf "$tmp_dir" "$output_file"' EXIT

fixture_root="$tmp_dir/fixture"

make_fixture() {
  rm -rf "$fixture_root"
  mkdir -p "$fixture_root/.github/workflows"
  cp dist-workspace.toml "$fixture_root/dist-workspace.toml"
  cp .github/workflows/release.yml "$fixture_root/.github/workflows/release.yml"
  cp .github/workflows/publish-crates.yml "$fixture_root/.github/workflows/publish-crates.yml"
  cp .github/workflows/publish-npm-trusted.yml "$fixture_root/.github/workflows/publish-npm-trusted.yml"
  cp .github/workflows/publish-homebrew-tap.yml "$fixture_root/.github/workflows/publish-homebrew-tap.yml"
}

run_contract_check() {
  LOGBREW_WORKFLOW_CONTRACT_ROOT="$fixture_root" \
    bash scripts/test-release-workflow-contracts.sh >"$output_file" 2>&1
}

expect_contract_failure() {
  local expected="$1"

  : >"$output_file"
  if run_contract_check; then
    printf 'expected release workflow contract self-test to fail\n' >&2
    cat "$output_file" >&2
    exit 1
  fi

  if ! grep -Fq "$expected" "$output_file"; then
    printf 'expected release workflow contract failure to contain: %s\n' "$expected" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
}

remove_literal_line() {
  local file="$1"
  local text="$2"
  local temp_file="${file}.tmp"

  grep -Fv "$text" "$file" >"$temp_file"
  mv "$temp_file" "$file"
}

make_fixture
: >"$output_file"
if ! run_contract_check; then
  printf 'expected current release workflow fixture to pass\n' >&2
  cat "$output_file" >&2
  exit 1
fi

make_fixture
remove_literal_line "$fixture_root/.github/workflows/publish-npm-trusted.yml" 'id-token: write'
expect_contract_failure 'npm trusted publishing OIDC permission missing from .github/workflows/publish-npm-trusted.yml'

make_fixture
remove_literal_line "$fixture_root/.github/workflows/publish-homebrew-tap.yml" 'token: ${{ secrets.HOMEBREW_TAP_TOKEN }}'
expect_contract_failure 'Homebrew tap token missing from .github/workflows/publish-homebrew-tap.yml'

make_fixture
remove_literal_line "$fixture_root/.github/workflows/publish-crates.yml" 'uses: rust-lang/crates-io-auth-action@v1.0.5'
expect_contract_failure 'crates.io trusted publishing auth action missing from .github/workflows/publish-crates.yml'

make_fixture
remove_literal_line "$fixture_root/dist-workspace.toml" 'targets = ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"]'
expect_contract_failure 'cargo-dist native target matrix missing from dist-workspace.toml'

make_fixture
remove_literal_line "$fixture_root/dist-workspace.toml" 'x86_64-pc-windows-msvc = "blacksmith-2vcpu-windows-2025"'
expect_contract_failure 'cargo-dist custom runner x86_64-pc-windows-msvc missing from dist-workspace.toml'

printf 'Release workflow contract self-test passed.\n'
