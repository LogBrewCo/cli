#!/usr/bin/env python3
"""Compile permission helpers for Windows with unused code denied."""

from __future__ import annotations

import pathlib
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
STORE = ROOT / "src" / "auth" / "store.rs"
TARGET = "x86_64-pc-windows-msvc"
FUNCTIONS = ("secure_file_permissions", "secure_path_permissions")
EXPECTED_UNUSED_DIAGNOSTICS = (
    "unused variable: `file`",
    "unused variable: `path`",
)


def extract_function(source: str, name: str) -> str:
    """Extract one complete function definition from the credential store."""
    start = source.index(f"fn {name}(")
    body_start = source.index("{", start)
    depth = 0
    for index in range(body_start, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[start : index + 1]
    raise ValueError(f"unterminated function: {name}")


def main() -> int:
    """Compile the actual permission helpers under the Windows cfg."""
    source = STORE.read_text(encoding="utf-8")
    functions = "\n\n".join(
        extract_function(source, name).replace(f"fn {name}(", f"pub fn {name}(", 1)
        for name in FUNCTIONS
    )
    fixture = (
        "#![deny(unused)]\n"
        "pub type RuntimeError = std::io::Error;\n\n"
        f"{functions}\n"
    )

    with tempfile.TemporaryDirectory(prefix="logbrew-windows-auth-store-") as raw_dir:
        fixture_path = pathlib.Path(raw_dir) / "auth_store.rs"
        fixture_path.write_text(fixture, encoding="utf-8")
        result = subprocess.run(
            [
                "rustc",
                "--crate-type=lib",
                "--edition=2024",
                "--emit=metadata",
                "--out-dir",
                raw_dir,
                "--target",
                TARGET,
                str(fixture_path),
            ],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )

    if result.returncode != 0:
        if all(message in result.stderr for message in EXPECTED_UNUSED_DIAGNOSTICS):
            print(
                "Windows auth-store compile contract failed on both permission parameters.",
                file=sys.stderr,
            )
            return 1
        print("Windows auth-store compile contract failed.", file=sys.stderr)
        return 2

    print("Windows auth-store compile contract passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
