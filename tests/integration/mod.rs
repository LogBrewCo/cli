//! Complete public CLI integration contract.

mod action_cursor_pagination;
mod action_filter_recovery;
mod action_investigation;
mod analytics_compare;
mod analytics_funnel;
mod analytics_lifecycle;
mod analytics_overview;
mod analytics_paths;
mod analytics_properties;
mod analytics_retention;
mod api_rendering;
mod command_shaped_help;
mod commands;
mod deployment;
mod execution_commands;
mod explain_contracts;
mod flag_recovery;
mod help_errors;
mod help_text;
mod issue_cursor_pagination;
mod issue_investigation;
mod issue_mutation_shortcuts;
mod issue_recovery;
mod local_commands;
mod log_cursor_pagination;
mod login_loopback;
mod logout_sessions;
mod native_debug_artifact_upload;
mod native_debug_artifact_upload_bounds;
mod native_debug_artifacts;
mod parse_errors;
mod project_archive;
mod project_create;
mod project_doctor;
mod project_ingest_key_create;
mod projects;
mod release_workflows;
mod runtime_errors;
mod search_commands;
mod setup_readiness;
mod shortcut_commands;
mod span_investigation;
mod status_commands;
mod support_context;
mod support_tickets;
mod trace_discovery;
mod usage;
mod watch_errors;
mod whoami;

/// Runs the built CLI against one loopback server with isolated credentials.
async fn run_cli<I, S>(
    server: &wiremock::MockServer,
    args: I,
) -> Result<std::process::Output, Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString>,
{
    let base_url = server.uri();
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    let process = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_logbrew"))
            .env_clear()
            .env("HOME", std::env::temp_dir())
            .env("LOGBREW_API_URL", base_url)
            .env("LOGBREW_TOKEN", "account-token")
            .args(args)
            .output()
    })
    .await??;
    Ok(process)
}

/// Executes one parsed command against isolated authenticated loopback state.
fn authenticated_env(
    server: &wiremock::MockServer,
    token: &str,
    home_name: Option<&str>,
) -> logbrew_cli::CliEnvironment {
    test_env(
        server,
        Some(token),
        home_name.map(|name| std::env::temp_dir().join(format!("logbrew-{name}"))),
    )
}

fn test_env(
    server: &wiremock::MockServer,
    token: Option<&str>,
    home: Option<std::path::PathBuf>,
) -> logbrew_cli::CliEnvironment {
    logbrew_cli::CliEnvironment {
        base_url: server.uri(),
        token: token.map(str::to_owned),
        home,
        cwd: None,
    }
}

async fn run_command<const N: usize>(
    server: &wiremock::MockServer,
    args: [&str; N],
    home_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let command = logbrew_cli::parse_command(args)?;
    let mut output = Vec::new();
    logbrew_cli::execute_command(
        &command,
        &authenticated_env(server, "test-token", Some(home_name)),
        &mut output,
    )
    .await?;
    Ok(String::from_utf8(output)?)
}
