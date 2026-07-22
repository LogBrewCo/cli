#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPO="${LOGBREW_RELEASE_REPO:-LogBrewCo/cli}"
HOMEBREW_TAP_REPO="${LOGBREW_HOMEBREW_TAP_REPO:-LogBrewCo/homebrew-tap}"
TAG="${1:-}"
REQUIRED_SECRETS=(
  HOMEBREW_TAP_TOKEN
)
REQUIRED_STATUS_CHECKS=(
  check
  plan
)
REQUIRED_WORKFLOWS=(
  CI
  Release
  "Publish crates.io"
)
CARGO_AUDIT_VERSION="$(bash scripts/cargo-audit-version.sh)"

fail() {
  printf 'Release preflight failed: %s\n' "$1" >&2
  printf 'Next: fix the issue, then rerun %s %s before pushing a release tag.\n' "$0" "${TAG:-v<version>}" >&2
  exit 1
}

fail_missing_secrets() {
  printf 'Release preflight failed: missing GitHub Actions secret names: %s\n' "$*" >&2
  printf 'Next: add the missing repository secret names in GitHub Actions secrets before tagging:\n' >&2
  for secret in "$@"; do
    printf "  gh secret set %s --repo %s --body '<token-value>'\n" "$secret" "$REPO" >&2
  done
  printf 'Then rerun %s %s before pushing a release tag.\n' "$0" "${TAG:-v<version>}" >&2
  exit 1
}

fail_ci_not_green() {
  local head="$1"
  local run_url="$2"

  printf 'Release preflight failed: latest main CI is not green for %s; latest run: %s\n' "$head" "$run_url" >&2
  printf 'Next: wait for main CI to pass on %s, rerun failed checks if needed, then rerun %s %s before tagging.\n' "$head" "$0" "$TAG" >&2
  exit 1
}

fail_missing_ci() {
  printf 'Release preflight failed: could not find a main CI run\n' >&2
  printf 'Next: push main or rerun CI, wait for a successful main CI run, then rerun %s %s before tagging.\n' "$0" "$TAG" >&2
  exit 1
}

fail_audit() {
  printf 'Release preflight failed: cargo audit found RustSec advisories or could not complete\n' >&2
  printf 'Next: review cargo audit output, update affected dependencies, then rerun %s %s before tagging.\n' "$0" "$TAG" >&2
  exit 1
}

fail_missing_command() {
  local command_name="$1"

  printf "Release preflight failed: missing required command '%s'\n" "$command_name" >&2
  case "$command_name" in
    cargo-audit)
      printf 'Next: install cargo-audit with:\n' >&2
      printf '  cargo install cargo-audit --version %s --locked\n' "$CARGO_AUDIT_VERSION" >&2
      printf 'Then rerun %s %s before pushing a release tag.\n' "$0" "${TAG:-v<version>}" >&2
      ;;
    gh)
      printf 'Next: install GitHub CLI, authenticate with gh auth login, then rerun %s %s before pushing a release tag.\n' "$0" "${TAG:-v<version>}" >&2
      ;;
    *)
      printf "Next: install '%s' so it is on PATH, then rerun %s %s before pushing a release tag.\n" "$command_name" "$0" "${TAG:-v<version>}" >&2
      ;;
  esac
  exit 1
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    fail_missing_command "$1"
  fi
}

require_command cargo
require_command curl
require_command gh
require_command git
require_command jq
require_command cargo-audit

http_json_status() {
  local url="$1"
  local output_file="$2"

  curl \
    --silent \
    --show-error \
    --location \
    --header 'User-Agent: logbrew-release-preflight' \
    --output "$output_file" \
    --write-out '%{http_code}' \
    "$url"
}

check_crates_version_available() {
  local crate_name="$1"
  local version="$2"
  local response_file="$3"
  local status

  if ! status="$(http_json_status "https://crates.io/api/v1/crates/${crate_name}" "$response_file")"; then
    fail "could not verify crates.io package ${crate_name}; registry request failed"
  fi
  case "$status" in
    200)
      if jq -e --arg version "$version" '.versions[] | select(.num == $version)' "$response_file" >/dev/null; then
        fail "crates.io package ${crate_name} already has version ${version}"
      fi
      ;;
    404)
      fail "crates.io package ${crate_name} does not exist yet; trusted publishing requires a first manual crate publish before CI release tags"
      ;;
    *)
      fail "could not verify crates.io package ${crate_name}; registry returned HTTP ${status}"
      ;;
  esac
}

check_npm_version_available() {
  local package_name="$1"
  local version="$2"
  local response_file="$3"
  local status

  if ! status="$(http_json_status "https://registry.npmjs.org/${package_name}" "$response_file")"; then
    fail "could not verify npm package ${package_name}; registry request failed"
  fi
  case "$status" in
    200)
      if jq -e --arg version "$version" '.versions | has($version)' "$response_file" >/dev/null; then
        fail "npm package ${package_name} already has version ${version}"
      fi
      ;;
    404)
      fail "npm package ${package_name} does not exist yet; trusted publishing requires a first manual package publish before CI release tags"
      ;;
    *)
      fail "could not verify npm package ${package_name}; registry returned HTTP ${status}"
      ;;
  esac
}

check_homebrew_tap_available() {
  local tap_repo="$1"
  local metadata
  local is_private
  local default_branch

  if ! metadata="$(
    gh repo view "$tap_repo" --json defaultBranchRef,isPrivate,nameWithOwner,url
  )"; then
    fail "could not verify Homebrew tap repository ${tap_repo}"
  fi

  is_private="$(jq -r '.isPrivate' <<<"$metadata")"
  default_branch="$(jq -r '.defaultBranchRef.name // ""' <<<"$metadata")"

  if [[ "$is_private" != "false" ]]; then
    fail "Homebrew tap repository ${tap_repo} is not public"
  fi

  if [[ -z "$default_branch" ]]; then
    fail "Homebrew tap repository ${tap_repo} has no default branch"
  fi
}

check_main_branch_protection() {
  local metadata
  local required_reviews
  local require_last_push_approval
  local enforce_admins
  local strict_status_checks
  local status_checks
  local conversation_resolution
  local allow_force_pushes
  local allow_deletions
  local linear_history

  if ! metadata="$(gh api "repos/${REPO}/branches/main/protection")"; then
    fail "could not verify main branch protection"
  fi

  required_reviews="$(
    jq -r '.required_pull_request_reviews.required_approving_review_count // 0' <<<"$metadata"
  )"
  require_last_push_approval="$(
    jq -r '.required_pull_request_reviews.require_last_push_approval // false' <<<"$metadata"
  )"
  enforce_admins="$(jq -r '.enforce_admins.enabled // false' <<<"$metadata")"
  strict_status_checks="$(jq -r '.required_status_checks.strict // false' <<<"$metadata")"
  conversation_resolution="$(
    jq -r '.required_conversation_resolution.enabled // false' <<<"$metadata"
  )"
  allow_force_pushes="$(jq -r '.allow_force_pushes.enabled // false' <<<"$metadata")"
  allow_deletions="$(jq -r '.allow_deletions.enabled // false' <<<"$metadata")"
  linear_history="$(jq -r '.required_linear_history.enabled // false' <<<"$metadata")"
  status_checks="$(
    jq -r '[.required_status_checks.checks[]?.context, .required_status_checks.contexts[]?] | unique[]' \
      <<<"$metadata"
  )"

  if (( required_reviews != 0 )); then
    fail "main branch protection must use solo-maintainer review policy"
  fi

  if [[ "$require_last_push_approval" != "false" ]]; then
    fail "main branch protection must not require a separate latest-push approver"
  fi

  if [[ "$enforce_admins" != "true" ]]; then
    fail "main branch protection must enforce admins"
  fi

  if [[ "$strict_status_checks" != "true" ]]; then
    fail "main branch protection must require strict status checks"
  fi

  if [[ "$conversation_resolution" != "true" ]]; then
    fail "main branch protection must require conversation resolution"
  fi

  if [[ "$allow_force_pushes" != "false" ]]; then
    fail "main branch protection must prohibit force pushes"
  fi

  if [[ "$allow_deletions" != "false" ]]; then
    fail "main branch protection must prohibit branch deletion"
  fi

  if [[ "$linear_history" != "true" ]]; then
    fail "main branch protection must require linear history"
  fi

  for check in "${REQUIRED_STATUS_CHECKS[@]}"; do
    if ! grep -Fxq "$check" <<<"$status_checks"; then
      fail "main branch protection must require status check ${check}"
    fi
  done
}

check_required_workflows_active() {
  local metadata
  local workflow
  local state

  if ! metadata="$(gh api "repos/${REPO}/actions/workflows")"; then
    fail "could not verify GitHub Actions workflows"
  fi

  for workflow in "${REQUIRED_WORKFLOWS[@]}"; do
    state="$(
      jq -r --arg workflow "$workflow" \
        '[.workflows[]? | select(.name == $workflow) | .state][0] // ""' \
        <<<"$metadata"
    )"

    if [[ -z "$state" ]]; then
      fail "GitHub Actions workflow ${workflow} is missing"
    fi

    if [[ "$state" != "active" ]]; then
      fail "GitHub Actions workflow ${workflow} is not active"
    fi
  done
}

check_dependency_advisories() {
  if ! cargo audit; then
    fail_audit
  fi
}

crate_version="$(
  cargo metadata --no-deps --format-version=1 |
    jq -r '.packages[] | select(.name == "logbrew-cli").version'
)"

if [[ -z "$crate_version" || "$crate_version" == "null" ]]; then
  fail "could not read logbrew-cli version from Cargo metadata"
fi

if [[ -z "$TAG" ]]; then
  TAG="v${crate_version}"
fi

tag_version="${TAG#v}"
if [[ "$tag_version" != "$crate_version" ]]; then
  fail "tag ${TAG} does not match Cargo.toml version ${crate_version}"
fi

if ! gh auth status >/dev/null 2>&1; then
  fail "GitHub CLI is not authenticated"
fi

git fetch origin main --tags --prune >/dev/null

branch="$(git branch --show-current)"
if [[ "$branch" != "main" ]]; then
  fail "release must be prepared from main, not ${branch}"
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
  fail "worktree has uncommitted changes"
fi

local_head="$(git rev-parse HEAD)"
remote_head="$(git rev-parse origin/main)"
if [[ "$local_head" != "$remote_head" ]]; then
  fail "local main is not synced with origin/main"
fi

if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  fail "local tag ${TAG} already exists"
fi

if git ls-remote --exit-code --tags origin "refs/tags/${TAG}" >/dev/null 2>&1; then
  fail "remote tag ${TAG} already exists"
fi

if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
  fail "GitHub Release ${TAG} already exists"
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

check_crates_version_available "logbrew-cli" "$crate_version" "$tmp_dir/crates.json"
check_npm_version_available "logbrew-cli" "$crate_version" "$tmp_dir/npm.json"
check_homebrew_tap_available "$HOMEBREW_TAP_REPO"
check_main_branch_protection
check_required_workflows_active
check_dependency_advisories

secret_names="$(
  gh secret list --repo "$REPO" --app actions --json name --jq '.[].name'
)"
missing_secrets=()
for secret in "${REQUIRED_SECRETS[@]}"; do
  if ! grep -Fxq "$secret" <<<"$secret_names"; then
    missing_secrets+=("$secret")
  fi
done

if (( ${#missing_secrets[@]} > 0 )); then
  fail_missing_secrets "${missing_secrets[@]}"
fi

ci_run="$(
  gh run list \
    --repo "$REPO" \
    --workflow CI \
    --branch main \
    --limit 1 \
    --json conclusion,headSha,status,url \
    --jq '.[0] // empty'
)"

if [[ -z "$ci_run" ]]; then
  fail_missing_ci
fi

ci_head="$(jq -r '.headSha' <<<"$ci_run")"
ci_status="$(jq -r '.status' <<<"$ci_run")"
ci_conclusion="$(jq -r '.conclusion' <<<"$ci_run")"
ci_url="$(jq -r '.url' <<<"$ci_run")"

if [[ "$ci_head" != "$local_head" || "$ci_status" != "completed" || "$ci_conclusion" != "success" ]]; then
  fail_ci_not_green "$local_head" "$ci_url"
fi

printf 'Release preflight passed for %s (%s).\n' "$TAG" "$local_head"
printf 'Next: run bash scripts/pre-commit.sh, then push the release tag when ready.\n'
