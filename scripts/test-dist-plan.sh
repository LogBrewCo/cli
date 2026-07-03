#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TAG="${1:-}"

fail() {
  printf 'Dist plan check failed: %s\n' "$1" >&2
  printf 'Next: fix cargo-dist release config, then rerun bash scripts/test-dist-plan.sh %s.\n' "${TAG:-v<version>}" >&2
  exit 1
}

fail_missing_command() {
  local command_name="$1"

  printf "Dist plan check failed: missing required command '%s'\n" "$command_name" >&2
  case "$command_name" in
    dist)
      printf 'Next: install cargo-dist with:\n' >&2
      printf "  curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v%s/cargo-dist-installer.sh | sh\n" "$dist_version" >&2
      printf 'Then rerun bash scripts/test-dist-plan.sh %s.\n' "${TAG:-v<version>}" >&2
      ;;
    *)
      printf "Next: install '%s' so it is on PATH, then rerun bash scripts/test-dist-plan.sh %s.\n" "$command_name" "${TAG:-v<version>}" >&2
      ;;
  esac
  exit 1
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    fail_missing_command "$1"
  fi
}

dist_version="$(
  sed -n 's/^cargo-dist-version = "\(.*\)"/\1/p' dist-workspace.toml
)"

if [[ -z "$dist_version" ]]; then
  fail "could not read cargo-dist version from dist-workspace.toml"
fi

require_command cargo
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

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

if [[ -n "${LOGBREW_DIST_PLAN_JSON:-}" ]]; then
  plan_file="$LOGBREW_DIST_PLAN_JSON"
  if [[ ! -f "$plan_file" ]]; then
    fail "provided dist plan JSON file does not exist"
  fi
else
  require_command dist
  plan_file="$tmp_dir/dist-plan.json"
  if ! dist plan --tag "$TAG" --output-format=json --no-local-paths >"$plan_file"; then
    fail "could not generate cargo-dist plan"
  fi
fi

if ! jq -e 'type == "object"' "$plan_file" >/dev/null; then
  fail "dist plan JSON is invalid"
fi

if grep -Fq "$ROOT_DIR" "$plan_file"; then
  fail "dist plan contains local workspace path"
fi

if ! jq -e --arg version "$dist_version" '.dist_version == $version' "$plan_file" >/dev/null; then
  fail "dist plan version must match pinned ${dist_version}"
fi

if ! jq -e --arg tag "$TAG" '.announcement_tag == $tag' "$plan_file" >/dev/null; then
  fail "dist plan announcement tag must be ${TAG}"
fi

if ! jq -e --arg version "$crate_version" '
  any(.releases[]?; .app_name == "logbrew-cli" and .app_version == $version)
' "$plan_file" >/dev/null; then
  fail "dist plan missing logbrew-cli release ${crate_version}"
fi

if ! jq -e '.ci.github.pr_run_mode == "plan"' "$plan_file" >/dev/null; then
  fail "dist plan PR run mode must be plan"
fi

required_artifacts=(
  source.tar.gz
  source.tar.gz.sha256
  logbrew-cli-installer.sh
  logbrew-cli-installer.ps1
  logbrew.rb
  logbrew-cli-npm-package.tar.gz
  sha256.sum
  logbrew-cli-aarch64-apple-darwin.tar.xz
  logbrew-cli-aarch64-apple-darwin.tar.xz.sha256
  logbrew-cli-aarch64-unknown-linux-gnu.tar.xz
  logbrew-cli-aarch64-unknown-linux-gnu.tar.xz.sha256
  logbrew-cli-x86_64-apple-darwin.tar.xz
  logbrew-cli-x86_64-apple-darwin.tar.xz.sha256
  logbrew-cli-x86_64-pc-windows-msvc.zip
  logbrew-cli-x86_64-pc-windows-msvc.zip.sha256
  logbrew-cli-x86_64-pc-windows-msvc.msi
  logbrew-cli-x86_64-pc-windows-msvc.msi.sha256
  logbrew-cli-x86_64-unknown-linux-gnu.tar.xz
  logbrew-cli-x86_64-unknown-linux-gnu.tar.xz.sha256
)

for artifact in "${required_artifacts[@]}"; do
  if ! jq -e --arg artifact "$artifact" '
    any(.releases[]?; .app_name == "logbrew-cli" and any(.artifacts[]?; . == $artifact))
  ' "$plan_file" >/dev/null; then
    fail "dist plan missing artifact ${artifact}"
  fi
done

required_target_runners=(
  "aarch64-apple-darwin|blacksmith-6vcpu-macos-15"
  "aarch64-unknown-linux-gnu|blacksmith-2vcpu-ubuntu-2404-arm"
  "x86_64-apple-darwin|blacksmith-6vcpu-macos-15"
  "x86_64-pc-windows-msvc|blacksmith-2vcpu-windows-2025"
  "x86_64-unknown-linux-gnu|blacksmith-2vcpu-ubuntu-2404"
)

for target_runner in "${required_target_runners[@]}"; do
  IFS='|' read -r target runner <<<"$target_runner"
  if ! jq -e --arg target "$target" --arg runner "$runner" '
    any(
      .ci.github.artifacts_matrix.include[]?;
      .runner == $runner and ((.targets // []) | index($target) != null)
    )
  ' "$plan_file" >/dev/null; then
    fail "dist plan missing target ${target} on runner ${runner}"
  fi
done

printf 'Dist plan check passed.\n'
