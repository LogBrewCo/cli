#!/usr/bin/env python3
"""Require a Windows release build before crates.io publication."""

from __future__ import annotations

import pathlib
import re
import sys


ROOT = pathlib.Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "publish-crates.yml"
JOB_PATTERN = re.compile(r"^  (?P<name>[a-z0-9-]+):\s*$")


def job_block(lines: list[str], name: str) -> str:
    """Return one top-level workflow job without parsing arbitrary YAML."""
    marker = f"  {name}:"
    try:
        start = lines.index(marker)
    except ValueError as error:
        raise ValueError(f"missing workflow job: {name}") from error

    end = len(lines)
    for index in range(start + 1, len(lines)):
        if JOB_PATTERN.match(lines[index]):
            end = index
            break
    return "\n".join(lines[start:end])


def main() -> int:
    """Validate the release-build dependency and publish-side-effect order."""
    source = WORKFLOW.read_text(encoding="utf-8")
    lines = source.splitlines()
    try:
        windows = job_block(lines, "windows-release-build")
        publish = job_block(lines, "publish")
    except ValueError:
        print("Crates publish workflow contract failed.", file=sys.stderr)
        return 1

    required_windows = (
        "runs-on: windows-2022",
        "cargo build --release --locked --bin logbrew",
    )
    required_publish = (
        "needs: windows-release-build",
        "uses: rust-lang/crates-io-auth-action@v1.0.5",
        "run: cargo publish --locked",
    )
    valid = (
        all(value in windows for value in required_windows)
        and all(value in publish for value in required_publish)
        and "cargo publish" not in windows
        and source.count("cargo publish --locked") == 1
        and source.count("rust-lang/crates-io-auth-action@v1.0.5") == 1
    )
    if not valid:
        print("Crates publish workflow contract failed.", file=sys.stderr)
        return 1

    print("Crates publish workflow contract passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
