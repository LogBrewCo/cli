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
            "version": "0.1.6",
            "version_requirement": {
                "kind": "up_to_next_major",
                "minimum_version": "0.1.6"
            },
            "dependency_declaration": ".package(url: \"https://github.com/LogBrewCo/sdk.git\", from: \"0.1.6\")",
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
        assert_eq!(body["install_plan"]["version"], "0.1.6");
        assert_eq!(
            body["install_plan"]["version_requirement"]["kind"],
            "up_to_next_major"
        );
        assert_eq!(
            body["install_plan"]["version_requirement"]["minimum_version"],
            "0.1.6"
        );
        assert_eq!(
            body["install_plan"]["dependency_declaration"],
            r#".package(url: "https://github.com/LogBrewCo/sdk.git", from: "0.1.6")"#
        );
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
async fn django_pyproject_emits_exact_public_python_install_plan()
-> Result<(), Box<dyn std::error::Error>> {
    let root = fixture_root("django-ready")?;
    std::fs::write(
        root.join("pyproject.toml"),
        r#"[project]
name = "conference-app"
dynamic = ["dependencies"]
classifiers = ["Framework :: Django", "Programming Language :: Python :: 3.10"]
"#,
    )?;
    std::fs::write(root.join("requirements.txt"), "Django>=4.2,<6\n")?;
    let command = parse_command(["logbrew", "setup", "--json"])?;
    let mut output = Vec::new();

    execute_command(&command, &environment(root.as_path()), &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["install_ready"], true);
    assert_eq!(
        body["install_plan"],
        serde_json::json!({
            "mode": "non_mutating",
            "ecosystem": "pypi",
            "package_manager": "pip",
            "integration": "django",
            "packages": [
                {
                    "name": "logbrew-sdk",
                    "role": "core",
                    "version_requirement": {
                        "kind": "latest_compatible"
                    }
                },
                {
                    "name": "logbrew-django",
                    "role": "framework",
                    "version_requirement": {
                        "kind": "latest_compatible"
                    }
                }
            ],
            "compatibility": {
                "status": "review_required",
                "requires_python": ">=3.11",
                "requires_framework": "Django>=5.2"
            },
            "install_command": "python3 -m pip install --upgrade logbrew-sdk logbrew-django",
            "next_action": {
                "code": "review_compatibility_and_install",
                "target": "project_environment"
            }
        })
    );
    assert_eq!(
        body["next"],
        "review the compatibility requirements, then run the install command; no files were changed"
    );
    assert_eq!(
        body["detected"],
        serde_json::json!([
            {
                "runtime": "python",
                "package_manager": "pip",
                "manifest": "pyproject.toml"
            }
        ])
    );
    Ok(())
}

#[tokio::test]
async fn released_python_frameworks_use_the_detected_package_manager()
-> Result<(), Box<dyn std::error::Error>> {
    for (
        label,
        dependency,
        lockfile,
        package_manager,
        integration,
        framework_package,
        framework_requirement,
        install_command,
    ) in [
        (
            "flask-uv",
            "Flask>=3.1",
            "uv.lock",
            "uv",
            "flask",
            "logbrew-flask",
            "Flask>=3.1",
            "uv add logbrew-sdk logbrew-flask",
        ),
        (
            "fastapi-poetry",
            "FastAPI>=0.111.1",
            "poetry.lock",
            "poetry",
            "fastapi",
            "logbrew-fastapi",
            "FastAPI>=0.111.1",
            "poetry add logbrew-sdk logbrew-fastapi",
        ),
    ] {
        let root = fixture_root(label)?;
        std::fs::write(
            root.join("pyproject.toml"),
            format!("[project]\nname = \"fixture\"\ndependencies = [\"{dependency}\"]\n"),
        )?;
        std::fs::write(root.join(lockfile), "")?;
        let command = parse_command(["logbrew", "setup", "--json"])?;
        let mut output = Vec::new();

        execute_command(&command, &environment(root.as_path()), &mut output).await?;

        let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
        assert_eq!(body["install_ready"], true);
        assert_eq!(body["install_plan"]["package_manager"], package_manager);
        assert_eq!(body["install_plan"]["integration"], integration);
        assert_eq!(body["install_plan"]["packages"][0]["name"], "logbrew-sdk");
        assert_eq!(
            body["install_plan"]["packages"][1]["name"],
            framework_package
        );
        assert_eq!(
            body["install_plan"]["compatibility"]["requires_framework"],
            framework_requirement
        );
        assert_eq!(body["install_plan"]["install_command"], install_command);
    }
    Ok(())
}

#[tokio::test]
async fn framework_neutral_pipenv_project_gets_the_core_python_plan()
-> Result<(), Box<dyn std::error::Error>> {
    let root = fixture_root("python-core-pipenv")?;
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"fixture\"\ndependencies = [\"requests\"]\n",
    )?;
    std::fs::write(root.join("Pipfile.lock"), "{}")?;
    let command = parse_command(["logbrew", "setup", "--json"])?;
    let mut output = Vec::new();

    execute_command(&command, &environment(root.as_path()), &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["install_ready"], true);
    assert_eq!(body["install_plan"]["package_manager"], "pipenv");
    assert_eq!(body["install_plan"]["integration"], "python");
    assert_eq!(
        body["install_plan"]["packages"],
        serde_json::json!([
            {
                "name": "logbrew-sdk",
                "role": "core",
                "version_requirement": {
                    "kind": "latest_compatible"
                }
            }
        ])
    );
    assert_eq!(
        body["install_plan"]["compatibility"]["requires_framework"],
        serde_json::Value::Null
    );
    assert_eq!(
        body["install_plan"]["install_command"],
        "pipenv install logbrew-sdk"
    );
    Ok(())
}

#[tokio::test]
async fn django_human_plan_is_explicit_and_path_free() -> Result<(), Box<dyn std::error::Error>> {
    let root = fixture_root("django-human")?;
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"fixture\"\ndependencies = [\"Django>=5.2\"]\n",
    )?;
    let command = parse_command(["logbrew", "setup"])?;
    let mut output = Vec::new();

    execute_command(&command, &environment(root.as_path()), &mut output).await?;

    let text = String::from_utf8(output)?;
    assert!(text.contains("Install: ready\n"));
    assert!(text.contains("Package manager: pip\n"));
    assert!(text.contains("Integration: Django\n"));
    assert!(text.contains("Packages: logbrew-sdk logbrew-django\n"));
    assert!(text.contains("Compatibility review: Python >=3.11; Django>=5.2\n"));
    assert!(
        text.contains("Command: python3 -m pip install --upgrade logbrew-sdk logbrew-django\n")
    );
    assert!(text.contains(
        "Next: review the compatibility requirements, then run the install command; no files were \
         changed\n"
    ));
    assert!(!text.contains(root.to_string_lossy().as_ref()));
    Ok(())
}

#[tokio::test]
async fn runtimes_without_structured_plans_get_truthful_recovery()
-> Result<(), Box<dyn std::error::Error>> {
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
        "use the released SDK guidance for this runtime; this CLI version does not yet provide a structured install plan"
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
    assert!(text.contains("Minimum version: 0.1.6"));
    assert!(text.contains(
        r#"Dependency: .package(url: "https://github.com/LogBrewCo/sdk.git", from: "0.1.6")"#
    ));
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
