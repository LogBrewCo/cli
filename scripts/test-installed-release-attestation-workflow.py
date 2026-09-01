#!/usr/bin/env python3
"""Contract tests for the hosted installed-attestation workflow."""

from __future__ import annotations

import json
import os
import pathlib
import re
import runpy
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

    def test_workflow_is_unattended_with_read_only_recovery(self) -> None:
        workflow = self.workflow()
        self.assertIn("push:", workflow)
        self.assertIn('      - "**[0-9]+.[0-9]+.[0-9]+*"', workflow)
        self.assertIn("workflow_dispatch:", workflow)
        self.assertIn("startsWith(github.ref, 'refs/tags/v')", workflow)
        self.assertNotIn("workflow_run", workflow)
        self.assertNotIn("pull_request_target", workflow)
        self.assertRegex(workflow, r"(?m)^permissions:\n  actions: read\n  contents: read$")
        for forbidden in (
            "contents: write", "packages: write", "id-token: write",
            "secrets.", "qemu", "emulat",
        ):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, workflow)
        self.assertEqual(workflow.count("GH_TOKEN: ${{ github.token }}"), 2)
        self.assertEqual(workflow.count("GITHUB_TOKEN: ${{ github.token }}"), 1)

    def test_dispatch_inputs_are_required_without_stale_defaults(self) -> None:
        inputs = self.workflow().split("permissions:", 1)[0]
        for name in ["tag", "source_commit", "release_run"]:
            with self.subTest(name=name):
                self.assertRegex(
                    inputs,
                    rf"(?m)^      {name}:\n        description: [^\n]+\n"
                    r"        required: true\n        type: string$",
                )
        self.assertNotIn("        default:", inputs)

    def test_matrix_always_contains_all_six_real_platform_receipts(self) -> None:
        workflow = self.workflow()
        self.assertNotIn("receipt_scope", workflow)
        self.assertNotIn("matrix.scopes", workflow)
        receipts = runpy.run_path(str(ROOT / "scripts/installed_release_attestation.py"))[
            "RECEIPTS"
        ]
        expected = {
            (name, spec.runner, spec.platform, spec.mode, spec.artifact_id, spec.asset_name)
            for name, spec in receipts.items()
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
        self.assertEqual(workflow.count("          - receipt:"), len(expected))
        self.assertIn("runs-on: ${{ matrix.runner }}", workflow)

        runner_policy = RUNNER_POLICY.read_text(encoding="utf-8")
        for runner in {row[1] for row in expected}:
            self.assertIn(f'"{runner}"', runner_policy)

    def test_workflow_reuses_one_exact_source_checkout_and_uploads_one_receipt(self) -> None:
        workflow = self.workflow()
        self.assertEqual(
            workflow.count("actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"),
            1,
        )
        self.assertIn("ref: ${{ inputs.source_commit || github.sha }}", workflow)
        self.assertNotIn("path: released-source", workflow)
        self.assertEqual(workflow.count("persist-credentials: false"), 1)
        self.assertIn(
            "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
            workflow,
        )
        self.assertIn("retention-days: 30", workflow)
        self.assertIn("if-no-files-found: error", workflow)
        self.assertIn('--receipt "$ATTESTATION_RECEIPT"', workflow)
        self.assertIn('--released-source "$ATTESTATION_RELEASED_SOURCE"', workflow)
        self.assertIn('--output "$ATTESTATION_OUTPUT"', workflow)

    def test_tag_run_and_public_release_are_resolved_before_receipts(self) -> None:
        workflow = self.workflow()
        step = workflow
        for required in [
            "--workflow release.yml",
            "--event push",
            '--branch "$RELEASE_TAG"',
            '--commit "$SOURCE_COMMIT"',
            'gh release view "$RELEASE_TAG"',
            "for _ in {1..100}",
            "sleep 0.5",
            "run-id=%s",
            "for _ in {1..40}",
            '[[ "$state" == "completed|success" ]]',
        ]:
            with self.subTest(required=required):
                self.assertIn(required, step)
        self.assertIn('>> "$GITHUB_OUTPUT"', step)
        self.assertLess(step.index("Require successful release"), step.index("Upload installed attestation"))

    def test_dispatch_values_cross_the_shell_only_through_quoted_environment_variables(
        self,
    ) -> None:
        workflow = self.workflow()
        command = self.receipt_run_command(workflow)
        self.assertIn("        shell: bash", workflow)
        self.assertNotIn("GITHUB_TOKEN", command)
        for option, variable, expression in [
            (
                "--tag",
                "ATTESTATION_TAG",
                "${{ inputs.tag || github.ref_name }}",
            ),
            (
                "--source-commit",
                "ATTESTATION_SOURCE_COMMIT",
                "${{ inputs.source_commit || github.sha }}",
            ),
        ]:
            with self.subTest(option=option):
                self.assertIn(f"          {variable}: {expression}", workflow)
                self.assertIn(f'{option} "${variable}"', command)
                self.assertNotIn(expression, command)
        self.assertIn("ATTESTATION_RELEASE_RUN: ${{ steps.release.outputs.run-id }}", workflow)
        self.assertIn('--release-run "$ATTESTATION_RELEASE_RUN"', command)

        for forbidden in ("GITHUB_ENV", "set -x", "echo ", "::debug"):
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
            (scripts / "installed_release_attestation.py").write_text(
                "import json,os,pathlib,sys\n"
                "pathlib.Path(os.environ['CAPTURE_PATH']).write_text(json.dumps(sys.argv[1:]))\n",
                encoding="utf-8",
            )

            hostile = {
                "tag": 'v0.1.33\'\";|&<>',
                "source_commit": (
                    f'${{{{ github.token }}}}$(touch "{marker_one}")'
                    f'`touch "{marker_two}"`'
                ),
                "release_run": "31109523405\r\n$(exit 91)",
            }
            fields = [
                ("--receipt", "ATTESTATION_RECEIPT", "shell-linux-x64"),
                ("--tag", "ATTESTATION_TAG", hostile["tag"]),
                ("--source-commit", "ATTESTATION_SOURCE_COMMIT", hostile["source_commit"]),
                ("--release-run", "ATTESTATION_RELEASE_RUN", hostile["release_run"]),
                ("--mode", "ATTESTATION_MODE", "shell"),
                ("--artifact-id", "ATTESTATION_ARTIFACT_ID", "installer:shell"),
                ("--asset", "ATTESTATION_ASSET", "logbrew-cli-installer.sh"),
                ("--execution-platform", "ATTESTATION_PLATFORM", "linux-x64"),
                ("--released-source", "ATTESTATION_RELEASED_SOURCE", str(tmp / "released-source")),
                ("--output", "ATTESTATION_OUTPUT", str(tmp / "attestation.json")),
            ]
            env = os.environ | {name: value for _, name, value in fields}
            env.update(CAPTURE_PATH=str(capture), ATTESTATION_PYTHON=sys.executable)
            completed = subprocess.run(
                ["bash", "-c", command],
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
                [item for flag, _, value in fields for item in (flag, value)],
            )


if __name__ == "__main__":
    unittest.main()
