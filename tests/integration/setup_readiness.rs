//! Truthful non-mutating setup readiness tests.

use logbrew_cli::{CliEnvironment, execute_command, parse_command};

const SWIFT_PACKAGE_URL: &str = "https://github.com/LogBrewCo/sdk.git";
const REACT_EXPRESS_PACKAGES: &str =
    "@logbrew/sdk @logbrew/browser @logbrew/react @logbrew/node @logbrew/express";
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn javascript_surfaces(browser: &str, server: &str) -> serde_json::Value {
    serde_json::json!([
        {"surface": "browser", "integration": browser, "credential_kind": "browser", "service_name_required": true, "deployment_context_required": ["environment", "release"]},
        {"surface": "server", "integration": server, "credential_kind": "server", "service_name_required": true, "deployment_context_required": ["environment", "release"]},
    ])
}

#[tokio::test]
async fn swiftpm_emits_exact_non_mutating_install_plan() -> TestResult {
    let root = fixture("swift-ready", &[("Package.swift", "// public fixture\n")])?;
    let body = setup_json(root.as_path()).await?;
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
    Ok(())
}

#[tokio::test]
async fn cmake_emits_exact_pinned_cpp_install_plan() -> TestResult {
    let root = fixture_file(
        "cmake-ready",
        "CMakeLists.txt",
        "project(Fixture LANGUAGES CXX)\n",
    )?;

    let body = setup_json(root.as_path()).await?;

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
    Ok(())
}

#[tokio::test]
async fn xcodegen_prefers_the_objective_c_app_plan() -> TestResult {
    let root = fixture(
        "xcodegen-objective-c",
        &[
            ("App/project.yml", "name: Checkout\n"),
            ("App/Sources/main.m", ""),
            ("Packages/Helper/Package.swift", ""),
        ],
    )?;
    let body = setup_json(&root).await?;
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
async fn released_python_frameworks_use_the_detected_package_manager() -> TestResult {
    for (
        dependency,
        lockfile,
        package_manager,
        integration,
        framework_package,
        framework_requirement,
        install_command,
    ) in [
        (
            "Django>=4.2",
            "",
            "pip",
            "django",
            "logbrew-django",
            "Django>=4.2.30,<6",
            "python3 -m pip install --upgrade logbrew-sdk logbrew-django",
        ),
        (
            "Flask>=3.1",
            "uv.lock",
            "uv",
            "flask",
            "logbrew-flask",
            "Flask>=3.1",
            "uv add logbrew-sdk logbrew-flask",
        ),
        (
            "FastAPI>=0.111.1\", \"celery>=5.3.6",
            "poetry.lock",
            "poetry",
            "fastapi",
            "logbrew-fastapi",
            "FastAPI>=0.111.1",
            "poetry add \"logbrew-sdk[celery]\" logbrew-fastapi",
        ),
        (
            "requests",
            "Pipfile.lock",
            "pipenv",
            "python",
            "",
            "",
            "pipenv install logbrew-sdk",
        ),
    ] {
        let manifest =
            format!("[project]\nname = \"fixture\"\ndependencies = [\"{dependency}\"]\n");
        let root = fixture(integration, &[("pyproject.toml", &manifest)])?;
        if !lockfile.is_empty() {
            std::fs::write(root.join(lockfile), "")?;
        }
        let body = setup_json(&root).await?;
        let mut core = serde_json::json!({
            "name": "logbrew-sdk", "role": "core",
            "version_requirement": {"kind": "latest_compatible"},
        });
        if install_command.contains("[celery]") {
            core["extras"] = serde_json::json!(["celery"]);
        }
        let mut packages = vec![core];
        if !framework_package.is_empty() {
            packages.push(serde_json::json!({
                "name": framework_package, "role": "framework",
                "version_requirement": {"kind": "latest_compatible"},
            }));
        }
        assert_eq!(
            body["install_plan"],
            serde_json::json!({
                "mode": "non_mutating", "ecosystem": "pypi",
                "package_manager": package_manager, "integration": integration,
                "packages": packages,
                "compatibility": {
                    "status": "review_required", "requires_python": ">=3.10",
                    "requires_framework": (!framework_requirement.is_empty()).then_some(framework_requirement),
                },
                "install_command": install_command,
                "next_action": {"code": "review_compatibility_and_install", "target": "project_environment"},
            })
        );
        assert_eq!(body["detected"][0]["runtime"], "python");
    }
    Ok(())
}

#[tokio::test]
async fn java_builds_emit_exact_released_dependency_plans() -> TestResult {
    for (manifest, manager, contents, integration, declaration) in [
        (
            "pom.xml",
            "maven",
            "<project><dependency><groupId>org.springframework.kafka</groupId></dependency></project>",
            "spring",
            "<dependency>\n  <groupId>co.logbrew</groupId>\n  <artifactId>logbrew-sdk</artifactId>\n  <version>0.1.6</version>\n</dependency>",
        ),
        (
            "build.gradle",
            "gradle",
            "plugins { id 'java' }",
            "java",
            "implementation 'co.logbrew:logbrew-sdk:0.1.6'",
        ),
        (
            "build.gradle.kts",
            "gradle-kotlin",
            "plugins { java }",
            "java",
            "implementation(\"co.logbrew:logbrew-sdk:0.1.6\")",
        ),
    ] {
        let root = fixture(manager, &[(manifest, contents)])?;
        let body = setup_json(&root).await?;
        assert_eq!(
            body["install_plan"],
            serde_json::json!({
                "mode": "non_mutating",
                "ecosystem": manager,
                "package_manager": manager,
                "integration": integration,
                "package": {"group_id": "co.logbrew", "artifact_id": "logbrew-sdk", "version": "0.1.6"},
                "compatibility": {
                    "status": "review_required",
                    "requires_java": ">=11",
                    "requires_framework": (integration == "spring").then_some("Spring Boot 3+"),
                },
                "dependency_declaration": declaration,
                "next_action": {"code": "review_compatibility_and_install", "target": "project_environment"},
            })
        );
        assert_eq!(
            body["detected"],
            serde_json::json!([{
                "runtime": "java", "package_manager": manager, "manifest": manifest
            }])
        );
        if manager == "maven" {
            assert_contains_all(
                &setup_text(&root, &["logbrew", "setup"]).await?,
                &[
                    "Integration: Spring",
                    "co.logbrew:logbrew-sdk:0.1.6",
                    declaration,
                ],
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn javascript_executables_and_frameworks_emit_scoped_plans() -> TestResult {
    let server = serde_json::json!([{
        "surface": "server", "integration": "node", "credential_kind": "server",
        "service_name_required": true, "deployment_context_required": ["environment", "release"],
    }]);
    for (
        label,
        files,
        manager,
        runtime,
        integration,
        packages,
        compatibility,
        surfaces,
        command,
        human,
    ) in [
        (
            "node-cli",
            &[("package.json", r#"{"bin":{"fixture":"dist/cli.js"}}"#)][..],
            "npm",
            "node",
            "node",
            serde_json::json!(["@logbrew/sdk", "@logbrew/node"]),
            serde_json::json!({"status": "review_required", "requires_node": ">=18"}),
            server,
            "npm install @logbrew/sdk @logbrew/node",
            &["Integration: Node.js", "Surface: server; key kind: server"][..],
        ),
        (
            "sveltekit-pnpm",
            &[
                ("pnpm-lock.yaml", "lockfileVersion: '9.0'\n"),
                (
                    "package.json",
                    r#"{"bin":"cli.js","devDependencies":{"@sveltejs/kit":"2.5.27"}}"#,
                ),
            ][..],
            "pnpm",
            "sveltekit",
            "sveltekit",
            serde_json::json!([
                {"name": "@logbrew/sdk", "role": "core", "version_requirement": {"kind": "latest_compatible"}},
                {"name": "@logbrew/browser", "role": "delivery", "version_requirement": {"kind": "latest_compatible"}},
                {"name": "@logbrew/svelte", "role": "framework", "version_requirement": {"kind": "latest_compatible"}},
            ]),
            serde_json::json!({"status": "review_required", "requires_node": ">=18", "requires_framework": "Svelte >=5"}),
            javascript_surfaces("svelte", "sveltekit"),
            "pnpm add @logbrew/sdk @logbrew/browser @logbrew/svelte",
            &[
                "Integration: SvelteKit",
                "browser: Svelte; key kind: browser",
                "server: SvelteKit; key kind: server",
            ][..],
        ),
        (
            "react-express",
            &[(
                "package.json",
                r#"{"bin":"cli.js","dependencies":{"react":"18.3.1","express":"4.21.2"}}"#,
            )][..],
            "npm",
            "react-express",
            "react_express",
            serde_json::json!(
                REACT_EXPRESS_PACKAGES
                    .split_whitespace()
                    .collect::<Vec<_>>()
            ),
            serde_json::json!({"status": "review_required", "requires_node": ">=18", "requires_frameworks": ["React >=18", "Express >=4"]}),
            javascript_surfaces("react", "express"),
            "npm install @logbrew/sdk @logbrew/browser @logbrew/react @logbrew/node @logbrew/express",
            &[
                "Integration: React + Express",
                "browser: React; key kind: browser",
                "server: Express; key kind: server",
            ][..],
        ),
    ] {
        let root = fixture(label, files)?;
        let body = setup_json(&root).await?;
        assert_eq!(body["detected"][0]["runtime"], runtime);
        assert_eq!(
            body["install_plan"],
            serde_json::json!({
                "mode": "non_mutating", "ecosystem": "npm", "package_manager": manager,
                "integration": integration, "packages": packages, "compatibility": compatibility,
                "surfaces": surfaces, "install_command": command,
                "next_action": {"code": "review_compatibility_and_install", "target": "project_environment"},
            })
        );
        assert_contains_all(&setup_text(&root, &["logbrew", "setup"]).await?, human);
    }
    Ok(())
}

#[tokio::test]
async fn aspnetcore_wins_a_mixed_repo_without_claiming_plain_dotnet() -> TestResult {
    let root = fixture(
        "aspnetcore-react",
        &[
            (
                "package.json",
                r#"{"dependencies":{"react":"18","express":"4"}}"#,
            ),
            (
                "src/Web/Web.csproj",
                r#"<Project Sdk="Microsoft.NET.Sdk.Web" />"#,
            ),
        ],
    )?;
    let body = setup_json(&root).await?;
    assert_eq!(
        body["install_plan"],
        serde_json::json!({
            "mode": "non_mutating",
            "ecosystem": "nuget",
            "package_manager": "dotnet",
            "integration": "aspnetcore",
            "package": {"id": "LogBrew.AspNetCore", "version": "0.1.2"},
            "compatibility": {
                "status": "review_required",
                "requires_dotnet": ">=10",
                "requires_framework": "ASP.NET Core 10+",
            },
            "surfaces": [{
                "surface": "server",
                "integration": "aspnetcore",
                "credential_kind": "server",
                "service_name_required": true,
                "deployment_context_required": ["environment", "release"],
            }],
            "install_command": "dotnet add package LogBrew.AspNetCore --version 0.1.2",
            "next_action": {"code": "review_compatibility_and_install", "target": "project_environment"},
        })
    );
    assert_eq!(
        body["detected"],
        serde_json::json!([
            {
                "runtime": "react-express",
                "package_manager": "npm",
                "manifest": "package.json",
            },
            {
                "runtime": "aspnetcore",
                "package_manager": "dotnet",
                "manifest": "src/Web/Web.csproj",
            },
        ])
    );
    assert_contains_all(
        &setup_text(&root, &["logbrew", "setup"]).await?,
        &[
            "Integration: ASP.NET Core",
            "Package: LogBrew.AspNetCore:0.1.2",
            "Surface: server; key kind: server",
        ],
    );

    let plain = fixture_file(
        "plain-dotnet",
        "Library.csproj",
        r#"<Project Sdk="Microsoft.NET.Sdk" />"#,
    )?;
    let body = setup_json(&plain).await?;
    assert_eq!(body["detected"][0]["runtime"], "dotnet");
    assert_eq!(body["install_plan"], serde_json::Value::Null);
    Ok(())
}

#[tokio::test]
async fn scanner_prefers_nearby_manifests_and_package_managers() -> TestResult {
    let root = fixture(
        "scanner-nearest",
        &[
            ("Cargo.toml", ""),
            ("package.json", "{}"),
            ("crates/nested/Cargo.toml", ""),
        ],
    )?;
    assert_eq!(
        setup_json(&root).await?["detected"],
        serde_json::json!([
            {"runtime": "node", "package_manager": "npm", "manifest": "package.json"},
            {"runtime": "rust", "package_manager": "cargo", "manifest": "Cargo.toml"},
        ])
    );

    let cmake = fixture(
        "scanner-build-skip",
        &[
            ("CMakeLists.txt", "project(Fixture LANGUAGES CXX)\n"),
            ("build/nested/CMakeLists.txt", "project(Generated)\n"),
        ],
    )?;
    let body = setup_json(&cmake).await?;
    assert_eq!(body["detected"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["detected"][0]["manifest"], "CMakeLists.txt");

    let parent = fixture("scanner-parent", &[("package.json", "{}")])?;
    std::fs::create_dir_all(parent.join("src"))?;
    let body = setup_json(&parent.join("src")).await?;
    assert_eq!(body["detected"][0]["manifest"], "../package.json");

    for (lockfile, manager) in [("yarn.lock", "yarn"), ("bun.lockb", "bun")] {
        let root = fixture(manager, &[("package.json", "{}"), (lockfile, "")])?;
        let body = setup_json(&root).await?;
        assert_eq!(body["detected"][0]["package_manager"], manager);
    }
    Ok(())
}

#[tokio::test]
async fn framework_detection_is_bounded_and_exact() -> TestResult {
    let django = fixture(
        "django-requirements",
        &[
            (
                "pyproject.toml",
                "[project]\ndynamic = [\"dependencies\"]\n",
            ),
            ("requirements.txt", "Django>=4.2,<6\n"),
        ],
    )?;
    let body = setup_json(&django).await?;
    assert_eq!(body["install_plan"]["integration"], "django");

    let large_manifest = "Django".repeat(50_000);
    let oversized = fixture("oversized-metadata", &[("pyproject.toml", &large_manifest)])?;
    let body = setup_json(&oversized).await?;
    assert_eq!(body["install_plan"]["integration"], "python");

    for (label, manifest, marker, integration) in [
        ("symfony", "composer.json", "config/bundles.php", "symfony"),
        ("rails", "Gemfile", "config/application.rb", "rails"),
    ] {
        let root = fixture(label, &[(manifest, ""), (marker, "framework marker\n")])?;
        let body = setup_json(&root).await?;
        assert_eq!(body["install_plan"]["integration"], integration);
        assert_eq!(body["install_plan"]["framework_manifest"], marker);
    }

    for manifest in [
        r#"{"dependencies":{"react":"18","next":"1"}}"#,
        r#"{"dependencies":{"react":"18","react-native":"1"}}"#,
        r#"{"dependencies":{"react":"18"}}"#,
        r#"{"dependencies":{"express":"4"}}"#,
        r#"{"bin":""}"#,
        r#"{"bin":{}}"#,
        r#"{"bin":{"fixture":""}}"#,
        r#"{"bin":[]}"#,
    ] {
        let root = fixture("unsupported-js", &[("package.json", manifest)])?;
        let body = setup_json(&root).await?;
        assert_eq!(body["detected"][0]["runtime"], "node");
        assert_eq!(body["install_plan"], serde_json::Value::Null);
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn scanner_does_not_follow_manifest_or_metadata_symlinks() -> TestResult {
    let root = fixture_file(
        "metadata-symlink",
        "pyproject.toml",
        "[project]\nname = \"fixture\"\n",
    )?;
    let outside = root.with_extension("outside-requirements");
    std::fs::write(&outside, "Django>=5.2\n")?;
    std::os::unix::fs::symlink(&outside, root.join("requirements.txt"))?;
    let body = setup_json(&root).await?;
    assert_eq!(body["install_plan"]["integration"], "python");

    let linked = fixture_root("manifest-symlink")?;
    let outside_manifest = linked.with_extension("outside-pyproject");
    std::fs::write(
        &outside_manifest,
        "[project]\ndependencies = [\"Django\"]\n",
    )?;
    std::os::unix::fs::symlink(&outside_manifest, linked.join("pyproject.toml"))?;
    let body = setup_json(&linked).await?;
    assert!(body["detected"].as_array().is_some_and(Vec::is_empty));
    std::fs::write(linked.join("Gemfile"), "")?;
    std::fs::create_dir_all(linked.join("config"))?;
    std::os::unix::fs::symlink(&outside_manifest, linked.join("config/application.rb"))?;
    let body = setup_json(&linked).await?;
    assert_eq!(body["install_plan"]["integration"], "ruby");
    std::fs::remove_file(outside)?;
    std::fs::remove_file(outside_manifest)?;
    Ok(())
}

#[tokio::test]
async fn scanner_classifies_and_prioritizes_apple_projects() -> TestResult {
    for (label, sources, runtime) in [
        ("swift", &["Sources/App.swift"][..], "swift-ios"),
        (
            "mixed",
            &["Sources/App.m", "Sources/Bridge.swift"][..],
            "swift-ios",
        ),
    ] {
        let root = fixture_root(label)?;
        std::fs::write(root.join("project.yml"), "name: Checkout\n")?;
        for source in sources {
            let path = root.join(source);
            std::fs::create_dir_all(path.parent().expect("source has parent"))?;
            std::fs::write(path, "// source evidence\n")?;
        }
        let body = setup_json(&root).await?;
        assert_eq!(body["detected"][0]["runtime"], runtime);
    }
    for (manifest, manager) in [
        ("Checkout.xcodeproj", "xcode"),
        ("Checkout.xcworkspace", "xcode workspace"),
    ] {
        let root = fixture_root(manifest)?;
        std::fs::create_dir_all(root.join(manifest))?;
        let body = setup_json(&root).await?;
        assert_eq!(body["detected"][0]["package_manager"], manager);
    }
    let root = fixture_root("xcodegen-preference")?;
    std::fs::write(root.join("project.yaml"), "name: Checkout\n")?;
    std::fs::create_dir_all(root.join("Checkout.xcodeproj"))?;
    std::fs::create_dir_all(root.join("Checkout.xcworkspace"))?;
    let body = setup_json(&root).await?;
    assert_eq!(body["detected"][0]["package_manager"], "xcodegen");
    Ok(())
}

#[tokio::test]
async fn runtimes_without_structured_plans_get_truthful_recovery() -> TestResult {
    let root = fixture_file(
        "rust-not-ready",
        "Cargo.toml",
        "[package]\nname = \"fixture\"\n",
    )?;
    let body = setup_json(root.as_path()).await?;
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
         package.json, pyproject.toml, Pipfile, pom.xml, build.gradle, build.gradle.kts, Cargo.toml, \
         Package.swift, project.yml, project.yaml, .xcodeproj, .xcworkspace, CMakeLists.txt, *.csproj, \
         go.mod, composer.json, or Gemfile.\n"
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
    let text = String::from_utf8(output)?;
    assert!(!text.contains(root.to_string_lossy().as_ref()));
    Ok(text)
}

async fn setup_json(root: &std::path::Path) -> TestResult<serde_json::Value> {
    let text = setup_text(root, &["logbrew", "setup", "--json"]).await?;
    Ok(serde_json::from_str(text.as_str())?)
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

fn fixture(label: &str, files: &[(&str, &str)]) -> Result<std::path::PathBuf, std::io::Error> {
    let root = fixture_root(label)?;
    for (name, contents) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().expect("fixture file has parent"))?;
        std::fs::write(path, contents)?;
    }
    Ok(root)
}

fn fixture_file(
    label: &str,
    name: &str,
    contents: &str,
) -> Result<std::path::PathBuf, std::io::Error> {
    fixture(label, &[(name, contents)])
}
