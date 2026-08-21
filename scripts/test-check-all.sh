#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

output_file="$(mktemp)"
trap 'rm -f "$output_file"' EXIT

cargo_audit_version="$(bash scripts/cargo-audit-version.sh)"

if LOGBREW_CHECK_ALL_SELF_TEST=0 PATH="/usr/bin:/bin" bash scripts/check-all.sh >"$output_file" 2>&1; then
  printf 'expected check-all to fail when cargo-audit is missing\n' >&2
  cat "$output_file" >&2
  exit 1
fi

for line in \
  "Check failed: missing required command 'cargo-audit'" \
  "Next: install cargo-audit with:" \
  "  cargo install cargo-audit --version ${cargo_audit_version} --locked"; do
  grep -Fq "$line" "$output_file" && continue
  printf 'expected check-all output to contain: %s\n' "$line" >&2
  cat "$output_file" >&2
  exit 1
done
