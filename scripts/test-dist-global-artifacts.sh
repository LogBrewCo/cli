#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TAG="${1:-}"

fail() {
  printf 'Dist global artifacts check failed: %s\n' "$1" >&2
  printf 'Next: fix cargo-dist global installers or package metadata, then rerun bash scripts/test-dist-global-artifacts.sh %s.\n' "${TAG:-v<version>}" >&2
  exit 1
}

fail_missing_command() {
  local command_name="$1"

  printf "Dist global artifacts check failed: missing required command '%s'\n" "$command_name" >&2
  case "$command_name" in
    dist)
      printf 'Next: install cargo-dist with:\n' >&2
      printf "  curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v%s/cargo-dist-installer.sh | sh\n" "$dist_version" >&2
      printf 'Then rerun bash scripts/test-dist-global-artifacts.sh %s.\n' "${TAG:-v<version>}" >&2
      ;;
    *)
      printf "Next: install '%s' so it is on PATH, then rerun bash scripts/test-dist-global-artifacts.sh %s.\n" "$command_name" "${TAG:-v<version>}" >&2
      ;;
  esac
  exit 1
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    fail_missing_command "$1"
  fi
}

require_file() {
  local file="$1"
  local name="$2"

  if [[ ! -s "$file" ]]; then
    fail "missing generated artifact ${name}"
  fi
}

require_contains() {
  local file="$1"
  local needle="$2"
  local label="$3"

  if ! grep -Fq "$needle" "$file"; then
    fail "${label} must contain ${needle}"
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
require_command tar

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

if [[ -n "${LOGBREW_DIST_GLOBAL_ARTIFACTS_DIR:-}" ]]; then
  artifact_dir="$LOGBREW_DIST_GLOBAL_ARTIFACTS_DIR"
else
  require_command dist
  artifact_dir="$ROOT_DIR/target/distrib"
  rm -rf "$artifact_dir"
  if ! dist build --tag "$TAG" --artifacts=global --output-format=json --no-local-paths >/dev/null; then
    fail "could not build cargo-dist global artifacts"
  fi
fi

if [[ ! -d "$artifact_dir" ]]; then
  fail "generated artifact directory does not exist"
fi

required_artifacts=(
  logbrew-cli-installer.sh
  logbrew-cli-installer.ps1
  logbrew.rb
  logbrew-cli-npm-package.tar.gz
  sha256.sum
  source.tar.gz
  source.tar.gz.sha256
)

for artifact in "${required_artifacts[@]}"; do
  require_file "$artifact_dir/$artifact" "$artifact"
done

require_contains "$artifact_dir/logbrew-cli-installer.sh" 'APP_NAME="logbrew-cli"' "logbrew-cli-installer.sh"
require_contains "$artifact_dir/logbrew-cli-installer.sh" "APP_VERSION=\"${crate_version}\"" "logbrew-cli-installer.sh"
require_contains "$artifact_dir/logbrew-cli-installer.sh" "releases/download/${TAG}" "logbrew-cli-installer.sh"

require_contains "$artifact_dir/logbrew-cli-installer.ps1" "\$app_name = 'logbrew-cli'" "logbrew-cli-installer.ps1"
require_contains "$artifact_dir/logbrew-cli-installer.ps1" "\$app_version = '${crate_version}'" "logbrew-cli-installer.ps1"
require_contains "$artifact_dir/logbrew-cli-installer.ps1" "releases/download/${TAG}" "logbrew-cli-installer.ps1"

require_contains "$artifact_dir/logbrew.rb" 'class Logbrew < Formula' "logbrew.rb"
require_contains "$artifact_dir/logbrew.rb" "version \"${crate_version}\"" "logbrew.rb"
require_contains "$artifact_dir/logbrew.rb" "releases/download/${TAG}" "logbrew.rb"
require_contains "$artifact_dir/logbrew.rb" 'license "MIT"' "logbrew.rb"
require_contains "$artifact_dir/logbrew.rb" 'bin.install "logbrew"' "logbrew.rb"

require_contains "$artifact_dir/source.tar.gz.sha256" '*source.tar.gz' "source.tar.gz.sha256"
require_contains "$artifact_dir/sha256.sum" '*source.tar.gz' "sha256.sum"
require_contains "$artifact_dir/sha256.sum" '*logbrew-cli-npm-package.tar.gz' "sha256.sum"

npm_extract_dir="$(mktemp -d)"
trap 'rm -rf "$npm_extract_dir"' EXIT

if ! tar -xzf "$artifact_dir/logbrew-cli-npm-package.tar.gz" -C "$npm_extract_dir"; then
  fail "could not extract logbrew-cli-npm-package.tar.gz"
fi

npm_package_dir="$npm_extract_dir/package"
npm_package_json="$npm_package_dir/package.json"
require_file "$npm_package_json" "npm package package.json"

if ! jq -e --arg version "$crate_version" '
  .name == "logbrew-cli" and
  .version == $version and
  .bin.logbrew == "run-logbrew.js" and
  .repository == "https://github.com/LogBrewCo/cli" and
  .license == "MIT"
' "$npm_package_json" >/dev/null; then
  fail "npm package metadata must match logbrew-cli ${crate_version}"
fi

required_npm_files=(
  LICENSE
  README.md
  binary-install.js
  binary.js
  install.js
  npm-shrinkwrap.json
  run-logbrew.js
)

for file in "${required_npm_files[@]}"; do
  require_file "$npm_package_dir/$file" "npm package ${file}"
done

printf 'Dist global artifacts check passed.\n'
