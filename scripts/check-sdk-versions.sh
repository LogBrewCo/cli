#!/usr/bin/env bash
# Fails when an SDK version that `logbrew setup` recommends is not the newest published
# release. The versions live only in src/setup.rs (or LOGBREW_SETUP_SOURCE for tests).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="${LOGBREW_SETUP_SOURCE:-$ROOT_DIR/src/setup.rs}"
SDK_REPOSITORY="https://github.com/LogBrewCo/sdk.git"

fetch() { curl -fsSL --retry 3 --retry-all-errors --max-time 30 "$1"; }
newest() { grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -n 1; }
newest_tag() {
  git ls-remote --tags --refs "$SDK_REPOSITORY" "refs/tags/$1*" | sed -E "s#^.*refs/tags/$1##" | newest
}
pinned() {
  sed -nE "s/^pub const $1: &str = \"([0-9]+\.[0-9]+\.[0-9]+)\";$/\1/p" "$SOURCE"
}

failed=0
check() {
  local name="$1" published="$2" current
  current="$(pinned "$name")"
  if [[ -z "$published" ]]; then
    printf 'SDK version check failed: no published release found for %s\n' "$name" >&2
    failed=1
  elif [[ "$current" != "$published" ]]; then
    printf 'SDK version check failed: %s is "%s"; the newest published release is "%s"\n' \
      "$name" "$current" "$published" >&2
    failed=1
  fi
}

check JAVA_SDK_VERSION "$(fetch https://repo1.maven.org/maven2/co/logbrew/logbrew-sdk/maven-metadata.xml |
  sed -nE 's#.*<version>([^<]+)</version>.*#\1#p' | newest)"
check RUBY_SDK_VERSION "$(fetch https://rubygems.org/api/v1/versions/logbrew-sdk.json |
  jq -r '.[] | select(.prerelease | not) | .number' | newest)"
check ASPNETCORE_VERSION "$(fetch https://api.nuget.org/v3-flatcontainer/logbrew.aspnetcore/index.json |
  jq -r '.versions[]' | newest)"
check SWIFT_SDK_VERSION "$(newest_tag v)"
check OBJC_VERSION "$(newest_tag objc/logbrew-objc/v)"
check CPP_VERSION "$(newest_tag cpp/logbrew-cpp/v)"

if ((failed)); then
  printf 'Next: set the version in %s to the newest published release, then rerun the tests.\n' \
    "src/setup.rs" >&2
  exit 1
fi
printf 'SDK versions match the newest published releases.\n'
