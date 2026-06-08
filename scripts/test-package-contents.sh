#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

package_files="$(cargo package --list --allow-dirty)"

required_files=(
  Cargo.lock
  Cargo.toml
  LICENSE
  README.md
  src/main.rs
  src/lib.rs
  tests/commands.rs
)

for file in "${required_files[@]}"; do
  if ! grep -Fxq "$file" <<<"$package_files"; then
    printf 'expected crates.io package to include %s\n' "$file" >&2
    exit 1
  fi
done

forbidden_patterns=(
  '^\.github/'
  '^AGENTS\.md$'
  '^scripts/'
  '^dist-workspace\.toml$'
  '^wix/'
)

for pattern in "${forbidden_patterns[@]}"; do
  if grep -Eq "$pattern" <<<"$package_files"; then
    printf 'crates.io package includes non-runtime repository file matching %s\n' "$pattern" >&2
    printf 'package files:\n%s\n' "$package_files" >&2
    exit 1
  fi
done
