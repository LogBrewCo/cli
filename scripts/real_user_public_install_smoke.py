#!/usr/bin/env python3
"""Verify one supplied public CLI artifact by installing and executing it."""

from __future__ import annotations

import hashlib
import http.server
import json
import os
import pathlib
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import threading
import tomllib
import zipfile
from collections.abc import Mapping, Sequence


RECEIPT_MODE_ENV = "LOGBREW_RELEASE_RECEIPT_MODE"
ARTIFACT_FILES_ENV = "LOGBREW_RELEASE_ARTIFACT_FILES_JSON"
MAX_ARTIFACT_BYTES = 512 * 1024 * 1024
MAX_COMMAND_OUTPUT_BYTES = 1024 * 1024
INSTALL_TIMEOUT_SECONDS = 900
EXECUTION_TIMEOUT_SECONDS = 30
VERSION_PATTERN = re.compile(
    r"^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$"
)
NATIVE_ARTIFACT_IDS = frozenset(
    {
        "native:linux-arm64",
        "native:linux-x64",
        "native:macos-arm64",
        "native:macos-x64",
        "native:windows-x64",
    }
)
MODE_ARTIFACT_IDS = {
    "crates": frozenset({"crates:logbrew-cli"}),
    "homebrew": frozenset({"homebrew:LogBrewCo/tap/logbrew"}),
    "powershell": frozenset({"installer:powershell"}),
    "shell": frozenset({"installer:shell"}),
    "native": NATIVE_ARTIFACT_IDS,
    "npm": frozenset({"npm:logbrew-cli"}),
}
MODE_SUFFIXES = {
    "crates": (".crate",),
    "homebrew": (".rb",),
    "powershell": (".ps1",),
    "shell": (".sh",),
    "native": (".tar.gz", ".tar.xz", ".zip"),
    "npm": (".tar.gz", ".tgz"),
}
SENSITIVE_ENVIRONMENT_NAMES = frozenset(
    {
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "NODE_AUTH_TOKEN",
        "NPM_TOKEN",
    }
)
STATUS_KEYS = frozenset(
    {
        "ok",
        "status",
        "status_code",
        "body",
        "api_url",
        "authenticated",
        "auth_source",
        "next",
    }
)


class VerificationError(RuntimeError):
    """Raised when supplied bytes do not prove the installed contract."""


class HealthHandler(http.server.BaseHTTPRequestHandler):
    """Serve the one public health response required by `logbrew status`."""

    def do_GET(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API
        self.server.request_paths.append(self.path)  # type: ignore[attr-defined]
        if self.path != "/health":
            self.send_error(404)
            return
        body = b"ok"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format: str, *args: object) -> None:
        del args


class HealthServer(http.server.ThreadingHTTPServer):
    """Track the exact loopback request made by the installed CLI."""

    def __init__(self) -> None:
        super().__init__(("127.0.0.1", 0), HealthHandler)
        self.request_paths: list[str] = []


def clean_environment() -> dict[str, str]:
    """Copy process state without forwarding credentials or verifier controls."""
    return {
        name: value
        for name, value in os.environ.items()
        if not name.startswith("LOGBREW_")
        and name not in SENSITIVE_ENVIRONMENT_NAMES
    }


def run_command(
    command: Sequence[str],
    environment: Mapping[str, str],
    *,
    timeout: int,
) -> str:
    """Run a child with bounded, fully captured output."""
    try:
        result = subprocess.run(
            list(command),
            env=dict(environment),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise VerificationError from error
    if result.returncode != 0:
        raise VerificationError
    if (
        len(result.stdout) > MAX_COMMAND_OUTPUT_BYTES
        or len(result.stderr) > MAX_COMMAND_OUTPUT_BYTES
    ):
        raise VerificationError
    try:
        return result.stdout.decode("utf-8")
    except UnicodeDecodeError as error:
        raise VerificationError from error


def artifact_suffix(mode: str, path: pathlib.Path) -> str:
    """Return the allowlisted release suffix without trusting the full filename."""
    for suffix in MODE_SUFFIXES[mode]:
        if path.name.endswith(suffix):
            return suffix
    raise VerificationError


def read_regular_file(path: pathlib.Path) -> bytes:
    """Read one bounded regular file without following a final symlink."""
    try:
        if path.is_symlink():
            raise VerificationError
    except OSError as error:
        raise VerificationError from error
    flags = os.O_RDONLY
    flags |= getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
        with os.fdopen(descriptor, "rb") as handle:
            metadata = os.fstat(handle.fileno())
            if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= MAX_ARTIFACT_BYTES:
                raise VerificationError
            content = handle.read(MAX_ARTIFACT_BYTES + 1)
    except (OSError, ValueError) as error:
        raise VerificationError from error
    if not content or len(content) > MAX_ARTIFACT_BYTES:
        raise VerificationError
    return content


def supplied_artifact(
    mode: str,
    workspace: pathlib.Path,
) -> tuple[str, pathlib.Path, str]:
    """Copy and hash the one exact artifact owned by this invocation."""
    if os.environ.get(RECEIPT_MODE_ENV) != "1":
        raise VerificationError
    try:
        payload = json.loads(os.environ[ARTIFACT_FILES_ENV])
    except (KeyError, json.JSONDecodeError, TypeError) as error:
        raise VerificationError from error
    if (
        not isinstance(payload, dict)
        or len(payload) != 1
        or not set(payload).issubset(MODE_ARTIFACT_IDS[mode])
    ):
        raise VerificationError
    artifact_id, raw_path = next(iter(payload.items()))
    if not isinstance(raw_path, str) or not raw_path or "\x00" in raw_path:
        raise VerificationError
    source = pathlib.Path(raw_path)
    suffix = artifact_suffix(mode, source)
    content = read_regular_file(source)
    destination_names = {
        "crates": "logbrew-cli.crate",
        "homebrew": "logbrew.rb",
        "powershell": "install.ps1",
        "shell": "install.sh",
        "native": f"logbrew{suffix}",
        "npm": f"logbrew-cli{suffix}",
    }
    destination = workspace / destination_names[mode]
    destination.write_bytes(content)
    digest = f"sha256:{hashlib.sha256(content).hexdigest()}"
    return artifact_id, destination, digest


def safe_member_path(destination: pathlib.Path, member_name: str) -> pathlib.Path:
    """Resolve one archive member without allowing absolute or parent traversal."""
    member = pathlib.PurePosixPath(member_name)
    if (
        "\x00" in member_name
        or "\\" in member_name
        or member.is_absolute()
        or not member.parts
        or ".." in member.parts
        or any(":" in part for part in member.parts)
    ):
        raise VerificationError
    parts = [part for part in member.parts if part not in {"", "."}]
    if not parts:
        raise VerificationError
    target = destination.joinpath(*parts)
    try:
        target.resolve(strict=False).relative_to(destination.resolve())
    except (OSError, ValueError) as error:
        raise VerificationError from error
    return target


def extract_tar(archive_path: pathlib.Path, destination: pathlib.Path) -> None:
    """Extract regular files and directories from a bounded tar archive."""
    try:
        with tarfile.open(archive_path, "r:*") as archive:
            members = archive.getmembers()
            if not members or sum(member.size for member in members) > MAX_ARTIFACT_BYTES:
                raise VerificationError
            for member in members:
                target = safe_member_path(destination, member.name)
                if member.isdir():
                    target.mkdir(parents=True, exist_ok=True)
                    continue
                if not member.isfile() or target.exists():
                    raise VerificationError
                target.parent.mkdir(parents=True, exist_ok=True)
                source = archive.extractfile(member)
                if source is None:
                    raise VerificationError
                with source, target.open("xb") as output:
                    shutil.copyfileobj(source, output)
                target.chmod(member.mode & 0o777)
    except (OSError, tarfile.TarError) as error:
        raise VerificationError from error


def extract_zip(archive_path: pathlib.Path, destination: pathlib.Path) -> None:
    """Extract regular files and directories from a bounded zip archive."""
    try:
        with zipfile.ZipFile(archive_path) as archive:
            members = archive.infolist()
            if not members or sum(member.file_size for member in members) > MAX_ARTIFACT_BYTES:
                raise VerificationError
            for member in members:
                target = safe_member_path(destination, member.filename)
                unix_mode = member.external_attr >> 16
                file_type = stat.S_IFMT(unix_mode)
                if member.is_dir():
                    target.mkdir(parents=True, exist_ok=True)
                    continue
                if (
                    member.flag_bits & 0x1
                    or file_type not in {0, stat.S_IFREG}
                    or target.exists()
                ):
                    raise VerificationError
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(member) as source, target.open("xb") as output:
                    shutil.copyfileobj(source, output)
                if unix_mode:
                    target.chmod(unix_mode & 0o777)
    except (OSError, zipfile.BadZipFile) as error:
        raise VerificationError from error


def extract_archive(archive_path: pathlib.Path, destination: pathlib.Path) -> None:
    """Safely extract a supported release archive."""
    destination.mkdir()
    if archive_path.name.endswith(".zip"):
        extract_zip(archive_path, destination)
    else:
        extract_tar(archive_path, destination)


def find_one(root: pathlib.Path, name: str) -> pathlib.Path:
    """Return exactly one non-symlink regular file with the requested name."""
    matches = [
        path
        for path in root.rglob(name)
        if path.is_file() and not path.is_symlink()
    ]
    if len(matches) != 1:
        raise VerificationError
    return matches[0]


def install_crate(
    artifact: pathlib.Path,
    version: str,
    workspace: pathlib.Path,
    environment: dict[str, str],
) -> pathlib.Path:
    """Install the exact supplied crate source into an isolated Cargo root."""
    source_tree = workspace / "crate"
    extract_archive(artifact, source_tree)
    manifest = find_one(source_tree, "Cargo.toml")
    try:
        package = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError, KeyError, TypeError) as error:
        raise VerificationError from error
    if package.get("name") != "logbrew-cli" or package.get("version") != version:
        raise VerificationError
    if not (manifest.parent / "Cargo.lock").is_file():
        raise VerificationError
    install_root = workspace / "cargo-install"
    environment["CARGO_HOME"] = str(workspace / "cargo-home")
    run_command(
        [
            "cargo",
            "install",
            "--locked",
            "--force",
            "--root",
            str(install_root),
            "--path",
            str(manifest.parent),
        ],
        environment,
        timeout=INSTALL_TIMEOUT_SECONDS,
    )
    return installed_binary(install_root)


def install_npm(
    artifact: pathlib.Path,
    version: str,
    workspace: pathlib.Path,
    environment: dict[str, str],
) -> pathlib.Path:
    """Install the exact supplied npm package into an isolated prefix."""
    source_tree = workspace / "npm-package"
    extract_archive(artifact, source_tree)
    package_json = find_one(source_tree, "package.json")
    try:
        package = json.loads(package_json.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise VerificationError from error
    if (
        not isinstance(package, dict)
        or package.get("name") != "logbrew-cli"
        or package.get("version") != version
    ):
        raise VerificationError
    install_root = workspace / "npm-install"
    environment["npm_config_cache"] = str(workspace / "npm-cache")
    run_command(
        [
            "npm",
            "install",
            "--global",
            "--prefix",
            str(install_root),
            "--no-audit",
            "--no-fund",
            str(artifact),
        ],
        environment,
        timeout=INSTALL_TIMEOUT_SECONDS,
    )
    return installed_binary(install_root, npm=True)


def install_shell(
    artifact: pathlib.Path,
    workspace: pathlib.Path,
    environment: dict[str, str],
) -> pathlib.Path:
    """Execute the exact supplied shell installer into an isolated Cargo home."""
    cargo_home = workspace / "shell-install"
    environment["CARGO_HOME"] = str(cargo_home)
    environment["CI"] = "true"
    run_command(
        ["bash", str(artifact)],
        environment,
        timeout=INSTALL_TIMEOUT_SECONDS,
    )
    return installed_binary(cargo_home)


def install_powershell(
    artifact: pathlib.Path,
    workspace: pathlib.Path,
    environment: dict[str, str],
) -> pathlib.Path:
    """Execute the exact supplied PowerShell installer into an isolated Cargo home."""
    cargo_home = workspace / "powershell-install"
    environment["CARGO_HOME"] = str(cargo_home)
    environment["CI"] = "true"
    shell = shutil.which("pwsh", path=environment.get("PATH")) or shutil.which(
        "powershell", path=environment.get("PATH")
    )
    if shell is None:
        raise VerificationError
    run_command(
        [
            shell,
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(artifact),
        ],
        environment,
        timeout=INSTALL_TIMEOUT_SECONDS,
    )
    return installed_binary(cargo_home, windows=True)


def install_homebrew(
    artifact: pathlib.Path,
    version: str,
    environment: dict[str, str],
) -> pathlib.Path:
    """Install the exact supplied formula and return its isolated Cellar binary."""
    try:
        formula = artifact.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as error:
        raise VerificationError from error
    version_declaration = re.compile(
        rf"(?m)^\s*version\s+[\"']{re.escape(version)}[\"']\s*$"
    )
    class_declaration = re.compile(r"(?m)^class Logbrew < Formula\s*$")
    if (
        len(version_declaration.findall(formula)) != 1
        or len(class_declaration.findall(formula)) != 1
    ):
        raise VerificationError
    environment["HOMEBREW_NO_AUTO_UPDATE"] = "1"
    run_command(
        ["brew", "install", "--formula", str(artifact)],
        environment,
        timeout=INSTALL_TIMEOUT_SECONDS,
    )
    try:
        prefix = run_command(
            ["brew", "--prefix", "logbrew"],
            environment,
            timeout=EXECUTION_TIMEOUT_SECONDS,
        ).strip()
        if not prefix or "\x00" in prefix:
            raise VerificationError
        return installed_binary(pathlib.Path(prefix))
    except VerificationError:
        uninstall_homebrew(environment)
        raise


def uninstall_homebrew(environment: Mapping[str, str]) -> None:
    """Remove the formula installed by this ephemeral verifier invocation."""
    run_command(
        ["brew", "uninstall", "--force", "logbrew"],
        environment,
        timeout=INSTALL_TIMEOUT_SECONDS,
    )


def install_native(artifact: pathlib.Path, workspace: pathlib.Path) -> pathlib.Path:
    """Extract and return the one executable from an exact native release archive."""
    source_tree = workspace / "native"
    extract_archive(artifact, source_tree)
    candidates: list[pathlib.Path] = []
    for name in ("logbrew", "logbrew.exe"):
        candidates.extend(
            path
            for path in source_tree.rglob(name)
            if path.is_file() and not path.is_symlink()
        )
    if len(candidates) != 1:
        raise VerificationError
    binary = candidates[0]
    if os.name != "nt":
        binary.chmod(binary.stat().st_mode | stat.S_IXUSR)
    return binary


def installed_binary(
    root: pathlib.Path,
    *,
    npm: bool = False,
    windows: bool = False,
) -> pathlib.Path:
    """Resolve one expected installed binary without scanning unrelated paths."""
    if npm and os.name == "nt":
        candidates = (root / "logbrew.cmd", root / "logbrew.exe")
    elif windows:
        candidates = (root / "bin" / "logbrew.exe",)
    else:
        candidates = (root / "bin" / "logbrew",)
    matches = [
        path
        for path in candidates
        if path.is_file() and not path.is_symlink()
    ]
    if len(matches) != 1:
        raise VerificationError
    return matches[0]


def verify_cli(
    binary: pathlib.Path,
    version: str,
    workspace: pathlib.Path,
    environment: Mapping[str, str],
) -> None:
    """Execute the installed version and its credential-free status contract."""
    version_output = run_command(
        [str(binary), "--version"],
        environment,
        timeout=EXECUTION_TIMEOUT_SECONDS,
    )
    if version_output.strip() != f"logbrew {version}":
        raise VerificationError

    server = HealthServer()
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        status_environment = dict(environment)
        status_environment["HOME"] = str(workspace / "home")
        status_environment["USERPROFILE"] = str(workspace / "home")
        status_environment["LOGBREW_API_URL"] = (
            f"http://127.0.0.1:{server.server_address[1]}"
        )
        status_output = run_command(
            [str(binary), "status", "--json"],
            status_environment,
            timeout=EXECUTION_TIMEOUT_SECONDS,
        )
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=EXECUTION_TIMEOUT_SECONDS)
    if server.request_paths != ["/health"]:
        raise VerificationError
    try:
        status = json.loads(status_output)
    except json.JSONDecodeError as error:
        raise VerificationError from error
    if (
        not isinstance(status, dict)
        or set(status) != STATUS_KEYS
        or status.get("ok") is not True
        or status.get("status") != "reachable"
        or status.get("status_code") != 200
        or status.get("body") != "ok"
        or status.get("api_url") != status_environment["LOGBREW_API_URL"]
        or status.get("authenticated") is not False
        or status.get("auth_source") != "missing"
        or status.get("next") != "run logbrew login"
    ):
        raise VerificationError


def verify_mode(
    mode: str,
    version: str,
    artifact: pathlib.Path,
    workspace: pathlib.Path,
) -> None:
    """Install one family through its public user surface and execute the result."""
    environment = clean_environment()
    isolated_home = workspace / "home"
    isolated_home.mkdir()
    environment["HOME"] = str(isolated_home)
    environment["USERPROFILE"] = str(isolated_home)
    environment["XDG_CONFIG_HOME"] = str(isolated_home / "config")
    cleanup_homebrew = False
    if mode == "crates":
        binary = install_crate(artifact, version, workspace, environment)
    elif mode == "homebrew":
        binary = install_homebrew(artifact, version, environment)
        cleanup_homebrew = True
    elif mode == "powershell":
        binary = install_powershell(artifact, workspace, environment)
    elif mode == "shell":
        binary = install_shell(artifact, workspace, environment)
    elif mode == "native":
        binary = install_native(artifact, workspace)
    elif mode == "npm":
        binary = install_npm(artifact, version, workspace, environment)
    else:
        raise VerificationError
    try:
        verify_cli(binary, version, workspace, environment)
    finally:
        if cleanup_homebrew:
            uninstall_homebrew(environment)


def parse_invocation(argv: Sequence[str]) -> tuple[str, str]:
    """Parse the fixed two-argument public verifier interface."""
    if len(argv) != 2 or argv[0] not in MODE_ARTIFACT_IDS:
        raise VerificationError
    mode, version = argv
    if VERSION_PATTERN.fullmatch(version) is None:
        raise VerificationError
    return mode, version


def main(argv: Sequence[str] | None = None) -> int:
    """Run one exact verifier invocation with fixed, value-safe failure output."""
    try:
        mode, version = parse_invocation(list(argv or sys.argv[1:]))
        with tempfile.TemporaryDirectory(prefix="logbrew-public-install-") as raw_workspace:
            workspace = pathlib.Path(raw_workspace)
            artifact_id, artifact, digest = supplied_artifact(mode, workspace)
            verify_mode(mode, version, artifact, workspace)
        print(
            json.dumps(
                {
                    "schema_version": 1,
                    "status": "passed",
                    "artifacts": [{"id": artifact_id, "digest": digest}],
                },
                separators=(",", ":"),
            )
        )
    except Exception:  # Fail closed without exposing artifact or subprocess values.
        print("verification_failed", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
