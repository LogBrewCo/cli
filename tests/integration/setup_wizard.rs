//! One-command setup: detection, sign-in reuse, project and key, install, init code,
//! agent config, and a verified test event against a mocked API.

#![cfg(unix)]

use crate::matchers::body_json;
use crate::{
    Mock, MockServer, Request, ResponseTemplate, TestResult, cli_command, run_cli_command,
};
use logbrew_cli::setup::{ASPNETCORE_VERSION, JAVA_SDK_VERSION, RUBY_SDK_VERSION, WizardOptions};
use logbrew_cli::{CliEnvironment, RuntimeError};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const OTHER_PROJECT_ID: &str = "423e4567-e89b-12d3-a456-426614174000";
const INGEST_ID: &str = "223e4567-e89b-12d3-a456-426614174000";
const ACCOUNT: &str = "account-token";
const NEW_KEY: &str = "lbw_ingest_setup_fixture_value";
const PACKAGE_MANAGERS: [&str; 12] = [
    "npm", "pnpm", "yarn", "bun", "python3", "uv", "poetry", "pipenv", "go", "composer", "bundle",
    "dotnet",
];

/// One framework fixture and what setup must do for it.
struct Case {
    label: &'static str,
    files: &'static [(&'static str, &'static str)],
    integration: &'static str,
    env_file: &'static str,
    /// Expected package-manager call; version names are replaced with the pinned releases.
    command: &'static str,
    install: &'static str,
    init: &'static str,
    edited: Option<(&'static str, &'static str)>,
}

const CASES: [Case; 18] = [
    Case {
        label: "node",
        files: &[
            ("package.json", r#"{"name":"tool","bin":"cli.js"}"#),
            (".gitignore", "node_modules\n"),
        ],
        integration: "node",
        env_file: ".env",
        command: "npm install @logbrew/sdk @logbrew/node",
        install: "done",
        init: "manual",
        edited: Some((".gitignore", "node_modules\n.env\n")),
    },
    Case {
        label: "sveltekit",
        files: &[
            (
                "package.json",
                r#"{"devDependencies":{"@sveltejs/kit":"2.0.0"}}"#,
            ),
            ("pnpm-lock.yaml", ""),
        ],
        integration: "sveltekit",
        env_file: ".env",
        command: "pnpm add @logbrew/sdk @logbrew/browser @logbrew/svelte",
        install: "done",
        init: "done",
        edited: Some(("src/hooks.server.js", "createLogBrewSvelteKitHooks(")),
    },
    Case {
        label: "react-express",
        files: &[
            (
                "package.json",
                r#"{"dependencies":{"react":"19.0.0","express":"5.0.0"}}"#,
            ),
            ("yarn.lock", ""),
            (
                "server.js",
                "const express = require(\"express\");\nconst app = express();\napp.listen(3000);\n",
            ),
        ],
        integration: "react_express",
        env_file: ".env",
        command: "yarn add @logbrew/sdk @logbrew/browser @logbrew/react @logbrew/node @logbrew/express",
        install: "done",
        init: "done",
        edited: Some((
            "server.js",
            "const { logbrewMiddleware } = require(\"@logbrew/express\");\nconst app = express();\nif (process.env.LOGBREW_SERVER_API_KEY) {\n  app.use(logbrewMiddleware());\n}\n",
        )),
    },
    Case {
        label: "python",
        files: &[("pyproject.toml", "[project]\nname = \"worker\"\n")],
        integration: "python",
        env_file: ".env",
        command: "python3 -m pip install --upgrade logbrew-sdk",
        install: "done",
        init: "manual",
        edited: None,
    },
    Case {
        label: "django",
        files: &[
            ("pyproject.toml", "[project]\ndependencies = [\"django\"]\n"),
            ("uv.lock", ""),
            (
                "manage.py",
                "os.environ.setdefault(\"DJANGO_SETTINGS_MODULE\", \"site_app.settings\")\n",
            ),
            ("site_app/settings.py", "MIDDLEWARE = []\n"),
        ],
        integration: "django",
        env_file: ".env",
        command: "uv add logbrew-sdk logbrew-django",
        install: "done",
        init: "done",
        edited: Some((
            "site_app/settings.py",
            "MIDDLEWARE = [\"logbrew_django.LogBrewDjangoMiddleware\", *MIDDLEWARE]",
        )),
    },
    Case {
        label: "flask",
        files: &[
            ("pyproject.toml", "[project]\ndependencies = [\"flask\"]\n"),
            ("poetry.lock", ""),
            (
                "app.py",
                "from flask import Flask\n\napp = Flask(__name__)\n",
            ),
        ],
        integration: "flask",
        env_file: ".env",
        command: "poetry add logbrew-sdk logbrew-flask",
        install: "done",
        init: "done",
        edited: Some((
            "app.py",
            "import os\nfrom logbrew_flask import init_logbrew\napp = Flask(__name__)\nif os.environ.get(\"LOGBREW_SERVER_API_KEY\"):\n    init_logbrew(app)\n",
        )),
    },
    Case {
        label: "fastapi",
        files: &[
            ("Pipfile", "[packages]\nfastapi = \"*\"\n"),
            (
                "main.py",
                "from fastapi import FastAPI\n\napi = FastAPI()\n",
            ),
        ],
        integration: "fastapi",
        env_file: ".env",
        command: "pipenv install logbrew-sdk logbrew-fastapi",
        install: "done",
        init: "done",
        edited: Some((
            "main.py",
            "    init_logbrew(api, api_key=os.environ[\"LOGBREW_SERVER_API_KEY\"], service_name=",
        )),
    },
    Case {
        label: "go",
        files: &[("go.mod", "module example.com/api\n")],
        integration: "go",
        env_file: ".env",
        command: "go get github.com/LogBrewCo/sdk/go/logbrew@latest",
        install: "done",
        init: "manual",
        edited: None,
    },
    Case {
        label: "maven",
        files: &[(
            "pom.xml",
            "<project>\n  <build><plugins><plugin>\n    <dependencies>\n    </dependencies>\n  </plugin></plugins></build>\n  <dependencies>\n  </dependencies>\n</project>\n",
        )],
        integration: "java",
        env_file: ".env",
        command: "",
        install: "done",
        init: "manual",
        edited: Some((
            "pom.xml",
            "  <dependencies>\n    <dependency>\n      <groupId>co.logbrew</groupId>",
        )),
    },
    Case {
        label: "spring-gradle",
        files: &[(
            "build.gradle",
            "plugins { id 'org.springframework.boot' version '3.4.0' }\n\ndependencies {\n}\n",
        )],
        integration: "spring",
        env_file: ".env",
        command: "",
        install: "done",
        init: "manual",
        edited: Some((
            "build.gradle",
            "dependencies {\n    implementation 'co.logbrew:logbrew-sdk:",
        )),
    },
    Case {
        label: "php",
        files: &[("composer.json", "{}")],
        integration: "php",
        env_file: ".env",
        command: "composer require logbrew/sdk",
        install: "done",
        init: "manual",
        edited: None,
    },
    Case {
        label: "symfony",
        files: &[
            ("composer.json", "{}"),
            (
                "config/bundles.php",
                "<?php\n\nreturn [\n    Symfony\\Bundle\\FrameworkBundle\\FrameworkBundle::class => ['all' => true],\n];\n",
            ),
        ],
        integration: "symfony",
        env_file: ".env.local",
        command: "composer require logbrew/sdk",
        install: "done",
        init: "done",
        edited: Some((
            "config/bundles.php",
            "    LogBrew\\Symfony\\LogBrewBundle::class => ['all' => true],\n];\n",
        )),
    },
    Case {
        label: "ruby",
        files: &[("Gemfile", "source \"https://rubygems.org\"\n")],
        integration: "ruby",
        env_file: ".env",
        command: "bundle add logbrew-sdk --version ~> RUBY_SDK_VERSION",
        install: "done",
        init: "manual",
        edited: None,
    },
    Case {
        label: "rails",
        files: &[
            ("Gemfile", "source \"https://rubygems.org\"\n"),
            ("config/application.rb", ""),
        ],
        integration: "rails",
        env_file: ".env",
        command: "bundle add logbrew-sdk --version ~> RUBY_SDK_VERSION",
        install: "done",
        init: "not_needed",
        edited: None,
    },
    Case {
        label: "aspnetcore",
        files: &[
            (
                "Api.csproj",
                "<Project Sdk=\"Microsoft.NET.Sdk.Web\"></Project>\n",
            ),
            (
                "Program.cs",
                "var builder = WebApplication.CreateBuilder(args);\nvar app = builder.Build();\napp.UseRouting();\napp.Run();\n",
            ),
        ],
        integration: "aspnetcore",
        env_file: ".env",
        command: "dotnet add package LogBrew.AspNetCore --version ASPNETCORE_VERSION",
        install: "done",
        init: "done",
        edited: Some((
            "Program.cs",
            "using LogBrew;\nvar builder = WebApplication.CreateBuilder(args);\nbuilder.AddLogBrew();\nvar app = builder.Build();\napp.UseRouting();\napp.UseLogBrew();\napp.Run();\n",
        )),
    },
    Case {
        label: "swift",
        files: &[("Package.swift", "// swift-tools-version:5.9\n")],
        integration: "swiftpm",
        env_file: ".env",
        command: "",
        install: "manual",
        init: "manual",
        edited: None,
    },
    Case {
        label: "objc",
        files: &[
            ("project.yml", "name: App\n"),
            ("Sources/main.m", "int main(void) { return 0; }\n"),
        ],
        integration: "source",
        env_file: ".env",
        command: "",
        install: "manual",
        init: "manual",
        edited: None,
    },
    Case {
        label: "cmake",
        files: &[("CMakeLists.txt", "project(App LANGUAGES CXX)\n")],
        integration: "cmake",
        env_file: ".env",
        command: "",
        install: "manual",
        init: "manual",
        edited: None,
    },
];

async_test!(setup_yes_covers_every_planned_framework_and_reruns_without_changes -> TestResult, {
    for case in CASES {
        let server = MockServer::start().await?;
        mount_api(
            &server,
            &[
                (OTHER_PROJECT_ID, "Another project"),
                (PROJECT_ID, "Created project"),
            ],
            NEW_KEY,
        );
        let dir = fixture(case.label, case.files)?;
        let bin = fake_package_managers(case.label)?;

        let first = run_setup(&server, &dir, &bin).await?;
        assert!(
            first.status.success(),
            "{}: {}",
            case.label,
            String::from_utf8_lossy(&first.stderr)
        );
        let text = String::from_utf8(first.stdout)?;
        assert!(
            !text.contains(NEW_KEY),
            "{}: the key must never be printed",
            case.label
        );
        let body: serde_json::Value = serde_json::from_str(&text)?;
        assert_eq!(body["ok"], true, "{}", case.label);
        assert_eq!(
            body["framework"]["integration"], case.integration,
            "{}",
            case.label
        );
        assert_eq!(
            step(&body, "project")["status"],
            "created",
            "{}",
            case.label
        );
        assert_eq!(
            step(&body, "project")["env_file"],
            case.env_file,
            "{}",
            case.label
        );
        assert_eq!(
            step(&body, "install")["status"],
            case.install,
            "{}",
            case.label
        );
        assert_eq!(step(&body, "init")["status"], case.init, "{}", case.label);
        assert_eq!(step(&body, "mcp")["status"], "no_clients", "{}", case.label);
        assert_eq!(step(&body, "verify")["status"], "arrived", "{}", case.label);
        let env_text = std::fs::read_to_string(dir.join(case.env_file))?;
        assert!(
            env_text.contains(&format!("LOGBREW_SERVER_API_KEY={NEW_KEY}\n")),
            "{}",
            case.label
        );
        assert!(
            env_text.contains(&format!("LOGBREW_PROJECT_ID={PROJECT_ID}\n")),
            "{}",
            case.label
        );
        assert_eq!(
            std::fs::metadata(dir.join(case.env_file))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let log = std::fs::read_to_string(bin.join("calls.log")).unwrap_or_default();
        let command = case
            .command
            .replace("RUBY_SDK_VERSION", RUBY_SDK_VERSION)
            .replace("ASPNETCORE_VERSION", ASPNETCORE_VERSION);
        assert_eq!(log.trim(), command, "{}", case.label);
        if let Some((path, expected)) = case.edited {
            let edited = std::fs::read_to_string(dir.join(path))?;
            assert!(
                edited.contains(expected),
                "{}: {path} was\n{edited}",
                case.label
            );
        }
        if case.init == "manual" {
            assert!(
                step(&body, "init")["instructions"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            );
        }
        let sent = sent_event(&server)?;
        assert_eq!(sent["sdk"]["name"], "logbrew-cli");
        assert_eq!(
            sent["events"][0]["attributes"]["context"]["resource"]["deployment"]["environment"],
            "development"
        );

        let second = run_setup(&server, &dir, &bin).await?;
        assert!(
            second.status.success(),
            "{}: {}",
            case.label,
            String::from_utf8_lossy(&second.stderr)
        );
        let body: serde_json::Value = serde_json::from_slice(&second.stdout)?;
        assert_eq!(
            body["files"],
            serde_json::json!([]),
            "{}: rerun must not change files",
            case.label
        );
        assert_eq!(step(&body, "project")["status"], "kept", "{}", case.label);
        assert_eq!(step(&body, "project")["key"], "kept", "{}", case.label);
        assert_eq!(
            std::fs::read_to_string(dir.join(case.env_file))?,
            env_text,
            "{}",
            case.label
        );
        let creations = server
            .received_requests()
            .iter()
            .filter(|request| {
                request.method == "POST" && request.url.path().starts_with("/api/projects")
            })
            .count();
        assert_eq!(
            creations, 1,
            "{}: rerun must not create another project or key",
            case.label
        );
    }
    Ok(())
});

async_test!(setup_yes_reuses_the_project_named_after_the_directory_with_a_server_key -> TestResult, {
    let dir = fixture("by-name", &[("build.gradle", "dependencies {\n}\n")])?;
    let name = directory_name(&dir)?;
    let server = MockServer::start().await?;
    mount_api(&server, &[(PROJECT_ID, name.as_str())], NEW_KEY);
    let mut output = Vec::new();
    let options = WizardOptions {
        yes: true,
        json: true,
        ..WizardOptions::default()
    };
    let mut input = std::io::empty();
    logbrew_cli::setup::run_wizard(
        &environment(&server, &dir),
        &options,
        &mut input,
        &mut output,
    )
    .await?;
    let body: serde_json::Value = serde_json::from_slice(&output)?;
    assert_eq!(step(&body, "project")["status"], "kept");
    assert_eq!(step(&body, "project")["key"], "created");
    assert_eq!(step(&body, "verify")["status"], "arrived");
    let key_request = server
        .received_requests()
        .into_iter()
        .find(|request| request.url.path() == format!("/api/projects/{PROJECT_ID}/ingest-keys"))
        .ok_or("setup must create a key for the existing project")?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&key_request.body)?,
        serde_json::json!({"label": "logbrew setup", "kind": "server", "expires_at": null})
    );
    assert!(key_request.headers.get("idempotency-key").is_some());
    Ok(())
});

async_test!(interactive_setup_shows_diffs_then_applies_after_confirmation -> TestResult, {
    let dir = fixture("interactive", &[("build.gradle", "dependencies {\n}\n")])?;
    let server = MockServer::start().await?;
    mount_api(&server, &[(OTHER_PROJECT_ID, "Another project")], NEW_KEY);
    let mut output = Vec::new();
    logbrew_cli::setup::run_wizard(
        &environment(&server, &dir),
        &WizardOptions::default(),
        &mut &b"n\n\n"[..],
        &mut output,
    )
    .await?;
    let text = String::from_utf8(output)?;
    assert!(!text.contains(NEW_KEY), "the key must never be printed");
    for expected in [
        "  1. Another project",
        "Project [Enter for n]: ",
        "--- /dev/null\n+++ b/.env\n+LOGBREW_SERVER_API_KEY=<new ingest key>\n",
        "+LOGBREW_PROJECT_ID=<new project id>\n",
        "--- a/build.gradle\n+++ b/build.gradle\n dependencies {\n+    implementation 'co.logbrew:logbrew-sdk:",
        "Apply these changes? [Y/n] ",
        "Wrote .env",
        "Test event: arrived in LogBrew",
        "Next: load the env file into your app's environment, then start it",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }
    let gradle = std::fs::read_to_string(dir.join("build.gradle"))?;
    assert_eq!(
        gradle,
        format!(
            "dependencies {{\n    implementation 'co.logbrew:logbrew-sdk:{JAVA_SDK_VERSION}'\n}}\n"
        )
    );
    Ok(())
});

async_test!(declined_setup_changes_nothing_and_creates_nothing -> TestResult, {
    let dir = fixture("declined", &[("build.gradle", "dependencies {\n}\n")])?;
    let server = MockServer::start().await?;
    mount_api(&server, &[], NEW_KEY);
    let mut output = Vec::new();
    logbrew_cli::setup::run_wizard(
        &environment(&server, &dir),
        &WizardOptions::default(),
        &mut &b"n\nn\n"[..],
        &mut output,
    )
    .await?;
    assert!(String::from_utf8(output)?.ends_with("Apply these changes? [Y/n] No changes made.\n"));
    assert!(!dir.join(".env").exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("build.gradle"))?,
        "dependencies {\n}\n"
    );
    assert!(
        server
            .received_requests()
            .iter()
            .all(|request| request.method == "GET")
    );

    let ended = logbrew_cli::setup::run_wizard(
        &environment(&server, &dir),
        &WizardOptions::default(),
        &mut std::io::empty(),
        &mut Vec::new(),
    )
    .await;
    assert!(matches!(
        ended,
        Err(RuntimeError::Unavailable {
            message: "setup canceled; no changes made",
            ..
        })
    ));
    assert!(!dir.join(".env").exists());
    Ok(())
});

async_test!(mcp_config_is_added_only_after_consent_and_keeps_other_servers -> TestResult, {
    let existing =
        "{\n  \"mcpServers\": {\n    \"other\": { \"command\": \"other-server\" }\n  }\n}\n";
    let dir = fixture(
        "mcp",
        &[
            ("build.gradle", "dependencies {\n}\n"),
            (".mcp.json", existing),
            (".vscode/settings.json", "{}"),
        ],
    )?;
    let server = MockServer::start().await?;
    mount_api(&server, &[], NEW_KEY);
    let mut output = Vec::new();
    logbrew_cli::setup::run_wizard(
        &environment(&server, &dir),
        &WizardOptions::default(),
        &mut &b"n\ny\ny\n"[..],
        &mut output,
    )
    .await?;
    let text = String::from_utf8(output)?;
    assert!(
        text.contains(
            "Add the LogBrew MCP server to Claude Code & VS Code for this project? [y/N] "
        )
    );
    let claude: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(".mcp.json"))?)?;
    assert_eq!(claude["mcpServers"]["other"]["command"], "other-server");
    assert_eq!(
        claude["mcpServers"]["logbrew"]["url"],
        "https://mcp.logbrew.co/mcp"
    );
    let vscode: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(".vscode/mcp.json"))?)?;
    assert_eq!(vscode["servers"]["logbrew"]["type"], "http");
    assert!(!dir.join(".cursor").exists());

    let rerun = fixture(
        "mcp-skip",
        &[
            ("build.gradle", "dependencies {\n}\n"),
            (".cursor/rules", ""),
        ],
    )?;
    let options = WizardOptions {
        yes: true,
        json: true,
        ..WizardOptions::default()
    };
    let mut output = Vec::new();
    logbrew_cli::setup::run_wizard(
        &environment(&server, &rerun),
        &options,
        &mut std::io::empty(),
        &mut output,
    )
    .await?;
    let body: serde_json::Value = serde_json::from_slice(&output)?;
    assert_eq!(step(&body, "mcp")["status"], "skipped");
    assert!(
        !rerun.join(".cursor/mcp.json").exists(),
        "--yes without --mcp must not touch agent config"
    );
    Ok(())
});

async_test!(an_existing_key_and_other_env_values_are_kept_and_never_printed -> TestResult, {
    let existing = "lbw_ingest_existing_private_value";
    let env_text = format!(
        "APP_SETTING=private-value\nLOGBREW_SERVER_API_KEY={existing}\nLOGBREW_PROJECT_ID={PROJECT_ID}\n"
    );
    let dir = fixture(
        "existing-key",
        &[
            ("build.gradle", "dependencies {\n}\n"),
            (".env", env_text.as_str()),
        ],
    )?;
    let server = MockServer::start().await?;
    mount_api(&server, &[(PROJECT_ID, "Kept project")], existing);
    let mut output = Vec::new();
    logbrew_cli::setup::run_wizard(
        &environment(&server, &dir),
        &WizardOptions::default(),
        &mut &b"y\n"[..],
        &mut output,
    )
    .await?;
    let text = String::from_utf8(output)?;
    assert!(
        !text.contains("private-value") && !text.contains(existing),
        "{text}"
    );
    assert!(text.contains("+LOGBREW_SERVICE_NAME="), "{text}");
    let written = std::fs::read_to_string(dir.join(".env"))?;
    assert!(
        written.starts_with(&env_text),
        "existing values must stay unchanged"
    );
    assert!(
        server
            .received_requests()
            .iter()
            .all(|request| request.method != "POST" || request.url.path() == "/v1/events"),
        "an existing key must not create a project or key"
    );
    Ok(())
});

async_test!(setup_never_writes_through_a_symlinked_env_file -> TestResult, {
    let dir = fixture(
        "symlink",
        &[("build.gradle", "dependencies {\n}\n"), ("outside.txt", "")],
    )?;
    std::os::unix::fs::symlink(dir.join("outside.txt"), dir.join(".env"))?;
    let server = MockServer::start().await?;
    mount_api(&server, &[], NEW_KEY);
    let options = WizardOptions {
        yes: true,
        ..WizardOptions::default()
    };
    let result = logbrew_cli::setup::run_wizard(
        &environment(&server, &dir),
        &options,
        &mut std::io::empty(),
        &mut Vec::new(),
    )
    .await;
    assert!(matches!(
        result,
        Err(RuntimeError::Unavailable {
            message: "setup does not write through symlinks or unreadable files",
            ..
        })
    ));
    assert_eq!(std::fs::read_to_string(dir.join("outside.txt"))?, "");
    assert!(server.received_requests().is_empty());

    let linked = fixture(
        "symlink-dir",
        &[("build.gradle", "dependencies {\n}\n"), (".git/HEAD", ""), ("elsewhere/.keep", "")],
    )?;
    std::os::unix::fs::symlink(linked.join("elsewhere"), linked.join(".cursor"))?;
    std::os::unix::fs::symlink(linked.join("elsewhere/ignore"), linked.join(".gitignore"))?;
    let options = WizardOptions {
        yes: true,
        mcp: true,
        ..WizardOptions::default()
    };
    let result = logbrew_cli::setup::run_wizard(
        &environment(&server, &linked),
        &options,
        &mut std::io::empty(),
        &mut Vec::new(),
    )
    .await;
    assert!(result.is_err(), "a symlinked target must stop setup");
    assert!(!linked.join(".env").exists(), "nothing may be written");
    assert!(!linked.join("elsewhere/mcp.json").exists());
    assert!(
        server.received_requests().iter().all(|request| request.method == "GET"),
        "setup must stop before it creates a project or key"
    );
    Ok(())
});

async_test!(failed_project_creation_writes_no_files -> TestResult, {
    let dir = fixture("create-fails", &[("build.gradle", "dependencies {\n}\n")])?;
    let server = MockServer::start().await?;
    Mock::auth("GET", "/api/projects", ACCOUNT)
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!([])))
        .mount(&server);
    Mock::auth("POST", "/api/projects", ACCOUNT)
        .respond_with(ResponseTemplate::new(500).set_body_json(
            &serde_json::json!({"error": "hostile detail", "code": "internal_error"}),
        ))
        .mount(&server);
    let options = WizardOptions {
        yes: true,
        json: true,
        ..WizardOptions::default()
    };
    let mut output = Vec::new();
    let result = logbrew_cli::setup::run_wizard(
        &environment(&server, &dir),
        &options,
        &mut std::io::empty(),
        &mut output,
    )
    .await;
    assert!(result.is_err());
    assert_eq!(output, Vec::<u8>::new());
    assert!(!dir.join(".env").exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("build.gradle"))?,
        "dependencies {\n}\n"
    );
    Ok(())
});

async_test!(a_rejected_test_event_fails_setup_with_a_json_report -> TestResult, {
    let dir = fixture("rejected", &[("build.gradle", "dependencies {\n}\n")])?;
    let server = MockServer::start().await?;
    mount_api(&server, &[], "lbw_ingest_some_other_key");
    let options = WizardOptions {
        yes: true,
        json: true,
        ..WizardOptions::default()
    };
    let mut output = Vec::new();
    let result = logbrew_cli::setup::run_wizard(
        &environment(&server, &dir),
        &options,
        &mut std::io::empty(),
        &mut output,
    )
    .await;
    assert!(matches!(
        result,
        Err(RuntimeError::Unavailable {
            message: "setup did not finish",
            ..
        })
    ));
    let body: serde_json::Value = serde_json::from_slice(&output)?;
    assert_eq!(body["ok"], false);
    assert_eq!(step(&body, "verify")["status"], "not_arrived");
    assert_eq!(
        body["next"],
        "check the key in the env file, then rerun logbrew setup"
    );
    Ok(())
});

async_test!(setup_without_a_supported_project_changes_nothing -> TestResult, {
    let dir = fixture("empty", &[("notes.txt", "")])?;
    let server = MockServer::start().await?;
    let options = WizardOptions {
        yes: true,
        ..WizardOptions::default()
    };
    let result = logbrew_cli::setup::run_wizard(
        &environment(&server, &dir),
        &options,
        &mut std::io::empty(),
        &mut Vec::new(),
    )
    .await;
    assert!(matches!(
        result,
        Err(RuntimeError::Unavailable {
            message: "no supported project found",
            ..
        })
    ));
    assert!(server.received_requests().is_empty());
    Ok(())
});

/// Mounts projects, project and key creation, ingest accepting `ingest_key`, and log read-back.
fn mount_api(server: &MockServer, projects: &[(&str, &str)], ingest_key: &str) {
    let rows = projects
        .iter()
        .map(|(id, name)| project_row(id, name))
        .collect::<Vec<_>>();
    Mock::auth("GET", "/api/projects", ACCOUNT)
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::Value::Array(rows)))
        .mount(server);
    Mock::auth("POST", "/api/projects", ACCOUNT)
        .respond_with(|request: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
            ResponseTemplate::new(201)
                .set_body_json(&created_project(body["name"].as_str().unwrap_or_default()))
        })
        .mount(server);
    Mock::auth(
        "POST",
        format!("/api/projects/{PROJECT_ID}/ingest-keys"),
        ACCOUNT,
    )
    .and(body_json(
        serde_json::json!({"label": "logbrew setup", "kind": "server", "expires_at": null}),
    ))
    .respond_with(
        ResponseTemplate::new(201).set_body_json(&serde_json::json!({
            "id": INGEST_ID,
            "label": "logbrew setup",
            "kind": "server",
            "token": NEW_KEY,
            "created_at": "2026-10-27T12:00:00Z",
            "expires_at": null,
            "next": "configure this ingest key in an SDK or client",
            "next_action": {"code": "send_first_telemetry", "target": "telemetry_ingest"}
        })),
    )
    .mount(server);
    Mock::auth("POST", "/v1/events", ingest_key)
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({"logs": 1})))
        .mount(server);
    Mock::auth("GET", "/api/logs", ACCOUNT)
        .respond_with(|request: &Request| {
            let search = request
                .url
                .query_pairs()
                .find(|(name, _)| name == "search")
                .map(|(_, value)| value.into_owned())
                .unwrap_or_default();
            ResponseTemplate::new(200).set_body_json(&serde_json::json!([{
                "id": "523e4567-e89b-12d3-a456-426614174000",
                "project_id": PROJECT_ID,
                "message": format!("LogBrew setup check {search}"),
            }]))
        })
        .mount(server);
}

fn project_row(id: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "provider_project_id": "provider-project",
        "provider_project_slug": null,
        "provider": "logbrew",
        "is_active": true,
        "access": {
            "kind": "owner",
            "organization_id": null,
            "role_id": null,
            "role_name": null,
            "permissions": ["project_read", "issue_manage", "project_access_manage"]
        },
        "language": null,
        "setup_status": "created",
        "setup_started_at": null,
        "first_telemetry_seen_at": null,
        "last_seen_at": null,
        "last_release": null,
        "last_environment": null,
        "created_at": "2026-10-27T12:00:00Z"
    })
}

fn created_project(name: &str) -> serde_json::Value {
    serde_json::json!({
        "project": project_row(PROJECT_ID, name),
        "setup": {
            "project_id": PROJECT_ID,
            "status": "created",
            "runtime": "go",
            "source": "cli",
            "created_at": "2026-10-27T12:00:00Z",
            "setup_started_at": null,
            "first_telemetry_seen_at": null,
            "last_seen_at": null,
            "last_release": null,
            "last_environment": null,
            "last_signal": null,
            "next": "choose an SDK or CLI setup path for this project",
            "next_action": {"code": "choose_setup_path", "target": "project_setup"}
        },
        "ingest": {
            "id": INGEST_ID,
            "label": "CLI ingest key",
            "kind": "cli",
            "token": NEW_KEY,
            "created_at": "2026-10-27T12:00:00Z",
            "expires_at": null,
            "next": "store this credential now",
            "next_action": {"code": "store_ingest_key", "target": "local_credentials"}
        }
    })
}

fn step<'a>(body: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    body["steps"]
        .as_array()
        .and_then(|steps| steps.iter().find(|step| step["step"] == name))
        .unwrap_or(&serde_json::Value::Null)
}

fn sent_event(server: &MockServer) -> TestResult<serde_json::Value> {
    let request = server
        .received_requests()
        .into_iter()
        .rfind(|request| request.url.path() == "/v1/events")
        .ok_or("setup must send a test event")?;
    Ok(serde_json::from_slice(&request.body)?)
}

fn environment(server: &MockServer, dir: &std::path::Path) -> CliEnvironment {
    CliEnvironment {
        base_url: server.uri(),
        token: Some(ACCOUNT.to_owned()),
        home: dir.parent().map(|parent| parent.join("no-home")),
        cwd: Some(dir.to_path_buf()),
    }
}

async fn run_setup(
    server: &MockServer,
    dir: &std::path::Path,
    bin: &std::path::Path,
) -> TestResult<std::process::Output> {
    let _removed = std::fs::remove_file(bin.join("calls.log"));
    let mut command = cli_command(server);
    let _command = command
        .args(["setup", "--yes", "--json"])
        .current_dir(dir)
        .env("HOME", bin)
        .env("PATH", bin)
        .env("FAKE_LOG", bin.join("calls.log"));
    run_cli_command(command).await
}

fn fixture(label: &str, files: &[(&str, &str)]) -> std::io::Result<std::path::PathBuf> {
    let root = crate::isolated_home("logbrew-setup-wizard", label)?;
    for (name, contents) in files {
        let path = root.join(name);
        std::fs::create_dir_all(
            path.parent()
                .ok_or_else(|| std::io::Error::other("no parent"))?,
        )?;
        std::fs::write(path, contents)?;
    }
    Ok(root)
}

fn fake_package_managers(label: &str) -> std::io::Result<std::path::PathBuf> {
    let bin = crate::isolated_home("logbrew-setup-wizard-bin", label)?;
    for name in PACKAGE_MANAGERS {
        let path = bin.join(name);
        std::fs::write(
            path.as_path(),
            "#!/bin/sh\nprintf '%s %s\\n' \"${0##*/}\" \"$*\" >> \"$FAKE_LOG\"\n",
        )?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(bin)
}

fn directory_name(dir: &std::path::Path) -> TestResult<String> {
    Ok(std::fs::canonicalize(dir)?
        .file_name()
        .ok_or("fixture has no name")?
        .to_string_lossy()
        .into_owned())
}

use std::os::unix::fs::PermissionsExt as _;
