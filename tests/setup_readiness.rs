//! Truthful non-mutating setup readiness tests.

use logbrew_cli::{CliEnvironment, execute_command, parse_command};

const SWIFT_PACKAGE_URL: &str = "https://github.com/LogBrewCo/sdk.git";

#[tokio::test]
async fn swiftpm_and_xcodegen_emit_exact_non_mutating_install_plan()
-> Result<(), Box<dyn std::error::Error>> {
    let root = fixture_root("swift-ready")?;
    std::fs::write(
        root.join("Package.swift"),
        "// deterministic public Swift package fixture\n",
    )?;
    std::fs::write(root.join("project.yml"), "name: Fixture\n")?;
    let command = parse_command(["logbrew", "setup", "--json"])?;
    let mut output = Vec::new();

    execute_command(&command, &environment(root.as_path()), &mut output).await?;

    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["install_ready"], true);
    assert_eq!(
        body["install_plan"],
        serde_json::json!({
            "mode": "non_mutating",
            "ecosystem": "swiftpm",
            "package_url": SWIFT_PACKAGE_URL,
            "product": "LogBrew",
            "version": "0.1.4",
            "next_action": {
                "code": "add_swift_package_dependency",
                "target": "project_manifest"
            }
        })
    );
    assert_eq!(
        body["next"],
        "add the LogBrew Swift package from the install plan; no files were changed"
    );
    assert_eq!(
        body["detected"],
        serde_json::json!([
            {
                "runtime": "swift",
                "package_manager": "swift package manager",
                "manifest": "Package.swift"
            },
            {
                "runtime": "swift-ios",
                "package_manager": "xcodegen",
                "manifest": "project.yml"
            }
        ])
    );
    for forbidden in [
        root.to_string_lossy().as_ref(),
        "installed\":true",
        "token",
        "credential",
    ] {
        assert!(!text.contains(forbidden));
    }
    Ok(())
}

#[tokio::test]
async fn setup_aliases_and_json_order_share_the_swift_install_plan()
-> Result<(), Box<dyn std::error::Error>> {
    let root = fixture_root("aliases")?;
    std::fs::write(root.join("Package.swift"), "// swift fixture\n")?;
    let cases = [
        &["logbrew", "setup", "--json"][..],
        &["logbrew", "--json", "setup"][..],
        &["logbrew", "init", "--json"][..],
        &["logbrew", "--json", "install"][..],
        &["logbrew", "configure", "--json"][..],
        &["logbrew", "--json", "sdk"][..],
    ];
    let mut expected = None;

    for args in cases {
        let command = parse_command(args.iter().copied())?;
        let mut output = Vec::new();
        execute_command(&command, &environment(root.as_path()), &mut output).await?;
        let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;

        assert_eq!(body["install_ready"], true);
        assert_eq!(body["install_plan"]["version"], "0.1.4");
        assert_eq!(body["install_plan"]["package_url"], SWIFT_PACKAGE_URL);
        if let Some(expected) = expected.as_ref() {
            assert_eq!(&body, expected);
        } else {
            expected = Some(body);
        }
    }
    Ok(())
}

#[tokio::test]
async fn non_swift_projects_keep_install_readiness_false() -> Result<(), Box<dyn std::error::Error>>
{
    let root = fixture_root("rust-not-ready")?;
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n")?;
    let command = parse_command(["logbrew", "setup", "--json"])?;
    let mut output = Vec::new();

    execute_command(&command, &environment(root.as_path()), &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["install_ready"], false);
    assert_eq!(body["install_plan"], serde_json::Value::Null);
    assert_eq!(
        body["next"],
        "install the matching LogBrew SDK package when packages are ready; send release and environment with logs, issues, actions, and traces"
    );
    Ok(())
}

#[tokio::test]
async fn swift_human_plan_is_truthful_and_path_free() -> Result<(), Box<dyn std::error::Error>> {
    let root = fixture_root("human")?;
    std::fs::write(root.join("project.yml"), "name: Fixture\n")?;
    let command = parse_command(["logbrew", "setup"])?;
    let mut output = Vec::new();

    execute_command(&command, &environment(root.as_path()), &mut output).await?;

    let text = String::from_utf8(output)?;
    assert!(text.contains("Install: ready"));
    assert!(text.contains("Package: https://github.com/LogBrewCo/sdk.git"));
    assert!(text.contains("Product: LogBrew"));
    assert!(text.contains("Version: 0.1.4"));
    assert!(text.contains("No files changed."));
    assert!(!text.contains(root.to_string_lossy().as_ref()));
    assert!(!text.contains("installed"));
    Ok(())
}

fn environment(root: &std::path::Path) -> CliEnvironment {
    CliEnvironment {
        base_url: String::from("https://example.invalid"),
        token: None,
        home: None,
        cwd: Some(root.to_path_buf()),
    }
}

fn fixture_root(label: &str) -> Result<std::path::PathBuf, std::io::Error> {
    let root = std::env::temp_dir().join(format!(
        "logbrew-setup-readiness-{label}-{}",
        std::process::id()
    ));
    match std::fs::remove_dir_all(root.as_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::create_dir_all(root.as_path())?;
    Ok(root)
}
