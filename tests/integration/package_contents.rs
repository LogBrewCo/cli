//! Verify the actual Cargo package inventory without publishing it.

#[test]
fn package_contains_runtime_and_tests_without_repository_operations() -> crate::TestResult {
    let output = super::run_bounded(
        std::process::Command::new("cargo")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                "package",
                "--list",
                "--allow-dirty",
                "--locked",
                "--offline",
            ]),
    )?;
    assert!(output.status.success(), "Cargo package inventory failed");
    let files = std::str::from_utf8(&output.stdout)?
        .lines()
        .collect::<Vec<_>>();
    for (pattern, expected) in [
        ("Cargo.lock", true),
        ("Cargo.toml", true),
        ("LICENSE", true),
        ("README.md", true),
        ("src/main.rs", true),
        ("src/lib.rs", true),
        ("tests/grammar/commands.rs", true),
        (".github/", false),
        ("scripts/", false),
        ("wix/", false),
        ("AGENTS.md", false),
        ("dist-workspace.toml", false),
    ] {
        let present = files.iter().any(|file| {
            if pattern.ends_with('/') {
                file.starts_with(pattern)
            } else {
                *file == pattern
            }
        });
        assert_eq!(
            present, expected,
            "incorrect package inclusion for {pattern}"
        );
    }
    Ok(())
}
