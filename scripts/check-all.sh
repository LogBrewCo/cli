#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CARGO_AUDIT_VERSION="$(bash scripts/cargo-audit-version.sh)"

for dependency in cargo-audit clippy-driver python3 ruby; do
  command -v "$dependency" >/dev/null 2>&1 && continue
  printf "Check failed: missing required command '%s'\n" "$dependency" >&2
  if [[ "$dependency" == cargo-audit ]]; then
    printf 'Next: install cargo-audit with:\n' >&2
    printf '  cargo install cargo-audit --version %s --locked\n' "$CARGO_AUDIT_VERSION" >&2
  else
    printf "Next: install '%s' so it is on PATH, then rerun bash scripts/check-all.sh.\n" "$dependency" >&2
  fi
  exit 1
done

(
bash scripts/confidentiality-check.sh
python3 scripts/brand_assets.py --check
python3 scripts/test-brand-assets.py
python3 scripts/check-github-hosted-runners.py
if [[ "${LOGBREW_CHECK_ALL_SELF_TEST:-1}" != "0" ]]; then
  bash scripts/test-check-all.sh
fi
bash scripts/test-package-contents.sh
ruby scripts/test-prepare-homebrew-formula.rb
bash scripts/test-release-preflight.sh
python3 scripts/test-cross-platform-auth-store.py
python3 scripts/test-installed-release-attestation.py
python3 scripts/test-installed-release-attestation-workflow.py
) &
portable_checks_pid=$!
audit_args=()
[[ -d "${CARGO_HOME:-$HOME/.cargo}/advisory-db/crates" ]] && audit_args+=(--no-fetch)
cargo audit "${audit_args[@]}" &
audit_pid=$!
trap 'kill "$portable_checks_pid" "$audit_pid" 2>/dev/null || true' EXIT
cargo fmt --all -- --check
rust_pids=()
RUSTC_WORKSPACE_WRAPPER="$(command -v clippy-driver)" CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}" cargo package --locked --allow-dirty --offline & rust_pids+=("$!")
trap 'kill "$portable_checks_pid" "$audit_pid" "${rust_pids[@]}" 2>/dev/null || true' EXIT
cargo test --all-targets --all-features & rust_pids+=("$!")
python3 scripts/test-real-user-public-install-smoke.py
for pid in "${rust_pids[@]}"; do
  wait "$pid"
done
wait "$portable_checks_pid"
wait "$audit_pid"
trap - EXIT
