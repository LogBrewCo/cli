//! Local non-HTTP command tests.

use super::{TestResult, assert_help};
use logbrew_cli::{CliEnvironment, Command, HelpTopic, execute_command, help, parse_command};

#[test]
fn parses_version_command_for_humans() -> TestResult {
    assert_local_command(
        &["logbrew", "version"],
        &Command::Version { json: false },
        false,
    )
}

#[test]
fn parses_logout_command_for_agents() -> TestResult {
    for args in [
        &["logbrew", "logout", "--json"][..],
        &["logbrew", "--json", "logout"],
        &["logbrew", "auth", "logout", "--json"],
        &["logbrew", "--json", "auth", "logout"],
    ] {
        assert_local_command(args, &Command::Logout { json: true }, true)?;
    }
    Ok(())
}

#[test]
fn parses_root_version_flag_for_agents() -> TestResult {
    assert_local_command(
        &["logbrew", "--version", "--json"],
        &Command::Version { json: true },
        true,
    )
}

#[test]
fn parses_global_json_version_forms_for_agents() -> TestResult {
    for args in [
        &["logbrew", "--json", "version"][..],
        &["logbrew", "--json", "--version"],
    ] {
        assert_local_command(args, &Command::Version { json: true }, true)?;
    }
    Ok(())
}

#[test]
fn version_help_is_discoverable() {
    assert_help(&["logbrew", "version", "--help"], HelpTopic::Version, false);

    assert!(help::help_text(HelpTopic::Root).contains("logbrew version [--json]"));
    assert!(help::help_text(HelpTopic::Version).contains("Prints the installed CLI version."));
    assert!(help::help_text(HelpTopic::Version).contains("The CLI is a native Rust binary."));
}

#[test]
fn logout_help_is_discoverable() {
    assert_help(&["logbrew", "logout", "--help"], HelpTopic::Logout, false);

    assert!(help::help_text(HelpTopic::Root).contains("logbrew logout [--json]"));
    let logout_help = help::help_text(HelpTopic::Logout);
    assert_contains_all(
        logout_help,
        &[
            "Attempts to revoke the stored server session",
            "always removes both local CLI credentials",
            "LOGBREW_TOKEN",
        ],
    );
}

#[test]
fn setup_help_is_honest_about_install_readiness() {
    assert_help(&["logbrew", "setup", "--help"], HelpTopic::Setup, false);
    let setup_help = help::help_text(HelpTopic::Setup);

    assert_contains_all(
        setup_help,
        &[
            "No files are changed.",
            "logbrew projects create <name> --ingest-key-file <path> [--runtime <runtime>]",
            "Authenticated project creation (no dashboard sign-in):",
            "logbrew setup --create-project shows secure project-creation help and does not create a project.",
            "compatible releases from the newest SDK release",
            "Python planning detects Django, Flask, and FastAPI",
            "Bundler planning detects Rails",
            "CMake planning pins the newest released C++ SDK tag",
            "mixed React + Express dependencies",
            "LogBrew::LogBrew",
            "LogBrew::HttpTransport",
            "without editing CMakeLists.txt",
            "Python packages: logbrew-sdk, logbrew-django, logbrew-flask,",
            "Other detected runtimes use released SDK guidance until a structured CLI install plan is enabled.",
            "Supported manifests: package.json, pyproject.toml, Pipfile,",
            "Cargo.toml, Package.swift, project.yml, project.yaml, .xcodeproj, .xcworkspace,",
            "CMakeLists.txt",
            "go.mod, composer.json, Gemfile.",
            "Package managers: npm, pnpm, yarn, bun, pip, uv, poetry, pipenv, cargo, SwiftPM, XcodeGen, Go, Composer, Bundler, CMake.",
        ],
    );
    assert!(!setup_help.contains("until their public package is ready"));
}

#[test]
fn project_catalog_executes_while_explicit_setup_help_remains_non_mutating() -> TestResult {
    for args in [
        &["logbrew", "setup", "--create-project", "--json"][..],
        &["logbrew", "--json", "setup", "--create-project"][..],
        &["logbrew", "setup", "--create-project", "--help", "--json"][..],
    ] {
        assert_help(args, HelpTopic::Projects, true);
    }
    for args in [
        &["logbrew", "projects", "--json"][..],
        &["logbrew", "project", "--json"][..],
        &["logbrew", "--json", "projects"][..],
    ] {
        let command = parse_command(args.iter().copied())?;
        assert!(!matches!(command, Command::Help { .. }));
        assert_eq!(command.http_path().as_deref(), Some("/api/projects"));
    }

    let projects = help::help_text(HelpTopic::Projects);
    assert_contains_all(
        projects,
        &[
            "backend-owned",
            "stores the one-time ingest key in a new owner-only file",
            "An authenticated CLI can create a project without dashboard sign-in or additional browser auth.",
            "logbrew setup --create-project shows this help and never creates a project.",
            "No local install, quota, or usage state is created.",
            "POST /api/projects/{project_id}/setup/seen",
            "Never use an account bearer token as SDK or ingest configuration.",
        ],
    );

    let usage = help::help_text(HelpTopic::Usage);
    assert_contains_all(
        usage,
        &[
            "Reads authenticated account usage",
            "without mutating account or billing state",
            "The CLI does not calculate or persist usage/quota state from local files.",
        ],
    );
    Ok(())
}

#[test]
fn setup_alias_help_is_discoverable() {
    for args in [
        &["logbrew", "init", "--help"][..],
        &["logbrew", "help", "install"],
        &["logbrew", "configure", "help"],
        &["logbrew", "sdk", "help", "--json"],
    ] {
        assert_help(args, HelpTopic::Setup, args.contains(&"--json"));
    }
}

#[test]
fn setup_aliases_are_non_mutating_setup() -> TestResult {
    for alias in ["init", "install", "configure", "sdk"] {
        let command = parse_command(["logbrew", alias, "--auto", "--yes", "--json"])?;

        assert_eq!(
            command,
            Command::Setup {
                auto: true,
                yes: true,
                json: true
            }
        );
        assert!(command.http_path().is_none());
    }
    Ok(())
}

async_test!(version_human_output_is_short -> TestResult, {
    let command = parse_command(["logbrew", "version"])?;
    let env = environment(None, None);
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    assert_eq!(text, format!("logbrew {}\n", env!("CARGO_PKG_VERSION")));
    Ok(())
});

async_test!(version_json_output_is_stable -> TestResult, {
    let command = parse_command(["logbrew", "version", "--json"])?;
    let env = environment(None, None);
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["name"], "logbrew");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["binary"], "native");
    assert_eq!(body["os"], std::env::consts::OS);
    assert_eq!(body["arch"], std::env::consts::ARCH);
    Ok(())
});

async_test!(logout_json_removes_legacy_local_credentials_without_leaking_them -> TestResult, {
    for (args, home_name) in [
        (&["logbrew", "logout", "--json"][..], "logout-json"),
        (&["logbrew", "--json", "logout"][..], "logout-global-json"),
        (
            &["logbrew", "--json", "auth", "logout"][..],
            "auth-logout-global-json",
        ),
    ] {
        let home = local_command_home(home_name)?;
        let auth_dir = home.join(".logbrew");
        let token_path = auth_dir.join("token");
        let refresh_path = auth_dir.join("refresh-token");
        let origin_path = auth_dir.join("auth-origin");
        let session_path = auth_dir.join("session.json");
        std::fs::create_dir_all(token_path.parent().ok_or("token path has no parent")?)?;
        std::fs::write(token_path.as_path(), "fixture-token\n")?;
        let command = parse_command(args.iter().copied())?;
        let env = environment(None, Some(home));
        let mut output = Vec::new();

        execute_command(&command, &env, &mut output).await?;

        let text = String::from_utf8(output)?;
        assert!(!text.contains("fixture-token"));
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;
        assert_eq!(body["ok"], true);
        assert_eq!(body["removed"], true);
        assert_eq!(body["auth_source"], "token_file");
        assert_eq!(body["env_token_active"], false);
        assert_eq!(body["server_session"], "not_applicable");
        assert_eq!(body["next"], "run logbrew login to authenticate again");
        assert!(!session_path.exists());
        assert!(!token_path.exists());
        assert!(!refresh_path.exists());
        assert!(!origin_path.exists());
    }
    Ok(())
});

async_test!(logout_human_is_idempotent_without_local_token -> TestResult, {
    let home = local_command_home("logout-empty")?;
    let command = parse_command(["logbrew", "logout"])?;
    let env = environment(None, Some(home));
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        "No local LogBrew token found.\nNext: run logbrew login to authenticate\n"
    );
    Ok(())
});

async_test!(logout_warns_when_env_token_still_authenticates -> TestResult, {
    let home = local_command_home("logout-env")?;
    let token_path = home.join(".logbrew").join("token");
    std::fs::create_dir_all(token_path.parent().ok_or("token path has no parent")?)?;
    std::fs::write(token_path.as_path(), "file-token\n")?;
    let command = parse_command(["logbrew", "logout"])?;
    let env = environment(Some("env-token"), Some(home));
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        "Local LogBrew token removed.\nAuth: env token still active\nNext: unset LOGBREW_TOKEN to \
         fully log out\n"
    );
    assert!(!text.contains("file-token"));
    assert!(!text.contains("env-token"));
    assert!(!token_path.exists());
    Ok(())
});

fn local_command_home(name: &str) -> Result<std::path::PathBuf, std::io::Error> {
    crate::isolated_home("logbrew-cli", name)
}

fn environment(token: Option<&str>, home: Option<std::path::PathBuf>) -> CliEnvironment {
    CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: token.map(str::to_owned),
        home,
        cwd: None,
    }
}

fn assert_contains_all(text: &str, expected: &[&str]) {
    for value in expected {
        assert!(text.contains(value), "missing {value:?}");
    }
}

fn assert_local_command(args: &[&str], expected: &Command, json: bool) -> TestResult {
    let command = parse_command(args.iter().copied())?;
    assert_eq!(&command, expected, "unexpected local command for {args:?}");
    assert_eq!(
        command.wants_json(),
        json,
        "unexpected output mode for {args:?}"
    );
    assert_eq!(
        command.http_path(),
        None,
        "local command must not have an endpoint"
    );
    Ok(())
}
