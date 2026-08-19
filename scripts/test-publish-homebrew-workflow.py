#!/usr/bin/env python3
"""Contract tests for strict Homebrew formula publication."""

from __future__ import annotations

import pathlib
import tomllib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "publish-homebrew-tap.yml"
CHECK_ALL = ROOT / "scripts" / "check-all.sh"
CARGO_MANIFEST = ROOT / "Cargo.toml"


class PublishHomebrewWorkflowTests(unittest.TestCase):
    def workflow(self) -> str:
        self.assertTrue(WORKFLOW.is_file(), "missing Homebrew publish workflow")
        return WORKFLOW.read_text(encoding="utf-8")

    def test_reusable_workflow_has_one_narrow_write_credential(self) -> None:
        workflow = self.workflow()
        self.assertIn("workflow_call:", workflow)
        self.assertRegex(workflow, r"(?m)^permissions:\n  contents: read$")
        self.assertNotIn("pull_request_target", workflow)
        self.assertNotIn("schedule:", workflow)
        self.assertEqual(workflow.count("${{ secrets.HOMEBREW_TAP_TOKEN }}"), 1)
        self.assertEqual(workflow.count("uses: actions/checkout@v7"), 2)

        tooling = workflow.split("      - name: Checkout release tooling\n", 1)[1]
        tooling = tooling.split("\n      - name:", 1)[0]
        self.assertIn("persist-credentials: false", tooling)
        self.assertIn("path: release-tooling", tooling)
        self.assertNotIn("HOMEBREW_TAP_TOKEN", tooling)

        tap = workflow.split("      - name: Checkout tap\n", 1)[1]
        tap = tap.split("\n      - name:", 1)[0]
        self.assertIn("persist-credentials: true", tap)
        self.assertIn('repository: "LogBrewCo/homebrew-tap"', tap)
        self.assertIn("path: tap", tap)
        self.assertIn("token: ${{ secrets.HOMEBREW_TAP_TOKEN }}", tap)

    def test_formula_artifact_is_prepared_and_audited_before_commit(self) -> None:
        workflow = self.workflow()
        required = [
            "path: tap/Formula/",
            "ruby ../release-tooling/scripts/prepare-homebrew-formula.rb",
            'brew style --fix "Formula/${filename}"',
            'ruby -c "Formula/${filename}"',
            'brew style "Formula/${filename}"',
            'brew audit --strict --online --formula "${audit_tap}/${name}"',
            'git add "Formula/${filename}"',
            'git commit -m "${name} ${version}"',
            "git push",
        ]
        for value in required:
            with self.subTest(value=value):
                self.assertIn(value, workflow)
        self.assertEqual(
            [workflow.index(value) for value in required],
            sorted(workflow.index(value) for value in required),
        )
        self.assertNotIn("--except-cops", workflow)
        self.assertNotIn("|| true\n            git", workflow)

    def test_plan_iteration_and_formula_identity_fail_closed(self) -> None:
        workflow = self.workflow()
        self.assertIn("set -euo pipefail", workflow)
        self.assertIn("while IFS= read -r release; do", workflow)
        self.assertNotIn("for release in $(", workflow)
        self.assertNotIn('echo "$PLAN"', workflow)
        self.assertIn(
            '[[ ! "${filename}" =~ ^[a-z0-9][a-z0-9._+-]*\\.rb$ ]]',
            workflow,
        )
        self.assertIn(
            '[[ ! "${version}" =~ ^[0-9]+\\.[0-9]+\\.[0-9]+$ ]]',
            workflow,
        )
        self.assertIn('if [[ "${release_count}" -eq 0 ]]', workflow)
        self.assertIn('<<<"${PLAN}"', workflow)

    def test_temporary_audit_tap_is_bounded_and_cleaned(self) -> None:
        workflow = self.workflow()
        self.assertIn('audit_tap="logbrew-release-verifier/formula"', workflow)
        self.assertIn('brew tap-new --no-git "${audit_tap}"', workflow)
        self.assertIn('audit_repo="$(brew --repository "${audit_tap}")"', workflow)
        self.assertIn("trap cleanup_audit_tap EXIT", workflow)
        self.assertIn('brew untap "${audit_tap}"', workflow)
        self.assertIn("trap - EXIT", workflow)
        self.assertEqual(workflow.count("cleanup_audit_tap"), 3)

    def test_package_description_and_repository_gate_match_homebrew_policy(
        self,
    ) -> None:
        manifest = tomllib.loads(CARGO_MANIFEST.read_text(encoding="utf-8"))
        self.assertEqual(
            manifest["package"]["description"],
            "Developer-first observability command-line interface",
        )
        check_all = CHECK_ALL.read_text(encoding="utf-8")
        self.assertIn("for dependency in cargo-audit python3 ruby", check_all)
        self.assertIn(
            "ruby scripts/test-prepare-homebrew-formula.rb",
            check_all,
        )
        self.assertIn(
            "python3 scripts/test-publish-homebrew-workflow.py",
            check_all,
        )


if __name__ == "__main__":
    unittest.main()
