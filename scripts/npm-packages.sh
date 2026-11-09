#!/usr/bin/env bash
# Pack the npm release: one @logbrew/cli-<os>-<cpu> package per native binary
# and the logbrew-cli launcher package that depends on them. No package has
# install scripts; npm selects the binary through os, cpu, and libc.
set -euo pipefail

usage() {
  printf 'usage: bash scripts/npm-packages.sh <version> <output-dir> <rust-target>=<binary>...\n' >&2
  exit 1
}
[[ $# -ge 3 && "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || usage
version="$1"
output="$2"
shift 2

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
template="$root/npm/package.json"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$output"

for pair in "$@"; do
  target="${pair%%=*}"
  binary="${pair#*=}"
  case "$target" in
    aarch64-apple-darwin) os=darwin cpu=arm64 ;;
    x86_64-apple-darwin) os=darwin cpu=x64 ;;
    aarch64-unknown-linux-gnu) os=linux cpu=arm64 ;;
    x86_64-unknown-linux-gnu) os=linux cpu=x64 ;;
    x86_64-pc-windows-msvc) os=win32 cpu=x64 ;;
    *) printf 'unsupported target: %s\n' "$target" >&2; exit 1 ;;
  esac
  [[ -f "$binary" && ! -L "$binary" ]] || { printf 'missing binary for %s\n' "$target" >&2; exit 1; }
  name="@logbrew/cli-$os-$cpu"
  jq -e --arg name "$name" '.optionalDependencies | has($name)' "$template" >/dev/null
  package="$work/$os-$cpu"
  executable=logbrew
  [[ "$os" == win32 ]] && executable=logbrew.exe
  mkdir -p "$package/bin"
  cp "$binary" "$package/bin/$executable"
  chmod 755 "$package/bin/$executable"
  cp "$root/LICENSE" "$package/"
  printf '# %s\n\nThe %s-%s binary for [logbrew-cli](https://www.npmjs.com/package/logbrew-cli).\nInstall logbrew-cli instead of this package.\n' \
    "$name" "$os" "$cpu" > "$package/README.md"
  jq -n --arg name "$name" --arg version "$version" --arg os "$os" --arg cpu "$cpu" '{
    name: $name,
    version: $version,
    description: "The \($os)-\($cpu) binary for logbrew-cli, the official LogBrew CLI.",
    homepage: "https://logbrew.co",
    repository: {type: "git", url: "git+https://github.com/LogBrewCo/cli.git"},
    license: "MIT",
    os: [$os],
    cpu: [$cpu],
    preferUnplugged: true
  } + (if $os == "linux" then {libc: ["glibc"]} else {} end)' > "$package/package.json"
  npm pack --loglevel=warn --pack-destination "$output" "$package" >/dev/null
done

main="$work/logbrew-cli"
mkdir -p "$main/bin"
cp "$root/npm/logbrew.js" "$main/bin/logbrew.js"
chmod 755 "$main/bin/logbrew.js"
cp "$root/npm/README.md" "$root/LICENSE" "$main/"
jq --arg version "$version" '.version = $version | .optionalDependencies |= map_values($version)' \
  "$template" > "$main/package.json"
npm pack --loglevel=warn --pack-destination "$output" "$main" >/dev/null
