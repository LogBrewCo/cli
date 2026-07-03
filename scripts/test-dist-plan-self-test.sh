#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
output_file="$(mktemp)"
trap 'rm -rf "$tmp_dir" "$output_file"' EXIT

crate_version="$(
  cargo metadata --no-deps --format-version=1 |
    jq -r '.packages[] | select(.name == "logbrew-cli").version'
)"
dist_version="$(
  sed -n 's/^cargo-dist-version = "\(.*\)"/\1/p' dist-workspace.toml
)"

if [[ -z "$crate_version" || "$crate_version" == "null" ]]; then
  printf 'could not read logbrew-cli version from Cargo metadata\n' >&2
  exit 1
fi

if [[ -z "$dist_version" ]]; then
  printf 'could not read cargo-dist version from dist-workspace.toml\n' >&2
  exit 1
fi

fixture="$tmp_dir/dist-plan.json"
cat >"$fixture" <<JSON
{
  "dist_version": "${dist_version}",
  "announcement_tag": "v${crate_version}",
  "ci": {
    "github": {
      "pr_run_mode": "plan",
      "artifacts_matrix": {
        "include": [
          {
            "runner": "blacksmith-6vcpu-macos-15",
            "targets": ["aarch64-apple-darwin"]
          },
          {
            "runner": "blacksmith-2vcpu-ubuntu-2404-arm",
            "targets": ["aarch64-unknown-linux-gnu"]
          },
          {
            "runner": "blacksmith-6vcpu-macos-15",
            "targets": ["x86_64-apple-darwin"]
          },
          {
            "runner": "blacksmith-2vcpu-windows-2025",
            "targets": ["x86_64-pc-windows-msvc"]
          },
          {
            "runner": "blacksmith-2vcpu-ubuntu-2404",
            "targets": ["x86_64-unknown-linux-gnu"]
          }
        ]
      }
    }
  },
  "releases": [
    {
      "app_name": "logbrew-cli",
      "app_version": "${crate_version}",
      "artifacts": [
        "source.tar.gz",
        "source.tar.gz.sha256",
        "logbrew-cli-installer.sh",
        "logbrew-cli-installer.ps1",
        "logbrew.rb",
        "logbrew-cli-npm-package.tar.gz",
        "sha256.sum",
        "logbrew-cli-aarch64-apple-darwin.tar.xz",
        "logbrew-cli-aarch64-apple-darwin.tar.xz.sha256",
        "logbrew-cli-aarch64-unknown-linux-gnu.tar.xz",
        "logbrew-cli-aarch64-unknown-linux-gnu.tar.xz.sha256",
        "logbrew-cli-x86_64-apple-darwin.tar.xz",
        "logbrew-cli-x86_64-apple-darwin.tar.xz.sha256",
        "logbrew-cli-x86_64-pc-windows-msvc.zip",
        "logbrew-cli-x86_64-pc-windows-msvc.zip.sha256",
        "logbrew-cli-x86_64-pc-windows-msvc.msi",
        "logbrew-cli-x86_64-pc-windows-msvc.msi.sha256",
        "logbrew-cli-x86_64-unknown-linux-gnu.tar.xz",
        "logbrew-cli-x86_64-unknown-linux-gnu.tar.xz.sha256"
      ]
    }
  ]
}
JSON

run_dist_plan_check() {
  local plan_file="$1"
  LOGBREW_DIST_PLAN_JSON="$plan_file" bash scripts/test-dist-plan.sh "v${crate_version}" >"$output_file" 2>&1
}

expect_failure() {
  local plan_file="$1"
  local expected_line="$2"

  : >"$output_file"
  if run_dist_plan_check "$plan_file"; then
    printf 'expected dist plan check to fail for fixture %s\n' "$plan_file" >&2
    exit 1
  fi

  if ! grep -Fq "$expected_line" "$output_file"; then
    printf 'expected dist plan output to contain: %s\n' "$expected_line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
}

: >"$output_file"
if ! run_dist_plan_check "$fixture"; then
  printf 'expected dist plan fixture to pass\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq 'Dist plan check passed.' "$output_file"; then
  printf 'expected dist plan success output\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

missing_msi="$tmp_dir/missing-msi.json"
jq '(.releases[] | select(.app_name == "logbrew-cli").artifacts) |= map(select(. != "logbrew-cli-x86_64-pc-windows-msvc.msi"))' \
  "$fixture" >"$missing_msi"
expect_failure "$missing_msi" "Dist plan check failed: dist plan missing artifact logbrew-cli-x86_64-pc-windows-msvc.msi"

missing_windows_target="$tmp_dir/missing-windows-target.json"
jq '(.ci.github.artifacts_matrix.include) |= map(select(((.targets // []) | index("x86_64-pc-windows-msvc")) == null))' \
  "$fixture" >"$missing_windows_target"
expect_failure "$missing_windows_target" "Dist plan check failed: dist plan missing target x86_64-pc-windows-msvc on runner blacksmith-2vcpu-windows-2025"

wrong_tag="$tmp_dir/wrong-tag.json"
jq '.announcement_tag = "v0.0.0"' "$fixture" >"$wrong_tag"
expect_failure "$wrong_tag" "Dist plan check failed: dist plan announcement tag must be v${crate_version}"

leaked_local_path="$tmp_dir/leaked-local-path.json"
jq --arg root "$ROOT_DIR" '.local_path = $root' "$fixture" >"$leaked_local_path"
expect_failure "$leaked_local_path" 'Dist plan check failed: dist plan contains local workspace path'

printf 'Dist plan self-test passed.\n'
