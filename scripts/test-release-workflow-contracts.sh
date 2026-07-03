#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTRACT_ROOT="${LOGBREW_WORKFLOW_CONTRACT_ROOT:-$ROOT_DIR}"

fail() {
  printf 'Release workflow contract check failed: %s\n' "$1" >&2
  printf 'Next: restore the release workflow contract, then rerun bash scripts/test-release-workflow-contracts.sh.\n' >&2
  exit 1
}

require_line() {
  local file="$1"
  local pattern="$2"
  local description="$3"

  if ! grep -Eq "$pattern" "$file"; then
    fail "${description} missing from ${file}"
  fi
}

require_literal() {
  local file="$1"
  local text="$2"
  local description="$3"

  if ! grep -Fq "$text" "$file"; then
    fail "${description} missing from ${file}"
  fi
}

cd "$CONTRACT_ROOT"

release_workflow=".github/workflows/release.yml"
crates_workflow=".github/workflows/publish-crates.yml"
npm_workflow=".github/workflows/publish-npm-trusted.yml"
homebrew_workflow=".github/workflows/publish-homebrew-tap.yml"
dist_config="dist-workspace.toml"

for file in "$release_workflow" "$crates_workflow" "$npm_workflow" "$homebrew_workflow" "$dist_config"; do
  if [[ ! -f "$file" ]]; then
    fail "required release file ${file} is missing"
  fi
done

require_literal "$dist_config" 'installers = ["shell", "powershell", "npm", "homebrew", "msi"]' \
  "cargo-dist installer set"
require_literal "$dist_config" 'publish-jobs = ["./publish-npm-trusted", "./publish-homebrew-tap"]' \
  "cargo-dist custom publish jobs"
require_literal "$dist_config" 'tap = "LogBrewCo/homebrew-tap"' \
  "public Homebrew tap target"
require_literal "$dist_config" 'github-build-setup = "../build-setup.yml"' \
  "cargo-dist build setup hook"
require_literal "$dist_config" 'targets = ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"]' \
  "cargo-dist native target matrix"

required_dist_runner_targets=(
  "x86_64-unknown-linux-gnu"
  "aarch64-unknown-linux-gnu"
  "x86_64-apple-darwin"
  "aarch64-apple-darwin"
  "x86_64-pc-windows-msvc"
)
require_line "$dist_config" '^global[[:space:]]*=' "cargo-dist global custom runner"
for target in "${required_dist_runner_targets[@]}"; do
  require_line "$dist_config" "^${target}[[:space:]]*=" "cargo-dist custom runner ${target}"
done

require_line "$release_workflow" 'tags:' "release tag trigger"
require_literal "$release_workflow" 'uses: ./.github/workflows/publish-npm-trusted.yml' \
  "release npm publish job"
require_literal "$release_workflow" 'uses: ./.github/workflows/publish-homebrew-tap.yml' \
  "release Homebrew publish job"
require_literal "$release_workflow" '"id-token": "write"' \
  "release npm OIDC permission handoff"
require_literal "$release_workflow" '"packages": "write"' \
  "release npm package permission handoff"

require_literal "$crates_workflow" 'name: Publish crates.io' \
  "crates.io workflow name"
require_line "$crates_workflow" 'tags:' "crates.io tag trigger"
require_literal "$crates_workflow" 'id-token: write' \
  "crates.io trusted publishing OIDC permission"
require_literal "$crates_workflow" 'uses: rust-lang/crates-io-auth-action@v1.0.5' \
  "crates.io trusted publishing auth action"
require_literal "$crates_workflow" 'CARGO_REGISTRY_TOKEN: ${{ steps.auth.outputs.token }}' \
  "crates.io token handoff"
require_literal "$crates_workflow" 'run: cargo publish --locked' \
  "crates.io locked publish command"

require_literal "$npm_workflow" 'on:' "npm reusable workflow trigger"
require_literal "$npm_workflow" 'workflow_call:' "npm workflow_call trigger"
require_literal "$npm_workflow" 'id-token: write' \
  "npm trusted publishing OIDC permission"
require_literal "$npm_workflow" 'registry-url: "https://registry.npmjs.org"' \
  "npm registry target"
require_literal "$npm_workflow" 'npm install --global "npm@^11.15.0"' \
  "npm trusted publishing client version"
require_literal "$npm_workflow" 'npm publish --access public "./npm/${pkg}"' \
  "npm public publish command"

require_literal "$homebrew_workflow" 'workflow_call:' "Homebrew reusable workflow trigger"
require_literal "$homebrew_workflow" 'repository: "LogBrewCo/homebrew-tap"' \
  "Homebrew tap repository"
require_literal "$homebrew_workflow" 'token: ${{ secrets.HOMEBREW_TAP_TOKEN }}' \
  "Homebrew tap token"
require_literal "$homebrew_workflow" 'ruby -c "Formula/${filename}"' \
  "Homebrew formula syntax check"
require_literal "$homebrew_workflow" 'git push' \
  "Homebrew tap push"

printf 'Release workflow contract check passed.\n'
