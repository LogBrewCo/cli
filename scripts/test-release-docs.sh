#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOCS_ROOT="${LOGBREW_RELEASE_DOCS_ROOT:-$ROOT_DIR}"
readme="$DOCS_ROOT/README.md"

fail() {
  printf 'Release docs check failed: %s\n' "$1" >&2
  printf 'Next: update README.md release/install guidance, then rerun bash scripts/test-release-docs.sh.\n' >&2
  exit 1
}

require_literal() {
  local text="$1"
  local description="$2"

  if [[ "$readme_text" != *"$text"* ]]; then
    fail "${description} missing from README.md"
  fi
}

if [[ ! -f "$readme" ]]; then
  fail "README.md is missing"
fi

readme_text="$(tr '\n' ' ' <"$readme")"

require_literal 'cargo install logbrew-cli' "cargo install guidance"
require_literal 'npm install -g logbrew-cli' "npm install guidance"
require_literal 'brew install LogBrewCo/tap/logbrew' "Homebrew install guidance"
require_literal 'logbrew-cli-installer.sh' "shell installer guidance"
require_literal 'logbrew-cli-installer.ps1' "PowerShell installer guidance"
require_literal 'latest MSI' "MSI installer guidance"
require_literal 'Rust 1.87' "Rust version guidance"
require_literal 'native release artifacts' "native artifact guidance"

require_literal 'native archives for Linux x64/ARM64, macOS x64/ARM64' \
  "native archive release guidance"
require_literal 'Windows x64' "Windows native archive guidance"
require_literal 'PowerShell, npm package, Homebrew formula, and Windows MSI' \
  "PowerShell installer guidance"
require_literal 'trusted publishing/OIDC' "trusted publishing guidance"
require_literal 'LogBrewCo/homebrew-tap' "Homebrew tap guidance"
require_literal 'HOMEBREW_TAP_TOKEN' "Homebrew tap token guidance"

required_preflight_terms=(
  'bash scripts/release-preflight.sh vX.Y.Z|release preflight command'
  'tag/version match|tag version preflight guidance'
  'clean synced `main`|clean main preflight guidance'
  'public crates.io/npm package bootstrap|registry bootstrap preflight guidance'
  'public Homebrew tap|Homebrew tap preflight guidance'
  'green CI|CI preflight guidance'
  'GitHub Actions secret names|secret-name preflight guidance'
  'package install smoke|package install smoke'
  'cargo-dist plan|cargo-dist plan guidance'
  'generated native artifacts|native artifact preflight guidance'
  'source artifact|source artifact guidance'
  'checksum|checksum guidance'
  'Homebrew formula|Homebrew formula guidance'
  'npm package install smoke|npm package install smoke'
  'shell installer smoke|shell installer smoke'
  'release/tag collisions|release collision preflight guidance'
)

for term in "${required_preflight_terms[@]}"; do
  IFS='|' read -r needle description <<<"$term"
  require_literal "$needle" "$description"
done

printf 'Release docs check passed.\n'
