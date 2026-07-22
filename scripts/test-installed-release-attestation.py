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
from dataclasses import replace
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
SUBJECT = ROOT / "scripts" / "installed_release_attestation.py"
SOURCE_COMMIT = "018b1a832d143c203b2822652d1bce9fb16401ab"
WORKFLOW_HEAD = "1" * 40
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
        "run_attempt": 1,
        "workflow_id": policy.release_workflow_id,
    }


def tag_ref_fixture(policy) -> dict[str, object]:
    return {
        "ref": f"refs/tags/{policy.tag}",
        "object": {
            "type": "tag",
            "sha": policy.tag_object_sha,
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


def release_fixture(policy, receipt) -> dict[str, object]:
    base = f"https://github.com/{policy.repository}/releases/download/{policy.tag}"
    assets = [
        {
            "id": receipt.asset_id,
            "name": receipt.asset_name,
            "state": "uploaded",
            "size": receipt.asset_size,
            "digest": f"sha256:{receipt.digest}",
            "browser_download_url": f"{base}/{receipt.asset_name}",
        }
    ]
    if receipt.checksum_required:
        assets.append(
            {
                "id": policy.checksum_asset_id,
                "name": policy.checksum_asset_name,
                "state": "uploaded",
                "size": policy.checksum_asset_size,
                "digest": f"sha256:{policy.checksum_asset_digest}",
                "browser_download_url": f"{base}/{policy.checksum_asset_name}",
            }
        )
    return {
        "id": policy.release_id,
        "tag_name": policy.tag,
        "target_commitish": policy.source_commit,
        "draft": False,
        "prerelease": False,
        "published_at": policy.published_at,
        "assets": assets,
    }


class InstalledReleaseAttestationTests(unittest.TestCase):
    def test_release_inputs_reject_replay_and_substitution(self) -> None:
        module = load_subject()
        policy = module.PUBLIC_POLICY
        module.validate_release_inputs(
            policy,
            policy.tag,
            policy.version,
            policy.source_commit,
            str(policy.release_run_id),
        )

        changes = [
            ("v0.1.19", policy.version, policy.source_commit, str(policy.release_run_id)),
            (policy.tag, "0.1.19", policy.source_commit, str(policy.release_run_id)),
            (policy.tag, policy.version, "2" * 40, str(policy.release_run_id)),
            (policy.tag, policy.version, policy.source_commit, "29935721686"),
        ]
        for changed in changes:
            with self.subTest(changed=changed):
                with self.assertRaises(module.AttestationError):
                    module.validate_release_inputs(policy, *changed)

    def test_tag_and_run_require_exact_source_and_release_workflow(self) -> None:
        module = load_subject()
        policy = module.PUBLIC_POLICY
        module.validate_tag(policy, tag_ref_fixture(policy), tag_object_fixture(policy))
        module.validate_release_run(policy, release_run_fixture(policy))

        bad_tag = tag_object_fixture(policy)
        bad_tag["object"]["sha"] = "3" * 40
        with self.assertRaises(module.AttestationError):
            module.validate_tag(policy, tag_ref_fixture(policy), bad_tag)

        for field, value in [
            ("path", ".github/workflows/release-copy.yml"),
            ("workflow_id", policy.release_workflow_id + 1),
            ("head_sha", "4" * 40),
            ("run_attempt", 2),
            ("conclusion", "failure"),
        ]:
            run = release_run_fixture(policy)
            run[field] = value
            with self.subTest(field=field):
                with self.assertRaises(module.AttestationError):
                    module.validate_release_run(policy, run)

    def test_release_assets_bind_exact_public_digest_and_checksum(self) -> None:
        module = load_subject()
        policy = module.PUBLIC_POLICY
        receipt = policy.receipts["native-linux-x64"]
        release = release_fixture(policy, receipt)
        artifact, checksum = module.select_release_assets(policy, receipt, release)
        self.assertEqual(artifact["name"], receipt.asset_name)
        self.assertEqual(checksum["name"], policy.checksum_asset_name)

        substituted = copy.deepcopy(release)
        substituted["assets"][0]["digest"] = "sha256:" + "0" * 64
        with self.assertRaises(module.AttestationError):
            module.select_release_assets(policy, receipt, substituted)

        missing = copy.deepcopy(release)
        missing["assets"] = missing["assets"][1:]
        with self.assertRaises(module.AttestationError):
            module.select_release_assets(policy, receipt, missing)

        duplicated = copy.deepcopy(release)
        duplicated["assets"].append(copy.deepcopy(duplicated["assets"][0]))
        with self.assertRaises(module.AttestationError):
            module.select_release_assets(policy, receipt, duplicated)

        payload = b"exact released bytes"
        digest = hashlib.sha256(payload).hexdigest()
        local_receipt = replace(
            receipt,
            asset_size=len(payload),
            digest=digest,
        )
        local_asset = {
            "id": local_receipt.asset_id,
            "name": local_receipt.asset_name,
            "state": "uploaded",
            "size": len(payload),
            "digest": f"sha256:{digest}",
            "browser_download_url": (
                "https://github.com/LogBrewCo/cli/releases/download/"
                f"v0.1.20/{local_receipt.asset_name}"
            ),
        }
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
        receipt = module.PUBLIC_POLICY.receipts["native-linux-x64"]
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
        receipt = module.PUBLIC_POLICY.receipts["native-linux-x64"]
        digest = f"sha256:{receipt.digest}"
        verifier_output = json.dumps(
            {
                "schema_version": 1,
                "status": "passed",
                "artifacts": [{"id": receipt.artifact_id, "digest": digest}],
            },
            separators=(",", ":"),
        ).encode()
        for terminator in (b"\n", b"\r\n"):
            module.validate_verifier_output(
                verifier_output + terminator,
                b"",
                receipt.artifact_id,
                digest,
            )

        rejected = [
            (verifier_output, b""),
            (verifier_output + b"\nextra\n", b""),
            (verifier_output + b"\n\n", b""),
            (verifier_output + b"\r\n\r\n", b""),
            (verifier_output.replace(b",", b",\n", 1) + b"\n", b""),
            (verifier_output + b"\r", b""),
            (verifier_output + b"\x00\n", b""),
            (verifier_output + b"\n", b"hostile backend text"),
            (
                verifier_output.replace(b'"passed"', b'"failed"') + b"\n",
                b"",
            ),
            (verifier_output[:-1] + b',"extra":true}\n', b""),
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
        receipt = module.PUBLIC_POLICY.receipts["native-linux-x64"]
        digest = f"sha256:{receipt.digest}"

        attestation = module.build_attestation(receipt, WORKFLOW_HEAD, digest)
        module.validate_attestation(attestation)
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
            dict(attestation, release_run=str(module.PUBLIC_POLICY.release_run_id)),
        ]
        for malformed in malformed_attestations:
            with self.subTest(malformed=malformed):
                with self.assertRaises(module.AttestationError):
                    module.validate_attestation(malformed)

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
        powershell = module.PUBLIC_POLICY.receipts["powershell-windows-x64"]
        native = module.PUBLIC_POLICY.receipts["native-windows-x64"]
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

        verifier_output = json.dumps(
            {
                "schema_version": 1,
                "status": "passed",
                "artifacts": [
                    {
                        "id": powershell.artifact_id,
                        "digest": f"sha256:{powershell.digest}",
                    }
                ],
            },
            separators=(",", ":"),
        ).encode() + b"\n"

        def completed(command, **kwargs):
            self.assertEqual(
                command,
                [sys.executable, "/fixed/verifier.py", "powershell", "0.1.20"],
            )
            self.assertEqual(kwargs["env"].get("INSTALLER_NO_MODIFY_PATH"), "1")
            return subprocess.CompletedProcess(command, 0, verifier_output, b"")

        with mock.patch.object(subprocess, "run", side_effect=completed):
            stdout, stderr = module.execute_verifier(
                pathlib.Path("/fixed/verifier.py"),
                powershell,
                "0.1.20",
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
        receipt = module.PUBLIC_POLICY.receipts["native-linux-x64"]
        attestation = module.build_attestation(
            receipt,
            WORKFLOW_HEAD,
            f"sha256:{receipt.digest}",
        )
        with tempfile.TemporaryDirectory() as raw_directory:
            directory = pathlib.Path(raw_directory)
            output = directory / "attestation.json"
            module.write_attestation(output, attestation)
            self.assertEqual(
                json.loads(output.read_text(encoding="utf-8")),
                attestation,
            )
            with self.assertRaises(module.AttestationError):
                module.write_attestation(output, attestation)

            external = directory / "external"
            external.write_text("preserve", encoding="utf-8")
            linked = directory / "linked.json"
            linked.symlink_to(external)
            with self.assertRaises(module.AttestationError):
                module.write_attestation(linked, attestation)
            self.assertEqual(external.read_text(encoding="utf-8"), "preserve")

    def test_released_verifier_must_match_the_exact_commit_blob(self) -> None:
        module = load_subject()
        with tempfile.TemporaryDirectory() as raw_directory:
            repository = pathlib.Path(raw_directory) / "released-source"
            verifier = repository / "scripts" / "real_user_public_install_smoke.py"
            verifier.parent.mkdir(parents=True)
            committed_verifier = b"print('exact fixture')\n"
            verifier.write_bytes(committed_verifier)
            commands = [
                ["git", "init", "--quiet"],
                ["git", "add", str(module.VERIFIER_PATH)],
                [
                    "git",
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "--quiet",
                    "-m",
                    "fixture",
                ],
            ]
            for command in commands:
                result = subprocess.run(
                    command,
                    cwd=repository,
                    check=False,
                    capture_output=True,
                )
                self.assertEqual(result.returncode, 0, result.stderr.decode())
            head = subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=repository,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            self.assertEqual(
                module.validate_released_source(repository, head),
                committed_verifier,
            )

            verifier.write_bytes(committed_verifier.replace(b"\n", b"\r\n"))
            self.assertEqual(
                module.validate_released_source(repository, head),
                committed_verifier,
            )

            verifier.write_bytes(b"print('substituted')\r\n")
            self.assertEqual(
                module.validate_released_source(repository, head),
                committed_verifier,
            )

    def test_released_source_accepts_one_windows_crlf_git_line(self) -> None:
        module = load_subject()
        source_commit = "1" * 40
        verifier_content = b"# exact fixture\n"
        verifier_blob = hashlib.sha1(
            f"blob {len(verifier_content)}\0".encode() + verifier_content,
            usedforsecurity=False,
        ).hexdigest()
        with tempfile.TemporaryDirectory() as raw_directory:
            repository = pathlib.Path(raw_directory) / "released-source"
            verifier = repository / "scripts" / "real_user_public_install_smoke.py"
            verifier.parent.mkdir(parents=True)
            verifier.write_bytes(verifier_content.replace(b"\n", b"\r\n"))
            results = [
                subprocess.CompletedProcess(
                    [], 0, f"{source_commit}\r\n".encode(), b""
                ),
                subprocess.CompletedProcess(
                    [],
                    0,
                    (
                        f"100644 blob {verifier_blob}\t"
                        "scripts/real_user_public_install_smoke.py\r\n"
                    ).encode(),
                    b"",
                ),
                subprocess.CompletedProcess(
                    [], 0, f"{len(verifier_content)}\r\n".encode(), b""
                ),
                subprocess.CompletedProcess([], 0, verifier_content, b""),
            ]
            with mock.patch.object(module.subprocess, "run", side_effect=results):
                self.assertEqual(
                    module.validate_released_source(repository, source_commit),
                    verifier_content,
                )

    def test_released_source_rejects_extra_and_lookalike_git_lines(self) -> None:
        module = load_subject()
        source_commit = "1" * 40
        verifier_content = b"# exact fixture\n"
        verifier_blob = hashlib.sha1(
            f"blob {len(verifier_content)}\0".encode() + verifier_content,
            usedforsecurity=False,
        ).hexdigest()
        exact_tree = (
            f"100644 blob {verifier_blob}\t"
            "scripts/real_user_public_install_smoke.py\n"
        ).encode()
        variants = [
            [
                f"{source_commit}\nextra\n".encode(),
            ],
            [
                f"{source_commit}\n".encode(),
                exact_tree + b"100644 blob " + b"3" * 40 + b"\tlookalike\n",
            ],
            [
                f"{source_commit}\n".encode(),
                exact_tree.replace(
                    b"real_user_public_install_smoke.py\n",
                    b"real_user_public_install_smoke.py.bak\n",
                ),
            ],
            [
                f"{source_commit}\n".encode(),
                exact_tree,
                f"{len(verifier_content)}\nextra\n".encode(),
            ],
            [
                f"{source_commit}\n".encode(),
                exact_tree,
                b"9" * 64 + b"\n",
            ],
            [
                f"{source_commit}\n".encode(),
                exact_tree,
                f"{len(verifier_content)}\n".encode(),
                b"# substituted fixture\n",
            ],
        ]
        with tempfile.TemporaryDirectory() as raw_directory:
            repository = pathlib.Path(raw_directory) / "released-source"
            verifier = repository / "scripts" / "real_user_public_install_smoke.py"
            verifier.parent.mkdir(parents=True)
            verifier.write_bytes(verifier_content)
            for outputs in variants:
                results = [
                    subprocess.CompletedProcess([], 0, output, b"")
                    for output in outputs
                ]
                with self.subTest(outputs=outputs):
                    with (
                        mock.patch.object(
                            module.subprocess,
                            "run",
                            side_effect=results,
                        ),
                        self.assertRaises(module.AttestationError),
                    ):
                        module.validate_released_source(repository, source_commit)

    def test_offline_orchestration_uses_only_exact_metadata_and_assets(self) -> None:
        module = load_subject()
        payload = b"bounded native artifact"
        digest = hashlib.sha256(payload).hexdigest()
        checksum = f"{digest} *fixture.tar.xz\n".encode()
        checksum_digest = hashlib.sha256(checksum).hexdigest()
        base_receipt = module.PUBLIC_POLICY.receipts["native-linux-x64"]
        receipt = replace(
            base_receipt,
            asset_name="fixture.tar.xz",
            asset_size=len(payload),
            digest=digest,
        )
        policy = replace(
            module.PUBLIC_POLICY,
            checksum_asset_size=len(checksum),
            checksum_asset_digest=checksum_digest,
            receipts={receipt.name: receipt},
        )
        urls = module.api_urls(policy)
        responses = {
            urls["tag_ref"]: tag_ref_fixture(policy),
            urls["tag_object"]: tag_object_fixture(policy),
            urls["release_run"]: release_run_fixture(policy),
            urls["release"]: release_fixture(policy, receipt),
        }
        metadata_requests: list[str] = []
        asset_requests: list[tuple[str, int]] = []

        def read_json(url: str):
            metadata_requests.append(url)
            return responses[url]

        release = responses[urls["release"]]
        artifact_url = release["assets"][0]["browser_download_url"]
        checksum_url = release["assets"][1]["browser_download_url"]

        def read_asset(url: str, maximum: int) -> bytes:
            asset_requests.append((url, maximum))
            if url == artifact_url and maximum == len(payload):
                return payload
            if url == checksum_url and maximum == len(checksum):
                return checksum
            raise AssertionError("unexpected asset request")

        verifier_receipt = json.dumps(
            {
                "schema_version": 1,
                "status": "passed",
                "artifacts": [
                    {"id": receipt.artifact_id, "digest": f"sha256:{digest}"}
                ],
            },
            separators=(",", ":"),
        ).encode() + b"\n"
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
                    receipt_name=receipt.name,
                    tag=policy.tag,
                    version=policy.version,
                    source_commit=policy.source_commit,
                    release_run=str(policy.release_run_id),
                    mode=receipt.mode,
                    artifact_id=receipt.artifact_id,
                    asset=receipt.asset_name,
                    execution_platform=receipt.platform,
                    released_source=released_source,
                    output=output,
                    environment=workflow_environment(),
                    policy=policy,
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
