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
    let body = setup_json(root.as_path(), &["logbrew", "setup", "--json"]).await?;
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
    assert_contains_all(
        &human,
        &[
            "Product: LogBrew",
            "Minimum version: 0.1.6",
            r#"Dependency: .package(url: "https://github.com/LogBrewCo/sdk.git", from: "0.1.6")"#,
        ],
    );
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

    let body = setup_json(root.as_path(), &["logbrew", "setup", "--json"]).await?;

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
    assert!(!body.to_string().contains(root.to_string_lossy().as_ref()));
    let human = setup_text(root.as_path(), &["logbrew", "setup"]).await?;
    assert_contains_all(
        &human,
        &[
            "Release tag: cpp/logbrew-cpp/v0.2.3",
            "Source subdirectory: cpp/logbrew-cpp",
            "Core target: LogBrew::LogBrew",
            "HTTP target: LogBrew::HttpTransport (optional; requires libcurl)",
            "GIT_SHALLOW TRUE",
            "No files changed.",
        ],
    );
    assert!(!human.contains(root.to_string_lossy().as_ref()));
    Ok(())
}

#[tokio::test]
async fn framework_package_managers_emit_exact_non_mutating_plans() -> TestResult {
    for (integration, compatibility, command) in [
        (
            "symfony",
            serde_json::json!({
                "status": "review_required",
                "requires_php": "^8.2",
                "requires_framework": "Symfony^6.4 || ^7.0 || ^8.0",
            }),
            "composer require logbrew/sdk",
        ),
        (
            "rails",
            serde_json::json!({
                "status": "review_required",
                "requires_ruby": ">=2.6",
                "requires_framework": null,
            }),
            "bundle add logbrew-sdk --version \"~> 0.1.5\"",
        ),
    ] {
        let rails = integration == "rails";
        let pick = |php, ruby| if rails { ruby } else { php };
        let manifest = pick("composer.json", "Gemfile");
        let marker = pick("config/bundles.php", "config/application.rb");
        let ecosystem = pick("composer", "rubygems");
        let manager = pick("composer", "bundler");
        let package = pick("logbrew/sdk", "logbrew-sdk");
        let integration_line = pick("Integration: Symfony", "Integration: Rails");
        let review_line = pick(
            "Compatibility review: PHP ^8.2; Symfony^6.4 || ^7.0 || ^8.0",
            "Compatibility review: Ruby >=2.6",
        );
        let root = fixture_root(integration)?;
        std::fs::create_dir_all(root.join("config"))?;
        std::fs::write(root.join(marker), "framework marker\n")?;
        std::fs::write(root.join(manifest), "framework dependency\n")?;
        let body = setup_json(&root, &["logbrew", "setup", "--json"]).await?;
        assert_eq!(
            body["install_plan"],
            serde_json::json!({
                "mode": "non_mutating",
                "ecosystem": ecosystem,
                "package_manager": manager,
                "integration": integration,
                "package": package,
                "framework_manifest": marker,
                "compatibility": compatibility,
                "install_command": command,
                "next_action": {
                    "code": "review_compatibility_and_install",
                    "target": "project_environment",
                },
            })
        );
        let human = setup_text(&root, &["logbrew", "setup"]).await?;
        assert_contains_all(
            &human,
            &[
                integration_line,
                review_line,
                &format!("Package: {package}"),
                &format!("Command: {command}"),
            ],
        );
        assert!(!human.contains(root.to_string_lossy().as_ref()));
    }
    Ok(())
}

#[tokio::test]
async fn xcodegen_prefers_the_objective_c_app_plan() -> TestResult {
    let root = fixture_root("xcodegen-objective-c")?;
    std::fs::create_dir_all(root.join("App/Sources"))?;
    std::fs::create_dir_all(root.join("Packages/Helper"))?;
    std::fs::write(root.join("App/project.yml"), "name: Checkout\n")?;
    std::fs::write(root.join("App/Sources/main.m"), "")?;
    std::fs::write(root.join("Packages/Helper/Package.swift"), "")?;
    let body = setup_json(&root, &["logbrew", "setup", "--json"]).await?;
    assert_eq!(
        body["install_plan"],
        serde_json::json!({
            "mode": "non_mutating",
            "ecosystem": "source",
            "language": "objective-c",
            "package_url": SWIFT_PACKAGE_URL,
            "release_tag": "objc/logbrew-objc/v0.2.3",
            "version": "0.2.3",
            "source_subdirectory": "objc/logbrew-objc",
            "header": "include/LogBrew.h",
            "source_directory": "src",
            "frameworks": ["Foundation"],
            "next_action": { "code": "vendor_objective_c_sources", "target": "application_target" }
        })
    );
    assert_eq!(
        body["detected"],
        serde_json::json!([
            { "runtime": "objective-c", "package_manager": "xcodegen", "manifest": "App/project.yml" },
            { "runtime": "swift", "package_manager": "swift package manager", "manifest": "Packages/Helper/Package.swift" }
        ])
    );
    let human = setup_text(&root, &["logbrew", "setup"]).await?;
    assert_contains_all(
        &human,
        &[
            "Release tag: objc/logbrew-objc/v0.2.3",
            "Source subdirectory: objc/logbrew-objc",
            "Header: include/LogBrew.h",
            "Source directory: src",
            "Framework: Foundation",
            "No files changed.",
        ],
    );
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
    let expected = setup_json(root.as_path(), cases[0]).await?;
    for args in &cases[1..] {
        assert_eq!(setup_json(root.as_path(), args).await?, expected);
    }
    let preferences = setup_json(
        root.as_path(),
        &["logbrew", "setup", "--auto", "--yes", "--json"],
    )
    .await?;
    assert_eq!(
        (preferences["auto"].as_bool(), preferences["yes"].as_bool()),
        (Some(true), Some(true))
    );
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
    let body = setup_json(root.as_path(), &["logbrew", "setup", "--json"]).await?;
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
        let body = setup_json(root.as_path(), &["logbrew", "setup", "--json"]).await?;
        assert_eq!(
            body["install_plan"],
            python_plan(
                package_manager,
                integration,
                Some(framework_package),
                Some(framework_requirement),
                install_command,
            )
        );
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
    let body = setup_json(root.as_path(), &["logbrew", "setup", "--json"]).await?;
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
    assert_contains_all(
        &text,
        &[
            "Install: ready\n",
            "Package manager: pip\n",
            "Integration: Django\n",
            "Packages: logbrew-sdk logbrew-django\n",
            "Compatibility review: Python >=3.10; Django>=4.2.30,<6\n",
            "Command: python3 -m pip install --upgrade logbrew-sdk logbrew-django\n",
            "Next: review the compatibility requirements, then run the install command; no files were changed\n",
        ],
    );
    assert!(!text.contains(root.to_string_lossy().as_ref()));
    Ok(())
}

#[tokio::test]
async fn runtimes_without_structured_plans_get_truthful_recovery() -> TestResult {
    let root = fixture_root("rust-not-ready")?;
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n")?;
    let body = setup_json(root.as_path(), &["logbrew", "setup", "--json"]).await?;
    assert_eq!(body["install_ready"], false);
    assert_eq!(body["install_plan"], serde_json::Value::Null);
    assert_eq!(
        body["next"],
        "use the released SDK guidance for this runtime; this CLI version does not yet provide a structured install plan"
    );
    let human = setup_text(root.as_path(), &["logbrew", "setup", "--auto", "--yes"]).await?;
    assert_eq!(
        human,
        "LogBrew setup plan\nMode: non-mutating plan\nPreferences: auto=true, yes=true\nNo files \
         changed.\nInstall: not ready\nDetected runtimes:\n- Rust (cargo) at Cargo.toml\nNext: \
         use the released SDK guidance for this runtime; this CLI version does not yet provide a \
         structured install plan\n"
    );

    let empty = fixture_root("empty")?;
    let human = setup_text(empty.as_path(), &["logbrew", "setup"]).await?;
    assert_eq!(
        human,
        "LogBrew setup plan\nMode: non-mutating plan\nNo files changed.\nInstall: not ready\nNo \
         supported project manifest found.\nNext: run logbrew setup from a project containing \
         package.json, pyproject.toml, Pipfile, Cargo.toml, Package.swift, project.yml, project.yaml, \
         .xcodeproj, .xcworkspace, CMakeLists.txt, go.mod, composer.json, or Gemfile.\n"
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

async fn setup_json(root: &std::path::Path, args: &[&str]) -> TestResult<serde_json::Value> {
    let text = setup_text(root, args).await?;
    Ok(serde_json::from_str(text.as_str())?)
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

fn assert_contains_all(text: &str, expected: &[&str]) {
    for value in expected {
        assert!(text.contains(value), "missing {value}");
    }
}

fn fixture_root(label: &str) -> Result<std::path::PathBuf, std::io::Error> {
    let root = std::env::temp_dir().join(format!(
        "logbrew-setup-readiness-{label}-{}",
        std::process::id()
    ));
    if root.try_exists()? {
        std::fs::remove_dir_all(&root)?;
    }
    std::fs::create_dir_all(&root)?;
    Ok(root)
}
