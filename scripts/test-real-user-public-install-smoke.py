#!/usr/bin/env python3
"""Offline contract tests for the installed-artifact release verifier."""

from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import os
import pathlib
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import textwrap
import time
import unittest
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
VERIFIER = ROOT / "scripts" / "real_user_public_install_smoke.py"
VERSION = "0.1.19"
sys.dont_write_bytecode = True


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
        import shutil
        import stat
        import sys
        import time

        def record(command):
            with open(os.environ["FAKE_COMMAND_LOG"], "a", encoding="utf-8") as handle:
                handle.write(json.dumps({{
                    "command": command,
                    "args": sys.argv[1:],
                    "home": os.environ.get("HOME"),
                    "has_token": "LOGBREW_TOKEN" in os.environ,
                    "has_receipt_control": "LOGBREW_RELEASE_RECEIPT_MODE" in os.environ,
                    "no_install_cleanup": os.environ.get("HOMEBREW_NO_INSTALL_CLEANUP"),
                }}) + "\\n")

        def install(root, windows=False):
            suffix = ".exe" if windows else ""
            destination = pathlib.Path(root) / "bin" / f"logbrew{{suffix}}"
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(os.environ["FAKE_CLI_BODY"], encoding="utf-8")
            destination.chmod(destination.stat().st_mode | stat.S_IXUSR)

        def install_npm(root):
            root = pathlib.Path(root)
            target = root / "lib" / "node_modules" / "logbrew-cli" / "run-logbrew.js"
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(os.environ["FAKE_CLI_BODY"], encoding="utf-8")
            target.chmod(target.stat().st_mode | stat.S_IXUSR)
            launcher = root / "bin" / "logbrew"
            launcher.parent.mkdir(parents=True, exist_ok=True)
            launcher.symlink_to("../lib/node_modules/logbrew-cli/run-logbrew.js")

        def tap_path(name):
            user, repository = name.split("/", 1)
            return (
                pathlib.Path(os.environ["FAKE_BREW_REPOSITORY"])
                / "Library"
                / "Taps"
                / user
                / f"homebrew-{{repository}}"
            )

        kind = {kind!r}
        record(kind)
        if kind == "cargo":
            install(sys.argv[sys.argv.index("--root") + 1])
        elif kind == "npm":
            install_npm(sys.argv[sys.argv.index("--prefix") + 1])
        elif kind == "pwsh":
            install(os.environ["CARGO_HOME"], windows=True)
        elif kind == "brew":
            args = sys.argv[1:]
            mode = os.environ.get("FAKE_BREW_MODE", "normal")
            if args == ["--repository"]:
                (
                    pathlib.Path(os.environ["FAKE_BREW_REPOSITORY"])
                    / "Library"
                    / "Taps"
                ).mkdir(parents=True, exist_ok=True)
                print(os.environ["FAKE_BREW_REPOSITORY"])
                raise SystemExit(0)
            if args[:2] == ["install", "--formula"] and len(args) == 3:
                qualified = args[2]
                tap = qualified.rsplit("/", 1)[0]
                formula_dir = tap_path(tap) / "Formula"
                formula = formula_dir / "logbrew.rb"
                files = sorted(path.name for path in formula_dir.iterdir())
                if files != ["logbrew.rb"]:
                    raise SystemExit(64)
                import hashlib
                formula_digest = hashlib.sha256(formula.read_bytes()).hexdigest()
                if formula_digest != os.environ["FAKE_FORMULA_SHA256"]:
                    raise SystemExit(64)
                formula_source = formula.read_text(encoding="utf-8")
                if '"aarch64-apple-darwin": {{}}' not in formula_source:
                    raise SystemExit(64)
                if "BINARY_ALIASES[target_triple.to_sym]" not in formula_source:
                    raise SystemExit(64)
                if mode == "install-failure":
                    raise SystemExit(1)
                if mode == "interrupt":
                    time.sleep(30)
                install(os.environ["FAKE_BREW_PREFIX"])
                raise SystemExit(0)
            if args[:2] == ["list", "--versions"] and len(args) == 3:
                print("logbrew 0.1.19")
                raise SystemExit(0)
            if len(args) == 2 and args[0] == "--prefix":
                print(os.environ["FAKE_BREW_PREFIX"])
                raise SystemExit(0)
            if args[:2] == ["uninstall", "--force"] and len(args) == 3:
                shutil.rmtree(os.environ["FAKE_BREW_PREFIX"], ignore_errors=True)
                raise SystemExit(0)
            if args[0:1] == ["untap"] and len(args) == 2:
                destination = tap_path(args[1])
                if destination.is_symlink():
                    destination.unlink()
                else:
                    shutil.rmtree(destination, ignore_errors=True)
                raise SystemExit(0)
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


def load_verifier_module():
    spec = importlib.util.spec_from_file_location("public_install_verifier", VERIFIER)
    if spec is None or spec.loader is None:
        raise AssertionError("verifier module is unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


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
                "FAKE_BREW_REPOSITORY": str(self.temp_dir / "brew-repository"),
                "FAKE_CLI_BODY": cli_source(VERSION),
                "FAKE_COMMAND_LOG": str(self.command_log),
                "FAKE_FORMULA_SHA256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
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
                    "logbrew-cli-0.1.19/Cargo.toml": (
                        b'[package]\nname = "logbrew-cli"\nversion = "0.1.19"\n',
                        0o644,
                    ),
                    "logbrew-cli-0.1.19/Cargo.lock": (b"# fixture\n", 0o644),
                    "logbrew-cli-0.1.19/src/main.rs": (b"fn main() {}\n", 0o644),
                },
            )
            return "crates:logbrew-cli", artifact
        if mode == "homebrew":
            artifact = artifact.with_suffix(".rb")
            artifact.write_text(
                textwrap.dedent(
                    '''
                    class Logbrew < Formula
                      version "0.1.19"
                      BINARY_ALIASES = {
                        "aarch64-apple-darwin": {}
                      }

                      def target_triple
                        "aarch64-apple-darwin"
                      end

                      def install_binary_aliases!
                        BINARY_ALIASES[target_triple.to_sym].each { |_source, _dests| }
                      end
                    end
                    '''
                ).lstrip(),
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
                {"logbrew-0.1.19/logbrew": (cli_source(VERSION).encode(), 0o755)},
            )
            return "native:linux-x64", artifact
        if mode == "npm":
            artifact = artifact.with_suffix(".tar.gz")
            create_tar(
                artifact,
                {
                    "package/package.json": (
                        b'{"name":"logbrew-cli","version":"0.1.19",'
                        b'"bin":{"logbrew":"run-logbrew.js"}}\n',
                        0o644,
                    ),
                    "package/run-logbrew.js": (b"#!/usr/bin/env node\n", 0o755),
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
        self.assertGreaterEqual(len(brew_args), 2)
        tap_name = brew_args[1][2].rsplit("/", 1)[0]
        self.assertRegex(tap_name, r"^logbrew-verifier-[0-9a-f]{16}/receipt$")
        self.assertEqual(
            brew_args,
            [
                ["--repository"],
                ["install", "--formula", f"{tap_name}/logbrew"],
                ["list", "--versions", f"{tap_name}/logbrew"],
                ["--prefix", f"{tap_name}/logbrew"],
                ["uninstall", "--force", f"{tap_name}/logbrew"],
                ["untap", tap_name],
            ],
        )
        self.assertFalse(
            any("LogBrewCo/tap" in argument for args in brew_args for argument in args)
        )
        brew_records = [record for record in records if record["command"] == "brew"]
        self.assertTrue(
            all(record["no_install_cleanup"] == "1" for record in brew_records)
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

    @unittest.skipIf(os.name == "nt", "POSIX npm launchers use symlinks")
    def test_npm_launcher_accepts_only_one_prefix_confined_package_target(self) -> None:
        module = load_verifier_module()
        resolver = getattr(module, "resolve_npm_launcher", None)
        self.assertIsNotNone(resolver)

        def layout(name: str) -> tuple[pathlib.Path, pathlib.Path, pathlib.Path]:
            root = self.temp_dir / name
            target = root / "lib" / "node_modules" / "logbrew-cli" / "run-logbrew.js"
            target.parent.mkdir(parents=True)
            target.write_text("#!/bin/sh\n", encoding="utf-8")
            target.chmod(0o700)
            launcher = root / "bin" / "logbrew"
            launcher.parent.mkdir(parents=True)
            return root, target, launcher

        root, target, launcher = layout("valid-npm")
        launcher.symlink_to("../lib/node_modules/logbrew-cli/run-logbrew.js")
        self.assertEqual(
            resolver(root, "run-logbrew.js"),
            target,
        )

        root, _target, launcher = layout("escaped-npm")
        external = self.temp_dir / "outside-launcher"
        external.write_text("#!/bin/sh\n", encoding="utf-8")
        launcher.symlink_to(external)
        with self.assertRaises(module.VerificationError):
            resolver(root, "run-logbrew.js")

        root, target, launcher = layout("chained-npm")
        real_target = target.with_name("real-target")
        target.rename(real_target)
        target.symlink_to(real_target.name)
        launcher.symlink_to("../lib/node_modules/logbrew-cli/run-logbrew.js")
        with self.assertRaises(module.VerificationError):
            resolver(root, "run-logbrew.js")

        root, target, launcher = layout("missing-npm")
        target.unlink()
        launcher.symlink_to("../lib/node_modules/logbrew-cli/run-logbrew.js")
        with self.assertRaises(module.VerificationError):
            resolver(root, "run-logbrew.js")

        native_root, native_target, native_launcher = layout("native-symlink")
        native_launcher.symlink_to(native_target)
        with self.assertRaises(module.VerificationError):
            module.installed_binary(native_root)

    def test_homebrew_install_failure_removes_only_the_owned_temporary_tap(self) -> None:
        artifact_id, artifact = self.artifact_for("homebrew")
        environment = self.environment(artifact_id, artifact)
        environment["FAKE_BREW_MODE"] = "install-failure"

        result = subprocess.run(
            [sys.executable, str(VERIFIER), "homebrew", VERSION],
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
        records = [
            json.loads(line)
            for line in self.command_log.read_text(encoding="utf-8").splitlines()
        ]
        brew_args = [record["args"] for record in records if record["command"] == "brew"]
        installs = [args for args in brew_args if args[0:2] == ["install", "--formula"]]
        self.assertEqual(len(installs), 1)
        tap_name = installs[0][2].rsplit("/", 1)[0]
        self.assertIn(["untap", tap_name], brew_args)
        self.assertFalse(
            any((self.temp_dir / "brew-repository").rglob("homebrew-receipt"))
        )

    def test_homebrew_interruption_after_tap_creation_still_cleans_up(self) -> None:
        module = load_verifier_module()
        repository = self.temp_dir / "interrupt-repository"
        (repository / "Library" / "Taps").mkdir(parents=True)
        artifact_id, artifact = self.artifact_for("homebrew")
        environment = self.environment(artifact_id, artifact)
        commands: list[list[str]] = []

        def run_command(command, _environment, *, timeout):
            del timeout
            command = list(command)
            commands.append(command)
            if command == ["brew", "--repository"]:
                return str(repository)
            if command[0:3] == ["brew", "install", "--formula"]:
                raise KeyboardInterrupt
            return ""

        with (
            mock.patch.object(module, "run_command", side_effect=run_command),
            mock.patch.object(module.secrets, "token_hex", return_value="c" * 16),
            self.assertRaises(KeyboardInterrupt),
        ):
            module.install_homebrew(artifact, VERSION, environment)

        tap_name = f"logbrew-verifier-{'c' * 16}/receipt"
        self.assertIn(
            ["brew", "uninstall", "--force", f"{tap_name}/logbrew"],
            commands,
        )
        self.assertIn(["brew", "untap", tap_name], commands)
        self.assertFalse(
            any((repository / "Library" / "Taps").rglob("homebrew-receipt"))
        )

    @unittest.skipIf(os.name == "nt", "SIGINT delivery differs on Windows")
    def test_homebrew_interruption_keeps_invocation_output_redacted(self) -> None:
        artifact_id, artifact = self.artifact_for("homebrew")
        environment = self.environment(artifact_id, artifact)
        environment["FAKE_BREW_MODE"] = "interrupt"
        process = subprocess.Popen(
            [sys.executable, str(VERIFIER), "homebrew", VERSION],
            cwd=ROOT,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                if self.command_log.exists() and '"install"' in self.command_log.read_text(
                    encoding="utf-8"
                ):
                    break
                time.sleep(0.05)
            else:
                self.fail("Homebrew install did not start")
            process.send_signal(signal.SIGINT)
            stdout, stderr = process.communicate(timeout=10)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)

        self.assertEqual(process.returncode, 2)
        self.assertEqual(stdout, "")
        self.assertEqual(stderr, "verification_failed\n")
        self.assertNotIn(str(artifact), stderr)
        records = [
            json.loads(line)
            for line in self.command_log.read_text(encoding="utf-8").splitlines()
        ]
        brew_args = [record["args"] for record in records if record["command"] == "brew"]
        installs = [args for args in brew_args if args[0:2] == ["install", "--formula"]]
        self.assertEqual(len(installs), 1)
        tap_name = installs[0][2].rsplit("/", 1)[0]
        self.assertIn(["uninstall", "--force", f"{tap_name}/logbrew"], brew_args)
        self.assertIn(["untap", tap_name], brew_args)
        self.assertFalse(
            any((self.temp_dir / "brew-repository").rglob("homebrew-receipt"))
        )

    def test_homebrew_collision_is_not_claimed_or_removed(self) -> None:
        module = load_verifier_module()
        creator = getattr(module, "create_homebrew_tap", None)
        self.assertIsNotNone(creator)
        repository = self.temp_dir / "collision-repository"
        taps = repository / "Library" / "Taps"
        taps.mkdir(parents=True)
        token = "a" * 16
        destination = taps / f"logbrew-verifier-{token}" / "homebrew-receipt"
        destination.mkdir(parents=True)
        sentinel = destination / "collision"
        sentinel.write_text("preserve", encoding="utf-8")

        with self.assertRaises(module.VerificationError):
            creator(repository, b"formula bytes", token)

        self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve")

    def test_homebrew_tap_symlink_escape_preserves_external_content(self) -> None:
        module = load_verifier_module()
        creator = getattr(module, "create_homebrew_tap", None)
        self.assertIsNotNone(creator)
        repository = self.temp_dir / "escape-repository"
        taps = repository / "Library" / "Taps"
        taps.mkdir(parents=True)
        token = "b" * 16
        external = self.temp_dir / "brew-external"
        external.mkdir()
        sentinel = external / "sentinel"
        sentinel.write_text("preserve", encoding="utf-8")
        user_path = taps / f"logbrew-verifier-{token}"
        user_path.symlink_to(external, target_is_directory=True)

        with self.assertRaises(module.VerificationError):
            creator(repository, b"formula bytes", token)

        self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve")
        self.assertFalse((external / "Formula" / "logbrew.rb").exists())

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
        module = load_verifier_module()

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
