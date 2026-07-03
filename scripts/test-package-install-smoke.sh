#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

fail() {
  printf 'Package install smoke failed: %s\n' "$1" >&2
  printf 'Next: fix the packaged crate install path, then rerun bash scripts/test-package-install-smoke.sh.\n' >&2
  exit 1
}

expect_contains() {
  local haystack="$1"
  local needle="$2"

  if [[ "$haystack" != *"$needle"* ]]; then
    printf 'Package install smoke failed: expected version JSON to contain %s\n' "$needle" >&2
    printf 'actual JSON:\n%s\n' "$haystack" >&2
    printf 'Next: preserve stable version --json fields for packaged installs.\n' >&2
    exit 1
  fi
}

crate_version="$(cargo pkgid | sed 's/.*@//')"
package_dir="$ROOT_DIR/target/package/logbrew-cli-${crate_version}"
install_root="$tmp_dir/install"
install_target_dir="$ROOT_DIR/target/package-install-smoke"

cargo package --allow-dirty --locked >/dev/null

if [[ ! -d "$package_dir" ]]; then
  fail "expected cargo package to unpack ${package_dir}"
fi

cargo install \
  --path "$package_dir" \
  --locked \
  --root "$install_root" \
  --target-dir "$install_target_dir" \
  --force \
  --quiet

binary="$install_root/bin/logbrew"
if [[ ! -x "$binary" ]]; then
  fail "expected installed binary at ${binary}"
fi

human_output="$("$binary" version)"
if [[ "$human_output" != "logbrew ${crate_version}" ]]; then
  printf 'Package install smoke failed: expected human version output "logbrew %s"\n' "$crate_version" >&2
  printf 'actual output:\n%s\n' "$human_output" >&2
  printf 'Next: preserve short human version output for packaged installs.\n' >&2
  exit 1
fi

json_output="$("$binary" --json version)"
expect_contains "$json_output" '"ok":true'
expect_contains "$json_output" '"name":"logbrew"'
expect_contains "$json_output" '"binary":"native"'
expect_contains "$json_output" "\"version\":\"${crate_version}\""
expect_contains "$json_output" '"os":'
expect_contains "$json_output" '"arch":'

printf 'Package install smoke passed: logbrew %s\n' "$crate_version"
