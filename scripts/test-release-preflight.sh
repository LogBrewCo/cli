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

if [[ "${1:-}" == "publish" ]]; then
  if [[ "$*" != "publish --dry-run --locked" ]]; then
    printf 'unexpected cargo publish args: %s\n' "$*" >&2
    exit 1
  fi
  if [[ "${LOGBREW_TEST_PUBLISH_DRY_RUN:-pass}" == "fail" ]]; then
    printf 'test cargo publish dry-run failed\n' >&2
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
    if [[ "${LOGBREW_TEST_CRATES_PACKAGE:-exists}" == "missing" ]]; then
      printf '{}\n' >"$output_file"
      printf '404'
    else
      printf '{"versions":[]}\n' >"$output_file"
      printf '200'
    fi
    ;;
  https://registry.npmjs.org/logbrew-cli)
    if [[ "${LOGBREW_TEST_NPM_PACKAGE:-exists}" == "missing" ]]; then
      printf '{}\n' >"$output_file"
      printf '404'
    else
      printf '{"versions":{}}\n' >"$output_file"
      printf '200'
    fi
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

if [[ "${1:-}" == "--version" ]]; then
  printf 'cargo-audit %s\n' "${LOGBREW_TEST_AUDIT_VERSION:-0.22.2}"
  exit 0
fi

exit 0
STUB

cat >"$tmp_dir/package-install-smoke" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${LOGBREW_TEST_PACKAGE_INSTALL_SMOKE:-pass}" == "fail" ]]; then
  printf 'test package install smoke failed\n' >&2
  exit 1
fi

printf 'Package install smoke passed: test\n'
STUB

cat >"$tmp_dir/dist-plan" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != "v0.1.0" ]]; then
  printf 'unexpected dist plan tag: %s\n' "${1:-}" >&2
  exit 1
fi

if [[ "${LOGBREW_TEST_DIST_PLAN:-pass}" == "fail" ]]; then
  printf 'test dist plan failed\n' >&2
  exit 1
fi

printf 'Dist plan check passed: test\n'
STUB

cat >"$tmp_dir/dist-global-artifacts" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != "v0.1.0" ]]; then
  printf 'unexpected dist global artifact tag: %s\n' "${1:-}" >&2
  exit 1
fi

if [[ "${LOGBREW_TEST_DIST_GLOBAL_ARTIFACTS:-pass}" == "fail" ]]; then
  printf 'test dist global artifacts failed\n' >&2
  exit 1
fi

printf 'Dist global artifacts check passed: test\n'
STUB

cat >"$tmp_dir/dist-local-artifacts" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != "v0.1.0" ]]; then
  printf 'unexpected dist local artifact tag: %s\n' "${1:-}" >&2
  exit 1
fi

if [[ "${LOGBREW_TEST_DIST_LOCAL_ARTIFACTS:-pass}" == "fail" ]]; then
  printf 'test dist local artifacts failed\n' >&2
  exit 1
fi

printf 'Dist local artifacts check passed: test\n'
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
      printf 'HOMEBREW_TAP_TOKEN\n'
    else
      printf ''
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
  ls-tree)
    if [[ "${LOGBREW_TEST_TRACKED_LOCAL_ARTIFACT:-absent}" != "absent" ]]; then
      printf '%s\n' "$LOGBREW_TEST_TRACKED_LOCAL_ARTIFACT"
    fi
    exit 0
    ;;
esac

printf 'unexpected git args: %s\n' "$*" >&2
exit 1
STUB

chmod +x "$tmp_dir/cargo" "$tmp_dir/cargo-audit" "$tmp_dir/curl" "$tmp_dir/gh" "$tmp_dir/git" "$tmp_dir/package-install-smoke" "$tmp_dir/dist-plan" "$tmp_dir/dist-global-artifacts" "$tmp_dir/dist-local-artifacts"
export LOGBREW_RELEASE_PACKAGE_INSTALL_SMOKE_SCRIPT="$tmp_dir/package-install-smoke"
export LOGBREW_RELEASE_DIST_PLAN_SCRIPT="$tmp_dir/dist-plan"
export LOGBREW_RELEASE_DIST_GLOBAL_ARTIFACTS_SCRIPT="$tmp_dir/dist-global-artifacts"
export LOGBREW_RELEASE_DIST_LOCAL_ARTIFACTS_SCRIPT="$tmp_dir/dist-local-artifacts"

missing_audit_dir="$(mktemp -d)"
cp "$tmp_dir/cargo" "$tmp_dir/curl" "$tmp_dir/gh" "$tmp_dir/git" "$missing_audit_dir"
cargo_audit_version="$(bash scripts/cargo-audit-version.sh)"
contract_fixture="$tmp_dir/contract-fixture"

make_contract_fixture() {
  rm -rf "$contract_fixture"
  mkdir -p "$contract_fixture/.github/workflows"
  cp dist-workspace.toml "$contract_fixture/dist-workspace.toml"
  cp .github/workflows/release.yml "$contract_fixture/.github/workflows/release.yml"
  cp .github/workflows/publish-crates.yml "$contract_fixture/.github/workflows/publish-crates.yml"
  cp .github/workflows/publish-npm-trusted.yml "$contract_fixture/.github/workflows/publish-npm-trusted.yml"
  cp .github/workflows/publish-homebrew-tap.yml "$contract_fixture/.github/workflows/publish-homebrew-tap.yml"
}

remove_literal_line() {
  local file="$1"
  local text="$2"
  local temp_file="${file}.tmp"

  grep -Fv "$text" "$file" >"$temp_file"
  mv "$temp_file" "$file"
}

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

: >"$output_file"
if LOGBREW_TEST_AUDIT_VERSION=0.0.0 PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail when cargo-audit has the wrong version\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_wrong_audit_version_lines=(
  "Release preflight failed: cargo-audit version 0.0.0 does not match pinned ${cargo_audit_version}"
  "Next: install cargo-audit with:"
  "  cargo install cargo-audit --version ${cargo_audit_version} --locked"
  "Then rerun scripts/release-preflight.sh v0.1.0 before pushing a release tag."
)

for line in "${expected_wrong_audit_version_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected wrong cargo-audit version output to contain: %s\n' "$line" >&2
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
  "Release preflight failed: missing GitHub Actions secret names: HOMEBREW_TAP_TOKEN"
  "Next: add the missing repository secret names in GitHub Actions secrets before tagging:"
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
if LOGBREW_TEST_SECRETS=all LOGBREW_TEST_CRATES_PACKAGE=missing PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail when trusted crates.io bootstrap is missing\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Release preflight failed: crates.io package logbrew-cli does not exist yet; trusted publishing requires a first manual crate publish before CI release tags" "$output_file"; then
  printf 'expected release preflight to explain missing crates.io trusted bootstrap\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

: >"$output_file"
if LOGBREW_TEST_SECRETS=all LOGBREW_TEST_NPM_PACKAGE=missing PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail when trusted npm bootstrap is missing\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Release preflight failed: npm package logbrew-cli does not exist yet; trusted publishing requires a first manual package publish before CI release tags" "$output_file"; then
  printf 'expected release preflight to explain missing npm trusted bootstrap\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

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

make_contract_fixture
remove_literal_line "$contract_fixture/.github/workflows/publish-npm-trusted.yml" 'id-token: write'
: >"$output_file"
if LOGBREW_TEST_SECRETS=all LOGBREW_WORKFLOW_CONTRACT_ROOT="$contract_fixture" PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail with release workflow contract drift\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_workflow_contract_lines=(
  "Release workflow contract check failed: npm trusted publishing OIDC permission missing from .github/workflows/publish-npm-trusted.yml"
  "Release preflight failed: release workflow contract drift must be fixed before tagging"
)

for line in "${expected_workflow_contract_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected release workflow contract output to contain: %s\n' "$line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
done

: >"$output_file"
if LOGBREW_TEST_DIST_PLAN=fail LOGBREW_TEST_SECRETS=all PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail when dist plan check fails\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_dist_plan_lines=(
  "test dist plan failed"
  "Release preflight failed: cargo-dist release plan failed"
  "Next: fix cargo-dist release config, then rerun bash scripts/test-dist-plan.sh and scripts/release-preflight.sh v0.1.0 before tagging."
)

for line in "${expected_dist_plan_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected dist plan output to contain: %s\n' "$line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
done

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
if LOGBREW_TEST_SECRETS=all LOGBREW_TEST_PUBLISH_DRY_RUN=fail PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail when cargo publish dry-run fails\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_publish_dry_run_lines=(
  "Release preflight failed: cargo publish dry-run failed"
  "Next: fix package metadata or crate publish blockers, then rerun scripts/release-preflight.sh v0.1.0 before tagging."
)

for line in "${expected_publish_dry_run_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected cargo publish dry-run output to contain: %s\n' "$line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
done

: >"$output_file"
if LOGBREW_TEST_SECRETS=all LOGBREW_TEST_PACKAGE_INSTALL_SMOKE=fail PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail when package install smoke fails\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_package_install_lines=(
  "test package install smoke failed"
  "Release preflight failed: package install smoke failed"
  "Next: fix the packaged crate install path, then rerun bash scripts/test-package-install-smoke.sh and scripts/release-preflight.sh v0.1.0 before tagging."
)

for line in "${expected_package_install_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected package install smoke output to contain: %s\n' "$line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
done

: >"$output_file"
if LOGBREW_TEST_SECRETS=all LOGBREW_TEST_DIST_GLOBAL_ARTIFACTS=fail PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail when dist global artifacts check fails\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_global_artifacts_lines=(
  "test dist global artifacts failed"
  "Release preflight failed: cargo-dist global artifact build failed"
  "Next: fix cargo-dist global installers or package metadata, then rerun bash scripts/test-dist-global-artifacts.sh and scripts/release-preflight.sh v0.1.0 before tagging."
)

for line in "${expected_global_artifacts_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected dist global artifacts output to contain: %s\n' "$line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
done

: >"$output_file"
if LOGBREW_TEST_SECRETS=all LOGBREW_TEST_DIST_LOCAL_ARTIFACTS=fail PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail when dist local artifacts check fails\n' >&2
  cat "$output_file" >&2
  exit 1
fi

expected_local_artifacts_lines=(
  "test dist local artifacts failed"
  "Release preflight failed: cargo-dist host native artifact build failed"
  "Next: fix cargo-dist native archive generation, then rerun bash scripts/test-dist-local-artifacts.sh and scripts/release-preflight.sh v0.1.0 before tagging."
)

for line in "${expected_local_artifacts_lines[@]}"; do
  if ! grep -Fq "$line" "$output_file"; then
    printf 'expected dist local artifacts output to contain: %s\n' "$line" >&2
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
if LOGBREW_TEST_SECRETS=all LOGBREW_TEST_TRACKED_LOCAL_ARTIFACT=AGENTS.md PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail when a local-only agent artifact is tracked\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Release preflight failed: tracked local-only release-blocked path AGENTS.md" "$output_file"; then
  printf 'expected release preflight to explain tracked local-only artifact\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

: >"$output_file"
if LOGBREW_TEST_SECRETS=all LOGBREW_TEST_TRACKED_LOCAL_ARTIFACT=docs/AGENTS.md PATH="$tmp_dir:$PATH" bash scripts/release-preflight.sh v0.1.0 >"$output_file" 2>&1; then
  printf 'expected release preflight to fail when a nested local-only agent artifact is tracked\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Release preflight failed: tracked local-only release-blocked path docs/AGENTS.md" "$output_file"; then
  printf 'expected release preflight to explain nested tracked local-only artifact\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

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
