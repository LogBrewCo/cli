#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
output_file="$(mktemp)"
trap 'rm -rf "$tmp_dir" "$output_file"' EXIT

cargo_audit_version="$(bash scripts/cargo-audit-version.sh)"

cat >"$tmp_dir/cargo-audit" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "--version" ]]; then
  printf 'cargo-audit 0.0.0\n'
  exit 0
fi

exit 0
STUB

chmod +x "$tmp_dir/cargo-audit"

if LOGBREW_CHECK_ALL_SELF_TEST=0 PATH="/usr/bin:/bin" bash scripts/check-all.sh >"$output_file" 2>&1; then
  printf 'expected check-all to fail when cargo-audit is missing\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_lines=(
  "Check failed: missing required command 'cargo-audit'"
  "Next: install cargo-audit with:"
  "  cargo install cargo-audit --version ${cargo_audit_version} --locked"
)

for line in "${expected_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected check-all output to contain: %s\n' "$line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
done

: >"$output_file"
if LOGBREW_CHECK_ALL_SELF_TEST=0 PATH="$tmp_dir:/usr/bin:/bin" bash scripts/check-all.sh >"$output_file" 2>&1; then
  printf 'expected check-all to fail when cargo-audit has the wrong version\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_wrong_version_lines=(
  "Check failed: cargo-audit version 0.0.0 does not match pinned ${cargo_audit_version}"
  "Next: install cargo-audit with:"
  "  cargo install cargo-audit --version ${cargo_audit_version} --locked"
)

for line in "${expected_wrong_version_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected wrong cargo-audit version output to contain: %s\n' "$line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
done
