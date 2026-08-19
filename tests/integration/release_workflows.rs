const RELEASE: &str = include_str!("../../.github/workflows/release.yml");
const CRATES: &str = include_str!("../../.github/workflows/publish-crates.yml");
const NPM: &str = include_str!("../../.github/workflows/publish-npm-trusted.yml");
const HOMEBREW: &str = include_str!("../../.github/workflows/publish-homebrew-tap.yml");
const DIST: &str = include_str!("../../dist-workspace.toml");

fn ordered(source: &str, required: &[&str]) {
    let mut cursor = 0;
    for value in required {
        cursor += source[cursor..]
            .find(value)
            .unwrap_or_else(|| panic!("missing {value}"));
    }
}

#[test]
fn release_workflows_prebuild_publish_and_recover_safely() {
    for required in [
        "workflow_dispatch:",
        "building: ${{ github.event_name == 'workflow_dispatch' }}",
        "publishing: ${{ github.event_name == 'push' }}",
        "[[ \"$GITHUB_REF\" == \"refs/heads/main\" ]]",
        "--workflow release.yml",
        "--event workflow_dispatch",
        "--branch main",
        "--commit \"$GITHUB_SHA\"",
        "--status success",
        "if: ${{ needs.plan.outputs.building == 'true' }}",
        "run-id: ${{ needs.plan.outputs.prebuild-run-id }}",
        "pattern: artifacts-build-*",
        "always() && needs.plan.result == 'success' && needs.host.result == 'success'",
        "artifacts_run_id: ${{ needs.plan.outputs.prebuild-run-id }}",
        "gh release create \"${{ needs.plan.outputs.tag }}\" --target \"$GITHUB_SHA\"",
    ] {
        assert!(RELEASE.contains(required), "missing {required}");
    }
    assert_eq!(RELEASE.matches("outputs.building").count(), 2);
    assert_eq!(RELEASE.matches("artifacts_run_id:").count(), 2);
    assert!(!RELEASE.contains("pull_request_target") && !RELEASE.contains("schedule:"));
    assert!(DIST.contains("allow-dirty = [\"ci\"]") && !DIST.contains("pr-run-mode"));

    for required in [
        "actions: read",
        "--workflow release.yml",
        "--event workflow_dispatch",
        "--branch main",
        "--commit \"$GITHUB_SHA\"",
        "--status success",
        "uses: rust-lang/crates-io-auth-action@v1.0.5",
        "cargo publish --locked --no-verify",
    ] {
        assert!(CRATES.contains(required), "missing {required}");
    }
    assert!(!CRATES.contains("cargo build") && !CRATES.contains("windows-release-build"));
    assert_eq!(CRATES.matches("cargo publish").count(), 1);
    assert!(!CRATES.contains("pull_request_target") && !CRATES.contains("schedule:"));

    for source in [NPM, HOMEBREW] {
        for required in [
            "workflow_dispatch:",
            "actions: read",
            "release_tag:",
            "--repo \"$GITHUB_REPOSITORY\"",
            "[[ \"$(jq -r '.headSha' <<<\"$run\")\" == \"$target\" ]]",
            "run-id: ${{ inputs.artifacts_run_id }}",
            "pattern: artifacts-build-global",
        ] {
            assert!(source.contains(required), "missing {required}");
        }
    }

    for required in [
        "workflow_call:",
        "permissions:\n  actions: read\n  contents: read",
        "persist-credentials: false",
        "persist-credentials: true",
        "token: ${{ secrets.HOMEBREW_TAP_TOKEN }}",
        "while IFS= read -r release; do",
        "path: tap/Formula/",
        "if [[ \"${release_count}\" -eq 0 ]]",
        "[[ ! \"${filename}\" =~ ^[a-z0-9][a-z0-9._+-]*\\.rb$ ]]",
        "[[ ! \"${version}\" =~ ^[0-9]+\\.[0-9]+\\.[0-9]+$ ]]",
        "set -euo pipefail",
        "trap cleanup_audit_tap EXIT",
        "brew untap \"${audit_tap}\"",
    ] {
        assert!(HOMEBREW.contains(required), "missing {required}");
    }
    ordered(
        HOMEBREW,
        &[
            "ruby ../release-tooling/scripts/prepare-homebrew-formula.rb",
            "brew style --fix \"Formula/${filename}\"",
            "ruby -c \"Formula/${filename}\"",
            "brew style \"Formula/${filename}\"",
            "brew audit --strict --online --formula \"${audit_tap}/${name}\"",
            "git add \"Formula/${filename}\"",
            "git commit -m \"${name} ${version}\"",
            "git push",
        ],
    );
    assert_eq!(HOMEBREW.matches("secrets.HOMEBREW_TAP_TOKEN").count(), 1);
    assert_eq!(HOMEBREW.matches("uses: actions/checkout@v7").count(), 2);
    assert!(!HOMEBREW.contains("brew update") && !HOMEBREW.contains("--except-cops"));
    assert!(!HOMEBREW.contains("for release in $("));
    assert!(!HOMEBREW.contains("echo \"$PLAN\""));
    assert!(!HOMEBREW.contains("pull_request_target") && !HOMEBREW.contains("schedule:"));
}
