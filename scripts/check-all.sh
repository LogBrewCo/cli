#!/usr/bin/env bash
set -euo pipefail

if [[ "${LOGBREW_CLIPPY_WRAPPER:-0}" == "1" ]]; then
  case " ${*:2} " in *" --test "*) exec "$@" ;; esac
  exec clippy-driver "$@"
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

for dependency in cargo-audit clippy-driver python3 ruby; do
  command -v "$dependency" >/dev/null 2>&1 && continue
  printf "Check failed: missing required command '%s'\n" "$dependency" >&2
  if [[ "$dependency" == cargo-audit ]]; then
    printf 'Next: install cargo-audit with:\n' >&2
    printf '  cargo install cargo-audit --version %s --locked\n' "$(bash scripts/cargo-audit-version.sh)" >&2
  else
    printf "Next: install '%s' so it is on PATH, then rerun bash scripts/check-all.sh.\n" "$dependency" >&2
  fi
  exit 1
done

cargo fmt --all -- --check
cargo fetch --locked
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
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}" cargo package --locked --allow-dirty --offline --no-verify &
package_pid=$!
LOGBREW_CLIPPY_WRAPPER=1 RUSTC_WORKSPACE_WRAPPER="$ROOT_DIR/scripts/check-all.sh" CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_CODEGEN_UNITS=512 cargo test --all-targets --all-features &
test_pid=$!
trap 'kill "$portable_checks_pid" "$audit_pid" "$package_pid" "$test_pid" 2>/dev/null || true' EXIT
python3 scripts/test-real-user-public-install-smoke.py
wait "$package_pid"
wait "$test_pid"
wait "$portable_checks_pid"
wait "$audit_pid"
trap - EXIT
