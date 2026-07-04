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
  mkdir -p "$fixture_root"
  cat >"$fixture_root/README.md" <<'MARKDOWN'
# LogBrew CLI

## Install

```bash
cargo install logbrew-cli
npm install -g logbrew-cli
brew install LogBrewCo/tap/logbrew
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/LogBrewCo/cli/releases/latest/download/logbrew-cli-installer.sh | sh
powershell -ExecutionPolicy Bypass -c "irm https://github.com/LogBrewCo/cli/releases/latest/download/logbrew-cli-installer.ps1 | iex"
```

Windows users can also download the latest MSI from the GitHub Release assets.

Cargo installs and source builds require Rust 1.87 or newer. The npm,
Homebrew, shell, PowerShell, and MSI installers use native release artifacts.

## Distribution

GitHub Releases publish native archives for Linux x64/ARM64, macOS x64/ARM64,
and Windows x64.
Installers: shell, PowerShell, npm package, Homebrew formula, and Windows MSI.
Trusted publishing/OIDC publishes package managers, and Homebrew uses the
LogBrewCo/homebrew-tap repository with HOMEBREW_TAP_TOKEN.

Before pushing a release tag, run the release preflight:

```bash
bash scripts/release-preflight.sh vX.Y.Z
```

The preflight checks the tag/version match, clean synced `main`, public
crates.io/npm package bootstrap and version collisions, the public Homebrew tap
repository, green CI, GitHub Actions secret names, cargo-dist plan output,
generated native artifacts, source artifact packaging, checksum integrity,
Homebrew formula metadata, npm package install smoke, shell installer smoke,
package install smoke, trusted publishing/OIDC, HOMEBREW_TAP_TOKEN, and
release/tag collisions.
MARKDOWN
}

run_docs_check() {
  LOGBREW_RELEASE_DOCS_ROOT="$fixture_root" \
    bash scripts/test-release-docs.sh >"$output_file" 2>&1
}

expect_docs_failure() {
  local expected="$1"

  : >"$output_file"
  if run_docs_check; then
    printf 'expected release docs self-test to fail\n' >&2
    cat "$output_file" >&2
    exit 1
  fi

  if ! grep -Fq "$expected" "$output_file"; then
    printf 'expected release docs failure to contain: %s\n' "$expected" >&2
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
if ! run_docs_check; then
  printf 'expected current release docs fixture to pass\n' >&2
  cat "$output_file" >&2
  exit 1
fi

make_fixture
remove_literal_line "$fixture_root/README.md" 'bash scripts/release-preflight.sh vX.Y.Z'
expect_docs_failure 'release preflight command missing from README.md'

make_fixture
remove_literal_line "$fixture_root/README.md" 'Homebrew formula metadata, npm package install smoke, shell installer smoke,'
expect_docs_failure 'npm package install smoke missing from README.md'

make_fixture
remove_literal_line "$fixture_root/README.md" 'Installers: shell, PowerShell, npm package, Homebrew formula, and Windows MSI.'
expect_docs_failure 'PowerShell installer guidance missing from README.md'

printf 'Release docs self-test passed.\n'
