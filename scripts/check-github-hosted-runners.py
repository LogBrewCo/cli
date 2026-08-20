#!/usr/bin/env python3
"""Enforce GitHub-hosted runners across public CI and release surfaces."""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys


ROOT = pathlib.Path(__file__).resolve().parents[1]
OFFICIAL_RUNNERS = frozenset(
    {
        "macos-14",
        "macos-15",
        "macos-15-intel",
        "macos-latest",
        "ubuntu-22.04",
        "ubuntu-22.04-arm",
        "ubuntu-24.04",
        "ubuntu-24.04-arm",
        "ubuntu-latest",
        "windows-2022",
        "windows-2025",
        "windows-latest",
    }
)
DYNAMIC_MATRIX_RUNNER = "${{ matrix.runner }}"
FORBIDDEN_PROVIDER = "black" + "smith"
RUNS_ON_PATTERN = re.compile(
    r"^\s*runs-on:\s*(?P<runner>[^#]+?)\s*(?:#.*)?$"
)
RUNS_ON_DECLARATION = re.compile(r"^\s*runs-on\s*:")


def main() -> int:
    errors: list[str] = []
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT)
    for raw in filter(None, tracked.split(b"\0")):
        relative = pathlib.Path(raw.decode())
        if relative not in {
            pathlib.Path("README.md"),
            pathlib.Path("dist-workspace.toml"),
        } and relative.parts[0] not in {".github", "docs", "scripts"}:
            continue
        try:
            content = (ROOT / relative).read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        if FORBIDDEN_PROVIDER in content.lower():
            errors.append(f"unsupported runner provider in {relative}")

    dist_config = (ROOT / "dist-workspace.toml").read_text(encoding="utf-8")
    custom_runner = 'github-custom-runners = { "x86_64-pc-windows-msvc" = "windows-2025" }'
    if dist_config.count("github-custom-runners") != 1 or custom_runner not in dist_config:
        errors.append("dist-workspace.toml must use the official Windows 2025 runner")

    for workflow in sorted((ROOT / ".github" / "workflows").glob("*.y*ml")):
        for line_number, line in enumerate(
            workflow.read_text(encoding="utf-8").splitlines(), start=1
        ):
            match = RUNS_ON_PATTERN.match(line)
            if match is None:
                if RUNS_ON_DECLARATION.match(line):
                    errors.append(
                        f"unsupported runs-on declaration in "
                        f"{workflow.relative_to(ROOT)}:{line_number}"
                    )
                continue
            runner = match.group("runner").strip()
            if len(runner) >= 2 and runner[0] == runner[-1] and runner[0] in {"'", '"'}:
                runner = runner[1:-1]
            if runner != DYNAMIC_MATRIX_RUNNER and runner not in OFFICIAL_RUNNERS:
                errors.append(
                    f"unsupported runs-on label in {workflow.relative_to(ROOT)}:{line_number}"
                )

    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1

    print("GitHub-hosted runner policy passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
