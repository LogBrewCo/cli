#!/usr/bin/env bash
# Proves that scripts/npm-lock-tarballs.sh pins the packed npm packages by hash:
# npm ci installs the packed bytes, refuses a tarball with other contents, and
# installs a new build once it is locked again.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export npm_config_cache="$work/npm-cache" npm_config_update_notifier=false

fail() {
  printf 'test-npm-lock-tarballs: %s\n' "$*" >&2
  exit 1
}

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) target=aarch64-apple-darwin os=darwin cpu=arm64 ;;
  Darwin-x86_64) target=x86_64-apple-darwin os=darwin cpu=x64 ;;
  Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-gnu os=linux cpu=arm64 ;;
  Linux-x86_64) target=x86_64-unknown-linux-gnu os=linux cpu=x64 ;;
  MINGW*-x86_64 | MSYS*-x86_64) target=x86_64-pc-windows-msvc os=win32 cpu=x64 ;;
  *) fail "unsupported host $(uname -s)-$(uname -m)" ;;
esac
executable=logbrew
[[ "$os" == win32 ]] && executable=logbrew.exe
installed="node_modules/@logbrew/cli-$os-$cpu/bin/$executable"

# pack TEXT: packs version 1.2.3 into $work/packed with a binary that holds TEXT.
pack() {
  rm -rf "$work/packed"
  printf '%s\n' "$1" >"$work/$executable"
  bash "$root/scripts/npm-packages.sh" 1.2.3 "$work/packed" "$target=$work/$executable"
}

sha512() {
  node -e 'process.stdout.write("sha512-" + require("node:crypto").createHash("sha512").update(require("node:fs").readFileSync(process.argv[1])).digest("base64"))' "$1"
}

pack one
mkdir "$work/app"
cd "$work/app"
bash "$root/scripts/npm-lock-tarballs.sh" "$work"/packed/*.tgz
npm ci --offline --ignore-scripts --no-audit --no-fund --silent
[[ "$(cat "$installed")" == one ]] || fail "npm ci did not install the packed binary"
[[ -f node_modules/logbrew-cli/bin/logbrew.js ]] || fail "npm ci did not install the launcher package"
launcher="logbrew-cli-1.2.3.tgz"
[[ "$(jq -r '.dependencies["logbrew-cli"]' package.json)" == "file:$launcher" ]] \
  || fail "package.json does not name the copied tarball"
[[ "$(jq -r '.packages["node_modules/logbrew-cli"].integrity' package-lock.json)" == "$(sha512 "$launcher")" ]] \
  || fail "package-lock.json does not pin the tarball hash"

pack two
cp "$work"/packed/*.tgz .
rm -rf node_modules "$npm_config_cache"
if npm ci --offline --ignore-scripts --no-audit --no-fund --silent >/dev/null 2>&1; then
  fail "npm ci installed a tarball whose hash the lockfile does not pin"
fi

bash "$root/scripts/npm-lock-tarballs.sh" "$work"/packed/*.tgz
npm ci --offline --ignore-scripts --no-audit --no-fund --silent
[[ "$(cat "$installed")" == two ]] || fail "npm ci did not install the new build"
printf 'test-npm-lock-tarballs: ok\n'
