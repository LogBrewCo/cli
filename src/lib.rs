//! Native `LogBrew` command-line interface library.
//!
//! The CLI is intentionally small and predictable so coding agents can learn it
//! quickly: `read`, `watch`, `explain`, and `set` cover data access and state
//! changes, while `login`, `setup`, and `status` cover local configuration.

#![forbid(unsafe_code)]

#[doc(hidden)]
pub mod auth;
#[doc(hidden)]
pub mod auth_namespace;
mod error;
#[doc(hidden)]
pub mod flags;
pub mod help;
#[doc(hidden)]
pub mod ids;
mod parser;
#[doc(hidden)]
pub mod render;
#[doc(hidden)]
pub mod setup;
#[doc(hidden)]
pub mod status;
#[doc(hidden)]
pub mod version;

use auth::{open_browser, resolve_credential, write_logout_result};
pub use error::{CliError, RuntimeError, write_cli_error, write_runtime_error};
pub use parser::parse_command;
use render::write_api_success;
use setup::write_setup_plan;
use status::execute_status;
use version::execute_version;

/// Accepted issue status values for generic recovery text.
pub(crate) const ISSUE_STATUS_VALUES_NEXT_STEP: &str =
    "use one of unresolved/open, resolved/closed, ignored";
/// Accepted issue status values for read filter recovery text.
pub(crate) const ISSUE_STATUS_FILTER_NEXT_STEP: &str =
    "use --status unresolved/open, --status resolved/closed, or --status ignored";
/// Accepted issue status values for missing mutation arguments.
pub(crate) const ISSUE_STATUS_ARGUMENT_NEXT_STEP: &str =
    "provide one of unresolved/open, resolved/closed, ignored";

/// Parsed `LogBrew` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Shows command usage.
    Help {
        /// Help topic to display.
        topic: HelpTopic,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Opens browser-based authentication.
    Login {
        /// Try to open the login URL in the default browser.
        open_browser: bool,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Removes the local CLI token.
    Logout {
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Detects the current project and prints a non-mutating SDK setup plan.
    Setup {
        /// Let the CLI pick the framework or runtime automatically.
        auto: bool,
        /// Suppress confirmation prompts.
        yes: bool,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Checks local auth and server reachability.
    Status {
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Prints the installed CLI version.
    Version {
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Reads historical observability data.
    Read {
        /// Resource to read.
        target: ReadTarget,
        /// Read filters.
        options: Box<ReadOptions>,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Watches live observability data.
    Watch {
        /// Resource to watch.
        target: WatchTarget,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Fetches context for an issue or trace so an agent can explain it.
    Explain {
        /// Resource to explain.
        target: ExplainTarget,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Mutates server-side state.
    Set {
        /// Target state mutation.
        target: SetTarget,
        /// Emit machine-readable JSON.
        json: bool,
    },
}

/// Help topic for CLI usage output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTopic {
    /// Root command overview.
    Root,
    /// Browser login command.
    Login,
    /// Local logout command.
    Logout,
    /// SDK setup command.
    Setup,
    /// Status check command.
    Status,
    /// Installed CLI version command.
    Version,
    /// Authentication workflow overview.
    Auth,
    /// Machine-readable output overview.
    Json,
    /// Read command overview.
    Read,
    /// Log reading command.
    ReadLogs,
    /// Issue reading command.
    ReadIssues,
    /// Action reading command.
    ReadActions,
    /// Release reading command.
    ReadReleases,
    /// Trace reading command.
    ReadTrace,
    /// Single issue reading command.
    ReadIssue,
    /// Live watch command.
    Watch,
    /// Explain command.
    Explain,
    /// State mutation command.
    Set,
}

impl HelpTopic {
    /// Returns a stable machine-readable topic name.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Login => "login",
            Self::Logout => "logout",
            Self::Setup => "setup",
            Self::Status => "status",
            Self::Version => "version",
            Self::Auth => "auth",
            Self::Json => "json",
            Self::Read => "read",
            Self::ReadLogs => "read_logs",
            Self::ReadIssues => "read_issues",
            Self::ReadActions => "read_actions",
            Self::ReadReleases => "read_releases",
            Self::ReadTrace => "read_trace",
            Self::ReadIssue => "read_issue",
            Self::Watch => "watch",
            Self::Explain => "explain",
            Self::Set => "set",
        }
    }
}

/// Historical data target for `read`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadTarget {
    /// Structured logs.
    Logs,
    /// Grouped issues.
    Issues,
    /// Product actions.
    Actions,
    /// Release summaries.
    Releases,
    /// One trace by ID.
    Trace(String),
    /// One issue by ID.
    Issue(String),
}

/// Filters for historical read commands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadOptions {
    /// Optional action name filter.
    pub name: Option<String>,
    /// Optional relative or absolute lower time bound.
    pub since: Option<String>,
    /// Optional user or actor filter.
    pub user: Option<String>,
    /// Optional trace ID filter.
    pub trace: Option<String>,
    /// Optional log level filter.
    pub level: Option<String>,
    /// Optional log message substring search.
    pub search: Option<String>,
    /// Optional project filter.
    pub project: Option<String>,
    /// Optional release filter.
    pub release: Option<String>,
    /// Optional environment filter.
    pub environment: Option<String>,
    /// Optional issue status filter.
    pub status: Option<String>,
    /// Optional row limit.
    pub limit: Option<String>,
}

impl ReadOptions {
    /// Returns the first filter that trace-detail reads cannot apply.
    #[must_use]
    pub(crate) fn first_trace_detail_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.name.is_some(), "--name"),
            (self.since.is_some(), "--since"),
            (self.user.is_some(), "--user"),
            (self.trace.is_some(), "--trace"),
            (self.level.is_some(), "--level"),
            (self.search.is_some(), "--search"),
            (self.status.is_some(), "--status"),
            (self.limit.is_some(), "--limit"),
        ])
    }

    /// Returns the first filter that issue-detail reads cannot apply.
    #[must_use]
    pub(crate) fn first_issue_detail_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.name.is_some(), "--name"),
            (self.since.is_some(), "--since"),
            (self.user.is_some(), "--user"),
            (self.trace.is_some(), "--trace"),
            (self.level.is_some(), "--level"),
            (self.search.is_some(), "--search"),
            (self.project.is_some(), "--project"),
            (self.release.is_some(), "--release"),
            (self.environment.is_some(), "--environment"),
            (self.status.is_some(), "--status"),
            (self.limit.is_some(), "--limit"),
        ])
    }

    /// Returns the first filter that log reads cannot apply.
    #[must_use]
    pub(crate) fn first_log_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.name.is_some(), "--name"),
            (self.user.is_some(), "--user"),
            (self.status.is_some(), "--status"),
        ])
    }

    /// Returns the first filter that issue list reads cannot apply.
    #[must_use]
    pub(crate) fn first_issue_list_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.name.is_some(), "--name"),
            (self.since.is_some(), "--since"),
            (self.user.is_some(), "--user"),
            (self.trace.is_some(), "--trace"),
            (self.level.is_some(), "--level"),
            (self.search.is_some(), "--search"),
        ])
    }

    /// Returns the first filter that action reads cannot apply.
    #[must_use]
    pub(crate) fn first_action_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.trace.is_some(), "--trace"),
            (self.level.is_some(), "--level"),
            (self.search.is_some(), "--search"),
            (self.status.is_some(), "--status"),
        ])
    }

    /// Returns the first filter that release reads cannot apply.
    #[must_use]
    pub(crate) fn first_release_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.name.is_some(), "--name"),
            (self.since.is_some(), "--since"),
            (self.user.is_some(), "--user"),
            (self.trace.is_some(), "--trace"),
            (self.level.is_some(), "--level"),
            (self.search.is_some(), "--search"),
            (self.status.is_some(), "--status"),
        ])
    }
}

/// Returns the first present flag in declaration order.
fn first_present_flag<const N: usize>(flags: [(bool, &'static str); N]) -> Option<&'static str> {
    flags
        .iter()
        .find_map(|(present, flag)| present.then_some(*flag))
}

/// Live stream target for `watch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchTarget {
    /// Structured logs.
    Logs,
    /// Product actions.
    Actions,
}

/// Context target for `explain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplainTarget {
    /// One issue by ID.
    Issue(String),
    /// One trace by ID.
    Trace(String),
}

/// Mutation target for `set`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetTarget {
    /// Update one issue status.
    IssueStatus {
        /// Issue identifier.
        id: String,
        /// New issue status.
        status: String,
    },
}

/// Process environment needed by the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliEnvironment {
    /// Base API URL.
    pub base_url: String,
    /// Optional bearer token.
    pub token: Option<String>,
    /// Optional home directory.
    pub home: Option<std::path::PathBuf>,
    /// Optional current working directory.
    pub cwd: Option<std::path::PathBuf>,
}

impl CliEnvironment {
    /// Loads CLI environment from process variables.
    #[must_use]
    pub fn from_process() -> Self {
        Self {
            base_url: std::env::var("LOGBREW_API_URL")
                .unwrap_or_else(|_| String::from("https://api.logbrew.co")),
            token: std::env::var("LOGBREW_TOKEN").ok(),
            home: std::env::var_os("HOME").map(std::path::PathBuf::from),
            cwd: std::env::current_dir().ok(),
        }
    }
}

impl Command {
    /// Returns the HTTP API path for commands backed by a single REST request.
    #[must_use]
    pub fn http_path(&self) -> Option<String> {
        match self {
            Self::Read {
                target, options, ..
            } => Some(read_path(
                target,
                &ReadPathFilters {
                    name: options.name.as_deref(),
                    since: options.since.as_deref(),
                    user: options.user.as_deref(),
                    trace: options.trace.as_deref(),
                    level: options.level.as_deref(),
                    search: options.search.as_deref(),
                    project: options.project.as_deref(),
                    release: options.release.as_deref(),
                    environment: options.environment.as_deref(),
                    status: options.status.as_deref(),
                    limit: options.limit.as_deref(),
                },
            )),
            Self::Explain { target, .. } => Some(explain_path(target)),
            Self::Set { target, .. } => Some(set_path(target)),
            Self::Help { .. }
            | Self::Login { .. }
            | Self::Logout { .. }
            | Self::Setup { .. }
            | Self::Status { .. }
            | Self::Version { .. }
            | Self::Watch { .. } => None,
        }
    }

    /// Returns whether command output should be JSON.
    #[must_use]
    pub const fn wants_json(&self) -> bool {
        match self {
            Self::Help { json, .. }
            | Self::Login { json, .. }
            | Self::Logout { json }
            | Self::Status { json }
            | Self::Version { json }
            | Self::Read { json, .. }
            | Self::Watch { json, .. }
            | Self::Explain { json, .. }
            | Self::Set { json, .. }
            | Self::Setup { json, .. } => *json,
        }
    }

    /// Returns the HTTP method for commands backed by a REST request.
    #[must_use]
    pub const fn http_method(&self) -> Option<HttpMethod> {
        match self {
            Self::Read { .. } | Self::Explain { .. } => Some(HttpMethod::Get),
            Self::Set { .. } => Some(HttpMethod::Patch),
            Self::Help { .. }
            | Self::Login { .. }
            | Self::Logout { .. }
            | Self::Setup { .. }
            | Self::Status { .. }
            | Self::Version { .. }
            | Self::Watch { .. } => None,
        }
    }

    /// Returns JSON request body for mutation commands.
    #[must_use]
    pub fn request_body(&self) -> Option<serde_json::Value> {
        match self {
            Self::Set {
                target: SetTarget::IssueStatus { status, .. },
                ..
            } => Some(serde_json::json!({ "status": status })),
            Self::Help { .. }
            | Self::Login { .. }
            | Self::Logout { .. }
            | Self::Setup { .. }
            | Self::Status { .. }
            | Self::Version { .. }
            | Self::Read { .. }
            | Self::Watch { .. }
            | Self::Explain { .. } => None,
        }
    }
}

/// HTTP method used by a CLI command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// GET request.
    Get,
    /// PATCH request.
    Patch,
}

/// Executes a parsed command.
///
/// # Errors
///
/// Returns [`RuntimeError`] if output, browser launch, auth, or HTTP fails.
pub async fn execute_command<W: std::io::Write>(
    command: &Command,
    env: &CliEnvironment,
    output: &mut W,
) -> Result<(), RuntimeError> {
    match command {
        Command::Help { topic, json } => execute_help(*topic, *json, output),
        Command::Login { open_browser, json } => execute_login(env, *open_browser, *json, output),
        Command::Logout { json } => execute_logout(env, *json, output),
        Command::Setup { auto, yes, json } => execute_setup(env, *auto, *yes, *json, output),
        Command::Status { json } => execute_status(env, *json, output).await,
        Command::Version { json } => execute_version(*json, output),
        Command::Read { .. } | Command::Explain { .. } | Command::Set { .. } => {
            execute_http(command, env, output).await
        }
        Command::Watch { target, .. } => execute_watch_placeholder(*target),
    }
}

/// Emits CLI help.
fn execute_help<W: std::io::Write>(
    topic: HelpTopic,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let help = help::help_text(topic);
    if json {
        let body = serde_json::json!({
            "ok": true,
            "topic": topic.key(),
            "help": help,
        });
        writeln!(output, "{body}")?;
    } else {
        writeln!(output, "{help}")?;
    }
    Ok(())
}

/// Executes browser login bootstrap.
fn execute_login<W: std::io::Write>(
    env: &CliEnvironment,
    should_open_browser: bool,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let auth_url = format!("{}/api/auth/cli/login", env.base_url.trim_end_matches('/'));
    let opened = should_open_browser && open_browser(auth_url.as_str());

    if json {
        let body = serde_json::json!({
            "ok": true,
            "auth_url": auth_url,
            "browser_opened": opened,
            "next": "open auth_url in a browser",
        });
        writeln!(output, "{body}")?;
    } else {
        writeln!(output, "Open this URL to log in: {auth_url}")?;
        writeln!(
            output,
            "Browser: {}",
            if opened { "opened" } else { "not opened" }
        )?;
        writeln!(output, "Next: open the URL in a browser")?;
    }
    Ok(())
}

/// Executes local logout.
fn execute_logout<W: std::io::Write>(
    env: &CliEnvironment,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    write_logout_result(env, json, output)?;
    Ok(())
}

/// Executes setup planning.
fn execute_setup<W: std::io::Write>(
    env: &CliEnvironment,
    auto: bool,
    yes: bool,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    write_setup_plan(env.cwd.as_deref(), auto, yes, json, output)?;
    Ok(())
}

/// Executes commands backed by one HTTP request.
async fn execute_http<W: std::io::Write>(
    command: &Command,
    env: &CliEnvironment,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let path = command.http_path().ok_or(CliError::UnknownCommand)?;
    let url = format!("{}{}", env.base_url.trim_end_matches('/'), path);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()?;

    let mut request = match command.http_method().unwrap_or(HttpMethod::Get) {
        HttpMethod::Get => client.get(url),
        HttpMethod::Patch => client.patch(url),
    };

    let credential = resolve_credential(env)?;
    request = request.bearer_auth(credential.token);

    if let Some(body) = command.request_body() {
        request = request.json(&body);
    }

    let response = request.send().await?;
    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        return Err(RuntimeError::Api {
            status: status.as_u16(),
            body,
            auth_source: credential.source,
            auth_label: credential.label,
        });
    }

    write_api_success(command, body.as_str(), output)?;
    Ok(())
}

/// Returns a stable unavailable error for reserved watch commands.
const fn execute_watch_placeholder(target: WatchTarget) -> Result<(), RuntimeError> {
    Err(RuntimeError::Unavailable {
        message: "watch is reserved for the live stream transport",
        next: watch_next_step(target),
    })
}

/// Returns the historical command fallback while live watch is reserved.
const fn watch_next_step(target: WatchTarget) -> &'static str {
    match target {
        WatchTarget::Logs => "use logbrew logs for historical data until live watch is available",
        WatchTarget::Actions => {
            "use logbrew actions for historical data until live watch is available"
        }
    }
}

/// Read endpoint filter values.
struct ReadPathFilters<'a> {
    /// Optional action name filter.
    name: Option<&'a str>,
    /// Optional lower time bound.
    since: Option<&'a str>,
    /// Optional user or actor filter.
    user: Option<&'a str>,
    /// Optional trace ID filter.
    trace: Option<&'a str>,
    /// Optional log level filter.
    level: Option<&'a str>,
    /// Optional log message substring search.
    search: Option<&'a str>,
    /// Optional project filter.
    project: Option<&'a str>,
    /// Optional release filter.
    release: Option<&'a str>,
    /// Optional environment filter.
    environment: Option<&'a str>,
    /// Optional issue status filter.
    status: Option<&'a str>,
    /// Optional row limit.
    limit: Option<&'a str>,
}

/// Builds a read endpoint path.
fn read_path(target: &ReadTarget, filters: &ReadPathFilters<'_>) -> String {
    match target {
        ReadTarget::Logs => path_with_query(
            "/api/logs",
            &[
                ("level", filters.level),
                ("search", filters.search),
                ("since", filters.since),
                ("trace_id", filters.trace),
                ("project_id", filters.project),
                ("release", filters.release),
                ("environment", filters.environment),
                ("limit", filters.limit),
            ],
        ),
        ReadTarget::Issues => path_with_query(
            "/api/telemetry/issues",
            &[
                ("status", filters.status),
                ("project_id", filters.project),
                ("release", filters.release),
                ("environment", filters.environment),
                ("limit", filters.limit),
            ],
        ),
        ReadTarget::Actions => path_with_query(
            "/api/telemetry/actions",
            &[
                ("name", filters.name),
                ("since", filters.since),
                ("distinct_id", filters.user),
                ("project_id", filters.project),
                ("release", filters.release),
                ("environment", filters.environment),
                ("limit", filters.limit),
            ],
        ),
        ReadTarget::Releases => path_with_query(
            "/api/telemetry/releases",
            &[
                ("project_id", filters.project),
                ("release", filters.release),
                ("environment", filters.environment),
                ("limit", filters.limit),
            ],
        ),
        ReadTarget::Trace(id) => path_with_query(
            &format!("/api/telemetry/traces/{}", encode_component(id)),
            &[
                ("project_id", filters.project),
                ("release", filters.release),
                ("environment", filters.environment),
            ],
        ),
        ReadTarget::Issue(id) => format!("/api/telemetry/issues/{}", encode_component(id)),
    }
}

/// Builds an explain endpoint path.
fn explain_path(target: &ExplainTarget) -> String {
    match target {
        ExplainTarget::Issue(id) => format!("/api/telemetry/issues/{}", encode_component(id)),
        ExplainTarget::Trace(id) => format!("/api/telemetry/traces/{}", encode_component(id)),
    }
}

/// Builds a mutation endpoint path.
fn set_path(target: &SetTarget) -> String {
    match target {
        SetTarget::IssueStatus { id, .. } => {
            format!("/api/telemetry/issues/{}", encode_component(id))
        }
    }
}

/// Builds a path with query parameters.
fn path_with_query(path: &str, params: &[(&str, Option<&str>)]) -> String {
    let query = params
        .iter()
        .filter_map(|(name, value)| value.map(|v| format!("{name}={}", encode_component(v))))
        .collect::<Vec<_>>();

    if query.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{}", query.join("&"))
    }
}

/// Percent-encodes a path or query component without adding a dependency.
fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

/// Converts a nibble to an uppercase hexadecimal digit.
fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'A' + (nibble - 10)),
        _ => '?',
    }
}
