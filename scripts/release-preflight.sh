#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPO="${LOGBREW_RELEASE_REPO:-LogBrewCo/cli}"
TAG="${1:-}"
REQUIRED_SECRETS=(
  CARGO_REGISTRY_TOKEN
  NPM_TOKEN
  HOMEBREW_TAP_TOKEN
)

fail() {
  printf 'Release preflight failed: %s\n' "$1" >&2
  printf 'Next: fix the issue, then rerun %s %s before pushing a release tag.\n' "$0" "${TAG:-v<version>}" >&2
  exit 1
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    fail "missing required command '$1'"
  fi
}

require_command cargo
require_command gh
require_command git
require_command jq

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
  fail "missing GitHub Actions secret names: ${missing_secrets[*]}"
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
  fail "could not find a main CI run"
fi

ci_head="$(jq -r '.headSha' <<<"$ci_run")"
ci_status="$(jq -r '.status' <<<"$ci_run")"
ci_conclusion="$(jq -r '.conclusion' <<<"$ci_run")"
ci_url="$(jq -r '.url' <<<"$ci_run")"

if [[ "$ci_head" != "$local_head" || "$ci_status" != "completed" || "$ci_conclusion" != "success" ]]; then
  fail "latest main CI is not green for ${local_head}; latest run: ${ci_url}"
fi

printf 'Release preflight passed for %s (%s).\n' "$TAG" "$local_head"
printf 'Next: run bash scripts/pre-commit.sh, then push the release tag when ready.\n'
