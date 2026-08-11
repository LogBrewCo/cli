//! Truthful non-mutating setup readiness tests.

use logbrew_cli::{CliEnvironment, execute_command, parse_command};

const SWIFT_PACKAGE_URL: &str = "https://github.com/LogBrewCo/sdk.git";
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn swiftpm_emits_exact_non_mutating_install_plan() -> TestResult {
    let root = fixture_root("swift-ready")?;
    std::fs::write(
        root.join("Package.swift"),
        "// deterministic public Swift package fixture\n",
    )?;
    let (body, _) = setup_json(root.as_path(), &["logbrew", "setup", "--json"]).await?;
    assert_eq!(
        body,
        serde_json::json!({
            "ok": true,
            "auto": false,
            "yes": false,
            "install_ready": true,
            "install_plan": {
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
            },
            "detected": [{
                "runtime": "swift",
                "package_manager": "swift package manager",
                "manifest": "Package.swift"
            }],
            "next": "add the LogBrew Swift package from the install plan; no files were changed"
        })
    );
    let human = setup_text(root.as_path(), &["logbrew", "setup"]).await?;
    for expected in [
        "Product: LogBrew",
        "Minimum version: 0.1.6",
        r#"Dependency: .package(url: "https://github.com/LogBrewCo/sdk.git", from: "0.1.6")"#,
    ] {
        assert!(human.contains(expected));
    }
    assert!(!human.contains(root.to_string_lossy().as_ref()));
    Ok(())
}

#[tokio::test]
async fn cmake_emits_exact_pinned_cpp_install_plan() -> TestResult {
    let root = fixture_root("cmake-ready")?;
    std::fs::write(
        root.join("CMakeLists.txt"),
        "cmake_minimum_required(VERSION 3.16)\nproject(Fixture LANGUAGES CXX)\n",
    )?;

    let (body, text) = setup_json(root.as_path(), &["logbrew", "setup", "--json"]).await?;

    assert_eq!(
        body["install_plan"],
        serde_json::json!({
            "mode": "non_mutating",
            "ecosystem": "cmake",
            "package_url": SWIFT_PACKAGE_URL,
            "release_tag": "cpp/logbrew-cpp/v0.2.3",
            "version": "0.2.3",
            "source_subdirectory": "cpp/logbrew-cpp",
            "targets": {
                "core": "LogBrew::LogBrew",
                "http_transport": "LogBrew::HttpTransport"
            },
            "http_transport": {
                "cmake_option": "LOGBREW_BUILD_HTTP_TRANSPORT",
                "default_enabled": false,
                "requires": "libcurl"
            },
            "dependency_declaration": "include(FetchContent)\nFetchContent_Declare(\n  logbrew\n  GIT_REPOSITORY https://github.com/LogBrewCo/sdk.git\n  GIT_TAG cpp/logbrew-cpp/v0.2.3\n  GIT_SHALLOW TRUE\n  SOURCE_SUBDIR cpp/logbrew-cpp\n)\nFetchContent_MakeAvailable(logbrew)",
            "next_action": {
                "code": "add_cmake_fetch_content",
                "target": "CMakeLists.txt"
            }
        })
    );
    assert_eq!(
        body["detected"],
        serde_json::json!([{
            "runtime": "cpp",
            "package_manager": "cmake",
            "manifest": "CMakeLists.txt"
        }])
    );
    assert_eq!(
        body["next"],
        "add the pinned LogBrew C++ FetchContent block and link the required target; no files were changed"
    );
    assert!(!text.contains(root.to_string_lossy().as_ref()));
    let human = setup_text(root.as_path(), &["logbrew", "setup"]).await?;
    for expected in [
        "Release tag: cpp/logbrew-cpp/v0.2.3",
        "Source subdirectory: cpp/logbrew-cpp",
        "Core target: LogBrew::LogBrew",
        "HTTP target: LogBrew::HttpTransport (optional; requires libcurl)",
        "GIT_SHALLOW TRUE",
        "No files changed.",
    ] {
        assert!(human.contains(expected));
    }
    assert!(!human.contains(root.to_string_lossy().as_ref()));
    Ok(())
}

#[tokio::test]
async fn setup_aliases_and_json_order_share_the_swift_install_plan() -> TestResult {
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
        let (body, _) = setup_json(root.as_path(), args).await?;

        if let Some(expected) = expected.as_ref() {
            assert_eq!(&body, expected);
        } else {
            expected = Some(body);
        }
    }
    Ok(())
}

#[tokio::test]
async fn django_pyproject_emits_exact_public_python_install_plan() -> TestResult {
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
    let (body, _) = setup_json(root.as_path(), &["logbrew", "setup", "--json"]).await?;
    assert_eq!(
        body["install_plan"],
        python_plan(
            "pip",
            "django",
            Some("logbrew-django"),
            Some("Django>=4.2.30,<6"),
            "python3 -m pip install --upgrade logbrew-sdk logbrew-django",
        )
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
async fn released_python_frameworks_use_the_detected_package_manager() -> TestResult {
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
        let (body, _) = setup_json(root.as_path(), &["logbrew", "setup", "--json"]).await?;
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
        assert_eq!(
            body["install_plan"]["compatibility"]["requires_python"],
            ">=3.10"
        );
        assert_eq!(body["install_plan"]["install_command"], install_command);
    }
    Ok(())
}

#[tokio::test]
async fn framework_neutral_pipenv_project_gets_the_core_python_plan() -> TestResult {
    let root = fixture_root("python-core-pipenv")?;
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"fixture\"\ndependencies = [\"requests\"]\n",
    )?;
    std::fs::write(root.join("Pipfile.lock"), "{}")?;
    let (body, _) = setup_json(root.as_path(), &["logbrew", "setup", "--json"]).await?;
    assert_eq!(
        body["install_plan"],
        python_plan("pipenv", "python", None, None, "pipenv install logbrew-sdk",)
    );
    Ok(())
}

#[tokio::test]
async fn django_human_plan_is_explicit_and_path_free() -> TestResult {
    let root = fixture_root("django-human")?;
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"fixture\"\ndependencies = [\"Django==4.2.30\"]\n",
    )?;
    let text = setup_text(root.as_path(), &["logbrew", "setup"]).await?;
    assert!(text.contains("Install: ready\n"));
    assert!(text.contains("Package manager: pip\n"));
    assert!(text.contains("Integration: Django\n"));
    assert!(text.contains("Packages: logbrew-sdk logbrew-django\n"));
    assert!(text.contains("Compatibility review: Python >=3.10; Django>=4.2.30,<6\n"));
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
async fn runtimes_without_structured_plans_get_truthful_recovery() -> TestResult {
    let root = fixture_root("rust-not-ready")?;
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n")?;
    let (body, _) = setup_json(root.as_path(), &["logbrew", "setup", "--json"]).await?;
    assert_eq!(body["install_ready"], false);
    assert_eq!(body["install_plan"], serde_json::Value::Null);
    assert_eq!(
        body["next"],
        "use the released SDK guidance for this runtime; this CLI version does not yet provide a structured install plan"
    );
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

async fn setup_text(root: &std::path::Path, args: &[&str]) -> TestResult<String> {
    let command = parse_command(args.iter().copied())?;
    let mut output = Vec::new();
    execute_command(&command, &environment(root), &mut output).await?;
    Ok(String::from_utf8(output)?)
}

async fn setup_json(
    root: &std::path::Path,
    args: &[&str],
) -> TestResult<(serde_json::Value, String)> {
    let text = setup_text(root, args).await?;
    Ok((serde_json::from_str(text.as_str())?, text))
}

fn python_plan(
    package_manager: &str,
    integration: &str,
    framework_package: Option<&str>,
    framework_requirement: Option<&str>,
    install_command: &str,
) -> serde_json::Value {
    let packages = std::iter::once(("logbrew-sdk", "core"))
        .chain(framework_package.map(|name| (name, "framework")))
        .map(|(name, role)| {
            serde_json::json!({
                "name": name,
                "role": role,
                "version_requirement": { "kind": "latest_compatible" }
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "mode": "non_mutating",
        "ecosystem": "pypi",
        "package_manager": package_manager,
        "integration": integration,
        "packages": packages,
        "compatibility": {
            "status": "review_required",
            "requires_python": ">=3.10",
            "requires_framework": framework_requirement
        },
        "install_command": install_command,
        "next_action": {
            "code": "review_compatibility_and_install",
            "target": "project_environment"
        }
    })
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
