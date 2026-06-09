#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
output_file="$(mktemp)"
missing_audit_dir=""
trap 'rm -rf "$tmp_dir" "$missing_audit_dir" "$output_file"' EXIT

cat >"$tmp_dir/cargo" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "metadata" ]]; then
  printf '{"packages":[{"name":"logbrew-cli","version":"0.1.0"}]}\n'
  exit 0
fi

if [[ "${1:-}" == "audit" ]]; then
  if [[ "${LOGBREW_TEST_AUDIT:-pass}" == "fail" ]]; then
    printf 'test advisory found\n' >&2
    exit 1
  fi
  exit 0
fi

printf 'unexpected cargo args: %s\n' "$*" >&2
exit 1
STUB

cat >"$tmp_dir/curl" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

output_file=""
url=""

while (( $# > 0 )); do
  case "$1" in
    --output)
      output_file="$2"
      shift 2
      ;;
    --write-out | --header)
      shift 2
      ;;
    --silent | --show-error | --location)
      shift
      ;;
    *)
      url="$1"
      shift
      ;;
  esac
done

case "$url" in
  https://crates.io/api/v1/crates/logbrew-cli)
    printf '{"versions":[]}\n' >"$output_file"
    printf '200'
    ;;
  https://registry.npmjs.org/logbrew-cli)
    printf '{"versions":{}}\n' >"$output_file"
    printf '200'
    ;;
  *)
    printf 'unexpected curl url: %s\n' "$url" >&2
    exit 1
    ;;
esac
STUB

cat >"$tmp_dir/cargo-audit" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

exit 0
STUB

cat >"$tmp_dir/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

case "${1:-} ${2:-}" in
  "auth status")
    exit 0
    ;;
  "release view")
    exit 1
    ;;
  "repo view")
    printf '{"defaultBranchRef":{"name":"main"},"isPrivate":false,"nameWithOwner":"LogBrewCo/homebrew-tap","url":"https://github.com/LogBrewCo/homebrew-tap"}\n'
    ;;
  "api repos/LogBrewCo/cli/branches/main/protection")
    if [[ "${LOGBREW_TEST_PROTECTION:-ok}" == "missing-plan" ]]; then
      printf '{"required_pull_request_reviews":{"required_approving_review_count":1,"dismiss_stale_reviews":true},"enforce_admins":{"enabled":true},"required_status_checks":{"strict":true,"checks":[{"context":"check"}],"contexts":["check"]}}\n'
    else
      printf '{"required_pull_request_reviews":{"required_approving_review_count":1,"dismiss_stale_reviews":true},"enforce_admins":{"enabled":true},"required_status_checks":{"strict":true,"checks":[{"context":"check"},{"context":"plan"}],"contexts":["check","plan"]}}\n'
    fi
    ;;
  "api repos/LogBrewCo/cli/actions/workflows")
    case "${LOGBREW_TEST_WORKFLOWS:-active}" in
      active)
        printf '{"workflows":[{"name":"CI","state":"active"},{"name":"Release","state":"active"},{"name":"Publish crates.io","state":"active"}]}\n'
        ;;
      missing-release)
        printf '{"workflows":[{"name":"CI","state":"active"},{"name":"Publish crates.io","state":"active"}]}\n'
        ;;
      disabled-crates)
        printf '{"workflows":[{"name":"CI","state":"active"},{"name":"Release","state":"active"},{"name":"Publish crates.io","state":"disabled_manually"}]}\n'
        ;;
      *)
        printf 'unexpected LOGBREW_TEST_WORKFLOWS: %s\n' "${LOGBREW_TEST_WORKFLOWS}" >&2
        exit 1
        ;;
    esac
    ;;
  "secret list")
    if [[ "${LOGBREW_TEST_SECRETS:-partial}" == "all" ]]; then
      printf 'CARGO_REGISTRY_TOKEN\nNPM_TOKEN\nHOMEBREW_TAP_TOKEN\n'
    else
      printf 'CARGO_REGISTRY_TOKEN\n'
    fi
    ;;
  "run list")
    case "${LOGBREW_TEST_CI:-green}" in
      missing)
        ;;
      stale)
        printf '{"conclusion":"success","headSha":"old123","status":"completed","url":"https://github.com/LogBrewCo/cli/actions/runs/1"}\n'
        ;;
      running)
        printf '{"conclusion":"","headSha":"abc123","status":"in_progress","url":"https://github.com/LogBrewCo/cli/actions/runs/1"}\n'
        ;;
      failed)
        printf '{"conclusion":"failure","headSha":"abc123","status":"completed","url":"https://github.com/LogBrewCo/cli/actions/runs/1"}\n'
        ;;
      green)
        printf '{"conclusion":"success","headSha":"abc123","status":"completed","url":"https://github.com/LogBrewCo/cli/actions/runs/1"}\n'
        ;;
      *)
        printf 'unexpected LOGBREW_TEST_CI: %s\n' "${LOGBREW_TEST_CI}" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    printf 'unexpected gh args: %s\n' "$*" >&2
    exit 1
    ;;
esac
STUB

cat >"$tmp_dir/git" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

case "${1:-}" in
  fetch | diff)
    exit 0
    ;;
  branch)
    if [[ "${2:-}" == "--show-current" ]]; then
      printf 'main\n'
      exit 0
    fi
    ;;
  rev-parse)
    if [[ "${2:-}" == "HEAD" || "${2:-}" == "origin/main" ]]; then
      printf 'abc123\n'
      exit 0
    fi
    if [[ "${2:-}" == "-q" && "${3:-}" == "--verify" ]]; then
      exit 1
    fi
    ;;
  ls-remote)
    exit 2
    ;;
esac

printf 'unexpected git args: %s\n' "$*" >&2
exit 1
STUB

chmod +x "$tmp_dir/cargo" "$tmp_dir/cargo-audit" "$tmp_dir/curl" "$tmp_dir/gh" "$tmp_dir/git"

missing_audit_dir="$(mktemp -d)"
cp "$tmp_dir/cargo" "$tmp_dir/curl" "$tmp_dir/gh" "$tmp_dir/git" "$missing_audit_dir"
cargo_audit_version="$(bash scripts/cargo-audit-version.sh)"

: >"$output_file"
if PATH="$missing_audit_dir:/usr/bin:/bin" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail when cargo-audit is missing\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_missing_audit_lines=(
  "Release preflight failed: missing required command 'cargo-audit'"
  "Next: install cargo-audit with:"
  "  cargo install cargo-audit --version ${cargo_audit_version} --locked"
  "Then rerun scripts/release-preflight.sh v0.1.0 before pushing a release tag."
)

for line in "${expected_missing_audit_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected missing cargo-audit output to contain: %s\n' "$line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
done

if PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail with missing secrets\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_lines=(
  "Release preflight failed: missing GitHub Actions secret names: NPM_TOKEN HOMEBREW_TAP_TOKEN"
  "Next: add the missing repository secret names in GitHub Actions secrets before tagging:"
  "gh secret set NPM_TOKEN --repo LogBrewCo/cli --body '<token-value>'"
  "gh secret set HOMEBREW_TAP_TOKEN --repo LogBrewCo/cli --body '<token-value>'"
)

for line in "${expected_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected release preflight output to contain: %s\n' "$line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
done

: >"$output_file"
if LOGBREW_TEST_PROTECTION=missing-plan PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail with missing branch protection check\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Release preflight failed: main branch protection must require status check plan" "$output_file"; then
  printf 'expected release preflight to explain the missing plan protection check\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

: >"$output_file"
if LOGBREW_TEST_WORKFLOWS=missing-release PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail with missing release workflow\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Release preflight failed: GitHub Actions workflow Release is missing" "$output_file"; then
  printf 'expected release preflight to explain the missing Release workflow\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

: >"$output_file"
if LOGBREW_TEST_WORKFLOWS=disabled-crates PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail with disabled crates publishing workflow\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Release preflight failed: GitHub Actions workflow Publish crates.io is not active" "$output_file"; then
  printf 'expected release preflight to explain the disabled crates publishing workflow\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

: >"$output_file"
if LOGBREW_TEST_AUDIT=fail PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail when cargo audit fails\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_audit_lines=(
  "Release preflight failed: cargo audit found RustSec advisories or could not complete"
  "Next: review cargo audit output, update affected dependencies, then rerun scripts/release-preflight.sh v0.1.0 before tagging."
)

for line in "${expected_audit_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected cargo audit output to contain: %s\n' "$line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
done

: >"$output_file"
if LOGBREW_TEST_SECRETS=all LOGBREW_TEST_CI=stale PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail with stale main CI\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_ci_lines=(
  "Release preflight failed: latest main CI is not green for abc123; latest run: https://github.com/LogBrewCo/cli/actions/runs/1"
  "Next: wait for main CI to pass on abc123, rerun failed checks if needed, then rerun scripts/release-preflight.sh v0.1.0 before tagging."
)

for line in "${expected_ci_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected stale CI output to contain: %s\n' "$line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
done

: >"$output_file"
if ! LOGBREW_TEST_SECRETS=all PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to pass when all gates are satisfied\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Release preflight passed for v0.1.0 (abc123)." "$output_file"; then
  printf 'expected release preflight success output\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi
