const RELEASE: &str = include_str!("../../.github/workflows/release.yml");
const CRATES: &str = include_str!("../../.github/workflows/publish-crates.yml");
const HOMEBREW: &str = include_str!("../../.github/workflows/publish-homebrew-tap.yml");
const DIST: &str = include_str!("../../dist-workspace.toml");

fn ordered(source: &str, required: &[&str]) {
    let positions = required
        .iter()
        .map(|value| {
            source
                .find(value)
                .unwrap_or_else(|| panic!("missing {value}"))
        })
        .collect::<Vec<_>>();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn release_prebuilds_exact_main_and_keeps_tag_publication_thin() {
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
        "gh release create \"${{ needs.plan.outputs.tag }}\" --target \"$GITHUB_SHA\"",
    ] {
        assert!(RELEASE.contains(required), "missing {required}");
    }
    assert_eq!(
        RELEASE
            .matches("needs.plan.outputs.building == 'true'")
            .count(),
        2
    );
    assert!(!RELEASE.contains("pull_request_target"));
    assert!(!RELEASE.contains("schedule:"));
    assert!(DIST.contains("allow-dirty = [\"ci\"]"));
    assert!(!DIST.contains("pr-run-mode"));
}

#[test]
fn crates_publish_reuses_the_exact_prebuild_without_recompiling() {
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
    assert!(!CRATES.contains("cargo build"));
    assert!(!CRATES.contains("windows-release-build"));
    assert_eq!(CRATES.matches("cargo publish").count(), 1);
    assert!(!CRATES.contains("pull_request_target"));
    assert!(!CRATES.contains("schedule:"));
}

#[test]
fn homebrew_formula_is_validated_before_the_single_write() {
    for required in [
        "workflow_call:",
        "permissions:\n  contents: read",
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
    assert!(!HOMEBREW.contains("brew update"));
    assert!(!HOMEBREW.contains("--except-cops"));
    assert!(!HOMEBREW.contains("for release in $("));
    assert!(!HOMEBREW.contains("echo \"$PLAN\""));
    assert!(!HOMEBREW.contains("pull_request_target"));
    assert!(!HOMEBREW.contains("schedule:"));
}
