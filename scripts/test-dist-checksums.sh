#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TAG="${1:-}"

fail() {
  printf 'Dist checksum check failed: %s\n' "$1" >&2
  printf 'Next: fix cargo-dist checksum artifacts, then rerun bash scripts/test-dist-checksums.sh %s.\n' "${TAG:-v<version>}" >&2
  exit 1
}

fail_missing_command() {
  local command_name="$1"

  printf "Dist checksum check failed: missing required command '%s'\n" "$command_name" >&2
  case "$command_name" in
    dist)
      printf 'Next: install cargo-dist with:\n' >&2
      printf "  curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v%s/cargo-dist-installer.sh | sh\n" "$dist_version" >&2
      printf 'Then rerun bash scripts/test-dist-checksums.sh %s.\n' "${TAG:-v<version>}" >&2
      ;;
    *)
      printf "Next: install '%s' so it is on PATH, then rerun bash scripts/test-dist-checksums.sh %s.\n" "$command_name" "${TAG:-v<version>}" >&2
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

calculate_sha256() {
  local file="$1"

  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    sha256sum "$file" | awk '{print $1}'
  fi
}

verify_checksum_file() {
  local checksum_file="$1"
  local checksum_name="${checksum_file##*/}"
  local line
  local line_no=0
  local entry_count=0

  while IFS= read -r line || [[ -n "$line" ]]; do
    line_no=$((line_no + 1))
    if [[ -z "$line" ]]; then
      continue
    fi

    local expected artifact extra
    read -r expected artifact extra <<<"$line"
    artifact="${artifact#\*}"

    if [[ -z "${expected:-}" || -z "${artifact:-}" || -n "${extra:-}" ]]; then
      fail "${checksum_name} has malformed checksum entry on line ${line_no}"
    fi

    if [[ ! "$expected" =~ ^[0-9A-Fa-f]{64}$ ]]; then
      fail "${checksum_name} has invalid SHA-256 digest for ${artifact}"
    fi

    case "$artifact" in
      /*|../*|*/../*|*/*)
        fail "${checksum_name} has unsafe artifact path ${artifact}"
        ;;
    esac

    local artifact_file="$artifact_dir/$artifact"
    if [[ ! -f "$artifact_file" ]]; then
      fail "checksum entry ${artifact} in ${checksum_name} points to missing artifact"
    fi

    local actual expected_lower
    actual="$(calculate_sha256 "$artifact_file" | tr 'A-F' 'a-f')"
    expected_lower="$(printf '%s' "$expected" | tr 'A-F' 'a-f')"
    if [[ "$actual" != "$expected_lower" ]]; then
      fail "checksum mismatch for ${artifact} listed in ${checksum_name}"
    fi

    entry_count=$((entry_count + 1))
  done <"$checksum_file"

  if (( entry_count == 0 )); then
    fail "${checksum_name} has no checksum entries"
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

if ! command -v shasum >/dev/null 2>&1; then
  require_command sha256sum
fi

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

if [[ -n "${LOGBREW_DIST_CHECKSUMS_ARTIFACTS_DIR:-}" ]]; then
  artifact_dir="$LOGBREW_DIST_CHECKSUMS_ARTIFACTS_DIR"
else
  require_command dist
  require_command rustc

  target="${LOGBREW_DIST_CHECKSUMS_TARGET:-$(rustc -vV | awk '/^host:/ {print $2}')}"
  if [[ -z "$target" ]]; then
    fail "could not read rustc host target"
  fi

  if ! grep -Fq "\"${target}\"" dist-workspace.toml; then
    fail "target ${target} is not in the cargo-dist target matrix"
  fi

  case "$target" in
    *windows*)
      archive="logbrew-cli-${target}.zip"
      ;;
    *)
      archive="logbrew-cli-${target}.tar.xz"
      ;;
  esac

  artifact_dir="$ROOT_DIR/target/distrib"
  rm -rf "$artifact_dir"
  if ! dist build --tag "$TAG" --artifacts=global --output-format=json --no-local-paths >/dev/null; then
    fail "could not build cargo-dist global artifacts"
  fi
  if ! dist build --tag "$TAG" --artifacts=local --target "$target" --output-format=json --no-local-paths >/dev/null; then
    fail "could not build cargo-dist local artifact for ${target}"
  fi

  require_file "$artifact_dir/source.tar.gz" "source.tar.gz"
  require_file "$artifact_dir/source.tar.gz.sha256" "source.tar.gz.sha256"
  require_file "$artifact_dir/logbrew-cli-npm-package.tar.gz" "logbrew-cli-npm-package.tar.gz"
  require_file "$artifact_dir/sha256.sum" "sha256.sum"
  require_file "$artifact_dir/$archive" "$archive"
  require_file "$artifact_dir/${archive}.sha256" "${archive}.sha256"
fi

if [[ ! -d "$artifact_dir" ]]; then
  fail "generated artifact directory does not exist"
fi

checksum_count=0
while IFS= read -r checksum_file; do
  checksum_count=$((checksum_count + 1))
  verify_checksum_file "$checksum_file"
done < <(find "$artifact_dir" -maxdepth 1 -type f \( -name '*.sha256' -o -name 'sha256.sum' \) | sort)

if (( checksum_count == 0 )); then
  fail "generated artifact directory has no checksum files"
fi

printf 'Dist checksum check passed.\n'
