#!/usr/bin/env bash
# Installs dist (cargo-dist) for the release workflow from its GitHub release
# archive into $CARGO_HOME/bin. The archive must match the sha256 pinned below
# for this runner, or nothing is installed. To update dist, change version and
# copy each hash from the .sha256 file next to the archive on its release page.
# Usage: bash scripts/install-dist.sh
set -euo pipefail

version=0.34.0
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) target=aarch64-apple-darwin sha256=d1eec30e63eed23e5a5bd48fe96eae314799811bffca254ec1d2264638ba1890 ;;
  Darwin-x86_64) target=x86_64-apple-darwin sha256=6aa89bb1e7c18f51ae8b148ce2fc6ab00ac46ed8173d4f2ed9afa12101218773 ;;
  Linux-aarch64) target=aarch64-unknown-linux-gnu sha256=58efb33edf01342875bfd0c3ce2befa9d73075579ca1b3e5dd75b98c88f92e83 ;;
  Linux-x86_64) target=x86_64-unknown-linux-gnu sha256=7fee44ff3ff71ed0064b32d4ec62fee6b58bd37c5887b12a86aedddc5af0c7b5 ;;
  MINGW*-x86_64 | MSYS*-x86_64) target=x86_64-pc-windows-msvc sha256=7bb53fbc998321f77a3bbb044084a52747c8ed041cf6d6a604802aa57b9c80c6 ;;
  *) printf 'install-dist: unsupported runner %s-%s\n' "$(uname -s)" "$(uname -m)" >&2; exit 1 ;;
esac
archive="cargo-dist-$target.tar.xz"
[[ "$target" == *windows* ]] && archive="cargo-dist-$target.zip"
# The test points this at a local copy; the pinned hash applies to every source.
base="${LOGBREW_DIST_DOWNLOAD_BASE:-https://github.com/axodotdev/cargo-dist/releases/download}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
curl --proto '=https,file' --tlsv1.2 -fLsS --retry 8 -o "$work/$archive" "$base/v$version/$archive"
if command -v sha256sum >/dev/null; then
  actual="$(sha256sum "$work/$archive" | cut -d ' ' -f 1)"
else
  actual="$(shasum -a 256 "$work/$archive" | cut -d ' ' -f 1)"
fi
[[ "$actual" == "$sha256" ]] || {
  printf 'install-dist: %s has sha256 %s, expected %s\n' "$archive" "$actual" "$sha256" >&2
  exit 1
}

bin="${CARGO_HOME:-$HOME/.cargo}/bin"
mkdir -p "$bin"
if [[ "$archive" == *.zip ]]; then
  unzip -q "$work/$archive" dist.exe -d "$bin"
else
  tar -xJf "$work/$archive" -C "$work"
  cp "$work/cargo-dist-$target/dist" "$bin/dist"
fi
"$bin/dist" --version
