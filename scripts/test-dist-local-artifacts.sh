#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TAG="${1:-}"

fail() {
  printf 'Dist local artifacts check failed: %s\n' "$1" >&2
  printf 'Next: fix cargo-dist native artifact generation, then rerun bash scripts/test-dist-local-artifacts.sh %s.\n' "${TAG:-v<version>}" >&2
  exit 1
}

fail_missing_command() {
  local command_name="$1"

  printf "Dist local artifacts check failed: missing required command '%s'\n" "$command_name" >&2
  case "$command_name" in
    dist)
      printf 'Next: install cargo-dist with:\n' >&2
      printf "  curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v%s/cargo-dist-installer.sh | sh\n" "$dist_version" >&2
      printf 'Then rerun bash scripts/test-dist-local-artifacts.sh %s.\n' "${TAG:-v<version>}" >&2
      ;;
    *)
      printf "Next: install '%s' so it is on PATH, then rerun bash scripts/test-dist-local-artifacts.sh %s.\n" "$command_name" "${TAG:-v<version>}" >&2
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
require_command rustc
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

target="${LOGBREW_DIST_LOCAL_TARGET:-$(rustc -vV | awk '/^host:/ {print $2}')}"
if [[ -z "$target" ]]; then
  fail "could not read rustc host target"
fi

if ! grep -Fq "\"${target}\"" dist-workspace.toml; then
  fail "target ${target} is not in the cargo-dist target matrix"
fi

case "$target" in
  *windows*)
    archive="logbrew-cli-${target}.zip"
    binary_name="logbrew.exe"
    require_command unzip
    ;;
  *)
    archive="logbrew-cli-${target}.tar.xz"
    binary_name="logbrew"
    ;;
esac
checksum="${archive}.sha256"

if [[ -n "${LOGBREW_DIST_LOCAL_ARTIFACTS_DIR:-}" ]]; then
  artifact_dir="$LOGBREW_DIST_LOCAL_ARTIFACTS_DIR"
else
  require_command dist
  artifact_dir="$ROOT_DIR/target/distrib"
  rm -rf "$artifact_dir"
  if ! dist build --tag "$TAG" --artifacts=local --target "$target" --output-format=json --no-local-paths >/dev/null; then
    fail "could not build cargo-dist local artifacts for ${target}"
  fi
fi

if [[ ! -d "$artifact_dir" ]]; then
  fail "generated artifact directory does not exist"
fi

require_file "$artifact_dir/$archive" "$archive"
require_file "$artifact_dir/$checksum" "$checksum"
require_contains "$artifact_dir/$checksum" "*${archive}" "$checksum"

extract_dir="$(mktemp -d)"
trap 'rm -rf "$extract_dir"' EXIT

case "$archive" in
  *.zip)
    if ! unzip -q "$artifact_dir/$archive" -d "$extract_dir"; then
      fail "could not extract ${archive}"
    fi
    ;;
  *.tar.xz)
    if ! tar -xJf "$artifact_dir/$archive" -C "$extract_dir"; then
      fail "could not extract ${archive}"
    fi
    ;;
  *)
    fail "unsupported native artifact archive ${archive}"
    ;;
esac

extracted_binary="$(
  find "$extract_dir" -type f -name "$binary_name" | head -n 1
)"

if [[ -z "$extracted_binary" || ! -f "$extracted_binary" ]]; then
  fail "archive must contain logbrew binary"
fi

if [[ ! -x "$extracted_binary" ]]; then
  fail "archive logbrew binary must be executable"
fi

if ! version_output="$("$extracted_binary" --version)"; then
  fail "archive logbrew binary must support --version"
fi

if [[ "$version_output" != "logbrew ${crate_version}" ]]; then
  fail "archive logbrew binary must report version ${crate_version}"
fi

version_json="$extract_dir/version.json"
if ! "$extracted_binary" --json version >"$version_json"; then
  fail "archive logbrew binary must support version --json"
fi

if ! jq -e --arg version "$crate_version" '
  .ok == true and
  .name == "logbrew" and
  .version == $version and
  (.binary | type == "string" and length > 0) and
  (.os | type == "string" and length > 0) and
  (.arch | type == "string" and length > 0)
' "$version_json" >/dev/null; then
  fail "archive logbrew version JSON must expose native binary metadata for ${crate_version}"
fi

printf 'Dist local artifacts check passed for %s.\n' "$target"
