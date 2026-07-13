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

use auth::{AuthCredential, execute_login, send_authenticated_with_refresh, write_logout_result};
pub use error::{CliError, RuntimeError, write_cli_error, write_runtime_error};
use futures_util::StreamExt as _;
pub use parser::parse_command;
use render::write_api_success;
use setup::write_setup_plan;
use status::execute_status;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::{Error as WebSocketError, Message};
use version::execute_version;

/// Initial delay before reconnecting a live watch stream.
const WATCH_RECONNECT_INITIAL_DELAY: std::time::Duration = std::time::Duration::from_secs(1);
/// Maximum delay before reconnecting a live watch stream.
const WATCH_RECONNECT_MAX_DELAY: std::time::Duration = std::time::Duration::from_secs(30);
/// Maximum jitter added to reconnect delays.
const WATCH_RECONNECT_JITTER_MAX_MILLIS: u64 = 250;

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
        /// Live watch filters applied client-side.
        options: WatchOptions,
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
    /// Marks backend-owned project setup as seen.
    ProjectSetupSeen {
        /// Project identifier.
        project_id: String,
        /// Optional setup metadata sent to the backend.
        options: ProjectSetupSeenOptions,
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
    /// First-run examples and common workflows.
    Examples,
    /// Backend-owned project setup and ingest key workflow overview.
    Projects,
    /// Backend-owned account usage and quota workflow overview.
    Usage,
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
    /// Recent trace discovery command.
    ReadTraces,
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
            Self::Examples => "examples",
            Self::Projects => "projects",
            Self::Usage => "usage",
            Self::Read => "read",
            Self::ReadLogs => "read_logs",
            Self::ReadIssues => "read_issues",
            Self::ReadActions => "read_actions",
            Self::ReadReleases => "read_releases",
            Self::ReadTraces => "read_traces",
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
    /// Recent trace summaries.
    Traces,
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
    /// Optional service name filter.
    pub service: Option<String>,
    /// Optional relative or absolute lower time bound.
    pub since: Option<String>,
    /// Optional user or actor filter.
    pub user: Option<String>,
    /// Optional trace ID filter.
    pub trace: Option<String>,
    /// Optional log severity filter.
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
    /// Optional minimum end-to-end trace duration in milliseconds.
    pub min_duration_ms: Option<String>,
    /// Optional pagination mode for endpoints with explicit page envelopes.
    pub pagination: Option<String>,
    /// Optional continuation timestamp.
    pub cursor_time: Option<String>,
    /// Optional continuation identifier.
    pub cursor_id: Option<String>,
}

impl ReadOptions {
    /// Returns the first filter that trace-detail reads cannot apply.
    #[must_use]
    pub(crate) fn first_trace_detail_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.name.is_some(), "--name"),
            (self.service.is_some(), "--service"),
            (self.since.is_some(), "--since"),
            (self.user.is_some(), "--user"),
            (self.trace.is_some(), "--trace"),
            (self.level.is_some(), "--severity"),
            (self.search.is_some(), "--search"),
            (self.status.is_some(), "--status"),
            (self.limit.is_some(), "--limit"),
            (self.min_duration_ms.is_some(), "--min-duration-ms"),
            (self.pagination.is_some(), "--pagination"),
            (self.cursor_time.is_some(), "--cursor-time"),
            (self.cursor_id.is_some(), "--cursor-id"),
        ])
    }

    /// Returns the first filter that issue-detail reads cannot apply.
    #[must_use]
    pub(crate) fn first_issue_detail_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.name.is_some(), "--name"),
            (self.service.is_some(), "--service"),
            (self.since.is_some(), "--since"),
            (self.user.is_some(), "--user"),
            (self.trace.is_some(), "--trace"),
            (self.level.is_some(), "--severity"),
            (self.search.is_some(), "--search"),
            (self.project.is_some(), "--project"),
            (self.release.is_some(), "--release"),
            (self.environment.is_some(), "--environment"),
            (self.status.is_some(), "--status"),
            (self.limit.is_some(), "--limit"),
            (self.min_duration_ms.is_some(), "--min-duration-ms"),
            (self.pagination.is_some(), "--pagination"),
            (self.cursor_time.is_some(), "--cursor-time"),
            (self.cursor_id.is_some(), "--cursor-id"),
        ])
    }

    /// Returns the first filter that log reads cannot apply.
    #[must_use]
    pub(crate) fn first_log_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.name.is_some(), "--name"),
            (self.user.is_some(), "--user"),
            (self.status.is_some(), "--status"),
            (self.min_duration_ms.is_some(), "--min-duration-ms"),
        ])
    }

    /// Returns the first filter that issue list reads cannot apply.
    #[must_use]
    pub(crate) fn first_issue_list_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.name.is_some(), "--name"),
            (self.user.is_some(), "--user"),
            (self.trace.is_some(), "--trace"),
            (self.level.is_some(), "--severity"),
            (self.search.is_some(), "--search"),
            (self.min_duration_ms.is_some(), "--min-duration-ms"),
            (self.pagination.is_some(), "--pagination"),
            (self.cursor_time.is_some(), "--cursor-time"),
            (self.cursor_id.is_some(), "--cursor-id"),
        ])
    }

    /// Returns the first filter that action reads cannot apply.
    #[must_use]
    pub(crate) fn first_action_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.trace.is_some(), "--trace"),
            (self.level.is_some(), "--severity"),
            (self.search.is_some(), "--search"),
            (self.status.is_some(), "--status"),
            (self.min_duration_ms.is_some(), "--min-duration-ms"),
        ])
    }

    /// Returns the first filter that release reads cannot apply.
    #[must_use]
    pub(crate) fn first_release_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.name.is_some(), "--name"),
            (self.user.is_some(), "--user"),
            (self.trace.is_some(), "--trace"),
            (self.level.is_some(), "--severity"),
            (self.search.is_some(), "--search"),
            (self.status.is_some(), "--status"),
            (self.min_duration_ms.is_some(), "--min-duration-ms"),
            (self.pagination.is_some(), "--pagination"),
            (self.cursor_time.is_some(), "--cursor-time"),
            (self.cursor_id.is_some(), "--cursor-id"),
        ])
    }

    /// Returns the first filter that recent trace discovery cannot apply.
    #[must_use]
    pub(crate) fn first_trace_list_unsupported_flag(&self) -> Option<&'static str> {
        first_present_flag([
            (self.name.is_some(), "--name"),
            (self.user.is_some(), "--user"),
            (self.trace.is_some(), "--trace"),
            (self.level.is_some(), "--severity"),
            (self.search.is_some(), "--search"),
            (self.pagination.is_some(), "--pagination"),
            (self.cursor_time.is_some(), "--cursor-time"),
            (self.cursor_id.is_some(), "--cursor-id"),
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
    /// All supported live event types.
    All,
    /// Structured logs.
    Logs,
    /// Grouped issues.
    Issues,
    /// Product actions.
    Actions,
}

/// Client-side filters for live watch commands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchOptions {
    /// Canonical severity filters for logs and issues.
    pub severity: Vec<String>,
}

/// Optional metadata for backend-owned project setup tracking.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSetupSeenOptions {
    /// Runtime or framework observed by setup.
    pub runtime: Option<String>,
    /// Setup source for account-token calls.
    pub source: Option<String>,
    /// Release environment observed by setup.
    pub environment: Option<String>,
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
                    service: options.service.as_deref(),
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
                    min_duration_ms: options.min_duration_ms.as_deref(),
                    pagination: options.pagination.as_deref(),
                    cursor_time: options.cursor_time.as_deref(),
                    cursor_id: options.cursor_id.as_deref(),
                },
            )),
            Self::Explain { target, .. } => Some(explain_path(target)),
            Self::Set { target, .. } => Some(set_path(target)),
            Self::ProjectSetupSeen { project_id, .. } => {
                Some(format!("/api/projects/{project_id}/setup/seen"))
            }
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
            | Self::ProjectSetupSeen { json, .. }
            | Self::Setup { json, .. } => *json,
        }
    }

    /// Returns the HTTP method for commands backed by a REST request.
    #[must_use]
    pub const fn http_method(&self) -> Option<HttpMethod> {
        match self {
            Self::Read { .. } | Self::Explain { .. } => Some(HttpMethod::Get),
            Self::ProjectSetupSeen { .. } => Some(HttpMethod::Post),
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
        self.request_body_for_token(None)
    }

    /// Returns JSON request body for mutation commands with auth-aware defaults.
    #[must_use]
    fn request_body_for_token(&self, token: Option<&str>) -> Option<serde_json::Value> {
        match self {
            Self::Set {
                target: SetTarget::IssueStatus { status, .. },
                ..
            } => Some(serde_json::json!({ "status": status })),
            Self::ProjectSetupSeen { options, .. } => Some(project_setup_seen_body(options, token)),
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
    /// POST request.
    Post,
    /// PATCH request.
    Patch,
}

/// Builds the `setup/seen` request body without local setup state.
fn project_setup_seen_body(
    options: &ProjectSetupSeenOptions,
    token: Option<&str>,
) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    if let Some(runtime) = options.runtime.as_ref() {
        drop(body.insert(
            "runtime".to_owned(),
            serde_json::Value::String(runtime.clone()),
        ));
    }
    if let Some(source) = setup_seen_source(options, token) {
        drop(body.insert("source".to_owned(), serde_json::Value::String(source)));
    }
    if let Some(environment) = options.environment.as_ref() {
        drop(body.insert(
            "environment".to_owned(),
            serde_json::Value::String(environment.clone()),
        ));
    }
    serde_json::Value::Object(body)
}

/// Resolves setup source while preserving ingest-key identity derivation.
fn setup_seen_source(options: &ProjectSetupSeenOptions, token: Option<&str>) -> Option<String> {
    if token_is_project_ingest_key(token) {
        return None;
    }
    Some(options.source.as_deref().unwrap_or("cli").to_owned())
}

/// Returns whether a token has the public project-scoped ingest key prefix.
fn token_is_project_ingest_key(token: Option<&str>) -> bool {
    token.is_some_and(|token| token.trim_start().starts_with("lbw_ingest_"))
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
        Command::Login { open_browser, json } => {
            execute_login(env, *open_browser, *json, output).await
        }
        Command::Logout { json } => execute_logout(env, *json, output),
        Command::Setup { auto, yes, json } => execute_setup(env, *auto, *yes, *json, output),
        Command::Status { json } => execute_status(env, *json, output).await,
        Command::Version { json } => execute_version(*json, output),
        Command::Read { .. }
        | Command::Explain { .. }
        | Command::Set { .. }
        | Command::ProjectSetupSeen { .. } => execute_http(command, env, output).await,
        Command::Watch {
            target,
            options,
            json,
        } => execute_watch(env, *target, options, *json, output).await,
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

    let (response, credential) =
        send_authenticated_with_refresh(&client, env, |client, credential| {
            build_command_request(client, command, url.as_str(), credential)
        })
        .await?;
    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        return Err(RuntimeError::Api {
            status: status.as_u16(),
            body: credential.redact_response_body(body.as_str()),
            auth_source: credential.source(),
            auth_label: credential.label(),
        });
    }

    write_api_success(command, body.as_str(), output)?;
    Ok(())
}

/// Builds one command request with the supplied credential.
fn build_command_request(
    client: &reqwest::Client,
    command: &Command,
    url: &str,
    credential: &AuthCredential,
) -> reqwest::RequestBuilder {
    let mut request = match command.http_method().unwrap_or(HttpMethod::Get) {
        HttpMethod::Get => client.get(url),
        HttpMethod::Post => client.post(url),
        HttpMethod::Patch => client.patch(url),
    }
    .bearer_auth(credential.token());
    if let Some(body) = command.request_body_for_token(Some(credential.token())) {
        request = request.json(&body);
    }
    request
}

/// Executes the public live WebSocket watch flow.
async fn execute_watch<W: std::io::Write>(
    env: &CliEnvironment,
    target: WatchTarget,
    options: &WatchOptions,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if !json {
        return Err(RuntimeError::Unavailable {
            message: "watch streams JSON for agents",
            next: "run logbrew watch --json",
        });
    }

    let mut reconnect_backoff = WatchReconnectBackoff::default();
    loop {
        let ticket = match request_feed_ticket(env).await {
            Ok(ticket) => ticket,
            Err(error) if reconnect_backoff.connected_once() && !runtime_error_is_auth(&error) => {
                tokio::time::sleep(reconnect_backoff.next_delay()).await;
                continue;
            }
            Err(error) => return Err(error),
        };
        let live_url = feed_live_url(env.base_url.as_str(), ticket.as_str())?;
        let (mut websocket, _) = match connect_async(live_url.as_str()).await {
            Ok(connection) => connection,
            Err(error)
                if reconnect_backoff.connected_once() && !websocket_error_is_auth(&error) =>
            {
                tokio::time::sleep(reconnect_backoff.next_delay()).await;
                continue;
            }
            Err(error) => return Err(map_websocket_connect_error(error)),
        };
        reconnect_backoff.mark_connected();

        let mut emitted_before_disconnect = false;
        loop {
            let Some(message) = websocket.next().await else {
                break;
            };
            let message = match message {
                Ok(message) => message,
                Err(error) if websocket_error_is_auth(&error) => {
                    return Err(map_websocket_stream_error(error));
                }
                Err(_) => break,
            };
            match message {
                Message::Text(text) => {
                    let event = parse_live_event(text.as_str())?;
                    if watch_event_matches(target, options, &event) {
                        writeln!(output, "{event}")?;
                    }
                    emitted_before_disconnect = true;
                }
                Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
                Message::Close(_) => return Ok(()),
            }
        }
        if emitted_before_disconnect {
            reconnect_backoff.reset();
        }
        tokio::time::sleep(reconnect_backoff.next_delay()).await;
    }
}

/// Reconnect state for long-running live watch streams.
#[derive(Debug, Default)]
struct WatchReconnectBackoff {
    /// Whether a live WebSocket connection has ever been established.
    connected_once: bool,
    /// Consecutive reconnect attempts since the last stable event.
    attempts: u32,
}

impl WatchReconnectBackoff {
    /// Returns whether the stream has connected at least once.
    const fn connected_once(&self) -> bool {
        self.connected_once
    }

    /// Records a successful WebSocket connection.
    const fn mark_connected(&mut self) {
        self.connected_once = true;
    }

    /// Resets retry delay after a stream successfully emits data.
    const fn reset(&mut self) {
        self.attempts = 0;
    }

    /// Returns the next capped exponential reconnect delay.
    fn next_delay(&mut self) -> std::time::Duration {
        let exponent = self.attempts.min(5);
        let multiplier = 1_u64 << exponent;
        self.attempts = self.attempts.saturating_add(1);
        let base = WATCH_RECONNECT_INITIAL_DELAY
            .as_secs()
            .saturating_mul(multiplier)
            .min(WATCH_RECONNECT_MAX_DELAY.as_secs());
        let delay = std::time::Duration::from_secs(base) + watch_reconnect_jitter();
        if delay > WATCH_RECONNECT_MAX_DELAY {
            WATCH_RECONNECT_MAX_DELAY
        } else {
            delay
        }
    }
}

/// Returns small jitter for reconnect delays without adding a random dependency.
fn watch_reconnect_jitter() -> std::time::Duration {
    let Ok(elapsed) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return std::time::Duration::ZERO;
    };
    std::time::Duration::from_millis(
        u64::from(elapsed.subsec_millis()) % WATCH_RECONNECT_JITTER_MAX_MILLIS,
    )
}

/// Returns whether a runtime error should stop watch reconnect attempts.
const fn runtime_error_is_auth(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::MissingToken | RuntimeError::Api { status: 401, .. }
    )
}

/// Returns whether a WebSocket error is an auth failure.
fn websocket_error_is_auth(error: &WebSocketError) -> bool {
    matches!(error, WebSocketError::Http(response) if response.status().as_u16() == 401)
}

/// Requests a short-lived WebSocket feed ticket from the public API.
async fn request_feed_ticket(env: &CliEnvironment) -> Result<String, RuntimeError> {
    let url = format!("{}/api/feed/ticket", env.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()?;
    let (response, credential) =
        send_authenticated_with_refresh(&client, env, |client, credential| {
            client.post(url.as_str()).bearer_auth(credential.token())
        })
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(RuntimeError::Api {
            status: status.as_u16(),
            body: credential.redact_response_body(body.as_str()),
            auth_source: credential.source(),
            auth_label: credential.label(),
        });
    }

    let value = serde_json::from_str::<serde_json::Value>(body.as_str()).map_err(|_| {
        RuntimeError::Unavailable {
            message: "feed ticket response was not valid JSON",
            next: "retry logbrew watch or run logbrew status",
        }
    })?;
    value
        .get("ticket")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|ticket| !ticket.is_empty())
        .map(ToOwned::to_owned)
        .ok_or(RuntimeError::Unavailable {
            message: "feed ticket response did not include a ticket",
            next: "retry logbrew watch or run logbrew status",
        })
}

/// Builds the WebSocket live feed URL without exposing the opaque ticket elsewhere.
fn feed_live_url(base_url: &str, ticket: &str) -> Result<String, RuntimeError> {
    let trimmed = base_url.trim_end_matches('/');
    let (scheme, rest) = websocket_base_parts(trimmed).ok_or(RuntimeError::Unavailable {
        message: "LOGBREW_API_URL must start with http:// or https://",
        next: "check LOGBREW_API_URL or run logbrew status",
    })?;
    Ok(format!(
        "{scheme}://{rest}/api/feed/live?ticket={}",
        encode_component(ticket)
    ))
}

/// Converts an HTTP API base URL into WebSocket scheme and authority/path base parts.
fn websocket_base_parts(base_url: &str) -> Option<(&'static str, &str)> {
    base_url
        .strip_prefix("https://")
        .map(|rest| ("wss", rest))
        .or_else(|| base_url.strip_prefix("http://").map(|rest| ("ws", rest)))
}

/// Parses one backend live event object.
fn parse_live_event(text: &str) -> Result<serde_json::Value, RuntimeError> {
    serde_json::from_str::<serde_json::Value>(text).map_err(|_| RuntimeError::Unavailable {
        message: "live watch event was not valid JSON",
        next: "retry logbrew watch or check LOGBREW_API_URL",
    })
}

/// Returns whether an event should be emitted for the requested watch target and filters.
fn watch_event_matches(
    target: WatchTarget,
    options: &WatchOptions,
    event: &serde_json::Value,
) -> bool {
    target_matches_event(target, event) && severity_matches(options, event)
}

/// Returns whether the event type belongs to the selected target.
fn target_matches_event(target: WatchTarget, event: &serde_json::Value) -> bool {
    let event_type = event
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    match target {
        WatchTarget::All => true,
        WatchTarget::Logs => event_type == "native_log",
        WatchTarget::Issues => event_type == "native_issue",
        WatchTarget::Actions => event_type == "native_action",
    }
}

/// Applies client-side severity filters to log and issue events.
fn severity_matches(options: &WatchOptions, event: &serde_json::Value) -> bool {
    if options.severity.is_empty() {
        return true;
    }
    let Some(severity) = event
        .get("data")
        .and_then(|data| data.get("severity").or_else(|| data.get("level")))
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    options
        .severity
        .iter()
        .any(|allowed| allowed.as_str() == severity)
}

/// Maps a WebSocket connection failure to a token-safe runtime error.
fn map_websocket_connect_error(error: WebSocketError) -> RuntimeError {
    match error {
        WebSocketError::Http(response) if response.status().as_u16() == 401 => {
            RuntimeError::Unavailable {
                message: "live watch ticket was rejected",
                next: "run logbrew login",
            }
        }
        WebSocketError::Http(_) => RuntimeError::Unavailable {
            message: "live watch websocket upgrade failed",
            next: "retry logbrew watch or check LOGBREW_API_URL",
        },
        WebSocketError::ConnectionClosed
        | WebSocketError::AlreadyClosed
        | WebSocketError::Io(_)
        | WebSocketError::Tls(_)
        | WebSocketError::Capacity(_)
        | WebSocketError::Protocol(_)
        | WebSocketError::WriteBufferFull(_)
        | WebSocketError::Utf8(_)
        | WebSocketError::AttackAttempt
        | WebSocketError::Url(_)
        | WebSocketError::HttpFormat(_) => RuntimeError::Unavailable {
            message: "live watch websocket failed",
            next: "retry logbrew watch or check LOGBREW_API_URL",
        },
    }
}

/// Maps an established WebSocket stream failure to a token-safe runtime error.
fn map_websocket_stream_error(error: WebSocketError) -> RuntimeError {
    match error {
        WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed => {
            RuntimeError::Unavailable {
                message: "live watch websocket closed",
                next: "retry logbrew watch",
            }
        }
        WebSocketError::Http(response) if response.status().as_u16() == 401 => {
            RuntimeError::Unavailable {
                message: "live watch ticket was rejected",
                next: "run logbrew login",
            }
        }
        WebSocketError::Http(_)
        | WebSocketError::Io(_)
        | WebSocketError::Tls(_)
        | WebSocketError::Capacity(_)
        | WebSocketError::Protocol(_)
        | WebSocketError::WriteBufferFull(_)
        | WebSocketError::Utf8(_)
        | WebSocketError::AttackAttempt
        | WebSocketError::Url(_)
        | WebSocketError::HttpFormat(_) => RuntimeError::Unavailable {
            message: "live watch websocket failed",
            next: "retry logbrew watch or check LOGBREW_API_URL",
        },
    }
}

/// Read endpoint filter values.
struct ReadPathFilters<'a> {
    /// Optional action name filter.
    name: Option<&'a str>,
    /// Optional service name filter.
    service: Option<&'a str>,
    /// Optional lower time bound.
    since: Option<&'a str>,
    /// Optional user or actor filter.
    user: Option<&'a str>,
    /// Optional trace ID filter.
    trace: Option<&'a str>,
    /// Optional log severity filter.
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
    /// Optional minimum end-to-end trace duration in milliseconds.
    min_duration_ms: Option<&'a str>,
    /// Optional pagination mode.
    pagination: Option<&'a str>,
    /// Optional continuation timestamp.
    cursor_time: Option<&'a str>,
    /// Optional continuation identifier.
    cursor_id: Option<&'a str>,
}

/// Builds a read endpoint path.
fn read_path(target: &ReadTarget, filters: &ReadPathFilters<'_>) -> String {
    match target {
        ReadTarget::Logs => path_with_query(
            "/api/logs",
            &[
                ("service_name", filters.service),
                ("severity", filters.level),
                ("search", filters.search),
                ("since", filters.since),
                ("trace_id", filters.trace),
                ("project_id", filters.project),
                ("release", filters.release),
                ("environment", filters.environment),
                ("pagination", filters.pagination),
                ("cursor_time", filters.cursor_time),
                ("cursor_id", filters.cursor_id),
                ("limit", filters.limit),
            ],
        ),
        ReadTarget::Issues => path_with_query(
            "/api/telemetry/issues",
            &[
                ("service_name", filters.service),
                ("since", filters.since),
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
                ("service_name", filters.service),
                ("name", filters.name),
                ("since", filters.since),
                ("distinct_id", filters.user),
                ("project_id", filters.project),
                ("release", filters.release),
                ("environment", filters.environment),
                ("pagination", filters.pagination),
                ("cursor_time", filters.cursor_time),
                ("cursor_id", filters.cursor_id),
                ("limit", filters.limit),
            ],
        ),
        ReadTarget::Releases => path_with_query(
            "/api/telemetry/releases",
            &[
                ("service_name", filters.service),
                ("since", filters.since),
                ("project_id", filters.project),
                ("release", filters.release),
                ("environment", filters.environment),
                ("limit", filters.limit),
            ],
        ),
        ReadTarget::Traces => path_with_query(
            "/api/telemetry/traces",
            &[
                ("project_id", filters.project),
                ("service_name", filters.service),
                ("release", filters.release),
                ("environment", filters.environment),
                ("status", filters.status),
                ("since", filters.since),
                ("min_duration_ms", filters.min_duration_ms),
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
