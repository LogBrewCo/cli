#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
metadata_file="$tmp_dir/metadata.json"
trap 'rm -rf "$tmp_dir"' EXIT

fail() {
  printf 'Package metadata check failed: %s\n' "$1" >&2
  printf 'Next: fix Cargo.toml package metadata, then rerun bash scripts/test-package-metadata.sh.\n' >&2
  exit 1
}

if [[ -n "${LOGBREW_PACKAGE_METADATA_JSON:-}" ]]; then
  if [[ ! -f "$LOGBREW_PACKAGE_METADATA_JSON" ]]; then
    fail "metadata fixture file is missing"
  fi
  cp "$LOGBREW_PACKAGE_METADATA_JSON" "$metadata_file"
else
  if ! cargo metadata --no-deps --format-version=1 >"$metadata_file"; then
    fail "could not read cargo metadata"
  fi
fi

require_metadata() {
  local jq_filter="$1"
  local description="$2"

  if ! jq -e "$jq_filter" "$metadata_file" >/dev/null; then
    fail "$description"
  fi
}

require_metadata 'any(.packages[]?; .name == "logbrew-cli")' \
  "logbrew-cli package must exist"
require_metadata 'any(.packages[]?; .name == "logbrew-cli" and .license == "MIT")' \
  "crate license must be MIT"
require_metadata 'any(.packages[]?; .name == "logbrew-cli" and .repository == "https://github.com/LogBrewCo/cli")' \
  "crate repository must be the public CLI repo"
require_metadata 'any(.packages[]?; .name == "logbrew-cli" and .homepage == "https://logbrew.co")' \
  "crate homepage must be logbrew.co"
require_metadata 'any(.packages[]?; .name == "logbrew-cli" and .readme == "README.md")' \
  "crate readme must be README.md"
require_metadata 'any(.packages[]?; .name == "logbrew-cli" and .publish == ["crates-io"])' \
  "crate publish target must be crates-io only"
require_metadata 'any(.packages[]? | select(.name == "logbrew-cli").targets[]?; .name == "logbrew" and (.kind | index("bin") != null))' \
  "native binary target must be logbrew"
require_metadata 'any(.packages[]? | select(.name == "logbrew-cli").targets[]?; .name == "logbrew_cli" and (.kind | index("lib") != null))' \
  "library target must be logbrew_cli"
require_metadata 'any(.packages[]?; .name == "logbrew-cli" and .metadata.dist.dist == true)' \
  "cargo-dist metadata must enable distribution"
require_metadata 'any(.packages[]?; .name == "logbrew-cli" and .metadata.dist.formula == "logbrew")' \
  "cargo-dist Homebrew formula must be logbrew"
require_metadata 'any(.packages[]?; .name == "logbrew-cli" and .metadata.dist."npm-package" == "logbrew-cli")' \
  "cargo-dist npm package name must be logbrew-cli"

printf 'Package metadata check passed.\n'
