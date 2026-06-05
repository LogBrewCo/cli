#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v rg >/dev/null 2>&1; then
    printf 'ripgrep is required for the confidentiality check.\n' >&2
    exit 1
fi

scan_pattern() {
    local pattern="$1"
    local status

    set +e
    rg -n --hidden --glob '!target/**' --glob '!.git/**' \
        --glob '!scripts/confidentiality-check.sh' \
        --glob '!.logbrew-confidential-denylist.local' "$pattern" .
    status="$?"
    set -e

    if [[ "$status" -eq 0 ]]; then
        printf 'Confidentiality check failed. Remove private/backend-only details before committing.\n' >&2
        exit 1
    fi

    if [[ "$status" -gt 1 ]]; then
        printf 'Confidentiality check failed because ripgrep could not scan the repo.\n' >&2
        exit "$status"
    fi
}

public_secret_pattern='AWS_ACCESS_KEY_ID|AWS_SECRET_ACCESS_KEY|GOOGLE_APPLICATION_CREDENTIALS|DATABASE_URL=|JWT_SECRET=|PASSWORD=|SECRET_KEY=|PRIVATE_KEY|-----BEGIN [A-Z ]*PRIVATE KEY-----|Bearer [A-Za-z0-9._=-]{20,}|ghp_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]+|sk-[A-Za-z0-9]{20,}'

scan_pattern "$public_secret_pattern"

local_denylist="$ROOT_DIR/.logbrew-confidential-denylist.local"
if [[ -f "$local_denylist" ]]; then
    while IFS= read -r pattern || [[ -n "$pattern" ]]; do
        if [[ -z "${pattern//[[:space:]]/}" || "$pattern" =~ ^[[:space:]]*# ]]; then
            continue
        fi

        scan_pattern "$pattern"
    done < "$local_denylist"
fi

printf 'Confidentiality check passed.\n'
