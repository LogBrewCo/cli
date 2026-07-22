#!/usr/bin/env python3
"""Contract tests for the hosted installed-attestation workflow."""

from __future__ import annotations

import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "installed-release-attestations.yml"
RUNNER_POLICY = ROOT / "scripts" / "check-github-hosted-runners.py"


class InstalledReleaseAttestationWorkflowTests(unittest.TestCase):
    def workflow(self) -> str:
        self.assertTrue(WORKFLOW.is_file(), "missing installed attestation workflow")
        return WORKFLOW.read_text(encoding="utf-8")

    def test_workflow_is_manual_read_only_and_secret_free(self) -> None:
        workflow = self.workflow()
        self.assertIn("workflow_dispatch:", workflow)
        self.assertNotIn("pull_request_target", workflow)
        self.assertRegex(workflow, r"(?m)^permissions:\n  contents: read$")
        for forbidden in [
            "contents: write",
            "packages: write",
            "id-token: write",
            "secrets.",
            "GITHUB_TOKEN",
            "qemu",
            "emulat",
        ]:
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, workflow)

    def test_dispatch_inputs_bind_the_exact_release(self) -> None:
        workflow = self.workflow()
        for name, value in [
            ("tag", "v0.1.20"),
            ("version", "0.1.20"),
            ("source_commit", "018b1a832d143c203b2822652d1bce9fb16401ab"),
            ("release_run", "29935721685"),
        ]:
            with self.subTest(name=name):
                self.assertRegex(
                    workflow,
                    rf"(?ms)^      {name}:\n.*?^        default: [\"']?{re.escape(value)}[\"']?$",
                )

    def test_matrix_contains_only_the_six_missing_real_platform_receipts(self) -> None:
        workflow = self.workflow()
        expected = {
            (
                "shell-linux-x64",
                "ubuntu-24.04",
                "linux-x64",
                "shell",
                "installer:shell",
                "logbrew-cli-installer.sh",
            ),
            (
                "native-linux-arm64",
                "ubuntu-24.04-arm",
                "linux-arm64",
                "native",
                "native:linux-arm64",
                "logbrew-cli-aarch64-unknown-linux-gnu.tar.xz",
            ),
            (
                "native-linux-x64",
                "ubuntu-24.04",
                "linux-x64",
                "native",
                "native:linux-x64",
                "logbrew-cli-x86_64-unknown-linux-gnu.tar.xz",
            ),
            (
                "powershell-windows-x64",
                "windows-2025",
                "windows-x64",
                "powershell",
                "installer:powershell",
                "logbrew-cli-installer.ps1",
            ),
            (
                "native-windows-x64",
                "windows-2025",
                "windows-x64",
                "native",
                "native:windows-x64",
                "logbrew-cli-x86_64-pc-windows-msvc.zip",
            ),
            (
                "native-macos-x64",
                "macos-15-intel",
                "macos-x64",
                "native",
                "native:macos-x64",
                "logbrew-cli-x86_64-apple-darwin.tar.xz",
            ),
        }
        rows = set(
            re.findall(
                r"(?ms)^          - receipt: ([^\n]+)\n"
                r"            runner: ([^\n]+)\n"
                r"            platform: ([^\n]+)\n"
                r"            mode: ([^\n]+)\n"
                r"            artifact_id: ([^\n]+)\n"
                r"            asset: ([^\n]+)$",
                workflow,
            )
        )
        self.assertEqual(rows, expected)
        self.assertEqual(workflow.count("          - receipt:"), 6)
        self.assertIn("runs-on: ${{ matrix.runner }}", workflow)

        runner_policy = RUNNER_POLICY.read_text(encoding="utf-8")
        for runner in {row[1] for row in expected}:
            self.assertIn(f'"{runner}"', runner_policy)

    def test_workflow_checks_out_released_source_and_uploads_one_fixed_receipt(self) -> None:
        workflow = self.workflow()
        self.assertEqual(
            workflow.count("actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803"),
            2,
        )
        self.assertIn("ref: ${{ inputs.source_commit }}", workflow)
        self.assertIn("path: released-source", workflow)
        self.assertEqual(workflow.count("persist-credentials: false"), 2)
        self.assertIn(
            "actions/upload-artifact@b7c566a772e6b6bfb58ed0dc250532a479d7789f",
            workflow,
        )
        self.assertIn("retention-days: 30", workflow)
        self.assertIn("if-no-files-found: error", workflow)
        self.assertIn("--receipt ${{ matrix.receipt }}", workflow)
        self.assertIn("--released-source", workflow)
        self.assertIn("${{ runner.temp }}/attestation.json", workflow)


if __name__ == "__main__":
    unittest.main()
