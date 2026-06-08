#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

bash scripts/confidentiality-check.sh
cargo fmt --all -- --check
cargo clippy --lib --bin logbrew --all-features -- -D warnings
cargo test --all-targets --all-features
cargo publish --dry-run --locked --allow-dirty
