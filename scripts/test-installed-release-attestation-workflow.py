#!/usr/bin/env python3
"""Contract tests for the hosted installed-attestation workflow."""

from __future__ import annotations

import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "installed-release-attestations.yml"
RUNNER_POLICY = ROOT / "scripts" / "check-github-hosted-runners.py"


class InstalledReleaseAttestationWorkflowTests(unittest.TestCase):
    def workflow(self) -> str:
        self.assertTrue(WORKFLOW.is_file(), "missing installed attestation workflow")
        return WORKFLOW.read_text(encoding="utf-8")

    def receipt_run_command(self, workflow: str) -> str:
        marker = "      - name: Execute installed release receipt\n"
        step = workflow.split(marker, 1)[1].split("\n      - name:", 1)[0]
        run_lines = step.split("        run: >-\n", 1)[1].splitlines()
        return " ".join(line.strip() for line in run_lines if line.strip())

    def test_workflow_is_manual_read_only_with_one_scoped_job_token(self) -> None:
        workflow = self.workflow()
        self.assertIn("workflow_dispatch:", workflow)
        self.assertNotIn("pull_request_target", workflow)
        self.assertRegex(workflow, r"(?m)^permissions:\n  contents: read$")
        for forbidden in [
            "contents: write",
            "packages: write",
            "id-token: write",
            "secrets.",
            "qemu",
            "emulat",
        ]:
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, workflow)
        token_binding = "GITHUB_TOKEN: ${{ github.token }}"
        self.assertEqual(workflow.count(token_binding), 1)
        self.assertEqual(workflow.count("${{ github.token }}"), 1)

    def test_dispatch_inputs_bind_the_exact_release(self) -> None:
        workflow = self.workflow()
        for name, value in [
            ("tag", "v0.1.42"),
            ("version", "0.1.42"),
            ("source_commit", "ef8801c5887d0352bf103ee0da6550e26bfb93d3"),
            ("release_run", "31462670939"),
        ]:
            with self.subTest(name=name):
                self.assertRegex(
                    workflow,
                    rf"(?ms)^      {name}:\n.*?^        default: [\"']?{re.escape(value)}[\"']?$",
                )

    def test_matrix_always_contains_all_six_real_platform_receipts(self) -> None:
        workflow = self.workflow()
        self.assertNotIn("receipt_scope", workflow)
        self.assertNotIn("matrix.scopes", workflow)
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
            workflow.count("actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"),
            2,
        )
        self.assertIn("ref: ef8801c5887d0352bf103ee0da6550e26bfb93d3", workflow)
        self.assertIn("path: released-source", workflow)
        self.assertEqual(workflow.count("persist-credentials: false"), 2)
        self.assertIn(
            "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
            workflow,
        )
        self.assertIn("retention-days: 30", workflow)
        self.assertIn("if-no-files-found: error", workflow)
        self.assertIn('--receipt "$ATTESTATION_RECEIPT"', workflow)
        self.assertIn('--released-source "$ATTESTATION_RELEASED_SOURCE"', workflow)
        self.assertIn('--output "$ATTESTATION_OUTPUT"', workflow)

    def test_dispatch_values_cross_the_shell_only_through_quoted_environment_variables(
        self,
    ) -> None:
        workflow = self.workflow()
        command = self.receipt_run_command(workflow)
        self.assertIn("        shell: bash", workflow)
        self.assertNotIn("GITHUB_TOKEN", command)
        for option, variable, expression in [
            ("--tag", "ATTESTATION_TAG", "${{ inputs.tag }}"),
            ("--version", "ATTESTATION_VERSION", "${{ inputs.version }}"),
            (
                "--source-commit",
                "ATTESTATION_SOURCE_COMMIT",
                "${{ inputs.source_commit }}",
            ),
            (
                "--release-run",
                "ATTESTATION_RELEASE_RUN",
                "${{ inputs.release_run }}",
            ),
        ]:
            with self.subTest(option=option):
                self.assertIn(f"          {variable}: {expression}", workflow)
                self.assertIn(f'{option} "${variable}"', command)
                self.assertNotIn(expression, command)
                self.assertEqual(workflow.count(expression), 1)

        for forbidden in [
            "GITHUB_OUTPUT",
            "GITHUB_ENV",
            "set -x",
            "echo ",
            "printf ",
            "::debug",
        ]:
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, workflow)

    def test_hostile_dispatch_values_remain_literal_single_arguments(self) -> None:
        workflow = self.workflow()
        command = self.receipt_run_command(workflow)

        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = pathlib.Path(raw_tmp)
            scripts = tmp / "scripts"
            scripts.mkdir()
            capture = tmp / "captured.json"
            marker_one = tmp / "command-substitution-ran"
            marker_two = tmp / "backtick-ran"
            recorder = scripts / "installed_release_attestation.py"
            recorder.write_text(
                "import json, os, pathlib, sys\n"
                "pathlib.Path(os.environ['CAPTURE_PATH']).write_text(\n"
                "    json.dumps(sys.argv[1:]), encoding='utf-8'\n"
                ")\n",
                encoding="utf-8",
            )

            hostile = {
                "tag": 'v0.1.33\'\";|&<>',
                "version": f'0.1.33;$(touch "{marker_one}")',
                "source_commit": f'${{{{ github.token }}}}`touch "{marker_two}"`',
                "release_run": "31109523405\r\n$(exit 91)",
            }
            replacements = {
                "${{ matrix.python }}": sys.executable,
                "${{ matrix.receipt }}": "shell-linux-x64",
                "${{ inputs.tag }}": hostile["tag"],
                "${{ inputs.version }}": hostile["version"],
                "${{ inputs.source_commit }}": hostile["source_commit"],
                "${{ inputs.release_run }}": hostile["release_run"],
                "${{ matrix.mode }}": "shell",
                "${{ matrix.artifact_id }}": "installer:shell",
                "${{ matrix.asset }}": "logbrew-cli-installer.sh",
                "${{ matrix.platform }}": "linux-x64",
                "${{ github.workspace }}": str(tmp),
                "${{ runner.temp }}": str(tmp),
            }
            resolved = command
            for expression, value in replacements.items():
                resolved = resolved.replace(expression, value)

            env = os.environ.copy()
            env.update(
                {
                    "CAPTURE_PATH": str(capture),
                    "ATTESTATION_PYTHON": sys.executable,
                    "ATTESTATION_RECEIPT": "shell-linux-x64",
                    "ATTESTATION_TAG": hostile["tag"],
                    "ATTESTATION_VERSION": hostile["version"],
                    "ATTESTATION_SOURCE_COMMIT": hostile["source_commit"],
                    "ATTESTATION_RELEASE_RUN": hostile["release_run"],
                    "ATTESTATION_MODE": "shell",
                    "ATTESTATION_ARTIFACT_ID": "installer:shell",
                    "ATTESTATION_ASSET": "logbrew-cli-installer.sh",
                    "ATTESTATION_PLATFORM": "linux-x64",
                    "ATTESTATION_RELEASED_SOURCE": str(tmp / "released-source"),
                    "ATTESTATION_OUTPUT": str(tmp / "attestation.json"),
                }
            )
            completed = subprocess.run(
                ["bash", "-c", resolved],
                cwd=tmp,
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)
            self.assertFalse(marker_one.exists())
            self.assertFalse(marker_two.exists())
            self.assertEqual(
                json.loads(capture.read_text(encoding="utf-8")),
                [
                    "--receipt",
                    "shell-linux-x64",
                    "--tag",
                    hostile["tag"],
                    "--version",
                    hostile["version"],
                    "--source-commit",
                    hostile["source_commit"],
                    "--release-run",
                    hostile["release_run"],
                    "--mode",
                    "shell",
                    "--artifact-id",
                    "installer:shell",
                    "--asset",
                    "logbrew-cli-installer.sh",
                    "--execution-platform",
                    "linux-x64",
                    "--released-source",
                    str(tmp / "released-source"),
                    "--output",
                    str(tmp / "attestation.json"),
                ],
            )


if __name__ == "__main__":
    unittest.main()
