#!/usr/bin/env python3
"""Produce one strict attestation for an installed public release artifact."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import platform as host_platform
import re
import stat
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass


MAX_API_BYTES = 1024 * 1024
MAX_ASSET_BYTES = 64 * 1024 * 1024
MAX_CHECKSUM_BYTES = 64 * 1024
MAX_VERIFIER_OUTPUT_BYTES = 16 * 1024
MAX_RELEASED_VERIFIER_BYTES = 256 * 1024
MAX_GITHUB_TOKEN_BYTES = 1024
NETWORK_TIMEOUT_SECONDS = 60
VERIFIER_TIMEOUT_SECONDS = 1200
COMMIT_PATTERN = re.compile(r"^[0-9a-f]{40}$")
RELEASE_RUN_PATTERN = re.compile(r"^[1-9][0-9]{0,19}$")
TAG_PATTERN = re.compile(
    r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$"
)
TIMESTAMP_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"
)
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
SAFE_ASSET_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
ATTESTATION_KEYS = frozenset(
    {
        "artifact_id",
        "version",
        "source",
        "release_run",
        "workflow_head",
        "execution_platform",
        "digest",
        "status",
    }
)
WORKFLOW_PATH = ".github/workflows/installed-release-attestations.yml"
RELEASE_WORKFLOW_PATH = ".github/workflows/release.yml"
VERIFIER_PATH = pathlib.PurePosixPath("scripts/real_user_public_install_smoke.py")
REPOSITORY = "LogBrewCo/cli"
RELEASE_WORKFLOW_ID = 289984708


class AttestationError(RuntimeError):
    """Raised when public evidence does not match the fixed release policy."""


@dataclass(frozen=True)
class ReceiptSpec:
    """One supported public artifact and real execution platform."""

    runner: str
    platform: str
    mode: str
    artifact_id: str
    asset_name: str


@dataclass(frozen=True)
class ReceiptPolicy(ReceiptSpec):
    """One release-bound public artifact."""

    asset_id: int
    asset_size: int
    digest: str


@dataclass(frozen=True)
class ReleasePolicy:
    """Immutable public inputs accepted by this attestation workflow."""

    tag: str
    version: str
    source_commit: str
    release_run_id: int
    checksum_asset_name: str
    checksum_asset_id: int
    checksum_asset_size: int
    checksum_asset_digest: str
    receipts: Mapping[str, ReceiptPolicy]


RECEIPTS = {
    "shell-linux-x64": ReceiptSpec(
        "ubuntu-24.04", "linux-x64", "shell", "installer:shell", "logbrew-cli-installer.sh"
    ),
    "native-linux-arm64": ReceiptSpec(
        "ubuntu-24.04-arm", "linux-arm64", "native", "native:linux-arm64",
        "logbrew-cli-aarch64-unknown-linux-gnu.tar.xz",
    ),
    "native-linux-x64": ReceiptSpec(
        "ubuntu-24.04", "linux-x64", "native", "native:linux-x64",
        "logbrew-cli-x86_64-unknown-linux-gnu.tar.xz",
    ),
    "powershell-windows-x64": ReceiptSpec(
        "windows-2025", "windows-x64", "powershell", "installer:powershell",
        "logbrew-cli-installer.ps1",
    ),
    "native-windows-x64": ReceiptSpec(
        "windows-2025", "windows-x64", "native", "native:windows-x64",
        "logbrew-cli-x86_64-pc-windows-msvc.zip",
    ),
    "native-macos-x64": ReceiptSpec(
        "macos-15-intel", "macos-x64", "native", "native:macos-x64",
        "logbrew-cli-x86_64-apple-darwin.tar.xz",
    ),
}


def release_identity(
    tag: str,
    source_commit: str,
    release_run: str,
) -> tuple[str, int]:
    """Validate and normalize one immutable release identity."""
    match = TAG_PATTERN.fullmatch(tag)
    if (
        match is None
        or COMMIT_PATTERN.fullmatch(source_commit) is None
        or RELEASE_RUN_PATTERN.fullmatch(release_run) is None
    ):
        raise AttestationError
    return tag[1:], int(release_run)


def validate_matrix_inputs(
    receipt: ReceiptSpec,
    mode: str,
    artifact_id: str,
    asset: str,
    execution_platform: str,
) -> None:
    """Bind every visible matrix value to the selected receipt policy."""
    identity = mode, artifact_id, asset, execution_platform
    policy_identity = receipt.mode, receipt.artifact_id, receipt.asset_name, receipt.platform
    if identity != policy_identity:
        raise AttestationError


def validated_tag_object_sha(
    tag: str,
    source_commit: str,
    reference: Mapping[str, object],
    tag_object: Mapping[str, object],
) -> str:
    """Require one annotated tag object to resolve to the requested source."""
    reference_object = reference.get("object")
    target_object = tag_object.get("object")
    if (
        reference.get("ref") != f"refs/tags/{tag}"
        or not isinstance(reference_object, dict)
        or reference_object.get("type") != "tag"
        or COMMIT_PATTERN.fullmatch(str(reference_object.get("sha", ""))) is None
        or tag_object.get("tag") != tag
        or not isinstance(target_object, dict)
        or target_object.get("type") != "commit"
        or target_object.get("sha") != source_commit
    ):
        raise AttestationError
    return str(reference_object["sha"])


def validate_release_run_identity(
    tag: str,
    source_commit: str,
    release_run_id: int,
    run: Mapping[str, object],
) -> None:
    """Require the exact successful authoritative release workflow run."""
    expected = {
        "id": release_run_id,
        "name": "Release",
        "path": RELEASE_WORKFLOW_PATH,
        "event": "push",
        "status": "completed",
        "conclusion": "success",
        "head_branch": tag,
        "head_sha": source_commit,
        "workflow_id": RELEASE_WORKFLOW_ID,
    }
    attempt = run.get("run_attempt")
    if type(attempt) is not int or attempt < 1 or not (expected.items() <= run.items()):
        raise AttestationError


def release_download_url(repository: str, tag: str, asset_name: str) -> str:
    """Return the only accepted browser download URL for one release asset."""
    if SAFE_ASSET_PATTERN.fullmatch(asset_name) is None:
        raise AttestationError
    return f"https://github.com/{repository}/releases/download/{tag}/{asset_name}"


def select_exact_asset(
    assets: Sequence[object],
    name: str,
) -> Mapping[str, object]:
    """Select one named release asset without accepting duplicates."""
    matches = [
        asset
        for asset in assets
        if isinstance(asset, dict) and asset.get("name") == name
    ]
    if len(matches) != 1:
        raise AttestationError
    return matches[0]


def asset_identity(
    tag: str,
    asset: Mapping[str, object],
    name: str,
    maximum_size: int,
) -> tuple[int, int, str]:
    """Read a bounded immutable asset identity from public release metadata."""
    asset_id = asset.get("id")
    size = asset.get("size")
    api_digest = asset.get("digest")
    if (
        not isinstance(asset_id, int)
        or asset_id <= 0
        or asset.get("name") != name
        or asset.get("state") != "uploaded"
        or not isinstance(size, int)
        or not 0 < size <= maximum_size
        or not isinstance(api_digest, str)
        or not api_digest.startswith("sha256:")
        or SHA256_PATTERN.fullmatch(api_digest[7:]) is None
        or asset.get("browser_download_url")
        != release_download_url(REPOSITORY, tag, name)
    ):
        raise AttestationError
    return asset_id, size, api_digest[7:]


def checksum_entries(content: bytes) -> Mapping[str, str]:
    """Parse one bounded cargo-dist SHA-256 manifest without path semantics."""
    if not content or len(content) > MAX_CHECKSUM_BYTES:
        raise AttestationError
    try:
        lines = content.decode("ascii").splitlines()
    except UnicodeDecodeError as error:
        raise AttestationError from error
    if lines and lines[-1] == "":
        lines.pop()
    entries: dict[str, str] = {}
    for line in lines:
        match = re.fullmatch(r"([0-9a-f]{64}) \*([A-Za-z0-9][A-Za-z0-9._-]{0,127})", line)
        if match is None or match.group(2) in entries:
            raise AttestationError
        entries[match.group(2)] = match.group(1)
    if not entries:
        raise AttestationError
    return entries


def validate_artifact_bytes(
    receipt: ReceiptPolicy,
    asset: Mapping[str, object],
    content: bytes,
    checksum_content: bytes | None,
) -> str:
    """Bind downloaded bytes to API digest and cargo-dist checksum metadata."""
    digest = hashlib.sha256(content).hexdigest()
    if (
        len(content) != receipt.asset_size
        or digest != receipt.digest
        or asset.get("digest") != f"sha256:{digest}"
    ):
        raise AttestationError
    if receipt.mode == "native":
        if checksum_content is None:
            raise AttestationError
        if checksum_entries(checksum_content).get(receipt.asset_name) != digest:
            raise AttestationError
    elif checksum_content is not None:
        raise AttestationError
    return f"sha256:{digest}"


def platform_identity(system: str, machine: str) -> tuple[str, str, str]:
    """Normalize only the actual hosted platforms used by this workflow."""
    normalized = (system.casefold(), machine.casefold())
    identities = {
        ("linux", "x86_64"): ("linux-x64", "Linux", "X64"),
        ("linux", "aarch64"): ("linux-arm64", "Linux", "ARM64"),
        ("linux", "arm64"): ("linux-arm64", "Linux", "ARM64"),
        ("windows", "amd64"): ("windows-x64", "Windows", "X64"),
        ("windows", "x86_64"): ("windows-x64", "Windows", "X64"),
        ("darwin", "x86_64"): ("macos-x64", "macOS", "X64"),
    }
    try:
        return identities[normalized]
    except KeyError as error:
        raise AttestationError from error


def validate_workflow_context(
    environment: Mapping[str, str],
    receipt: ReceiptSpec,
    *,
    system: str,
    machine: str,
) -> str:
    """Bind execution to the protected workflow and physical runner platform."""
    workflow_head = environment.get("GITHUB_SHA", "")
    expected_workflow_ref = f"{REPOSITORY}/{WORKFLOW_PATH}@refs/heads/main"
    actual_platform, runner_os, runner_arch = platform_identity(system, machine)
    if (
        environment.get("GITHUB_ACTIONS") != "true"
        or environment.get("GITHUB_EVENT_NAME")
        not in {"workflow_dispatch", "workflow_run"}
        or environment.get("GITHUB_REF") != "refs/heads/main"
        or environment.get("GITHUB_REPOSITORY") != REPOSITORY
        or environment.get("GITHUB_WORKFLOW_REF") != expected_workflow_ref
        or COMMIT_PATTERN.fullmatch(workflow_head) is None
        or environment.get("GITHUB_WORKFLOW_SHA") != workflow_head
        or environment.get("RUNNER_OS") != runner_os
        or environment.get("RUNNER_ARCH") != runner_arch
        or actual_platform != receipt.platform
    ):
        raise AttestationError
    return workflow_head


def validate_verifier_output(
    stdout: bytes,
    stderr: bytes,
    artifact_id: str,
    digest: str,
) -> None:
    """Accept only the released verifier's exact single-line receipt."""
    if (
        stderr
        or not stdout
        or len(stdout) > MAX_VERIFIER_OUTPUT_BYTES
    ):
        raise AttestationError
    if stdout.endswith(b"\r\n"):
        payload = stdout[:-2]
    elif stdout.endswith(b"\n"):
        payload = stdout[:-1]
    else:
        raise AttestationError
    if (
        not payload
        or b"\r" in payload
        or b"\n" in payload
        or b"\x00" in payload
    ):
        raise AttestationError
    try:
        receipt = json.loads(payload)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AttestationError from error
    expected = {
        "schema_version": 1,
        "status": "passed",
        "artifacts": [{"id": artifact_id, "digest": digest}],
    }
    canonical = json.dumps(expected, separators=(",", ":")).encode()
    if receipt != expected or payload != canonical:
        raise AttestationError


def build_attestation(
    receipt: ReceiptPolicy,
    workflow_head: str,
    digest: str,
    policy: ReleasePolicy,
) -> dict[str, object]:
    """Build the fixed minimal public attestation."""
    attestation: dict[str, object] = {
        "artifact_id": receipt.artifact_id,
        "version": policy.version,
        "source": policy.source_commit,
        "release_run": policy.release_run_id,
        "workflow_head": workflow_head,
        "execution_platform": receipt.platform,
        "digest": digest,
        "status": "passed",
    }
    validate_attestation(attestation, policy)
    return attestation


def validate_attestation(
    attestation: Mapping[str, object],
    policy: ReleasePolicy,
) -> None:
    """Validate the exact public attestation schema and policy values."""
    if set(attestation) != ATTESTATION_KEYS:
        raise AttestationError
    artifact_id = attestation.get("artifact_id")
    platform = attestation.get("execution_platform")
    matching = [
        receipt
        for receipt in policy.receipts.values()
        if receipt.artifact_id == artifact_id and receipt.platform == platform
    ]
    if len(matching) != 1:
        raise AttestationError
    receipt = matching[0]
    if (
        attestation.get("version") != policy.version
        or attestation.get("source") != policy.source_commit
        or attestation.get("release_run") != policy.release_run_id
        or not isinstance(attestation.get("workflow_head"), str)
        or COMMIT_PATTERN.fullmatch(str(attestation["workflow_head"])) is None
        or attestation.get("digest") != f"sha256:{receipt.digest}"
        or attestation.get("status") != "passed"
    ):
        raise AttestationError


def write_attestation(
    path: pathlib.Path,
    attestation: Mapping[str, object],
    policy: ReleasePolicy,
) -> None:
    """Create one owner-only attestation without following or replacing links."""
    validate_attestation(attestation, policy)
    if not path.is_absolute() or "\x00" in str(path):
        raise AttestationError
    try:
        parent = path.parent
        parent_metadata = parent.lstat()
        if not stat.S_ISDIR(parent_metadata.st_mode) or parent.is_symlink():
            raise AttestationError
        content = json.dumps(attestation, separators=(",", ":")).encode() + b"\n"
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
        flags |= getattr(os, "O_CLOEXEC", 0)
        flags |= getattr(os, "O_NOFOLLOW", 0)
        descriptor = os.open(path, flags, 0o600)
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        metadata = path.lstat()
        if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
            raise AttestationError
        if path.read_bytes() != content:
            raise AttestationError
    except (OSError, ValueError) as error:
        raise AttestationError from error


class NoRedirectHandler(urllib.request.HTTPRedirectHandler):
    """Reject redirects for GitHub metadata API reads."""

    def redirect_request(self, request, file_pointer, code, message, headers, new_url):
        del request, file_pointer, code, message, headers, new_url
        raise AttestationError


class ReleaseRedirectHandler(urllib.request.HTTPRedirectHandler):
    """Allow only GitHub's release asset redirect destination."""

    def redirect_request(self, request, file_pointer, code, message, headers, new_url):
        parsed = urllib.parse.urlsplit(new_url)
        if (
            parsed.scheme != "https"
            or parsed.hostname != "release-assets.githubusercontent.com"
            or parsed.username is not None
            or parsed.password is not None
            or parsed.fragment
        ):
            raise AttestationError
        return super().redirect_request(
            request,
            file_pointer,
            code,
            message,
            headers,
            new_url,
        )


def read_response(response, maximum: int) -> bytes:
    """Read one bounded HTTP response."""
    length = response.headers.get("Content-Length")
    if length is not None:
        try:
            if not 0 < int(length) <= maximum:
                raise AttestationError
        except ValueError as error:
            raise AttestationError from error
    content = response.read(maximum + 1)
    if not content or len(content) > maximum:
        raise AttestationError
    return content


def github_api_headers(token: str) -> Mapping[str, str]:
    """Build fixed API headers from one bounded GitHub Actions job token."""
    if (
        not isinstance(token, str)
        or not token
        or not token.isascii()
        or len(token.encode("ascii")) > MAX_GITHUB_TOKEN_BYTES
        or any(ord(character) < 0x20 or ord(character) == 0x7F for character in token)
    ):
        raise AttestationError
    return {
        "Accept": "application/vnd.github+json",
        "Authorization": f"Bearer {token}",
        "User-Agent": "logbrew-installed-attestation",
        "X-GitHub-Api-Version": "2022-11-28",
    }


def fetch_json(url: str, token: str) -> Mapping[str, object]:
    """Read one bounded public GitHub API object with the scoped job token."""
    parsed = urllib.parse.urlsplit(url)
    if (
        parsed.scheme != "https"
        or parsed.hostname != "api.github.com"
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
    ):
        raise AttestationError
    request = urllib.request.Request(
        url,
        headers=github_api_headers(token),
    )
    try:
        with urllib.request.build_opener(NoRedirectHandler()).open(
            request, timeout=NETWORK_TIMEOUT_SECONDS
        ) as response:
            content = read_response(response, MAX_API_BYTES)
        payload = json.loads(content)
    except (
        OSError,
        TimeoutError,
        urllib.error.URLError,
        json.JSONDecodeError,
    ) as error:
        raise AttestationError from error
    if not isinstance(payload, dict):
        raise AttestationError
    return payload


def github_metadata_reader(
    environment: Mapping[str, str],
) -> Callable[[str], Mapping[str, object]]:
    """Bind public API reads to the one scoped GitHub Actions job token."""
    token = environment.get("GITHUB_TOKEN", "")
    github_api_headers(token)
    return lambda url: fetch_json(url, token)


def download_release_asset(url: str, expected_size: int) -> bytes:
    """Download one exact bounded release asset over allowlisted HTTPS redirects."""
    parsed = urllib.parse.urlsplit(url)
    if (
        parsed.scheme != "https"
        or parsed.hostname != "github.com"
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or expected_size <= 0
    ):
        raise AttestationError
    request = urllib.request.Request(
        url,
        headers={"User-Agent": "logbrew-installed-attestation"},
    )
    try:
        with urllib.request.build_opener(ReleaseRedirectHandler()).open(
            request, timeout=NETWORK_TIMEOUT_SECONDS
        ) as response:
            content = read_response(response, expected_size)
    except (OSError, TimeoutError, urllib.error.URLError) as error:
        raise AttestationError from error
    if len(content) != expected_size:
        raise AttestationError
    return content


def metadata_urls(tag: str, release_run_id: int) -> dict[str, str]:
    """Return the public metadata endpoints known before tag resolution."""
    encoded_tag = urllib.parse.quote(tag, safe="")
    return {
        "tag_ref": f"https://api.github.com/repos/{REPOSITORY}/git/ref/tags/{encoded_tag}",
        "release_run": (
            f"https://api.github.com/repos/{REPOSITORY}/actions/runs/"
            f"{release_run_id}"
        ),
        "release": (
            f"https://api.github.com/repos/{REPOSITORY}/releases/tags/{encoded_tag}"
        ),
    }


def discover_release_policy(
    tag: str,
    source_commit: str,
    release_run: str,
    metadata_reader: Callable[[str], Mapping[str, object]],
) -> tuple[ReleasePolicy, Mapping[str, object]]:
    """Bind one successful tag release to its immutable public asset metadata."""
    version, release_run_id = release_identity(tag, source_commit, release_run)
    urls = metadata_urls(tag, release_run_id)
    reference = metadata_reader(urls["tag_ref"])
    reference_object = reference.get("object")
    if not isinstance(reference_object, dict):
        raise AttestationError
    tag_object_sha = str(reference_object.get("sha", ""))
    if COMMIT_PATTERN.fullmatch(tag_object_sha) is None:
        raise AttestationError
    tag_object = metadata_reader(
        f"https://api.github.com/repos/{REPOSITORY}/git/tags/{tag_object_sha}"
    )
    if validated_tag_object_sha(tag, source_commit, reference, tag_object) != tag_object_sha:
        raise AttestationError
    validate_release_run_identity(
        tag,
        source_commit,
        release_run_id,
        metadata_reader(urls["release_run"]),
    )
    release = metadata_reader(urls["release"])
    release_id = release.get("id")
    published_at = release.get("published_at")
    assets = release.get("assets")
    if (
        not isinstance(release_id, int)
        or release_id <= 0
        or release.get("tag_name") != tag
        or release.get("target_commitish") != source_commit
        or release.get("draft") is not False
        or release.get("prerelease") is not False
        or not isinstance(published_at, str)
        or TIMESTAMP_PATTERN.fullmatch(published_at) is None
        or not isinstance(assets, list)
    ):
        raise AttestationError

    checksum = select_exact_asset(assets, "sha256.sum")
    checksum_id, checksum_size, checksum_digest = asset_identity(
        tag,
        checksum,
        "sha256.sum",
        MAX_CHECKSUM_BYTES,
    )
    receipts = {}
    for name, spec in RECEIPTS.items():
        asset_id, asset_size, digest = asset_identity(
            tag,
            select_exact_asset(assets, spec.asset_name),
            spec.asset_name,
            MAX_ASSET_BYTES,
        )
        receipts[name] = ReceiptPolicy(
            spec.runner,
            spec.platform,
            spec.mode,
            spec.artifact_id,
            spec.asset_name,
            asset_id,
            asset_size,
            digest,
        )
    return (
        ReleasePolicy(
            tag,
            version,
            source_commit,
            release_run_id,
            "sha256.sum",
            checksum_id,
            checksum_size,
            checksum_digest,
            receipts,
        ),
        release,
    )


def exact_git_output_line(output: bytes) -> bytes:
    """Remove exactly one platform newline from one bounded Git output line."""
    if output.endswith(b"\r\n"):
        line = output[:-2]
    elif output.endswith(b"\n"):
        line = output[:-1]
    else:
        raise AttestationError
    if not line or b"\r" in line or b"\n" in line or b"\x00" in line:
        raise AttestationError
    return line


def validate_released_source(path: pathlib.Path, source_commit: str) -> bytes:
    """Read the exact released verifier blob from a checkout at the fixed commit."""
    try:
        if not path.is_absolute() or path.is_symlink():
            raise AttestationError
        metadata = path.lstat()
        if not stat.S_ISDIR(metadata.st_mode):
            raise AttestationError
        result = subprocess.run(
            ["git", "-C", str(path), "rev-parse", "HEAD"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
        if (
            result.returncode != 0
            or result.stderr
            or exact_git_output_line(result.stdout) != source_commit.encode()
        ):
            raise AttestationError
        tracked = subprocess.run(
            [
                "git",
                "-C",
                str(path),
                "ls-tree",
                source_commit,
                "--",
                str(VERIFIER_PATH),
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
        tree_match = re.fullmatch(
            rb"100(?:644|755) blob ([0-9a-f]{40})\t"
            rb"scripts/real_user_public_install_smoke\.py",
            exact_git_output_line(tracked.stdout),
        )
        if (
            tracked.returncode != 0
            or tracked.stderr
            or tree_match is None
        ):
            raise AttestationError
        blob_id = tree_match.group(1)
        blob_size_result = subprocess.run(
            ["git", "-C", str(path), "cat-file", "-s", blob_id.decode()],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
        blob_size_line = exact_git_output_line(blob_size_result.stdout)
        if (
            blob_size_result.returncode != 0
            or blob_size_result.stderr
            or re.fullmatch(rb"[1-9][0-9]*", blob_size_line) is None
            or len(blob_size_line) > len(str(MAX_RELEASED_VERIFIER_BYTES))
        ):
            raise AttestationError
        blob_size = int(blob_size_line)
        if blob_size > MAX_RELEASED_VERIFIER_BYTES:
            raise AttestationError
        blob_result = subprocess.run(
            ["git", "-C", str(path), "cat-file", "blob", blob_id.decode()],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
        verifier_bytes = blob_result.stdout
        object_bytes = (
            f"blob {len(verifier_bytes)}\0".encode() + verifier_bytes
        )
        computed_blob_id = hashlib.sha1(
            object_bytes,
            usedforsecurity=False,
        ).hexdigest().encode()
        if (
            blob_result.returncode != 0
            or blob_result.stderr
            or len(verifier_bytes) != blob_size
            or computed_blob_id != blob_id
        ):
            raise AttestationError
    except (OSError, subprocess.SubprocessError) as error:
        raise AttestationError from error
    return verifier_bytes


def verifier_environment(
    artifact_id: str,
    artifact_path: pathlib.Path,
) -> dict[str, str]:
    """Build a minimal execution environment without credentials."""
    allowed = {
        "COMSPEC",
        "HOME",
        "LANG",
        "LC_ALL",
        "PATH",
        "PATHEXT",
        "SHELL",
        "SYSTEMROOT",
        "TEMP",
        "TMP",
        "TMPDIR",
        "USERPROFILE",
        "WINDIR",
    }
    environment = {
        name: value
        for name, value in os.environ.items()
        if name.upper() in allowed
    }
    environment["CI"] = "true"
    environment["LOGBREW_RELEASE_RECEIPT_MODE"] = "1"
    environment["LOGBREW_RELEASE_ARTIFACT_FILES_JSON"] = json.dumps(
        {artifact_id: str(artifact_path)}, separators=(",", ":")
    )
    if artifact_id == "installer:powershell":
        environment["INSTALLER_NO_MODIFY_PATH"] = "1"
    return environment


def execute_verifier(
    verifier: pathlib.Path,
    receipt: ReceiptPolicy,
    version: str,
    artifact_path: pathlib.Path,
) -> tuple[bytes, bytes]:
    """Execute the exact released verifier with bounded captured output."""
    try:
        result = subprocess.run(
            [sys.executable, str(verifier), receipt.mode, version],
            env=verifier_environment(receipt.artifact_id, artifact_path),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=VERIFIER_TIMEOUT_SECONDS,
            check=False,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise AttestationError from error
    if result.returncode != 0:
        raise AttestationError
    return result.stdout, result.stderr


def write_artifact(path: pathlib.Path, content: bytes) -> None:
    """Create one fixed artifact path inside a fresh verifier workspace."""
    try:
        descriptor = os.open(
            path,
            os.O_WRONLY
            | os.O_CREAT
            | os.O_EXCL
            | getattr(os, "O_CLOEXEC", 0)
            | getattr(os, "O_NOFOLLOW", 0),
            0o600,
        )
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        metadata = path.lstat()
        if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
            raise AttestationError
    except (OSError, ValueError) as error:
        raise AttestationError from error


def run_attestation(
    *,
    receipt_name: str,
    tag: str,
    source_commit: str,
    release_run: str,
    mode: str,
    artifact_id: str,
    asset: str,
    execution_platform: str,
    released_source: pathlib.Path,
    output: pathlib.Path,
    environment: Mapping[str, str],
    json_reader: Callable[[str], Mapping[str, object]] | None = None,
    asset_reader: Callable[[str, int], bytes] = download_release_asset,
) -> None:
    """Produce one exact installed attestation from public immutable inputs."""
    try:
        receipt_spec = RECEIPTS[receipt_name]
    except KeyError as error:
        raise AttestationError from error
    validate_matrix_inputs(
        receipt_spec,
        mode,
        artifact_id,
        asset,
        execution_platform,
    )
    workflow_head = validate_workflow_context(
        environment,
        receipt_spec,
        system=host_platform.system(),
        machine=host_platform.machine(),
    )
    metadata_reader = (
        json_reader
        if json_reader is not None
        else github_metadata_reader(environment)
    )
    policy, release = discover_release_policy(
        tag,
        source_commit,
        release_run,
        metadata_reader,
    )
    receipt = policy.receipts[receipt_name]
    verifier_bytes = validate_released_source(released_source, source_commit)
    assets = release["assets"]
    artifact_metadata = select_exact_asset(assets, receipt.asset_name)
    checksum_metadata = (
        select_exact_asset(assets, policy.checksum_asset_name)
        if receipt.mode == "native"
        else None
    )
    artifact_bytes = asset_reader(
        str(artifact_metadata["browser_download_url"]),
        receipt.asset_size,
    )
    checksum_bytes = None
    if checksum_metadata is not None:
        checksum_bytes = asset_reader(
            str(checksum_metadata["browser_download_url"]),
            policy.checksum_asset_size,
        )
        if (
            len(checksum_bytes) != policy.checksum_asset_size
            or hashlib.sha256(checksum_bytes).hexdigest()
            != policy.checksum_asset_digest
        ):
            raise AttestationError
    digest = validate_artifact_bytes(
        receipt,
        artifact_metadata,
        artifact_bytes,
        checksum_bytes,
    )

    with tempfile.TemporaryDirectory(prefix="logbrew-installed-attestation-") as raw:
        workspace = pathlib.Path(raw)
        verifier = workspace / "released-verifier.py"
        artifact_path = workspace / receipt.asset_name
        write_artifact(verifier, verifier_bytes)
        write_artifact(artifact_path, artifact_bytes)
        stdout, stderr = execute_verifier(
            verifier,
            receipt,
            policy.version,
            artifact_path,
        )
        validate_verifier_output(
            stdout,
            stderr,
            receipt.artifact_id,
            digest,
        )
    write_attestation(
        output,
        build_attestation(receipt, workflow_head, digest, policy),
        policy,
    )


def parse_arguments(argv: Sequence[str]) -> Mapping[str, str]:
    """Parse a fixed flag/value interface without reflecting hostile input."""
    allowed = {
        "--receipt": "receipt_name",
        "--tag": "tag",
        "--source-commit": "source_commit",
        "--release-run": "release_run",
        "--mode": "mode",
        "--artifact-id": "artifact_id",
        "--asset": "asset",
        "--execution-platform": "execution_platform",
        "--released-source": "released_source",
        "--output": "output",
    }
    if len(argv) != len(allowed) * 2:
        raise AttestationError
    parsed: dict[str, str] = {}
    for index in range(0, len(argv), 2):
        flag = argv[index]
        value = argv[index + 1]
        name = allowed.get(flag)
        if (
            name is None
            or name in parsed
            or not value
            or "\x00" in value
            or any(ord(character) < 0x20 for character in value)
        ):
            raise AttestationError
        parsed[name] = value
    if set(parsed) != set(allowed.values()):
        raise AttestationError
    return parsed


def main(argv: Sequence[str] | None = None) -> int:
    """Run one receipt with fixed, value-safe failure output."""
    try:
        arguments = parse_arguments(list(argv or sys.argv[1:]))
        run_attestation(
            receipt_name=arguments["receipt_name"],
            tag=arguments["tag"],
            source_commit=arguments["source_commit"],
            release_run=arguments["release_run"],
            mode=arguments["mode"],
            artifact_id=arguments["artifact_id"],
            asset=arguments["asset"],
            execution_platform=arguments["execution_platform"],
            released_source=pathlib.Path(arguments["released_source"]),
            output=pathlib.Path(arguments["output"]),
            environment=os.environ,
        )
    except BaseException:
        print("attestation_failed", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
