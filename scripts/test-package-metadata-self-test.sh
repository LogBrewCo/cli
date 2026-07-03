#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
output_file="$(mktemp)"
trap 'rm -rf "$tmp_dir" "$output_file"' EXIT

metadata_file="$tmp_dir/metadata.json"

cargo metadata --no-deps --format-version=1 >"$metadata_file"

run_metadata_check() {
  LOGBREW_PACKAGE_METADATA_JSON="$1" bash scripts/test-package-metadata.sh >"$output_file" 2>&1
}

expect_failure() {
  local fixture_file="$1"
  local expected="$2"

  : >"$output_file"
  if run_metadata_check "$fixture_file"; then
    printf 'expected package metadata self-test to fail\n' >&2
    cat "$output_file" >&2
    exit 1
  fi

  if ! grep -Fq "$expected" "$output_file"; then
    printf 'expected package metadata failure to contain: %s\n' "$expected" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
}

mutate_metadata() {
  local jq_filter="$1"
  local fixture_file="$2"

  jq "$jq_filter" "$metadata_file" >"$fixture_file"
}

: >"$output_file"
if ! run_metadata_check "$metadata_file"; then
  printf 'expected current package metadata fixture to pass\n' >&2
  cat "$output_file" >&2
  exit 1
fi

mutate_metadata '(.packages[] | select(.name == "logbrew-cli").metadata.dist."npm-package") = "logbrew"' "$tmp_dir/bad-npm.json"
expect_failure "$tmp_dir/bad-npm.json" "cargo-dist npm package name must be logbrew-cli"

mutate_metadata '(.packages[] | select(.name == "logbrew-cli").targets[] | select(.name == "logbrew").name) = "logbrew-cli"' "$tmp_dir/bad-bin.json"
expect_failure "$tmp_dir/bad-bin.json" "native binary target must be logbrew"

mutate_metadata '(.packages[] | select(.name == "logbrew-cli").publish) = null' "$tmp_dir/bad-publish.json"
expect_failure "$tmp_dir/bad-publish.json" "crate publish target must be crates-io only"

printf 'Package metadata self-test passed.\n'
