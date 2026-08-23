#!/usr/bin/env python3
"""Contract tests for installed release attestations."""

from __future__ import annotations

import copy
import hashlib
import importlib.util
import io
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr
from dataclasses import astuple, replace
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
SUBJECT = ROOT / "scripts" / "installed_release_attestation.py"
WORKFLOW_HEAD = "1" * 40
TAG_OBJECT_SHA = "b" * 40
RELEASE_ID = 654321
PUBLISHED_AT = "2026-08-22T20:33:21Z"
sys.dont_write_bytecode = True


def load_subject():
    if not SUBJECT.is_file():
        raise AssertionError("missing installed release attestation implementation")
    spec = importlib.util.spec_from_file_location(
        "installed_release_attestation", SUBJECT
    )
    if spec is None or spec.loader is None:
        raise AssertionError("could not load installed release attestation module")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def workflow_environment(
    *, runner_os: str = "Linux", runner_arch: str = "X64"
) -> dict[str, str]:
    return {
        "GITHUB_ACTIONS": "true",
        "GITHUB_EVENT_NAME": "workflow_dispatch",
        "GITHUB_REF": "refs/heads/main",
        "GITHUB_REPOSITORY": "LogBrewCo/cli",
        "GITHUB_SHA": WORKFLOW_HEAD,
        "GITHUB_WORKFLOW_REF": (
            "LogBrewCo/cli/.github/workflows/"
            "installed-release-attestations.yml@refs/heads/main"
        ),
        "GITHUB_WORKFLOW_SHA": WORKFLOW_HEAD,
        "RUNNER_ARCH": runner_arch,
        "RUNNER_OS": runner_os,
    }


def release_run_fixture(policy) -> dict[str, object]:
    return {
        "id": policy.release_run_id,
        "name": "Release",
        "path": ".github/workflows/release.yml",
        "event": "push",
        "status": "completed",
        "conclusion": "success",
        "head_branch": policy.tag,
        "head_sha": policy.source_commit,
        "run_attempt": 2,
        "workflow_id": 289984708,
    }


def tag_ref_fixture(policy) -> dict[str, object]:
    return {
        "ref": f"refs/tags/{policy.tag}",
        "object": {
            "type": "tag",
            "sha": TAG_OBJECT_SHA,
        },
    }


def tag_object_fixture(policy) -> dict[str, object]:
    return {
        "tag": policy.tag,
        "object": {
            "type": "commit",
            "sha": policy.source_commit,
        },
    }


def release_asset(tag, name, asset_id, size, digest) -> dict[str, object]:
    base = f"https://github.com/LogBrewCo/cli/releases/download/{tag}"
    return {
        "id": asset_id,
        "name": name,
        "state": "uploaded",
        "size": size,
        "digest": f"sha256:{digest}",
        "browser_download_url": f"{base}/{name}",
    }


def release_fixture(policy, receipt) -> dict[str, object]:
    assets = [
        release_asset(
            policy.tag,
            receipt.asset_name,
            receipt.asset_id,
            receipt.asset_size,
            receipt.digest,
        )
    ]
    if receipt.mode == "native":
        assets.append(
            release_asset(
                policy.tag,
                policy.checksum_asset_name,
                policy.checksum_asset_id,
                policy.checksum_asset_size,
                policy.checksum_asset_digest,
            )
        )
    return {
        "id": RELEASE_ID,
        "tag_name": policy.tag,
        "target_commitish": policy.source_commit,
        "draft": False,
        "prerelease": False,
        "published_at": PUBLISHED_AT,
        "assets": assets,
    }


def complete_release_fixture(policy) -> dict[str, object]:
    release = release_fixture(policy, next(iter(policy.receipts.values())))
    release["assets"] = [
        release_asset(
            policy.tag,
            receipt.asset_name,
            receipt.asset_id,
            receipt.asset_size,
            receipt.digest,
        )
        for receipt in policy.receipts.values()
    ]
    release["assets"].append(
        release_asset(
            policy.tag,
            policy.checksum_asset_name,
            policy.checksum_asset_id,
            policy.checksum_asset_size,
            policy.checksum_asset_digest,
        )
    )
    return release


def policy_urls(module, policy) -> dict[str, str]:
    urls = module.metadata_urls(policy.tag, policy.release_run_id)
    return {
        "tag_ref": urls["tag_ref"],
        "tag_object": (
            "https://api.github.com/repos/LogBrewCo/cli/git/tags/"
            f"{TAG_OBJECT_SHA}"
        ),
        "release_run": urls["release_run"],
        "release": urls["release"],
    }


def metadata_fixture(module, policy, release=None):
    urls = policy_urls(module, policy)
    return urls, {
        urls["tag_ref"]: tag_ref_fixture(policy),
        urls["tag_object"]: tag_object_fixture(policy),
        urls["release_run"]: release_run_fixture(policy),
        urls["release"]: release or complete_release_fixture(policy),
    }


def verifier_output(artifact_id: str, digest: str) -> bytes:
    return json.dumps(
        {
            "schema_version": 1,
            "status": "passed",
            "artifacts": [{"id": artifact_id, "digest": digest}],
        },
        separators=(",", ":"),
    ).encode() + b"\n"


def git_outputs(
    source_commit: str,
    verifier: bytes,
    newline: bytes = b"\n",
) -> list[bytes]:
    blob = hashlib.sha1(
        f"blob {len(verifier)}\0".encode() + verifier,
        usedforsecurity=False,
    ).hexdigest()
    tree = f"100644 blob {blob}\tscripts/real_user_public_install_smoke.py".encode()
    return [
        source_commit.encode() + newline,
        tree + newline,
        str(len(verifier)).encode() + newline,
        verifier,
    ]


def fixture_policy(module):
    receipts = {
        name: module.ReceiptPolicy(
            spec.runner,
            spec.platform,
            spec.mode,
            spec.artifact_id,
            spec.asset_name,
            100 + index,
            128 + index,
            f"{index + 1:x}" * 64,
        )
        for index, (name, spec) in enumerate(module.RECEIPTS.items())
    }
    return module.ReleasePolicy(
        "v1.2.3",
        "1.2.3",
        "a" * 40,
        123456,
        "sha256.sum",
        99,
        96,
        "f" * 64,
        receipts,
    )


class InstalledReleaseAttestationTests(unittest.TestCase):
    def test_receipt_specs_are_complete_and_platform_exact(self) -> None:
        module = load_subject()
        self.assertEqual(
            {
                "|".join((name, *astuple(spec)))
                for name, spec in module.RECEIPTS.items()
            },
            {
                "shell-linux-x64|ubuntu-24.04|linux-x64|shell|installer:shell|logbrew-cli-installer.sh",
                "native-linux-arm64|ubuntu-24.04-arm|linux-arm64|native|native:linux-arm64|logbrew-cli-aarch64-unknown-linux-gnu.tar.xz",
                "native-linux-x64|ubuntu-24.04|linux-x64|native|native:linux-x64|logbrew-cli-x86_64-unknown-linux-gnu.tar.xz",
                "powershell-windows-x64|windows-2025|windows-x64|powershell|installer:powershell|logbrew-cli-installer.ps1",
                "native-windows-x64|windows-2025|windows-x64|native|native:windows-x64|logbrew-cli-x86_64-pc-windows-msvc.zip",
                "native-macos-x64|macos-15-intel|macos-x64|native|native:macos-x64|logbrew-cli-x86_64-apple-darwin.tar.xz",
            },
        )

    def test_release_identity_rejects_noncanonical_inputs(self) -> None:
        module = load_subject()
        policy = fixture_policy(module)
        self.assertEqual(
            module.release_identity(
                policy.tag,
                policy.source_commit,
                str(policy.release_run_id),
            ),
            (policy.version, policy.release_run_id),
        )

        changes = [
            ("v01.2.3", policy.source_commit, str(policy.release_run_id)),
            ("1.2.3", policy.source_commit, str(policy.release_run_id)),
            (policy.tag, "A" * 40, str(policy.release_run_id)),
            (policy.tag, policy.source_commit, "0"),
            (policy.tag, policy.source_commit, "1" * 21),
        ]
        for changed in changes:
            with self.subTest(changed=changed):
                with self.assertRaises(module.AttestationError):
                    module.release_identity(*changed)

    def test_tag_and_run_require_exact_source_and_release_workflow(self) -> None:
        module = load_subject()
        policy = fixture_policy(module)
        tag_sha = module.validated_tag_object_sha(
            policy.tag,
            policy.source_commit,
            tag_ref_fixture(policy),
            tag_object_fixture(policy),
        )
        self.assertEqual(tag_sha, TAG_OBJECT_SHA)
        module.validate_release_run_identity(
            policy.tag,
            policy.source_commit,
            policy.release_run_id,
            release_run_fixture(policy),
        )

        bad_tag = tag_object_fixture(policy)
        bad_tag["object"]["sha"] = "3" * 40
        with self.assertRaises(module.AttestationError):
            module.validated_tag_object_sha(
                policy.tag,
                policy.source_commit,
                tag_ref_fixture(policy),
                bad_tag,
            )

        for field, value in [
            ("path", ".github/workflows/release-copy.yml"),
            ("workflow_id", module.RELEASE_WORKFLOW_ID + 1),
            ("head_sha", "4" * 40),
            ("run_attempt", 0),
            ("run_attempt", True),
            ("conclusion", "failure"),
        ]:
            run = release_run_fixture(policy)
            run[field] = value
            with self.subTest(field=field, value=value):
                with self.assertRaises(module.AttestationError):
                    module.validate_release_run_identity(
                        policy.tag,
                        policy.source_commit,
                        policy.release_run_id,
                        run,
                    )

    def test_release_policy_is_discovered_from_exact_public_metadata(self) -> None:
        module = load_subject()
        policy = fixture_policy(module)
        urls, responses = metadata_fixture(module, policy)
        release = responses[urls["release"]]
        requests = []

        def read_json(url):
            requests.append(url)
            return responses[url]

        discovered, discovered_release = module.discover_release_policy(
            policy.tag,
            policy.source_commit,
            str(policy.release_run_id),
            read_json,
        )
        self.assertEqual(discovered, policy)
        self.assertIs(discovered_release, release)
        self.assertEqual(requests, list(urls.values()))

    def test_release_assets_bind_exact_public_digest_and_checksum(self) -> None:
        module = load_subject()
        policy = fixture_policy(module)
        receipt = policy.receipts["native-linux-x64"]
        urls = policy_urls(module, policy)
        releases = [complete_release_fixture(policy) for _ in range(4)]
        releases[0]["assets"][0]["digest"] = "sha256:" + "0" * 63
        releases[1]["assets"] = releases[1]["assets"][1:]
        releases[2]["assets"].append(copy.deepcopy(releases[2]["assets"][0]))
        releases[3]["target_commitish"] = "0" * 40
        for index, release in enumerate(releases):
            _, responses = metadata_fixture(module, policy, release)
            with self.subTest(index=index):
                with self.assertRaises(module.AttestationError):
                    module.discover_release_policy(
                        policy.tag,
                        policy.source_commit,
                        str(policy.release_run_id),
                        responses.__getitem__,
                    )

        payload = b"exact released bytes"
        digest = hashlib.sha256(payload).hexdigest()
        local_receipt = replace(
            receipt,
            asset_size=len(payload),
            digest=digest,
        )
        local_asset = release_asset(
            policy.tag,
            local_receipt.asset_name,
            local_receipt.asset_id,
            len(payload),
            digest,
        )
        checksum_bytes = f"{digest} *{local_receipt.asset_name}\n".encode()
        self.assertEqual(
            module.validate_artifact_bytes(
                local_receipt,
                local_asset,
                payload,
                checksum_bytes,
            ),
            f"sha256:{digest}",
        )
        with self.assertRaises(module.AttestationError):
            module.validate_artifact_bytes(
                local_receipt,
                local_asset,
                payload + b"changed",
                checksum_bytes,
            )

    def test_checksum_manifest_accepts_only_one_terminal_blank_line(self) -> None:
        module = load_subject()
        digest = "a" * 64
        first = "logbrew-cli-aarch64-unknown-linux-gnu.tar.xz"
        second = "logbrew-cli-x86_64-unknown-linux-gnu.tar.xz"
        immutable_shape = (
            f"{digest} *{first}\n"
            f"{digest} *{second}\n"
            "\n"
        ).encode()
        self.assertEqual(
            module.checksum_entries(immutable_shape),
            {first: digest, second: digest},
        )

        rejected = [
            f"{digest} *{first}\n\n{digest} *{second}\n".encode(),
            f"{digest} *{first}\n\n\n".encode(),
        ]
        for content in rejected:
            with self.subTest(content=content):
                with self.assertRaises(module.AttestationError):
                    module.checksum_entries(content)

    def test_workflow_context_rejects_lookalikes_and_platform_substitution(self) -> None:
        module = load_subject()
        receipt = fixture_policy(module).receipts["native-linux-x64"]
        module.validate_matrix_inputs(
            receipt,
            receipt.mode,
            receipt.artifact_id,
            receipt.asset_name,
            receipt.platform,
        )
        for field, value in [
            ("mode", "shell"),
            ("artifact_id", "native:linux-arm64"),
            ("asset", "lookalike.tar.xz"),
            ("platform", "linux-arm64"),
        ]:
            values = {
                "mode": receipt.mode,
                "artifact_id": receipt.artifact_id,
                "asset": receipt.asset_name,
                "platform": receipt.platform,
            }
            values[field] = value
            with self.subTest(matrix_field=field):
                with self.assertRaises(module.AttestationError):
                    module.validate_matrix_inputs(
                        receipt,
                        values["mode"],
                        values["artifact_id"],
                        values["asset"],
                        values["platform"],
                    )
        self.assertEqual(
            module.validate_workflow_context(
                workflow_environment(),
                receipt,
                system="Linux",
                machine="x86_64",
            ),
            WORKFLOW_HEAD,
        )

        for name, value in [
            (
                "GITHUB_WORKFLOW_REF",
                "LogBrewCo/cli/.github/workflows/ci.yml@refs/heads/main",
            ),
            ("GITHUB_REPOSITORY", "LogBrewCo/cli-lookalike"),
            ("GITHUB_REF", "refs/heads/replay"),
            ("RUNNER_ARCH", "ARM64"),
        ]:
            environment = workflow_environment()
            environment[name] = value
            with self.subTest(name=name):
                with self.assertRaises(module.AttestationError):
                    module.validate_workflow_context(
                        environment,
                        receipt,
                        system="Linux",
                        machine="x86_64",
                    )

        with self.assertRaises(module.AttestationError):
            module.validate_workflow_context(
                workflow_environment(),
                receipt,
                system="Linux",
                machine="aarch64",
            )

    def test_verifier_output_requires_one_canonical_platform_newline(self) -> None:
        module = load_subject()
        receipt = fixture_policy(module).receipts["native-linux-x64"]
        digest = f"sha256:{receipt.digest}"
        output = verifier_output(receipt.artifact_id, digest).removesuffix(b"\n")
        for terminator in (b"\n", b"\r\n"):
            module.validate_verifier_output(
                output + terminator,
                b"",
                receipt.artifact_id,
                digest,
            )

        rejected = [
            (output, b""),
            (output + b"\nextra\n", b""),
            (output + b"\n\n", b""),
            (output + b"\r\n\r\n", b""),
            (output.replace(b",", b",\n", 1) + b"\n", b""),
            (output + b"\r", b""),
            (output + b"\x00\n", b""),
            (output + b"\n", b"hostile backend text"),
            (
                output.replace(b'"passed"', b'"failed"') + b"\n",
                b"",
            ),
            (output[:-1] + b',"extra":true}\n', b""),
        ]
        for stdout, stderr in rejected:
            with self.assertRaises(module.AttestationError):
                module.validate_verifier_output(
                    stdout,
                    stderr,
                    receipt.artifact_id,
                    digest,
                )

    def test_attestation_schema_rejects_extra_output(self) -> None:
        module = load_subject()
        policy = fixture_policy(module)
        receipt = policy.receipts["native-linux-x64"]
        digest = f"sha256:{receipt.digest}"

        attestation = module.build_attestation(
            receipt,
            WORKFLOW_HEAD,
            digest,
            policy,
        )
        module.validate_attestation(attestation, policy)
        self.assertEqual(
            set(attestation),
            {
                "artifact_id",
                "version",
                "source",
                "release_run",
                "workflow_head",
                "execution_platform",
                "digest",
                "status",
            },
        )
        malformed_attestations = [
            dict(attestation, backend="hidden"),
            {name: value for name, value in attestation.items() if name != "digest"},
            dict(attestation, status="unknown"),
            dict(attestation, release_run=str(fixture_policy(module).release_run_id)),
        ]
        for malformed in malformed_attestations:
            with self.subTest(malformed=malformed):
                with self.assertRaises(module.AttestationError):
                    module.validate_attestation(malformed, policy)

    def test_github_api_headers_require_one_bounded_job_token(self) -> None:
        module = load_subject()
        token = "job-token-value"
        self.assertEqual(
            module.github_api_headers(token),
            {
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {token}",
                "User-Agent": "logbrew-installed-attestation",
                "X-GitHub-Api-Version": "2022-11-28",
            },
        )
        for rejected in (
            "",
            "token\nvalue",
            "token\x00value",
            "tökén",
            "x" * 1025,
        ):
            with self.subTest(rejected_length=len(rejected)):
                with self.assertRaises(module.AttestationError):
                    module.github_api_headers(rejected)

    def test_github_metadata_reader_uses_only_the_scoped_job_token(self) -> None:
        module = load_subject()
        token = "job-token-value"
        url = "https://api.github.com/repos/LogBrewCo/cli/releases/tags/v0.1.33"
        payload = {"tag_name": "v0.1.33"}
        with mock.patch.object(module, "fetch_json", return_value=payload) as fetch:
            reader = module.github_metadata_reader({"GITHUB_TOKEN": token})
            self.assertEqual(reader(url), payload)
        fetch.assert_called_once_with(url, token)

        for environment in ({}, {"GITHUB_TOKEN": "token\nvalue"}):
            with self.subTest(environment=environment):
                with self.assertRaises(module.AttestationError):
                    module.github_metadata_reader(environment)

    def test_verifier_environment_drops_credentials_and_workflow_controls(self) -> None:
        module = load_subject()
        with mock.patch.dict(
            os.environ,
            {
                "PATH": os.environ.get("PATH", ""),
                "GITHUB_TOKEN": "not-forwarded",
                "GH_TOKEN": "not-forwarded",
                "LOGBREW_TOKEN": "not-forwarded",
                "LOGBREW_API_URL": "not-forwarded",
            },
            clear=True,
        ):
            environment = module.verifier_environment(
                "native:linux-x64",
                pathlib.Path("/tmp/artifact.tar.xz"),
            )
        self.assertEqual(
            set(environment),
            {
                "PATH",
                "CI",
                "LOGBREW_RELEASE_RECEIPT_MODE",
                "LOGBREW_RELEASE_ARTIFACT_FILES_JSON",
            },
        )
        self.assertNotIn("not-forwarded", json.dumps(environment))

    def test_powershell_verifier_alone_disables_persistent_path_mutation(self) -> None:
        module = load_subject()
        policy = fixture_policy(module)
        powershell = policy.receipts["powershell-windows-x64"]
        native = policy.receipts["native-windows-x64"]
        artifact = pathlib.Path("/tmp/fixed-release-artifact")

        with mock.patch.dict(
            os.environ,
            {
                "PATH": os.environ.get("PATH", ""),
                "INSTALLER_NO_MODIFY_PATH": "0",
            },
            clear=True,
        ):
            powershell_environment = module.verifier_environment(
                powershell.artifact_id,
                artifact,
            )
            native_environment = module.verifier_environment(
                native.artifact_id,
                artifact,
            )
        self.assertEqual(
            powershell_environment.get("INSTALLER_NO_MODIFY_PATH"),
            "1",
        )
        self.assertNotIn("INSTALLER_NO_MODIFY_PATH", native_environment)

        output = verifier_output(
            powershell.artifact_id,
            f"sha256:{powershell.digest}",
        )

        def completed(command, **kwargs):
            self.assertEqual(
                command,
                [
                    sys.executable,
                    "/fixed/verifier.py",
                    "powershell",
                    policy.version,
                ],
            )
            self.assertEqual(kwargs["env"].get("INSTALLER_NO_MODIFY_PATH"), "1")
            return subprocess.CompletedProcess(command, 0, output, b"")

        with mock.patch.object(subprocess, "run", side_effect=completed):
            stdout, stderr = module.execute_verifier(
                pathlib.Path("/fixed/verifier.py"),
                powershell,
                policy.version,
                artifact,
            )
        module.validate_verifier_output(
            stdout,
            stderr,
            powershell.artifact_id,
            f"sha256:{powershell.digest}",
        )

    def test_attestation_output_rejects_symlink_and_overwrite(self) -> None:
        module = load_subject()
        policy = fixture_policy(module)
        receipt = policy.receipts["native-linux-x64"]
        attestation = module.build_attestation(
            receipt,
            WORKFLOW_HEAD,
            f"sha256:{receipt.digest}",
            policy,
        )
        with tempfile.TemporaryDirectory() as raw_directory:
            directory = pathlib.Path(raw_directory)
            output = directory / "attestation.json"
            module.write_attestation(output, attestation, policy)
            self.assertEqual(
                json.loads(output.read_text(encoding="utf-8")),
                attestation,
            )
            with self.assertRaises(module.AttestationError):
                module.write_attestation(output, attestation, policy)

            external = directory / "external"
            external.write_text("preserve", encoding="utf-8")
            linked = directory / "linked.json"
            linked.symlink_to(external)
            with self.assertRaises(module.AttestationError):
                module.write_attestation(linked, attestation, policy)
            self.assertEqual(external.read_text(encoding="utf-8"), "preserve")

    def test_released_source_binds_the_commit_and_rejects_hostile_git_output(
        self,
    ) -> None:
        module = load_subject()
        with tempfile.TemporaryDirectory() as raw_directory:
            repository = pathlib.Path(raw_directory) / "released-source"
            verifier = repository / "scripts" / "real_user_public_install_smoke.py"
            verifier.parent.mkdir(parents=True)
            committed_verifier = b"print('exact fixture')\n"
            verifier.write_bytes(committed_verifier)
            for command in [
                ["git", "init", "--quiet"],
                ["git", "add", str(module.VERIFIER_PATH)],
                [
                    "git",
                    "-c", "user.name=Fixture",
                    "-c", "user.email=fixture@example.invalid",
                    "commit", "--quiet", "-m",
                    "fixture",
                ],
            ]:
                subprocess.run(command, cwd=repository, check=True, capture_output=True)
            head = subprocess.check_output(
                ["git", "rev-parse", "HEAD"],
                cwd=repository,
                text=True,
            ).strip()
            for working_copy in (
                committed_verifier,
                committed_verifier.replace(b"\n", b"\r\n"),
                b"print('substituted')\r\n",
            ):
                verifier.write_bytes(working_copy)
                self.assertEqual(
                    module.validate_released_source(repository, head),
                    committed_verifier,
                )

        source_commit = "1" * 40
        verifier_content = b"# exact fixture\n"
        exact = git_outputs(source_commit, verifier_content)
        variants = [
            [exact[0].replace(b"\n", b"\nextra\n")],
            [exact[0], exact[1] + b"100644 blob " + b"3" * 40 + b"\tlookalike\n"],
            [exact[0], exact[1].replace(b".py\n", b".py.bak\n")],
            [exact[0], exact[1], exact[2].replace(b"\n", b"\nextra\n")],
            [exact[0], exact[1], b"9" * 64 + b"\n"],
            [*exact[:3], b"# substituted fixture\n"],
        ]
        with tempfile.TemporaryDirectory() as raw_directory:
            repository = pathlib.Path(raw_directory) / "released-source"
            repository.mkdir()
            crlf_results = [
                subprocess.CompletedProcess([], 0, output, b"")
                for output in git_outputs(source_commit, verifier_content, b"\r\n")
            ]
            with mock.patch.object(module.subprocess, "run", side_effect=crlf_results):
                self.assertEqual(
                    module.validate_released_source(repository, source_commit),
                    verifier_content,
                )
            for outputs in variants:
                results = [
                    subprocess.CompletedProcess([], 0, output, b"")
                    for output in outputs
                ]
                with self.subTest(outputs=outputs):
                    with (
                        mock.patch.object(module.subprocess, "run", side_effect=results),
                        self.assertRaises(module.AttestationError),
                    ):
                        module.validate_released_source(repository, source_commit)

    def test_offline_orchestration_uses_only_exact_metadata_and_assets(self) -> None:
        module = load_subject()
        payload = b"bounded native artifact"
        digest = hashlib.sha256(payload).hexdigest()
        base_policy = fixture_policy(module)
        base_receipt = base_policy.receipts["native-linux-x64"]
        checksum = f"{digest} *{base_receipt.asset_name}\n".encode()
        receipt = replace(
            base_receipt,
            asset_size=len(payload),
            digest=digest,
        )
        receipt_name = "native-linux-x64"
        policy = replace(
            base_policy,
            checksum_asset_size=len(checksum),
            checksum_asset_digest=hashlib.sha256(checksum).hexdigest(),
            receipts=base_policy.receipts | {receipt_name: receipt},
        )
        urls, responses = metadata_fixture(module, policy)
        metadata_requests: list[str] = []
        asset_requests: list[tuple[str, int]] = []

        def read_json(url: str):
            metadata_requests.append(url)
            return responses[url]

        release = responses[urls["release"]]
        artifact_url = next(
            item["browser_download_url"]
            for item in release["assets"]
            if item["name"] == receipt.asset_name
        )
        checksum_url = next(
            item["browser_download_url"]
            for item in release["assets"]
            if item["name"] == policy.checksum_asset_name
        )

        def read_asset(url: str, maximum: int) -> bytes:
            asset_requests.append((url, maximum))
            if url == artifact_url and maximum == len(payload):
                return payload
            if url == checksum_url and maximum == len(checksum):
                return checksum
            raise AssertionError("unexpected asset request")

        verifier_receipt = verifier_output(
            receipt.artifact_id,
            f"sha256:{digest}",
        )
        with tempfile.TemporaryDirectory() as raw_directory:
            directory = pathlib.Path(raw_directory)
            released_source = directory / "released-source"
            released_source.mkdir()
            verifier_content = b"# exact released verifier\n"
            output = directory / "attestation.json"

            def execute_exact_verifier(
                verifier_path,
                actual_receipt,
                actual_version,
                artifact_path,
            ):
                self.assertEqual(verifier_path.read_bytes(), verifier_content)
                self.assertTrue(verifier_path.is_file())
                self.assertFalse(verifier_path.is_symlink())
                self.assertEqual(actual_receipt, receipt)
                self.assertEqual(actual_version, policy.version)
                self.assertEqual(artifact_path.read_bytes(), payload)
                return verifier_receipt, b""

            with (
                mock.patch.object(
                    module,
                    "validate_released_source",
                    return_value=verifier_content,
                ),
                mock.patch.object(module.host_platform, "system", return_value="Linux"),
                mock.patch.object(
                    module.host_platform,
                    "machine",
                    return_value="x86_64",
                ),
                mock.patch.object(
                    module,
                    "execute_verifier",
                    side_effect=execute_exact_verifier,
                ),
            ):
                module.run_attestation(
                    receipt_name=receipt_name,
                    tag=policy.tag,
                    source_commit=policy.source_commit,
                    release_run=str(policy.release_run_id),
                    mode=receipt.mode,
                    artifact_id=receipt.artifact_id,
                    asset=receipt.asset_name,
                    execution_platform=receipt.platform,
                    released_source=released_source,
                    output=output,
                    environment=workflow_environment(),
                    json_reader=read_json,
                    asset_reader=read_asset,
                )
            attestation = json.loads(output.read_text(encoding="utf-8"))
            module.validate_attestation(attestation, policy)

        self.assertEqual(metadata_requests, list(urls.values()))
        self.assertEqual(
            asset_requests,
            [(artifact_url, len(payload)), (checksum_url, len(checksum))],
        )

    def test_public_failure_is_fixed_and_value_safe(self) -> None:
        module = load_subject()
        stderr = io.StringIO()
        with redirect_stderr(stderr):
            result = module.main(["--receipt", "secret/control\nvalue"])
        self.assertEqual(result, 2)
        self.assertEqual(stderr.getvalue(), "attestation_failed\n")


if __name__ == "__main__":
    unittest.main()
