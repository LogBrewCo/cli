#!/usr/bin/env bash
#
# check-suppressions.sh
# Keep this file identical in every public LogBrew repository.
# Fails when a tracked source file turns off a lint or type check inline, for
# example with an allow or expect attribute, a noqa comment, or a ts-expect-error
# comment. Fix the finding instead. When a rule is wrong for a whole kind of
# file, change the linter configuration and give the reason there.
# Usage: bash scripts/check-suppressions.sh [ROOT]

set -euo pipefail

root="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
# Each alternative is written so that this line does not match itself.
pattern='#!?\[(allow|expect)\(|eslint-[d]isable|oxlint-[d]isable|biome-[i]gnore|@ts-(ignore|expect-error|nocheck)|#[[:space:]]*type:[[:space:]]*ignore|#[[:space:]]*noqa|pyright:[[:space:]]*ignore|swiftlint:[d]isable|//[[:space:]]*nolint|NOLIN[T]|@Suppress[W]arnings|@Suppress\(|#pragma warning [d]isable|rubocop:[d]isable|phpstan-[i]gnore|shellcheck [d]isable'
files=('*.rs' '*.ts' '*.tsx' '*.js' '*.mjs' '*.cjs' '*.py' '*.swift' '*.go' '*.rb' '*.java' '*.kt' '*.php' '*.cs' '*.sh' '*.c' '*.h' '*.cpp' '*.m')

status=0
found="$(git -C "$root" grep -n -I -E "$pattern" -- "${files[@]}")" || status=$?
case "$status" in
    0)
        printf '%s\n' "$found" >&2
        printf 'suppressions: %s inline suppressions found; fix each finding instead\n' "$(grep -c '' <<<"$found")" >&2
        exit 1
        ;;
    1) echo "suppressions: none in tracked source files" ;;
    *)
        printf 'suppressions: cannot search %s (git grep exit %s)\n' "$root" "$status" >&2
        exit 2
        ;;
esac
