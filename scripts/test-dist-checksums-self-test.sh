#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
output_file="$(mktemp)"
trap 'rm -rf "$tmp_dir" "$output_file"' EXIT

artifact_fixture="$tmp_dir/artifacts"

write_checksum_file() {
  local artifact="$1"
  local checksum_file="$2"

  if command -v shasum >/dev/null 2>&1; then
    (cd "$artifact_fixture" && shasum -a 256 "$artifact" | awk '{print $1 " *" $2}' >"$checksum_file")
  else
    (cd "$artifact_fixture" && sha256sum "$artifact" >"$checksum_file")
  fi
}

make_fixture() {
  rm -rf "$artifact_fixture"
  mkdir -p "$artifact_fixture"
  printf 'source contents\n' >"$artifact_fixture/source.tar.gz"
  printf 'npm contents\n' >"$artifact_fixture/logbrew-cli-npm-package.tar.gz"
  printf 'native contents\n' >"$artifact_fixture/logbrew-cli-test-target.tar.xz"

  write_checksum_file source.tar.gz source.tar.gz.sha256
  write_checksum_file logbrew-cli-npm-package.tar.gz sha256.sum
  write_checksum_file source.tar.gz source.sum
  cat "$artifact_fixture/source.sum" >>"$artifact_fixture/sha256.sum"
  rm "$artifact_fixture/source.sum"
  write_checksum_file logbrew-cli-test-target.tar.xz logbrew-cli-test-target.tar.xz.sha256
}

run_check() {
  LOGBREW_DIST_CHECKSUMS_ARTIFACTS_DIR="$artifact_fixture" \
    bash scripts/test-dist-checksums.sh v0.1.17 >"$output_file" 2>&1
}

expect_failure() {
  local expected_line="$1"

  : >"$output_file"
  if run_check; then
    printf 'expected dist checksum check to fail\n' >&2
    exit 1
  fi

  if ! grep -Fq "$expected_line" "$output_file"; then
    printf 'expected dist checksum output to contain: %s\n' "$expected_line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
}

make_fixture
: >"$output_file"
if ! run_check; then
  printf 'expected dist checksum fixture to pass\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Dist checksum check passed." "$output_file"; then
  printf 'expected dist checksum success output\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

make_fixture
printf '0000000000000000000000000000000000000000000000000000000000000000 *source.tar.gz\n' >"$artifact_fixture/source.tar.gz.sha256"
expect_failure "Dist checksum check failed: checksum mismatch for source.tar.gz listed in source.tar.gz.sha256"

make_fixture
rm "$artifact_fixture/logbrew-cli-npm-package.tar.gz"
expect_failure "Dist checksum check failed: checksum entry logbrew-cli-npm-package.tar.gz in sha256.sum points to missing artifact"

make_fixture
printf 'not-a-sha *source.tar.gz\n' >"$artifact_fixture/source.tar.gz.sha256"
expect_failure "Dist checksum check failed: source.tar.gz.sha256 has invalid SHA-256 digest for source.tar.gz"

printf 'Dist checksum self-test passed.\n'
