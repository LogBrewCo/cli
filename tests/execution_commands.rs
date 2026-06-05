//! CLI execution tests for local command flows.

use std::fs;

use logbrew_cli::{
    CliEnvironment, Command, RuntimeError, WatchTarget, execute_command, parse_command,
    write_runtime_error,
};

#[tokio::test]
async fn authenticated_reads_without_token_explain_login_step() {
    let command = parse_command(["logbrew", "read", "logs", "--release", "api@1", "--json"])
        .expect("command parses");
    let env = CliEnvironment {
        base_url: "http://127.0.0.1:1".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-missing-token-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("missing token fails");

    assert!(matches!(error, RuntimeError::MissingToken));
}

#[tokio::test]
async fn login_no_open_json_prints_auth_url_without_browser_side_effect() {
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-login-no-open-test")),
        cwd: None,
    };
    for args in [
        &["logbrew", "login", "--no-open", "--json"][..],
        &["logbrew", "--json", "login"][..],
    ] {
        let command = parse_command(args.iter().copied()).expect("command");
        let mut output = Vec::new();

        execute_command(&command, &env, &mut output)
            .await
            .expect("login succeeds");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], true);
        assert_eq!(body["auth_url"], "https://example.test/api/auth/cli/login");
        assert_eq!(body["browser_opened"], false);
        assert_eq!(body["next"], "open auth_url in a browser");
    }
}

#[tokio::test]
async fn login_no_open_human_prints_browser_state_and_next_step() {
    let command = parse_command(["logbrew", "login", "--no-open"]).expect("command");
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-login-no-open-human-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .expect("login succeeds");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "Open this URL to log in: https://example.test/api/auth/cli/login\nBrowser: not \
         opened\nNext: open the URL in a browser\n"
    );
}

#[tokio::test]
async fn setup_json_detects_node_project_without_claiming_install()
-> Result<(), Box<dyn std::error::Error>> {
    let project_dir = setup_fixture("setup-node")?;
    fs::write(project_dir.join("package.json"), "{}")?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-setup-node-home")),
        cwd: Some(project_dir),
    };

    for args in [
        &["logbrew", "setup", "--auto", "--yes", "--json"][..],
        &["logbrew", "--json", "setup", "--auto", "--yes"][..],
    ] {
        let command = parse_command(args.iter().copied())?;
        let mut output = Vec::new();

        execute_command(&command, &env, &mut output).await?;

        let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
        assert_eq!(body["ok"], true);
        assert_eq!(body["auto"], true);
        assert_eq!(body["yes"], true);
        assert_eq!(body["install_ready"], false);
        assert_eq!(body["detected"][0]["runtime"], "node");
        assert_eq!(body["detected"][0]["package_manager"], "npm");
        assert_eq!(body["detected"][0]["manifest"], "package.json");
        assert_eq!(
            body["next"],
            "install the matching LogBrew SDK package when packages are ready; send release and \
             environment with logs, issues, actions, and traces"
        );
    }
    Ok(())
}

#[tokio::test]
async fn setup_json_detects_parent_project_when_run_from_subdirectory()
-> Result<(), Box<dyn std::error::Error>> {
    let project_dir = setup_fixture("setup-parent-node")?;
    fs::write(project_dir.join("package.json"), "{}")?;
    let source_dir = project_dir.join("src");
    fs::create_dir_all(source_dir.as_path())?;
    let command = parse_command(["logbrew", "setup", "--json"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-setup-parent-node-home")),
        cwd: Some(source_dir),
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["install_ready"], false);
    assert_eq!(body["detected"][0]["runtime"], "node");
    assert_eq!(body["detected"][0]["package_manager"], "npm");
    assert_eq!(body["detected"][0]["manifest"], "../package.json");
    Ok(())
}

#[tokio::test]
async fn setup_json_detects_xcodegen_ios_project() -> Result<(), Box<dyn std::error::Error>> {
    let project_dir = setup_fixture("setup-xcodegen-ios")?;
    fs::write(project_dir.join("project.yml"), "name: Checkout\n")?;
    let command = parse_command(["logbrew", "setup", "--json"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-setup-xcodegen-ios-home")),
        cwd: Some(project_dir),
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["install_ready"], false);
    assert_eq!(body["detected"][0]["runtime"], "swift-ios");
    assert_eq!(body["detected"][0]["package_manager"], "xcodegen");
    assert_eq!(body["detected"][0]["manifest"], "project.yml");
    Ok(())
}

#[tokio::test]
async fn setup_human_explains_empty_project_next_step() -> Result<(), Box<dyn std::error::Error>> {
    let project_dir = setup_fixture("setup-empty")?;
    let command = parse_command(["logbrew", "setup"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-setup-empty-home")),
        cwd: Some(project_dir),
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        "LogBrew setup plan\nMode: non-mutating plan\nNo files changed.\nInstall: not ready\nNo \
         supported project manifest found.\nNext: run logbrew setup from a project containing \
         package.json, pyproject.toml, Pipfile, Cargo.toml, Package.swift, project.yml, \
         project.yaml, .xcodeproj, .xcworkspace, go.mod, or composer.json.\n"
    );
    Ok(())
}

#[tokio::test]
async fn setup_human_detects_project_without_claiming_install()
-> Result<(), Box<dyn std::error::Error>> {
    let project_dir = setup_fixture("setup-rust-human")?;
    fs::write(project_dir.join("Cargo.toml"), "")?;
    let command = parse_command(["logbrew", "setup"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-setup-rust-human-home")),
        cwd: Some(project_dir),
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        "LogBrew setup plan\nMode: non-mutating plan\nNo files changed.\nInstall: not \
         ready\nDetected runtimes:\n- Rust (cargo) at Cargo.toml\nNext: install the matching \
         LogBrew SDK package when packages are ready; send release and environment with logs, \
         issues, actions, and traces\n"
    );
    Ok(())
}

#[tokio::test]
async fn setup_human_echoes_non_mutating_preferences() -> Result<(), Box<dyn std::error::Error>> {
    let project_dir = setup_fixture("setup-rust-human-prefs")?;
    fs::write(project_dir.join("Cargo.toml"), "")?;
    let command = parse_command(["logbrew", "setup", "--auto", "--yes"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-setup-rust-human-prefs-home")),
        cwd: Some(project_dir),
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    assert!(text.contains("Mode: non-mutating plan\n"));
    assert!(text.contains("Preferences: auto=true, yes=true\n"));
    assert!(text.contains("No files changed.\n"));
    assert!(text.contains("Install: not ready\n"));
    Ok(())
}

#[tokio::test]
async fn watch_json_points_to_historical_fallback() {
    let command = parse_command(["logbrew", "tail", "logs", "--json"]).expect("command parses");
    assert_eq!(
        command,
        Command::Watch {
            target: WatchTarget::Logs,
            json: true
        }
    );
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-watch-json-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("watch unavailable");
    write_runtime_error(&error, command.wants_json(), &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unavailable");
    assert_eq!(
        body["message"],
        "watch is reserved for the live stream transport"
    );
    assert_eq!(
        body["next"],
        "use logbrew logs for historical data until live watch is available"
    );
}

#[tokio::test]
async fn watch_human_points_to_historical_fallback() {
    let command = parse_command(["logbrew", "follow", "events"]).expect("command parses");
    assert_eq!(
        command,
        Command::Watch {
            target: WatchTarget::Actions,
            json: false
        }
    );
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-watch-human-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("watch unavailable");
    write_runtime_error(&error, command.wants_json(), &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "watch is reserved for the live stream transport\nNext: use logbrew actions for \
         historical data until live watch is available\n"
    );
}

fn setup_fixture(name: &str) -> Result<std::path::PathBuf, std::io::Error> {
    let dir = std::env::temp_dir().join(format!("logbrew-cli-{name}-{}", std::process::id()));
    remove_dir_if_exists(dir.as_path())?;
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn remove_dir_if_exists(path: &std::path::Path) -> Result<(), std::io::Error> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
