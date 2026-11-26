#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

output_file="$(mktemp)"
stubs="$(mktemp -d)"
trap 'rm -rf "$output_file" "$stubs"' EXIT

expect_failure() {
  local path="$1" reason="$2" line
  shift 2
  if LOGBREW_CHECK_ALL_SELF_TEST=0 PATH="$path" "$BASH" scripts/check-all.sh >"$output_file" 2>&1; then
    printf 'expected check-all to fail when %s\n' "$reason" >&2
    cat "$output_file" >&2
    exit 1
  fi
  for line in "$@"; do
    grep -Fxq -- "$line" "$output_file" && continue
    printf 'expected check-all output to contain the line: %s\n' "$line" >&2
    cat "$output_file" >&2
    exit 1
  done
}

expect_failure "/usr/bin:/bin" "cargo-audit is missing" \
  "Check failed: missing required command 'cargo-audit'" \
  "Next: install cargo-audit with:" \
  "  cargo install cargo-audit --version $(bash scripts/cargo-audit-version.sh) --locked"

# Every tool check-all runs is a stub that passes.
for tool in bash cargo cargo-audit clippy-driver jq python3 ruby rustc; do
  printf '#!/bin/sh\n' >"$stubs/$tool"
done
chmod +x "$stubs"/*
if ! LOGBREW_CHECK_ALL_SELF_TEST=0 PATH="$stubs:/usr/bin:/bin" "$BASH" scripts/check-all.sh >"$output_file" 2>&1 ||
  grep -q '^Check failed: ' "$output_file"; then
  printf 'expected check-all to pass without a report when every step passes\n' >&2
  cat "$output_file" >&2
  exit 1
fi

cat >"$stubs/cargo" <<'STUB'
#!/bin/sh
case "$1" in
  test) echo 'error[E0308]: mismatched types' >&2; exit 101 ;;
  audit) echo 'error: 1 vulnerability found!'; exit 1 ;;
esac
STUB
expect_failure "$stubs:/usr/bin:/bin" "a cargo build and cargo audit fail" \
  "Check failed: rust-build (exit code 101). Last lines of its output:" \
  "  error[E0308]: mismatched types" \
  "Check failed: cargo-audit (exit code 1). Last lines of its output:" \
  "  error: 1 vulnerability found!"
if [[ "$(grep -c '^Check failed: ' "$output_file")" != 2 ]]; then
  printf 'expected check-all to report only the 2 failed steps\n' >&2
  cat "$output_file" >&2
  exit 1
fi
