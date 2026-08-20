const RELEASE: &str = include_str!("../../.github/workflows/release.yml");
const CRATES: &str = include_str!("../../.github/workflows/publish-crates.yml");
const NPM: &str = include_str!("../../.github/workflows/publish-npm-trusted.yml");
const HOMEBREW: &str = include_str!("../../.github/workflows/publish-homebrew-tap.yml");
const DIST: &str = include_str!("../../dist-workspace.toml");

fn ordered(source: &str, required: &[&str]) {
    let mut cursor = 0;
    for value in required {
        cursor += source[cursor..].find(value).expect("missing ordered step");
    }
}

#[test]
fn release_workflows_prebuild_publish_and_recover_safely() {
    for required in [
        "workflow_dispatch:",
        "building: ${{ github.event_name == 'workflow_dispatch' && inputs.release_tag == '' }}",
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
        "inputs.release_tag != '' && inputs.artifacts_run_id != ''",
        "artifacts_run_id: ${{ inputs.artifacts_run_id || needs.plan.outputs.prebuild-run-id }}",
        "gh release create \"${{ needs.plan.outputs.tag }}\" --target \"$GITHUB_SHA\"",
    ] {
        assert!(RELEASE.contains(required), "missing {required}");
    }
    assert_eq!(RELEASE.matches("outputs.building").count(), 2);
    assert_eq!(RELEASE.matches("artifacts_run_id:").count(), 3);
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
    for source in [NPM, HOMEBREW] {
        for required in [
            "workflow_call:",
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
    assert!(NPM.contains("npm publish --access public \"./${packages[0]}\""));
    assert!(!NPM.contains("  workflow_dispatch:"));

    for required in [
        "  workflow_dispatch:",
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
        "HOMEBREW_NO_AUTO_UPDATE: \"1\"",
        "ln -s \"${GITHUB_WORKSPACE}/tap\" \"${audit_repo}\"",
        "unlink \"${audit_repo}\"",
    ] {
        assert!(HOMEBREW.contains(required), "missing {required}");
    }
    ordered(
        HOMEBREW,
        &[
            "ruby ../release-tooling/scripts/prepare-homebrew-formula.rb",
            "brew audit --fix --strict --online --formula \"${audit_tap}/${name}\"",
            "ruby -c \"Formula/${filename}\"",
            "git add \"Formula/${filename}\"",
            "git commit -m \"${name} ${version}\"",
            "git push",
        ],
    );
    assert_eq!(HOMEBREW.matches("secrets.HOMEBREW_TAP_TOKEN").count(), 1);
    assert_eq!(HOMEBREW.matches("uses: actions/checkout@v7").count(), 2);
    assert!(!HOMEBREW.contains("brew update") && !HOMEBREW.contains("brew style"));
    assert!(!HOMEBREW.contains("brew tap-new") && !HOMEBREW.contains("--except-cops"));
    assert!(!HOMEBREW.contains("for release in $(") && !HOMEBREW.contains("echo \"$PLAN\""));
    for source in [RELEASE, CRATES, NPM, HOMEBREW] {
        assert!(!source.contains("pull_request_target") && !source.contains("schedule:"));
    }
}
