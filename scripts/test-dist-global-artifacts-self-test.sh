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

if [[ -z "$crate_version" || "$crate_version" == "null" ]]; then
  printf 'could not read logbrew-cli version from Cargo metadata\n' >&2
  exit 1
fi

artifact_fixture="$tmp_dir/artifacts"
npm_fixture="$tmp_dir/npm"

make_fixture() {
  rm -rf "$artifact_fixture" "$npm_fixture"
  mkdir -p "$artifact_fixture" "$npm_fixture/package"

  printf 'APP_NAME="logbrew-cli"\nAPP_VERSION="%s"\nhttps://github.com/LogBrewCo/cli/releases/download/v%s\n' \
    "$crate_version" "$crate_version" >"$artifact_fixture/logbrew-cli-installer.sh"
  {
    printf "\$app_name = 'logbrew-cli'\n"
    printf "\$app_version = '%s'\n" "$crate_version"
    printf "https://github.com/LogBrewCo/cli/releases/download/v%s\n" "$crate_version"
    printf '"artifact_name" = "logbrew-cli-x86_64-pc-windows-msvc.zip"\n'
    printf '"bins" = @("logbrew.exe")\n'
  } >"$artifact_fixture/logbrew-cli-installer.ps1"
  cat >"$artifact_fixture/logbrew.rb" <<RUBY
class Logbrew < Formula
  desc "Public command-line interface for LogBrew."
  homepage "https://logbrew.co"
  version "${crate_version}"
  url "https://github.com/LogBrewCo/cli/releases/download/v${crate_version}/logbrew-cli-x86_64-unknown-linux-gnu.tar.xz"
  license "MIT"

  def install
    bin.install "logbrew"
  end
end
RUBY

  cat >"$npm_fixture/package/package.json" <<JSON
{
  "name": "logbrew-cli",
  "version": "${crate_version}",
  "bin": {
    "logbrew": "run-logbrew.js"
  },
  "repository": "https://github.com/LogBrewCo/cli",
  "license": "MIT"
}
JSON
  for file in LICENSE README.md binary-install.js binary.js install.js npm-shrinkwrap.json run-logbrew.js; do
    printf '%s\n' "$file" >"$npm_fixture/package/$file"
  done
  (cd "$npm_fixture" && tar -czf "$artifact_fixture/logbrew-cli-npm-package.tar.gz" package)

  printf 'source\n' >"$artifact_fixture/source.tar.gz"
  printf 'abc123 *source.tar.gz\n' >"$artifact_fixture/source.tar.gz.sha256"
  printf 'abc123 *source.tar.gz\nabc123 *logbrew-cli-npm-package.tar.gz\n' >"$artifact_fixture/sha256.sum"
}

run_artifact_check() {
  LOGBREW_DIST_GLOBAL_ARTIFACTS_DIR="$artifact_fixture" \
    bash scripts/test-dist-global-artifacts.sh "v${crate_version}" >"$output_file" 2>&1
}

expect_failure() {
  local expected_line="$1"

  : >"$output_file"
  if run_artifact_check; then
    printf 'expected dist global artifact check to fail\n' >&2
    exit 1
  fi

  if ! grep -Fq "$expected_line" "$output_file"; then
    printf 'expected dist global artifact output to contain: %s\n' "$expected_line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
}

make_fixture
: >"$output_file"
if ! run_artifact_check; then
  printf 'expected dist global artifact fixture to pass\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq 'Dist global artifacts check passed.' "$output_file"; then
  printf 'expected dist global artifact success output\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

make_fixture
rm "$artifact_fixture/logbrew-cli-npm-package.tar.gz"
expect_failure 'Dist global artifacts check failed: missing generated artifact logbrew-cli-npm-package.tar.gz'

make_fixture
(cd "$npm_fixture" && jq '.version = "0.0.0"' package/package.json >package/package.json.tmp && mv package/package.json.tmp package/package.json && tar -czf "$artifact_fixture/logbrew-cli-npm-package.tar.gz" package)
expect_failure "Dist global artifacts check failed: npm package metadata must match logbrew-cli ${crate_version}"

make_fixture
sed -i.bak '/releases\/download/d' "$artifact_fixture/logbrew.rb"
expect_failure "Dist global artifacts check failed: logbrew.rb must contain releases/download/v${crate_version}"

make_fixture
sed -i.bak '/logbrew-cli-x86_64-pc-windows-msvc.zip/d' "$artifact_fixture/logbrew-cli-installer.ps1"
expect_failure 'Dist global artifacts check failed: logbrew-cli-installer.ps1 must contain logbrew-cli-x86_64-pc-windows-msvc.zip'

make_fixture
sed -i.bak '/logbrew.exe/d' "$artifact_fixture/logbrew-cli-installer.ps1"
expect_failure 'Dist global artifacts check failed: logbrew-cli-installer.ps1 must contain logbrew.exe'

make_fixture
sed -i.bak '/logbrew-cli-npm-package.tar.gz/d' "$artifact_fixture/sha256.sum"
expect_failure 'Dist global artifacts check failed: sha256.sum must contain *logbrew-cli-npm-package.tar.gz'

printf 'Dist global artifacts self-test passed.\n'
