#!/usr/bin/env bash
set -euo pipefail

if [[ "${LOGBREW_CLIPPY_WRAPPER:-0}" == "1" ]]; then
  exec clippy-driver "$@"
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

for dependency in cargo-audit clippy-driver jq python3 ruby; do
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

# Steps run in parallel, so their output interleaves. Each step keeps its own log, and the run ends
# with the name, exit code, and last output lines of every step that failed.
logs="$(mktemp -d)"
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$logs"' EXIT
# Call it as `STEP 2>&1 | tee "$logs/NAME" || failed NAME`.
failed() (
  status=$?
  report="$(printf '\nCheck failed: %s (exit code %s). Last lines of its output:\n' "$1" "$status"; tail -n 20 "$logs/$1" | sed 's/^/  /')"
  printf '%s\n' "$report" >>"$logs/failed"
)

test_rustflags="${RUSTFLAGS:+$RUSTFLAGS }-C prefer-dynamic -C link-arg=-Wl,-rpath,$(rustc --print target-libdir)"
[[ "$(uname -s)" == "Darwin" ]] && test_rustflags+=" -C link-arg=-Wl,-no_deduplicate"
rust_checks() {
  { LOGBREW_CLIPPY_WRAPPER=1 RUSTC_WORKSPACE_WRAPPER="$ROOT_DIR/scripts/check-all.sh" RUSTFLAGS="$test_rustflags" CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_CODEGEN_UNITS=512 cargo test --quiet --lib --tests --examples --all-features --no-run --message-format=json 2>&1 >"$logs/test-manifest"; } | tee "$logs/rust-build" || failed rust-build
  jq -j 'select(.reason == "compiler-artifact" and .profile.test == true and .executable != null) | .executable, "\u0000"' "$logs/test-manifest" |
    xargs -0 -r -n 1 -P 2 sh -c 'exec "$1" --quiet' sh 2>&1 | tee "$logs/rust-tests" || failed rust-tests &
  audit_args=()
  [[ -d "${CARGO_HOME:-$HOME/.cargo}/advisory-db/crates" ]] && audit_args+=(--no-fetch)
  cargo audit "${audit_args[@]}" 2>&1 | tee "$logs/cargo-audit" || failed cargo-audit &
  CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}" cargo package --locked --allow-dirty --offline --no-verify 2>&1 | tee "$logs/cargo-package" || failed cargo-package &
  wait
}
rust_checks &
(
bash scripts/check-tool-matrix.sh 2>&1 | tee "$logs/check-tool-matrix" || failed check-tool-matrix
bash scripts/test-check-tool-matrix.sh 2>&1 | tee "$logs/test-check-tool-matrix" || failed test-check-tool-matrix
bash scripts/check-suppressions.sh 2>&1 | tee "$logs/check-suppressions" || failed check-suppressions
bash scripts/test-check-suppressions.sh 2>&1 | tee "$logs/test-check-suppressions" || failed test-check-suppressions
bash scripts/check-dependabot.sh 2>&1 | tee "$logs/check-dependabot" || failed check-dependabot
bash scripts/test-check-dependabot.sh 2>&1 | tee "$logs/test-check-dependabot" || failed test-check-dependabot
cargo fmt --all -- --check 2>&1 | tee "$logs/cargo-fmt" || failed cargo-fmt
if [[ "${LOGBREW_CHECK_ALL_SELF_TEST:-1}" != "0" ]]; then
  bash scripts/test-check-all.sh 2>&1 | tee "$logs/test-check-all" || failed test-check-all
fi
ruby scripts/test-prepare-homebrew-formula.rb 2>&1 | tee "$logs/test-prepare-homebrew-formula" || failed test-prepare-homebrew-formula
bash scripts/test-release-preflight.sh 2>&1 | tee "$logs/test-release-preflight" || failed test-release-preflight
bash scripts/test-install-dist.sh 2>&1 | tee "$logs/test-install-dist" || failed test-install-dist
python3 scripts/test-installed-release-attestation.py 2>&1 | tee "$logs/test-installed-release-attestation" || failed test-installed-release-attestation
python3 scripts/test-installed-release-attestation-workflow.py 2>&1 | tee "$logs/test-installed-release-attestation-workflow" || failed test-installed-release-attestation-workflow
) &
python3 scripts/test-real-user-public-install-smoke.py 2>&1 | tee "$logs/test-real-user-public-install-smoke" || failed test-real-user-public-install-smoke
wait
if [[ -s "$logs/failed" ]]; then
  cat "$logs/failed" >&2
  exit 1
fi
