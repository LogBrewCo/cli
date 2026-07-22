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


def tracked_paths() -> list[pathlib.Path]:
    output = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    ).stdout
    return [ROOT / raw.decode() for raw in output.split(b"\0") if raw]


def is_policy_surface(path: pathlib.Path) -> bool:
    relative = path.relative_to(ROOT)
    return (
        relative == pathlib.Path("README.md")
        or relative == pathlib.Path("dist-workspace.toml")
        or relative.parts[0] in {".github", "docs", "scripts"}
    )


def normalized_runner(raw: str) -> str:
    value = raw.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        return value[1:-1]
    return value


def main() -> int:
    errors: list[str] = []
    paths = tracked_paths()

    for path in paths:
        if not is_policy_surface(path):
            continue
        try:
            content = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        if FORBIDDEN_PROVIDER in content.lower():
            errors.append(f"unsupported runner provider in {path.relative_to(ROOT)}")

    dist_config = (ROOT / "dist-workspace.toml").read_text(encoding="utf-8")
    if re.search(r"^\[dist\.github-custom-runners\]\s*$", dist_config, re.MULTILINE):
        errors.append("dist-workspace.toml must use cargo-dist runner defaults")

    workflows_dir = ROOT / ".github" / "workflows"
    workflows = sorted(
        {path for suffix in ("*.yml", "*.yaml") for path in workflows_dir.glob(suffix)}
    )
    for workflow in workflows:
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
            runner = normalized_runner(match.group("runner"))
            if runner != DYNAMIC_MATRIX_RUNNER and runner not in OFFICIAL_RUNNERS:
                errors.append(
                    f"unsupported runs-on label in {workflow.relative_to(ROOT)}:{line_number}"
                )

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    print("GitHub-hosted runner policy passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
