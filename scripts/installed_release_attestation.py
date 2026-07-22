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
MAX_CHECKSUM_BYTES = 64 * 1024
MAX_VERIFIER_OUTPUT_BYTES = 16 * 1024
MAX_RELEASED_VERIFIER_BYTES = 256 * 1024
NETWORK_TIMEOUT_SECONDS = 60
VERIFIER_TIMEOUT_SECONDS = 1200
COMMIT_PATTERN = re.compile(r"^[0-9a-f]{40}$")
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


class AttestationError(RuntimeError):
    """Raised when public evidence does not match the fixed release policy."""


@dataclass(frozen=True)
class ReceiptPolicy:
    """One exact public artifact and real execution platform."""

    name: str
    runner: str
    platform: str
    mode: str
    artifact_id: str
    asset_name: str
    asset_id: int
    asset_size: int
    digest: str
    checksum_required: bool


@dataclass(frozen=True)
class ReleasePolicy:
    """Immutable public inputs accepted by this attestation workflow."""

    repository: str
    tag: str
    version: str
    source_commit: str
    tag_object_sha: str
    release_run_id: int
    release_workflow_id: int
    release_id: int
    published_at: str
    checksum_asset_name: str
    checksum_asset_id: int
    checksum_asset_size: int
    checksum_asset_digest: str
    receipts: Mapping[str, ReceiptPolicy]


PUBLIC_POLICY = ReleasePolicy(
    repository="LogBrewCo/cli",
    tag="v0.1.20",
    version="0.1.20",
    source_commit="018b1a832d143c203b2822652d1bce9fb16401ab",
    tag_object_sha="11e6f9870154063cbae25333ffd628303b078def",
    release_run_id=29935721685,
    release_workflow_id=289984708,
    release_id=358147169,
    published_at="2026-07-22T16:01:18Z",
    checksum_asset_name="sha256.sum",
    checksum_asset_id=486136719,
    checksum_asset_size=820,
    checksum_asset_digest=(
        "cc8c2f767c15cd733b23fa619b20c5e7d5b4aa561ab4c5d0f41acd6d279aed02"
    ),
    receipts={
        "shell-linux-x64": ReceiptPolicy(
            name="shell-linux-x64",
            runner="ubuntu-24.04",
            platform="linux-x64",
            mode="shell",
            artifact_id="installer:shell",
            asset_name="logbrew-cli-installer.sh",
            asset_id=486136700,
            asset_size=54183,
            digest=(
                "80b1c203422b38f703b25d91e6f6512d000a9f2cf49b9909ab3b5d382e67ec84"
            ),
            checksum_required=False,
        ),
        "native-linux-arm64": ReceiptPolicy(
            name="native-linux-arm64",
            runner="ubuntu-24.04-arm",
            platform="linux-arm64",
            mode="native",
            artifact_id="native:linux-arm64",
            asset_name="logbrew-cli-aarch64-unknown-linux-gnu.tar.xz",
            asset_id=486136690,
            asset_size=1831656,
            digest=(
                "25506d03eade84ee5bd3118dbc7d15178f6081fdfd8bf1fc2465fc79848f0225"
            ),
            checksum_required=True,
        ),
        "native-linux-x64": ReceiptPolicy(
            name="native-linux-x64",
            runner="ubuntu-24.04",
            platform="linux-x64",
            mode="native",
            artifact_id="native:linux-x64",
            asset_name="logbrew-cli-x86_64-unknown-linux-gnu.tar.xz",
            asset_id=486136712,
            asset_size=2052132,
            digest=(
                "a8b3d8bc55a84053c8abee78f8bd67c6a75ebe5a3ecb847693a160607ceff91a"
            ),
            checksum_required=True,
        ),
        "powershell-windows-x64": ReceiptPolicy(
            name="powershell-windows-x64",
            runner="windows-2025",
            platform="windows-x64",
            mode="powershell",
            artifact_id="installer:powershell",
            asset_name="logbrew-cli-installer.ps1",
            asset_id=486136699,
            asset_size=22325,
            digest=(
                "6d673493ce394ea4f265995d41658303df76850517c670c5fe918e9aa9b789cf"
            ),
            checksum_required=False,
        ),
        "native-windows-x64": ReceiptPolicy(
            name="native-windows-x64",
            runner="windows-2025",
            platform="windows-x64",
            mode="native",
            artifact_id="native:windows-x64",
            asset_name="logbrew-cli-x86_64-pc-windows-msvc.zip",
            asset_id=486136707,
            asset_size=2563815,
            digest=(
                "2dad2c645b55a10f487e97449c11408aa4b80d059c454c2f840ec55bef8b43a3"
            ),
            checksum_required=True,
        ),
        "native-macos-x64": ReceiptPolicy(
            name="native-macos-x64",
            runner="macos-15-intel",
            platform="macos-x64",
            mode="native",
            artifact_id="native:macos-x64",
            asset_name="logbrew-cli-x86_64-apple-darwin.tar.xz",
            asset_id=486136702,
            asset_size=2025652,
            digest=(
                "8786f2fb3201fa69cf473b5070c3e1e18bc0c26714cd8812e4dd607a9a8c86b8"
            ),
            checksum_required=True,
        ),
    },
)


def validate_release_inputs(
    policy: ReleasePolicy,
    tag: str,
    version: str,
    source_commit: str,
    release_run: str,
) -> None:
    """Reject dispatch replay or substitution outside the fixed release."""
    if (
        tag != policy.tag
        or version != policy.version
        or source_commit != policy.source_commit
        or release_run != str(policy.release_run_id)
    ):
        raise AttestationError


def validate_matrix_inputs(
    receipt: ReceiptPolicy,
    mode: str,
    artifact_id: str,
    asset: str,
    execution_platform: str,
) -> None:
    """Bind every visible matrix value to the selected receipt policy."""
    if (
        mode != receipt.mode
        or artifact_id != receipt.artifact_id
        or asset != receipt.asset_name
        or execution_platform != receipt.platform
    ):
        raise AttestationError


def validate_tag(
    policy: ReleasePolicy,
    reference: Mapping[str, object],
    tag_object: Mapping[str, object],
) -> None:
    """Require the exact annotated tag object to resolve to the released source."""
    reference_object = reference.get("object")
    target_object = tag_object.get("object")
    if (
        reference.get("ref") != f"refs/tags/{policy.tag}"
        or not isinstance(reference_object, dict)
        or reference_object.get("type") != "tag"
        or reference_object.get("sha") != policy.tag_object_sha
        or tag_object.get("tag") != policy.tag
        or not isinstance(target_object, dict)
        or target_object.get("type") != "commit"
        or target_object.get("sha") != policy.source_commit
    ):
        raise AttestationError


def validate_release_run(
    policy: ReleasePolicy,
    run: Mapping[str, object],
) -> None:
    """Require the exact successful authoritative release workflow run."""
    expected = {
        "id": policy.release_run_id,
        "name": "Release",
        "path": RELEASE_WORKFLOW_PATH,
        "event": "push",
        "status": "completed",
        "conclusion": "success",
        "head_branch": policy.tag,
        "head_sha": policy.source_commit,
        "run_attempt": 1,
        "workflow_id": policy.release_workflow_id,
    }
    if any(run.get(name) != value for name, value in expected.items()):
        raise AttestationError


def expected_download_url(policy: ReleasePolicy, asset_name: str) -> str:
    """Return the only accepted browser download URL for a release asset."""
    if SAFE_ASSET_PATTERN.fullmatch(asset_name) is None:
        raise AttestationError
    return (
        f"https://github.com/{policy.repository}/releases/download/"
        f"{policy.tag}/{asset_name}"
    )


def validate_asset(
    policy: ReleasePolicy,
    asset: Mapping[str, object],
    *,
    name: str,
    asset_id: int,
    size: int,
    digest: str,
) -> None:
    """Validate one release asset against its frozen public identity."""
    if (
        asset.get("id") != asset_id
        or asset.get("name") != name
        or asset.get("state") != "uploaded"
        or asset.get("size") != size
        or asset.get("digest") != f"sha256:{digest}"
        or asset.get("browser_download_url") != expected_download_url(policy, name)
    ):
        raise AttestationError


def select_release_assets(
    policy: ReleasePolicy,
    receipt: ReceiptPolicy,
    release: Mapping[str, object],
) -> tuple[Mapping[str, object], Mapping[str, object] | None]:
    """Select exactly the policy-owned artifact and optional checksum metadata."""
    if (
        release.get("id") != policy.release_id
        or release.get("tag_name") != policy.tag
        or release.get("target_commitish") != policy.source_commit
        or release.get("draft") is not False
        or release.get("prerelease") is not False
        or release.get("published_at") != policy.published_at
    ):
        raise AttestationError
    assets = release.get("assets")
    if not isinstance(assets, list):
        raise AttestationError

    matches = [
        asset
        for asset in assets
        if isinstance(asset, dict) and asset.get("name") == receipt.asset_name
    ]
    if len(matches) != 1:
        raise AttestationError
    artifact = matches[0]
    validate_asset(
        policy,
        artifact,
        name=receipt.asset_name,
        asset_id=receipt.asset_id,
        size=receipt.asset_size,
        digest=receipt.digest,
    )

    if not receipt.checksum_required:
        return artifact, None
    checksum_matches = [
        asset
        for asset in assets
        if isinstance(asset, dict)
        and asset.get("name") == policy.checksum_asset_name
    ]
    if len(checksum_matches) != 1:
        raise AttestationError
    checksum = checksum_matches[0]
    validate_asset(
        policy,
        checksum,
        name=policy.checksum_asset_name,
        asset_id=policy.checksum_asset_id,
        size=policy.checksum_asset_size,
        digest=policy.checksum_asset_digest,
    )
    return artifact, checksum


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
    if receipt.checksum_required:
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
    receipt: ReceiptPolicy,
    *,
    policy: ReleasePolicy = PUBLIC_POLICY,
    system: str,
    machine: str,
) -> str:
    """Bind execution to the protected workflow and physical runner platform."""
    workflow_head = environment.get("GITHUB_SHA", "")
    expected_workflow_ref = (
        f"{policy.repository}/{WORKFLOW_PATH}@refs/heads/main"
    )
    actual_platform, runner_os, runner_arch = platform_identity(system, machine)
    if (
        environment.get("GITHUB_ACTIONS") != "true"
        or environment.get("GITHUB_EVENT_NAME") != "workflow_dispatch"
        or environment.get("GITHUB_REF") != "refs/heads/main"
        or environment.get("GITHUB_REPOSITORY") != policy.repository
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
    policy: ReleasePolicy = PUBLIC_POLICY,
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
    policy: ReleasePolicy = PUBLIC_POLICY,
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
    policy: ReleasePolicy = PUBLIC_POLICY,
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


def fetch_json(url: str) -> Mapping[str, object]:
    """Read one bounded public GitHub API object without credentials."""
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
        headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": "logbrew-installed-attestation",
            "X-GitHub-Api-Version": "2022-11-28",
        },
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


def api_urls(policy: ReleasePolicy) -> Mapping[str, str]:
    """Return the fixed public metadata endpoints used by one receipt."""
    repository = policy.repository
    encoded_tag = urllib.parse.quote(policy.tag, safe="")
    return {
        "tag_ref": f"https://api.github.com/repos/{repository}/git/ref/tags/{encoded_tag}",
        "tag_object": (
            f"https://api.github.com/repos/{repository}/git/tags/"
            f"{policy.tag_object_sha}"
        ),
        "release_run": (
            f"https://api.github.com/repos/{repository}/actions/runs/"
            f"{policy.release_run_id}"
        ),
        "release": (
            f"https://api.github.com/repos/{repository}/releases/tags/{encoded_tag}"
        ),
    }


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
    version: str,
    source_commit: str,
    release_run: str,
    mode: str,
    artifact_id: str,
    asset: str,
    execution_platform: str,
    released_source: pathlib.Path,
    output: pathlib.Path,
    environment: Mapping[str, str],
    policy: ReleasePolicy = PUBLIC_POLICY,
    json_reader: Callable[[str], Mapping[str, object]] = fetch_json,
    asset_reader: Callable[[str, int], bytes] = download_release_asset,
) -> None:
    """Produce one exact installed attestation from public immutable inputs."""
    validate_release_inputs(policy, tag, version, source_commit, release_run)
    try:
        receipt = policy.receipts[receipt_name]
    except KeyError as error:
        raise AttestationError from error
    validate_matrix_inputs(receipt, mode, artifact_id, asset, execution_platform)
    workflow_head = validate_workflow_context(
        environment,
        receipt,
        policy=policy,
        system=host_platform.system(),
        machine=host_platform.machine(),
    )
    verifier_bytes = validate_released_source(released_source, source_commit)

    urls = api_urls(policy)
    tag_reference = json_reader(urls["tag_ref"])
    tag_object = json_reader(urls["tag_object"])
    validate_tag(policy, tag_reference, tag_object)
    validate_release_run(policy, json_reader(urls["release_run"]))
    artifact_metadata, checksum_metadata = select_release_assets(
        policy,
        receipt,
        json_reader(urls["release"]),
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
            version,
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
        "--version": "version",
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
            version=arguments["version"],
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
