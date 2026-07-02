#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CARGO_AUDIT_VERSION="$(bash scripts/cargo-audit-version.sh)"

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf "Check failed: missing required command '%s'\n" "$1" >&2
    case "$1" in
      cargo-audit)
        printf 'Next: install cargo-audit with:\n' >&2
        printf '  cargo install cargo-audit --version %s --locked\n' "$CARGO_AUDIT_VERSION" >&2
        ;;
      *)
        printf "Next: install '%s' so it is on PATH, then rerun bash scripts/check-all.sh.\n" "$1" >&2
        ;;
    esac
    exit 1
  fi
}

check_cargo_audit_version() {
  local version_output
  local installed_version

  if ! version_output="$(cargo-audit --version)"; then
    printf 'Check failed: could not verify cargo-audit version\n' >&2
    printf 'Next: install cargo-audit with:\n' >&2
    printf '  cargo install cargo-audit --version %s --locked\n' "$CARGO_AUDIT_VERSION" >&2
    exit 1
  fi

  read -r _ installed_version _ <<<"$version_output"
  if [[ "$installed_version" != "$CARGO_AUDIT_VERSION" ]]; then
    printf 'Check failed: cargo-audit version %s does not match pinned %s\n' "$installed_version" "$CARGO_AUDIT_VERSION" >&2
    printf 'Next: install cargo-audit with:\n' >&2
    printf '  cargo install cargo-audit --version %s --locked\n' "$CARGO_AUDIT_VERSION" >&2
    exit 1
  fi
}

require_command cargo-audit
check_cargo_audit_version

bash scripts/confidentiality-check.sh
if [[ "${LOGBREW_CHECK_ALL_SELF_TEST:-1}" != "0" ]]; then
  bash scripts/test-check-all.sh
fi
bash scripts/test-package-contents.sh
bash scripts/test-release-preflight.sh
cargo audit
cargo fmt --all -- --check
cargo clippy --lib --bin logbrew --all-features -- -D warnings
cargo test --all-targets --all-features
cargo publish --dry-run --locked --allow-dirty
