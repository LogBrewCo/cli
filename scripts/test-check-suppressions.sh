#!/usr/bin/env bash
#
# test-check-suppressions.sh
# Keep this file identical in every public LogBrew repository.
# Proves that scripts/check-suppressions.sh passes on this repository and on a
# clean repository, fails on each seeded inline suppression with its path and
# line, and fails when it cannot search the repository. The seeded lines are
# built at run time, so this file holds no suppression itself.

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
check="$root/scripts/check-suppressions.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

seed() {
    rm -rf "$work/repo"
    git init -q "$work/repo"
    mkdir -p "$(dirname "$work/repo/$1")"
    printf 'let clean = 1;\n%s\n' "$2" >"$work/repo/$1"
    git -C "$work/repo" add -A
}

expect_failure() {
    local name="$1" file="$2" output
    seed "$file" "$3"
    if output="$(bash "$check" "$work/repo" 2>&1)"; then
        printf 'FAIL: %s passed\n' "$name" >&2
        exit 1
    fi
    grep -qF -- "$file:2:" <<<"$output" || {
        printf 'FAIL: %s did not report %s:2:\n%s\n' "$name" "$file" "$output" >&2
        exit 1
    }
    printf 'ok: %s fails\n' "$name"
}

bash "$check" "$root" >/dev/null
printf 'ok: this repository passes\n'

seed src/lib.rs 'let other = 2;'
bash "$check" "$work/repo" >/dev/null
printf 'ok: a clean repository passes\n'

expect_failure "a Rust allow attribute" src/lib.rs "$(printf '#[%s(dead_code)]' allow)"
expect_failure "a Rust inner expect attribute" src/main.rs "$(printf '#![%s(clippy::unwrap_used)]' expect)"
expect_failure "a Python noqa comment" scripts/tool.py "$(printf 'import os  # %s: E402' noqa)"
expect_failure "a Python type ignore comment" scripts/tool.py "$(printf 'value = 1  # type: %s' ignore)"
expect_failure "a TypeScript expect-error comment" src/index.ts "$(printf '// @ts-%s' expect-error)"
expect_failure "an ESLint disable comment" src/index.js "$(printf '// eslint-%s-next-line' disable)"
expect_failure "a ShellCheck disable comment" scripts/run.sh "$(printf '# shellcheck %s=SC2034' disable)"
expect_failure "a Go nolint comment" main.go "$(printf 'x := 1 //%s:gosec' nolint)"
expect_failure "a Java SuppressWarnings annotation" src/Main.java "$(printf '@Suppress%s("unchecked")' Warnings)"
expect_failure "a SwiftLint disable comment" Sources/App.swift "$(printf '// swiftlint:%s force_cast' disable)"

mkdir -p "$work/plain"
if bash "$check" "$work/plain" >/dev/null 2>&1; then
    printf 'FAIL: a directory outside Git passed\n' >&2
    exit 1
fi
printf 'ok: a directory outside Git fails\n'
