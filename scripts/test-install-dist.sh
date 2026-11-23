#!/usr/bin/env bash
# Proves that scripts/install-dist.sh refuses a dist archive whose sha256 is not
# the one it pins, and installs nothing from it.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

version="$(sed -n 's/^version=//p' "$root/scripts/install-dist.sh")"
mkdir -p "$work/release/v$version" "$work/payload"
printf '#!/bin/sh\necho tampered\n' >"$work/payload/dist"
chmod 755 "$work/payload/dist"
for target in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu; do
  mkdir -p "$work/payload/cargo-dist-$target"
  cp "$work/payload/dist" "$work/payload/cargo-dist-$target/dist"
  tar -cJf "$work/release/v$version/cargo-dist-$target.tar.xz" -C "$work/payload" "cargo-dist-$target"
done

if CARGO_HOME="$work/cargo" LOGBREW_DIST_DOWNLOAD_BASE="file://$work/release" \
  bash "$root/scripts/install-dist.sh" 2>"$work/error"; then
  printf 'test-install-dist: install-dist.sh installed an archive with another sha256\n' >&2
  exit 1
fi
grep -q 'expected' "$work/error" || { cat "$work/error" >&2; exit 1; }
[[ ! -e "$work/cargo/bin/dist" ]] || {
  printf 'test-install-dist: install-dist.sh left a dist binary behind\n' >&2
  exit 1
}
printf 'test-install-dist: ok\n'
