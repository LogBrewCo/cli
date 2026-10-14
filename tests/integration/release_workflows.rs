const RELEASE: &str = include_str!("../../.github/workflows/release.yml");
const CRATES: &str = include_str!("../../.github/workflows/publish-crates.yml");
const NPM: &str = include_str!("../../.github/workflows/publish-npm-trusted.yml");
const HOMEBREW: &str = include_str!("../../.github/workflows/publish-homebrew-tap.yml");
const PUBLISH_BOUNDARY: &str = include_str!("../../scripts/validate-publish-boundary.sh");
const DIST: &str = include_str!("../../dist-workspace.toml");
const NPM_PACKAGE: &str = include_str!("../../npm/package.json");
const NPM_PACKAGES: &str = include_str!("../../scripts/npm-packages.sh");
const PREFLIGHT: &str = include_str!("../../scripts/release-preflight.sh");

fn ordered(text: &str, values: &[&str]) -> bool {
    values
        .iter()
        .try_fold(text, |remaining, value| {
            remaining.split_once(value).map(|(_, tail)| tail)
        })
        .is_some()
}

#[test]
fn ordering_requires_distinct_sequential_matches() {
    for (text, values, expected) in [
        ("first second", ["first", "second"], true),
        ("second first", ["first", "second"], false),
        ("first", ["first", "second"], false),
        ("first", ["first", "first"], false),
        ("first first", ["first", "first"], true),
    ] {
        assert_eq!(
            ordered(text, &values),
            expected,
            "incorrect ordering: {text}"
        );
    }
}

#[test]
fn release_workflows_prebuild_publish_and_recover_safely() -> crate::TestResult {
    for required in [
        "workflow_dispatch:",
        "publishing: ${{ github.event_name == 'push' }}",
        "[[ \"$GITHUB_REF\" == \"refs/heads/main\" ]]",
        "--workflow release.yml",
        "--event workflow_dispatch",
        "--branch main",
        "--commit \"$GITHUB_SHA\"",
        "--status success",
        "if: ${{ github.event_name == 'workflow_dispatch' && inputs.release_tag == '' && github.ref == 'refs/heads/main' }}",
        "key: workspace-v1-${{ matrix.target }}",
        "cache-workspace-crates: true",
        "runner: windows-2025\n            linker: lld-link",
        "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER: ${{ matrix.linker }}",
        "run-id: ${{ steps.prebuild.outputs.run-id }}",
        "pattern: artifacts-build-*",
        "name: Prepare and audit Homebrew formula",
        "ruby scripts/prepare-homebrew-formula.rb target/distrib/logbrew.rb \"$version\"",
        "brew audit --fix --strict --formula logbrew-release-verifier/formula/logbrew || true",
        "brew audit --strict --formula logbrew-release-verifier/formula/logbrew",
        "inputs.release_tag != '' && inputs.artifacts_run_id != ''",
        "artifacts_run_id: ${{ inputs.artifacts_run_id || needs.plan.outputs.prebuild-run-id }}",
        "gh release create \"$GITHUB_REF_NAME\" --target \"$GITHUB_SHA\"",
        "Verify public shell installation",
        "cmp \"$installer\" target/distrib/logbrew-cli-installer.sh",
        "bash scripts/npm-packages.sh \"$version\" target/npm \"${binaries[@]}\"",
        "name: npm-packages\n          path: target/npm/*.tgz",
    ] {
        assert!(RELEASE.contains(required), "missing {required}");
    }
    assert!(ordered(
        RELEASE,
        &[
            "  plan:\n    runs-on: ubuntu-22.04\n    permissions:",
            "      id-token: write\n      attestations: write\n",
            "-name '*-dist-manifest.json' -delete",
            "uses: actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8 # v4.2.2",
            "subject-path: target/distrib/*",
            "gh release create \"$GITHUB_REF_NAME\"",
        ]
    ));
    assert_eq!(RELEASE.matches("id-token: write").count(), 2);
    assert_eq!(RELEASE.matches("attestations: write").count(), 1);
    assert_eq!(RELEASE.matches("refs/heads/main' }}").count(), 2);
    assert_eq!(RELEASE.matches("artifacts_run_id:").count(), 3);
    assert!(!RELEASE.contains("\n  host:\n"));
    assert!(!RELEASE.contains("fromJson(steps.host.outputs.manifest)"));
    let targets = DIST
        .lines()
        .find(|line| line.starts_with("targets = "))
        .ok_or("distribution targets missing")?;
    for target in targets.split('"').skip(1).step_by(2) {
        assert!(RELEASE.contains(target), "missing {target}");
    }
    assert!(DIST.contains("allow-dirty = [\"ci\"]") && !DIST.contains("pr-run-mode"));
    assert!(
        ![
            "--print=linkage",
            "submodules: recursive",
            "Install dependencies"
        ]
        .iter()
        .any(|value| RELEASE.contains(value))
    );

    Ok(())
}

#[test]
fn publication_requires_verified_source_and_single_upload() {
    for required in [
        "actions: read",
        "--workflow release.yml",
        "--event workflow_dispatch",
        "--branch main",
        "--commit \"$GITHUB_SHA\"",
        "--status success",
        "uses: rust-lang/crates-io-auth-action@c6f97d42243bad5fab37ca0427f495c86d5b1a18 # v1.0.5",
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
            ".headSha == $target",
            "run-id: ${{ inputs.artifacts_run_id }}",
        ] {
            assert!(
                source.contains(required) || PUBLISH_BOUNDARY.contains(required),
                "missing {required}"
            );
        }
        assert!(source.contains("validate-publish-boundary.sh"));
    }
    assert_eq!(PUBLISH_BOUNDARY.matches("|| exit 1").count(), 5);
    assert!(HOMEBREW.contains("pattern: artifacts-build-global"));
    assert!(NPM.contains("name: npm-packages") && !NPM.contains("artifacts-build-global"));
    assert_eq!(
        NPM.matches("npm publish --access public \"./$tarball\"")
            .count(),
        1
    );
    assert!(NPM.contains("(has(\"scripts\") | not)"));
    assert!(NPM.contains("[[ \"$published\" == \"$integrity\" ]] || exit 1"));
    assert!(NPM.contains("node-version: \"24.19.0\"") && !NPM.contains("npm install --global"));
    assert!(!NPM.contains("  workflow_dispatch:"));

    for required in [
        "  workflow_dispatch:",
        "permissions:\n  actions: read\n  contents: read",
        "persist-credentials: false",
        "persist-credentials: true",
        "token: ${{ secrets.HOMEBREW_TAP_TOKEN }}",
        "path: tap/Formula/",
        "($formulae | length == 1)",
        "mapfile -t formulae < <(find Formula -maxdepth 1 -name '*.rb' -print)",
        "set -euo pipefail",
    ] {
        assert!(HOMEBREW.contains(required), "missing {required}");
    }
    assert!(
        ordered(
            HOMEBREW,
            &[
                "git add Formula/logbrew.rb",
                "git commit -m \"logbrew ${expected_version}\"",
                "validate-publish-boundary.sh",
                "git push",
            ],
        ),
        "formula commit, validation and push must remain ordered"
    );
    assert_eq!(HOMEBREW.matches("secrets.HOMEBREW_TAP_TOKEN").count(), 1);
    assert_eq!(
        HOMEBREW
            .matches("uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1")
            .count(),
        2
    );
    assert!(!HOMEBREW.contains("\n          brew ") && !HOMEBREW.contains("\n          ruby "));
    assert!(!HOMEBREW.contains("for release in $(") && !HOMEBREW.contains("echo \"$PLAN\""));
    for source in [RELEASE, CRATES, NPM, HOMEBREW] {
        assert!(!source.contains("pull_request_target") && !source.contains("schedule:"));
    }
}

#[test]
fn npm_release_publishes_script_free_platform_packages_first() -> crate::TestResult {
    let manifest: serde_json::Value = serde_json::from_str(NPM_PACKAGE)?;
    assert_eq!(manifest["name"], "logbrew-cli");
    assert_eq!(
        manifest["bin"],
        serde_json::json!({"logbrew": "bin/logbrew.js"})
    );
    assert!(manifest.get("scripts").is_none());
    let platforms = manifest["optionalDependencies"]
        .as_object()
        .ok_or("optional dependencies missing")?
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(platforms.len(), 5);
    let mut publish_order = platforms.clone();
    publish_order.push("logbrew-cli; do");
    assert!(
        ordered(NPM, &publish_order),
        "platform packages must publish first"
    );
    for platform in platforms {
        let suffix = platform
            .strip_prefix("@logbrew/cli-")
            .ok_or("unexpected platform package scope")?;
        let (os, cpu) = suffix
            .split_once('-')
            .ok_or("unexpected platform package")?;
        assert!(
            NPM_PACKAGES.contains(&format!("os={os} cpu={cpu} ;;")),
            "packer misses {platform}"
        );
        assert!(PREFLIGHT.contains(platform), "preflight misses {platform}");
    }
    assert!(!DIST.contains("\"npm\""));
    Ok(())
}
