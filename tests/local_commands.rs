//! Local non-HTTP command tests.

use logbrew_cli::{CliEnvironment, Command, HelpTopic, execute_command, help, parse_command};

#[test]
fn parses_version_command_for_humans() {
    let command = parse_command(["logbrew", "version"]).expect("version parses");

    assert_eq!(command, Command::Version { json: false });
    assert!(!command.wants_json());
    assert!(command.http_path().is_none());
}

#[test]
fn parses_logout_command_for_agents() {
    for args in [
        &["logbrew", "logout", "--json"][..],
        &["logbrew", "--json", "logout"][..],
        &["logbrew", "auth", "logout", "--json"][..],
        &["logbrew", "--json", "auth", "logout"][..],
    ] {
        let command = parse_command(args.iter().copied()).expect("logout parses");

        assert_eq!(command, Command::Logout { json: true });
        assert!(command.wants_json());
        assert!(command.http_path().is_none());
    }
}

#[test]
fn parses_root_version_flag_for_agents() {
    let command = parse_command(["logbrew", "--version", "--json"]).expect("version parses");

    assert_eq!(command, Command::Version { json: true });
    assert!(command.wants_json());
}

#[test]
fn parses_global_json_version_forms_for_agents() {
    for args in [
        &["logbrew", "--json", "version"][..],
        &["logbrew", "--json", "--version"][..],
    ] {
        let command = parse_command(args.iter().copied()).expect("version parses");

        assert_eq!(command, Command::Version { json: true });
        assert!(command.wants_json());
        assert!(command.http_path().is_none());
    }
}

#[test]
fn version_help_is_discoverable() {
    let command = parse_command(["logbrew", "version", "--help"]).expect("help parses");

    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::Version,
            json: false
        }
    );
    assert!(help::help_text(HelpTopic::Root).contains("logbrew version [--json]"));
    assert!(help::help_text(HelpTopic::Version).contains("Prints the installed CLI version."));
    assert!(help::help_text(HelpTopic::Version).contains("The CLI is a native Rust binary."));
}

#[test]
fn logout_help_is_discoverable() {
    let command = parse_command(["logbrew", "logout", "--help"]).expect("help parses");

    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::Logout,
            json: false
        }
    );
    assert!(help::help_text(HelpTopic::Root).contains("logbrew logout [--json]"));
    assert!(help::help_text(HelpTopic::Logout).contains("Removes both local CLI credentials."));
}

#[test]
fn setup_help_is_honest_about_install_readiness() {
    let command = parse_command(["logbrew", "setup", "--help"]).expect("help parses");
    let setup_help = help::help_text(HelpTopic::Setup);

    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::Setup,
            json: false
        }
    );
    assert!(setup_help.contains("No files are changed."));
    assert!(setup_help.contains("Install: not ready"));
    assert!(setup_help.contains("Supported manifests: package.json, pyproject.toml, Pipfile,"));
    assert!(setup_help.contains(
        "Cargo.toml, Package.swift, project.yml, project.yaml, .xcodeproj, .xcworkspace,"
    ));
    assert!(setup_help.contains("go.mod, composer.json."));
    assert!(setup_help.contains(
        "Package managers: npm, pnpm, yarn, bun, pip, uv, poetry, pipenv, cargo, SwiftPM, \
         XcodeGen, Go, Composer."
    ));
}

#[test]
fn project_and_usage_help_are_backend_owned_and_non_mutating() {
    for args in [
        &["logbrew", "projects", "--json"][..],
        &["logbrew", "project", "--json"][..],
        &["logbrew", "--json", "projects"][..],
        &["logbrew", "projects", "create", "checkout", "--json"][..],
        &["logbrew", "setup", "--create-project", "--json"][..],
        &["logbrew", "--json", "setup", "--create-project"][..],
        &["logbrew", "setup", "--create-project", "--help", "--json"][..],
    ] {
        let command = parse_command(args.iter().copied()).expect("project help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::Projects,
                json: true
            }
        );
    }

    for args in [
        &["logbrew", "usage", "--json"][..],
        &["logbrew", "--json", "usage"][..],
        &["logbrew", "account", "usage", "--json"][..],
    ] {
        let command = parse_command(args.iter().copied()).expect("usage help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::Usage,
                json: true
            }
        );
    }

    let projects = help::help_text(HelpTopic::Projects);
    assert!(projects.contains("backend-owned"));
    assert!(projects.contains("Current mode: projects setup marks backend-owned setup as seen;"));
    assert!(projects.contains("No local project, install, quota, or usage state is created."));
    assert!(projects.contains("POST /api/projects/{project_id}/setup/seen"));
    assert!(projects.contains("Never use an account bearer token as SDK or ingest configuration."));

    let usage = help::help_text(HelpTopic::Usage);
    assert!(usage.contains("backend-owned"));
    assert!(usage.contains("Current mode: help only."));
    assert!(
        usage.contains("The CLI does not calculate or persist usage/quota state from local files.")
    );
}

#[test]
fn setup_alias_help_is_discoverable() {
    for args in [
        &["logbrew", "init", "--help"][..],
        &["logbrew", "help", "install"][..],
        &["logbrew", "configure", "help"][..],
        &["logbrew", "sdk", "help", "--json"][..],
    ] {
        let command = parse_command(args.iter().copied()).expect("setup alias help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::Setup,
                json: args.contains(&"--json"),
            }
        );
    }
}

#[test]
fn setup_aliases_are_non_mutating_setup() {
    for alias in ["init", "install", "configure", "sdk"] {
        let command = parse_command(["logbrew", alias, "--auto", "--yes", "--json"])
            .expect("setup alias parses");

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
}

#[tokio::test]
async fn version_human_output_is_short() -> Result<(), Box<dyn std::error::Error>> {
    let command = parse_command(["logbrew", "version"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: None,
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    assert_eq!(text, format!("logbrew {}\n", env!("CARGO_PKG_VERSION")));
    Ok(())
}

#[tokio::test]
async fn version_json_output_is_stable() -> Result<(), Box<dyn std::error::Error>> {
    let command = parse_command(["logbrew", "version", "--json"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: None,
        cwd: None,
    };
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
}

#[tokio::test]
async fn logout_json_removes_local_token_without_leaking_it()
-> Result<(), Box<dyn std::error::Error>> {
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
        std::fs::create_dir_all(token_path.parent().expect("token path has parent"))?;
        std::fs::write(token_path.as_path(), "fixture-token\n")?;
        std::fs::write(refresh_path.as_path(), "fixture-refresh-token\n")?;
        std::fs::write(origin_path.as_path(), "https://example.test\n")?;
        std::fs::write(
            session_path.as_path(),
            serde_json::json!({
                "access_token": "session-access",
                "refresh_token": "session-refresh",
                "origin": "https://example.test",
            })
            .to_string(),
        )?;
        let command = parse_command(args.iter().copied())?;
        let env = CliEnvironment {
            base_url: "https://example.test".to_owned(),
            token: None,
            home: Some(home),
            cwd: None,
        };
        let mut output = Vec::new();

        execute_command(&command, &env, &mut output).await?;

        let text = String::from_utf8(output)?;
        assert!(!text.contains("fixture-token"));
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;
        assert_eq!(body["ok"], true);
        assert_eq!(body["removed"], true);
        assert_eq!(body["auth_source"], "token_file");
        assert_eq!(body["env_token_active"], false);
        assert_eq!(body["next"], "run logbrew login to authenticate again");
        assert!(!text.contains("fixture-refresh-token"));
        assert!(!text.contains("session-access"));
        assert!(!text.contains("session-refresh"));
        assert!(!session_path.exists());
        assert!(!token_path.exists());
        assert!(!refresh_path.exists());
        assert!(!origin_path.exists());
    }
    Ok(())
}

#[tokio::test]
async fn logout_human_is_idempotent_without_local_token() -> Result<(), Box<dyn std::error::Error>>
{
    let home = local_command_home("logout-empty")?;
    let command = parse_command(["logbrew", "logout"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(home),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        "No local LogBrew token found.\nNext: run logbrew login to authenticate\n"
    );
    Ok(())
}

#[tokio::test]
async fn logout_warns_when_env_token_still_authenticates() -> Result<(), Box<dyn std::error::Error>>
{
    let home = local_command_home("logout-env")?;
    let token_path = home.join(".logbrew").join("token");
    std::fs::create_dir_all(token_path.parent().expect("token path has parent"))?;
    std::fs::write(token_path.as_path(), "file-token\n")?;
    let command = parse_command(["logbrew", "logout"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: Some("env-token".to_owned()),
        home: Some(home),
        cwd: None,
    };
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
}

fn local_command_home(name: &str) -> Result<std::path::PathBuf, std::io::Error> {
    let dir = std::env::temp_dir().join(format!("logbrew-cli-{name}-{}", std::process::id()));
    match std::fs::remove_dir_all(dir.as_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::create_dir_all(dir.as_path())?;
    Ok(dir)
}
