#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TAG="${1:-}"

fail() {
  printf 'Dist Homebrew formula smoke failed: %s\n' "$1" >&2
  printf 'Next: fix cargo-dist Homebrew formula generation, then rerun bash scripts/test-dist-homebrew-formula-smoke.sh %s.\n' "${TAG:-v<version>}" >&2
  exit 1
}

fail_missing_command() {
  local command_name="$1"

  printf "Dist Homebrew formula smoke failed: missing required command '%s'\n" "$command_name" >&2
  case "$command_name" in
    dist)
      printf 'Next: install cargo-dist with:\n' >&2
      printf "  curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v%s/cargo-dist-installer.sh | sh\n" "$dist_version" >&2
      printf 'Then rerun bash scripts/test-dist-homebrew-formula-smoke.sh %s.\n' "${TAG:-v<version>}" >&2
      ;;
    *)
      printf "Next: install '%s' so it is on PATH, then rerun bash scripts/test-dist-homebrew-formula-smoke.sh %s.\n" "$command_name" "${TAG:-v<version>}" >&2
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

evaluate_formula() {
  local formula_file="$1"
  local platform="$2"
  local cpu="$3"

  LOGBREW_HOMEBREW_FORMULA_PLATFORM="$platform" \
    LOGBREW_HOMEBREW_FORMULA_CPU="$cpu" \
    ruby - "$formula_file" <<'RUBY'
require "json"

$formula_platform = ENV.fetch("LOGBREW_HOMEBREW_FORMULA_PLATFORM")
$formula_cpu = ENV.fetch("LOGBREW_HOMEBREW_FORMULA_CPU")

module OS
  def self.mac?
    $formula_platform == "mac"
  end

  def self.linux?
    $formula_platform == "linux"
  end
end

module Hardware
  module CPU
    def self.arm?
      $formula_cpu == "arm"
    end

    def self.intel?
      $formula_cpu == "intel"
    end
  end
end

class Formula
  class << self
    def desc(value = nil)
      @desc_value = value unless value.nil?
      @desc_value
    end

    def homepage(value = nil)
      @homepage_value = value unless value.nil?
      @homepage_value
    end

    def version(value = nil)
      @version_value = value unless value.nil?
      @version_value
    end

    def url(value = nil)
      urls << value unless value.nil?
      urls
    end

    def license(value = nil)
      @license_value = value unless value.nil?
      @license_value
    end

    def urls
      @urls ||= []
    end

    attr_reader :desc_value, :homepage_value, :version_value, :license_value
  end
end

formula_file = ARGV.fetch(0)
load formula_file

formula = Object.const_get("Logbrew")
aliases = if formula.const_defined?(:BINARY_ALIASES, false)
  formula.const_get(:BINARY_ALIASES).keys.map(&:to_s).sort
else
  []
end

puts JSON.generate(
  "class_name" => formula.name,
  "desc" => formula.desc_value,
  "homepage" => formula.homepage_value,
  "version" => formula.version_value,
  "license" => formula.license_value,
  "urls" => formula.urls,
  "target_triple" => formula.new.target_triple,
  "aliases" => aliases
)
RUBY
}

dist_version="$(
  sed -n 's/^cargo-dist-version = "\(.*\)"/\1/p' dist-workspace.toml
)"

if [[ -z "$dist_version" ]]; then
  fail "could not read cargo-dist version from dist-workspace.toml"
fi

require_command cargo
require_command jq
require_command ruby

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

if [[ -n "${LOGBREW_DIST_HOMEBREW_TARGET:-}" ]]; then
  target="$LOGBREW_DIST_HOMEBREW_TARGET"
else
  require_command rustc
  target="$(rustc -vV | awk '/^host:/ {print $2}')"
fi

if [[ -z "$target" ]]; then
  fail "could not read host target"
fi

if ! grep -Fq "\"${target}\"" dist-workspace.toml; then
  fail "target ${target} is not in the cargo-dist target matrix"
fi

case "$target" in
  aarch64-apple-darwin|aarch64-unknown-linux-gnu|x86_64-apple-darwin|x86_64-unknown-linux-gnu)
    archive="logbrew-cli-${target}.tar.xz"
    ;;
  *)
    fail "Homebrew formula smoke requires a macOS or Linux target"
    ;;
esac

if [[ -n "${LOGBREW_DIST_HOMEBREW_ARTIFACTS_DIR:-}" ]]; then
  artifact_dir="$LOGBREW_DIST_HOMEBREW_ARTIFACTS_DIR"
else
  require_command dist
  artifact_dir="$ROOT_DIR/target/distrib"
  rm -rf "$artifact_dir"
  if ! dist build --tag "$TAG" --artifacts=global --output-format=json --no-local-paths >/dev/null; then
    fail "could not build cargo-dist global artifacts"
  fi
  if ! dist build --tag "$TAG" --artifacts=local --target "$target" --output-format=json --no-local-paths >/dev/null; then
    fail "could not build cargo-dist local artifact for ${target}"
  fi
fi

if [[ ! -d "$artifact_dir" ]]; then
  fail "generated artifact directory does not exist"
fi

formula="$artifact_dir/logbrew.rb"
require_file "$formula" "logbrew.rb"
require_file "$artifact_dir/$archive" "$archive"

platform_specs=(
  "mac arm aarch64-apple-darwin"
  "mac intel x86_64-apple-darwin"
  "linux arm aarch64-unknown-linux-gnu"
  "linux intel x86_64-unknown-linux-gnu"
)

required_aliases=(
  aarch64-apple-darwin
  aarch64-unknown-linux-gnu
  x86_64-apple-darwin
  x86_64-unknown-linux-gnu
)

for spec in "${platform_specs[@]}"; do
  read -r platform cpu formula_target <<<"$spec"

  if ! formula_json="$(evaluate_formula "$formula" "$platform" "$cpu")"; then
    fail "could not evaluate logbrew.rb for ${formula_target}"
  fi

  expected_url="https://github.com/LogBrewCo/cli/releases/download/${TAG}/logbrew-cli-${formula_target}.tar.xz"

  if ! jq -e '.class_name == "Logbrew"' <<<"$formula_json" >/dev/null; then
    fail "formula class must be Logbrew"
  fi

  if ! jq -e '.desc == "Public command-line interface for LogBrew."' <<<"$formula_json" >/dev/null; then
    fail "formula desc must describe the public LogBrew CLI"
  fi

  if ! jq -e '.homepage == "https://logbrew.co"' <<<"$formula_json" >/dev/null; then
    fail "formula homepage must be https://logbrew.co"
  fi

  if ! jq -e --arg version "$crate_version" '.version == $version' <<<"$formula_json" >/dev/null; then
    fail "formula version must be ${crate_version}"
  fi

  if ! jq -e '.license == "MIT"' <<<"$formula_json" >/dev/null; then
    fail "formula license must be MIT"
  fi

  if ! jq -e --arg target "$formula_target" '.target_triple == $target' <<<"$formula_json" >/dev/null; then
    fail "target_triple must resolve ${platform}/${cpu} to ${formula_target}"
  fi

  if ! jq -e --arg url "$expected_url" '.urls == [$url]' <<<"$formula_json" >/dev/null; then
    fail "target ${formula_target} URL must be ${expected_url}"
  fi

  for alias_target in "${required_aliases[@]}"; do
    if ! jq -e --arg target "$alias_target" '.aliases | index($target)' <<<"$formula_json" >/dev/null; then
      fail "BINARY_ALIASES must include ${alias_target}"
    fi
  done
done

printf 'Dist Homebrew formula smoke passed for %s.\n' "$target"
