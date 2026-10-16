#!/usr/bin/env bash
# Proves check-sdk-versions.sh passes on the current source and catches a seeded stale version.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
seeded="$(mktemp)"
trap 'rm -f "$seeded"' EXIT

bash "$ROOT_DIR/scripts/check-sdk-versions.sh" >/dev/null
sed -E 's/^(pub const JAVA_SDK_VERSION: &str = )"[^"]+";$/\1"0.0.1";/' "$ROOT_DIR/src/setup.rs" >"$seeded"
if output="$(LOGBREW_SETUP_SOURCE="$seeded" bash "$ROOT_DIR/scripts/check-sdk-versions.sh" 2>&1)"; then
  printf 'test-check-sdk-versions failed: a stale Java version passed\n' >&2
  exit 1
fi
[[ "$output" == *'JAVA_SDK_VERSION is "0.0.1"'* ]] || {
  printf 'test-check-sdk-versions failed: unexpected output:\n%s\n' "$output" >&2
  exit 1
}
printf 'test-check-sdk-versions: ok\n'
