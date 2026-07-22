#!/usr/bin/env python3
"""Offline contract tests for the installed-artifact release verifier."""

from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import os
import pathlib
import stat
import subprocess
import sys
import tarfile
import tempfile
import textwrap
import unittest
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
VERIFIER = ROOT / "scripts" / "real_user_public_install_smoke.py"
VERSION = "0.1.18"


def write_executable(path: pathlib.Path, source: str) -> None:
    path.write_text(textwrap.dedent(source).lstrip(), encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def cli_source(version: str, *, request_health: bool = True) -> str:
    return textwrap.dedent(
        f"""
        #!/usr/bin/env python3
        import json
        import os
        import sys
        import urllib.request

        REQUEST_HEALTH = {request_health!r}

        with open(os.environ["FAKE_COMMAND_LOG"], "a", encoding="utf-8") as handle:
            handle.write(json.dumps({{
                "command": "logbrew",
                "home": os.environ.get("HOME"),
                "has_token": "LOGBREW_TOKEN" in os.environ,
                "has_receipt_control": "LOGBREW_RELEASE_RECEIPT_MODE" in os.environ,
            }}) + "\\n")

        if sys.argv[1:] == ["--version"]:
            print("logbrew {version}")
        elif sys.argv[1:] == ["status", "--json"]:
            body = "ok"
            if REQUEST_HEALTH:
                with urllib.request.urlopen(
                    os.environ["LOGBREW_API_URL"] + "/health",
                    timeout=5,
                ) as response:
                    body = response.read().decode("utf-8")
            print(json.dumps({{
                "ok": True,
                "status": "reachable",
                "status_code": 200,
                "body": body,
                "api_url": os.environ["LOGBREW_API_URL"],
                "authenticated": False,
                "auth_source": "missing",
                "next": "run logbrew login",
            }}, separators=(",", ":")))
        else:
            raise SystemExit(64)
        """
    ).lstrip()


def write_fake_installer_command(path: pathlib.Path, kind: str) -> None:
    write_executable(
        path,
        f"""
        #!/usr/bin/env python3
        import json
        import os
        import pathlib
        import stat
        import sys

        def record(command):
            with open(os.environ["FAKE_COMMAND_LOG"], "a", encoding="utf-8") as handle:
                handle.write(json.dumps({{
                    "command": command,
                    "args": sys.argv[1:],
                    "home": os.environ.get("HOME"),
                    "has_token": "LOGBREW_TOKEN" in os.environ,
                    "has_receipt_control": "LOGBREW_RELEASE_RECEIPT_MODE" in os.environ,
                }}) + "\\n")

        def install(root, windows=False):
            suffix = ".exe" if windows else ""
            destination = pathlib.Path(root) / "bin" / f"logbrew{{suffix}}"
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(os.environ["FAKE_CLI_BODY"], encoding="utf-8")
            destination.chmod(destination.stat().st_mode | stat.S_IXUSR)

        kind = {kind!r}
        record(kind)
        if kind == "cargo":
            install(sys.argv[sys.argv.index("--root") + 1])
        elif kind == "npm":
            install(sys.argv[sys.argv.index("--prefix") + 1])
        elif kind == "pwsh":
            install(os.environ["CARGO_HOME"], windows=True)
        elif kind == "brew":
            if sys.argv[1:3] == ["--prefix", "logbrew"]:
                print(os.environ["FAKE_BREW_PREFIX"])
                raise SystemExit(0)
            elif sys.argv[1:3] == ["install", "--formula"]:
                install(os.environ["FAKE_BREW_PREFIX"])
            elif sys.argv[1:3] != ["uninstall", "--force"]:
                raise SystemExit(64)
        print("unsafe-child-output")
        """,
    )


def create_tar(
    path: pathlib.Path,
    members: dict[str, tuple[bytes, int]],
) -> None:
    with tarfile.open(path, "w:gz") as archive:
        for name, (content, mode) in members.items():
            info = tarfile.TarInfo(name)
            info.size = len(content)
            info.mode = mode
            archive.addfile(info, io.BytesIO(content))


class PublicInstallVerifierTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.temp_dir = pathlib.Path(self.temp.name)
        self.fake_bin = self.temp_dir / "fake-bin"
        self.fake_bin.mkdir()
        self.command_log = self.temp_dir / "commands.jsonl"
        for command in ("cargo", "npm", "pwsh", "brew"):
            write_fake_installer_command(self.fake_bin / command, command)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def environment(self, artifact_id: str, artifact: pathlib.Path) -> dict[str, str]:
        environment = os.environ.copy()
        environment.update(
            {
                "PATH": f"{self.fake_bin}{os.pathsep}{environment['PATH']}",
                "FAKE_BREW_PREFIX": str(self.temp_dir / "brew-prefix"),
                "FAKE_CLI_BODY": cli_source(VERSION),
                "FAKE_COMMAND_LOG": str(self.command_log),
                "LOGBREW_RELEASE_RECEIPT_MODE": "1",
                "LOGBREW_RELEASE_ARTIFACT_FILES_JSON": json.dumps(
                    {artifact_id: str(artifact)}, separators=(",", ":")
                ),
                "LOGBREW_TOKEN": "must-not-reach-child-processes",
            }
        )
        return environment

    def artifact_for(self, mode: str) -> tuple[str, pathlib.Path]:
        artifact = self.temp_dir / f"{mode}.artifact"
        if mode == "crates":
            artifact = artifact.with_suffix(".crate")
            create_tar(
                artifact,
                {
                    "logbrew-cli-0.1.18/Cargo.toml": (
                        b'[package]\nname = "logbrew-cli"\nversion = "0.1.18"\n',
                        0o644,
                    ),
                    "logbrew-cli-0.1.18/Cargo.lock": (b"# fixture\n", 0o644),
                    "logbrew-cli-0.1.18/src/main.rs": (b"fn main() {}\n", 0o644),
                },
            )
            return "crates:logbrew-cli", artifact
        if mode == "homebrew":
            artifact = artifact.with_suffix(".rb")
            artifact.write_text(
                'class Logbrew < Formula\n  version "0.1.18"\nend\n',
                encoding="utf-8",
            )
            return "homebrew:LogBrewCo/tap/logbrew", artifact
        if mode == "powershell":
            artifact = artifact.with_suffix(".ps1")
            artifact.write_text("# fixture installer\n", encoding="utf-8")
            return "installer:powershell", artifact
        if mode == "shell":
            artifact = artifact.with_suffix(".sh")
            write_executable(
                artifact,
                """
                #!/bin/sh
                python3 - <<'PY'
                import json
                import os
                with open(os.environ["FAKE_COMMAND_LOG"], "a", encoding="utf-8") as handle:
                    handle.write(json.dumps({
                        "command": "shell",
                        "home": os.environ.get("HOME"),
                        "has_token": "LOGBREW_TOKEN" in os.environ,
                        "has_receipt_control": "LOGBREW_RELEASE_RECEIPT_MODE" in os.environ,
                    }) + "\\n")
                PY
                mkdir -p "$CARGO_HOME/bin"
                printf '%s' "$FAKE_CLI_BODY" > "$CARGO_HOME/bin/logbrew"
                chmod +x "$CARGO_HOME/bin/logbrew"
                printf 'unsafe-installer-output\\n'
                """,
            )
            return "installer:shell", artifact
        if mode == "native":
            artifact = artifact.with_suffix(".tar.gz")
            create_tar(
                artifact,
                {"logbrew-0.1.18/logbrew": (cli_source(VERSION).encode(), 0o755)},
            )
            return "native:linux-x64", artifact
        if mode == "npm":
            artifact = artifact.with_suffix(".tar.gz")
            create_tar(
                artifact,
                {
                    "package/package.json": (
                        b'{"name":"logbrew-cli","version":"0.1.18"}\n',
                        0o644,
                    )
                },
            )
            return "npm:logbrew-cli", artifact
        self.fail(f"unsupported fixture mode: {mode}")

    def run_verifier(
        self,
        mode: str,
        version: str = VERSION,
        artifact_override: tuple[str, pathlib.Path] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        artifact_id, artifact = artifact_override or self.artifact_for(mode)
        return subprocess.run(
            [sys.executable, str(VERIFIER), mode, version],
            cwd=ROOT,
            env=self.environment(artifact_id, artifact),
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )

    def test_each_mode_executes_installed_cli_and_emits_exact_attestation(self) -> None:
        for mode in ("crates", "homebrew", "powershell", "shell", "native", "npm"):
            with self.subTest(mode=mode):
                artifact_id, artifact = self.artifact_for(mode)
                result = self.run_verifier(mode, artifact_override=(artifact_id, artifact))

                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")
                self.assertEqual(
                    json.loads(result.stdout),
                    {
                        "schema_version": 1,
                        "status": "passed",
                        "artifacts": [
                            {
                                "id": artifact_id,
                                "digest": "sha256:"
                                + hashlib.sha256(artifact.read_bytes()).hexdigest(),
                            }
                        ],
                    },
                )

        records = [
            json.loads(line)
            for line in self.command_log.read_text(encoding="utf-8").splitlines()
        ]
        self.assertTrue(records)
        self.assertTrue(
            all(record["home"] != os.environ.get("HOME") for record in records)
        )
        self.assertTrue(all(record["has_token"] is False for record in records))
        self.assertTrue(
            all(record["has_receipt_control"] is False for record in records)
        )
        brew_args = [
            record["args"] for record in records if record["command"] == "brew"
        ]
        self.assertEqual(
            brew_args,
            [
                ["install", "--formula", mock.ANY],
                ["--prefix", "logbrew"],
                ["uninstall", "--force", "logbrew"],
            ],
        )

    def test_wrong_installed_version_fails_without_echoing_values(self) -> None:
        artifact = self.temp_dir / "wrong-version.tar.gz"
        create_tar(
            artifact,
            {"logbrew/logbrew": (cli_source("0.1.17").encode(), 0o755)},
        )

        result = self.run_verifier(
            "native",
            artifact_override=("native:linux-x64", artifact),
        )

        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "verification_failed\n")
        self.assertNotIn("0.1.17", result.stderr)
        self.assertNotIn(str(artifact), result.stderr)

    def test_unsafe_native_archive_fails_closed(self) -> None:
        artifact = self.temp_dir / "unsafe.tar.gz"
        create_tar(artifact, {"../escape": (b"unsafe", 0o755)})

        result = self.run_verifier(
            "native",
            artifact_override=("native:linux-x64", artifact),
        )

        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "verification_failed\n")
        self.assertFalse((self.temp_dir.parent / "escape").exists())

    def test_windows_separator_archive_traversal_is_rejected_by_contract(self) -> None:
        spec = importlib.util.spec_from_file_location("public_install_verifier", VERIFIER)
        if spec is None or spec.loader is None:
            self.fail("verifier module is unavailable")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)

        with self.assertRaises(module.VerificationError):
            module.safe_member_path(self.temp_dir, "..\\escape")

    def test_status_output_without_a_real_health_request_is_rejected(self) -> None:
        artifact = self.temp_dir / "status-stub.tar.gz"
        create_tar(
            artifact,
            {
                "logbrew/logbrew": (
                    cli_source(VERSION, request_health=False).encode(),
                    0o755,
                )
            },
        )

        result = self.run_verifier(
            "native",
            artifact_override=("native:linux-x64", artifact),
        )

        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "verification_failed\n")

    def test_artifact_map_rejects_extra_surface(self) -> None:
        artifact_id, artifact = self.artifact_for("npm")
        environment = self.environment(artifact_id, artifact)
        environment["LOGBREW_RELEASE_ARTIFACT_FILES_JSON"] = json.dumps(
            {artifact_id: str(artifact), "extra": str(artifact)}
        )

        result = subprocess.run(
            [sys.executable, str(VERIFIER), "npm", VERSION],
            cwd=ROOT,
            env=environment,
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )

        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "verification_failed\n")


if __name__ == "__main__":
    unittest.main()
