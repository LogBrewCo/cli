#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TAG="${1:-}"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

fail() {
  printf 'Dist source artifact smoke failed: %s\n' "$1" >&2
  printf 'Next: fix cargo-dist source artifact installability, then rerun bash scripts/test-dist-source-artifact-smoke.sh %s.\n' "${TAG:-v<version>}" >&2
  exit 1
}

fail_missing_command() {
  local command_name="$1"

  printf "Dist source artifact smoke failed: missing required command '%s'\n" "$command_name" >&2
  case "$command_name" in
    dist)
      printf 'Next: install cargo-dist with:\n' >&2
      printf "  curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v%s/cargo-dist-installer.sh | sh\n" "$dist_version" >&2
      printf 'Then rerun bash scripts/test-dist-source-artifact-smoke.sh %s.\n' "${TAG:-v<version>}" >&2
      ;;
    *)
      printf "Next: install '%s' so it is on PATH, then rerun bash scripts/test-dist-source-artifact-smoke.sh %s.\n" "$command_name" "${TAG:-v<version>}" >&2
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

if [[ -n "${LOGBREW_DIST_SOURCE_ARTIFACTS_DIR:-}" ]]; then
  artifact_dir="$LOGBREW_DIST_SOURCE_ARTIFACTS_DIR"
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

source_archive="source.tar.gz"
source_checksum="source.tar.gz.sha256"

require_file "$artifact_dir/$source_archive" "$source_archive"
require_file "$artifact_dir/$source_checksum" "$source_checksum"
require_contains "$artifact_dir/$source_checksum" "*${source_archive}" "$source_checksum"

extract_dir="$tmp_dir/source-extract"
mkdir -p "$extract_dir"
if ! tar -xzf "$artifact_dir/$source_archive" -C "$extract_dir"; then
  fail "could not extract ${source_archive}"
fi

source_dir="$extract_dir/logbrew-cli-${crate_version}"
if [[ ! -d "$source_dir" ]]; then
  fail "source archive must contain logbrew-cli-${crate_version}"
fi

require_file "$source_dir/Cargo.toml" "source Cargo.toml"
require_file "$source_dir/Cargo.lock" "source Cargo.lock"
require_file "$source_dir/src/main.rs" "source src/main.rs"

blocked_path="$(
  find "$source_dir" \
    \( \
      -name AGENTS.md -o \
      -name CLAUDE.md -o \
      -name skills-lock.json -o \
      -path '*/.agents' -o \
      -path '*/.agents/*' -o \
      -path '*/docs/superpowers' -o \
      -path '*/docs/superpowers/*' -o \
      -path '*/plans' -o \
      -path '*/plans/*' \
    \) \
    -print -quit
)"

if [[ -n "$blocked_path" ]]; then
  fail "source archive contains release-blocked path ${blocked_path#"$source_dir"/}"
fi

source_version="$(
  cd "$source_dir"
  cargo metadata --no-deps --format-version=1 |
    jq -r '.packages[] | select(.name == "logbrew-cli").version'
)"

if [[ "$source_version" != "$crate_version" ]]; then
  fail "source Cargo metadata must report version ${crate_version}"
fi

target_dir="$tmp_dir/source-target"
if ! (cd "$source_dir" && CARGO_TARGET_DIR="$target_dir" cargo build --locked --bin logbrew >"$tmp_dir/source-build.log" 2>&1); then
  sed -n '1,160p' "$tmp_dir/source-build.log" >&2 || true
  fail "source archive must build with cargo build --locked --bin logbrew"
fi

binary="$target_dir/debug/logbrew"
if [[ ! -x "$binary" ]]; then
  fail "source build did not create executable logbrew"
fi

if ! human_output="$("$binary" --version)"; then
  fail "source-built logbrew must support --version"
fi

if [[ "$human_output" != "logbrew ${crate_version}" ]]; then
  fail "source-built logbrew must report version ${crate_version}"
fi

if ! json_output="$("$binary" --json version)"; then
  fail "source-built logbrew must support version --json"
fi

if ! jq -e --arg version "$crate_version" '
  .ok == true and
  .name == "logbrew" and
  .version == $version and
  (.binary | type == "string" and length > 0) and
  (.os | type == "string" and length > 0) and
  (.arch | type == "string" and length > 0)
' <<<"$json_output" >/dev/null; then
  fail "source-built logbrew version JSON must expose native binary metadata for ${crate_version}"
fi

printf 'Dist source artifact smoke passed for %s.\n' "$source_archive"
