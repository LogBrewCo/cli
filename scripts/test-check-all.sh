#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

output_file="$(mktemp)"
trap 'rm -f "$output_file"' EXIT

if LOGBREW_CHECK_ALL_SELF_TEST=0 PATH="/usr/bin:/bin" bash scripts/check-all.sh >"$output_file" 2>&1; then
  printf 'expected check-all to fail when cargo-audit is missing\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_lines=(
  "Check failed: missing required command 'cargo-audit'"
  "Next: install cargo-audit with:"
  "  cargo install cargo-audit --version 0.22.1 --locked"
)

for line in "${expected_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected check-all output to contain: %s\n' "$line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
done
