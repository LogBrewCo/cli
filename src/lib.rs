//! Native `LogBrew` command-line interface library.
//!
//! The CLI is intentionally small and predictable so coding agents can learn it
//! quickly: `read`, `watch`, `explain`, and `set` cover data access and state
//! changes, while `login`, `setup`, and `status` cover local configuration.

#![forbid(unsafe_code)]

mod analytics;
mod analytics_contract;
mod analytics_funnel;
mod analytics_lifecycle;
mod analytics_overview;
mod analytics_properties;
mod analytics_property_contract;
mod analytics_request;
mod analytics_retention;
mod analytics_segments;
#[doc(hidden)]
pub mod auth;
#[doc(hidden)]
pub mod auth_namespace;
mod deployment;
#[doc(hidden)]
pub mod doctor;
mod error;
mod explain;
#[doc(hidden)]
pub mod flags;
pub mod help;
mod http;
#[doc(hidden)]
pub mod ids;
#[doc(hidden)]
pub mod investigate;
mod native_debug_artifacts;
mod parser;
mod project_archive;
mod project_create;
mod projects;
#[doc(hidden)]
pub mod render;
mod repositories;
#[doc(hidden)]
pub mod setup;
#[doc(hidden)]
pub mod status;
mod support;
mod time;
mod usage;
#[doc(hidden)]
pub mod version;

use auth::{
    AuthCredential, execute_login, execute_logout, execute_whoami, send_authenticated_with_refresh,
    token_is_project_ingest_key,
};
pub use error::{
    CliError, RuntimeError, write_cli_error, write_native_debug_runtime_error, write_runtime_error,
};
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

/// Runs the installed CLI process while keeping the binary target minimal.
#[doc(hidden)]
pub async fn run_process() -> std::process::ExitCode {
    let args = std::env::args().collect::<Vec<_>>();
    let wants_json = args.iter().any(|arg| arg == "--json");
    let native_debug_json = wants_json
        && args
            .get(1..)
            .and_then(|args| args.iter().find(|arg| arg.as_str() != "--json"))
            .is_some_and(|command| command == "debug-artifacts");
    let command = match parse_command(args) {
        Ok(command) => command,
        Err(error) => {
            let (mut stdout, mut stderr) = (std::io::stdout(), std::io::stderr());
            let _result = if native_debug_json {
                write_cli_error(&error, true, &mut stdout)
            } else {
                write_cli_error(&error, wants_json, &mut stderr)
            };
            return std::process::ExitCode::from(2);
        }
    };

    let env = CliEnvironment::from_process();
    let mut stdout = std::io::stdout();
    if let Err(error) = execute_command(&command, &env, &mut stdout).await {
        if matches!(command, Command::NativeDebugArtifacts { json: true, .. }) {
            let _result = write_native_debug_runtime_error(&error, &mut stdout);
        } else {
            let _result = write_runtime_error(&error, command.wants_json(), &mut std::io::stderr());
        }
        return std::process::ExitCode::from(1);
    }
    std::process::ExitCode::SUCCESS
}

/// Accepted issue status values for generic recovery text.
pub(crate) const ISSUE_STATUS_VALUES_NEXT_STEP: &str =
    "use one of unresolved/open, resolved/closed, ignored";
/// Accepted issue status values for read filter recovery text.
pub(crate) const ISSUE_STATUS_FILTER_NEXT_STEP: &str =
    "use --status unresolved/open, --status resolved/closed, or --status ignored";
/// Accepted issue status values for missing mutation arguments.
pub(crate) const ISSUE_STATUS_ARGUMENT_NEXT_STEP: &str =
    "provide one of unresolved/open, resolved/closed, ignored";

/// OAuth provider used for native CLI browser login.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LoginProvider {
    /// GitHub OAuth.
    #[default]
    GitHub,
    /// GitLab OAuth.
    GitLab,
    /// Bitbucket OAuth.
    Bitbucket,
}

impl LoginProvider {
    /// Returns the canonical provider slug used by the public auth API.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::GitLab => "gitlab",
            Self::Bitbucket => "bitbucket",
        }
    }
}

impl std::str::FromStr for LoginProvider {
    type Err = CliError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "github" => Ok(Self::GitHub),
            "gitlab" => Ok(Self::GitLab),
            "bitbucket" => Ok(Self::Bitbucket),
            _ => Err(CliError::InvalidLoginProvider),
        }
    }
}

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
        /// OAuth provider to use for browser authentication.
        provider: LoginProvider,
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
    /// Returns the authenticated account identity.
    WhoAmI {
        /// Emit the exact validated server identity object.
        json: bool,
    },
    /// Checks one project through a bounded read-only diagnostic sequence.
    Doctor {
        /// Account-owned project UUID.
        project_id: String,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Creates one project and securely persists its one-time ingest key.
    ProjectCreate {
        /// Normalized project creation fields and local persistence choice.
        options: ProjectCreateOptions,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Creates one ingest key for an existing project and persists it securely.
    ProjectIngestKeyCreate {
        /// Normalized project, credential, and local persistence choices.
        options: ProjectIngestKeyCreateOptions,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Archives one active account-owned project after explicit confirmation.
    ProjectArchive {
        /// Canonical lowercase project UUID.
        project_id: String,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Permanently deletes one project after exact identifier confirmation.
    ProjectDeletion {
        /// Canonical lowercase project UUID.
        project_id: String,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Lists repository candidates or discovers components for one candidate.
    ProjectRepositories {
        /// Read-only repository setup operation.
        target: RepositorySetupTarget,
        /// Emit the exact validated response as JSON.
        json: bool,
    },
    /// Lists active account-owned projects.
    Projects {
        /// Emit the exact validated bare server array.
        json: bool,
    },
    /// Reads authenticated account usage and configured limits.
    Usage {
        /// Emit the exact validated server object.
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
    /// Fetches bounded investigation context for one telemetry subject.
    Explain {
        /// Resource to explain.
        target: ExplainTarget,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Records one completed deployment boundary for release investigation.
    Deploy {
        /// Exact normalized deployment identity and completed boundary.
        options: DeploymentRecordOptions,
        /// Emit the exact validated server response.
        json: bool,
    },
    /// Summarizes bounded product activity and capture quality for one project.
    AnalyticsOverview {
        /// Normalized privacy-safe overview query.
        options: AnalyticsOverviewOptions,
        /// Emit the exact validated server response.
        json: bool,
    },
    /// Discovers bounded safe property keys and aggregate capture coverage.
    AnalyticsProperties {
        /// Normalized privacy-safe property-catalog query.
        options: AnalyticsPropertyOptions,
        /// Emit the exact validated server response.
        json: bool,
    },
    /// Compares one exact product outcome across named context segments.
    AnalyticsCompare {
        /// Normalized privacy-safe segment-comparison query.
        options: AnalyticsSegmentComparisonOptions,
        /// Emit the exact validated server response.
        json: bool,
    },
    /// Explores bounded aggregate product journeys around one exact event.
    AnalyticsPaths {
        /// Normalized privacy-safe path query.
        options: AnalyticsPathOptions,
        /// Emit the exact validated server response.
        json: bool,
    },
    /// Measures ordered conversion and drop-off across exact product events.
    AnalyticsFunnel {
        /// Normalized privacy-safe funnel query.
        options: AnalyticsFunnelOptions,
        /// Emit the exact validated server response.
        json: bool,
    },
    /// Measures maturity-aware typed-user retention between two exact product events.
    AnalyticsRetention {
        /// Normalized privacy-safe retention query.
        options: AnalyticsRetentionOptions,
        /// Emit the exact validated server response.
        json: bool,
    },
    /// Classifies bounded identified-user lifecycle state for one exact product event.
    AnalyticsLifecycle {
        /// Normalized privacy-safe lifecycle query.
        options: AnalyticsLifecycleOptions,
        /// Emit the exact validated server response.
        json: bool,
    },
    /// Follows the backend-directed, read-only investigation for one issue.
    InvestigateIssue {
        /// Grouped issue identifier.
        issue_id: String,
        /// Retained occurrence selected for detailed evidence.
        occurrence: IssueOccurrenceSelection,
        /// Emit machine-readable JSON.
        json: bool,
    },
    /// Uploads or verifies Apple native debug artifacts.
    NativeDebugArtifacts {
        /// Native debug-artifact operation.
        target: NativeDebugArtifactsTarget,
        /// Emit bounded machine-readable JSON.
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
    /// Creates or reads account support tickets.
    Support {
        /// Support-ticket operation.
        target: SupportTarget,
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
    /// Completed deployment capture command.
    Deploy,
    /// Product-analytics command overview.
    Analytics,
    /// Product-analytics project overview command.
    AnalyticsOverview,
    /// Product-analytics property catalog command.
    AnalyticsProperties,
    /// Product-analytics segment comparison command.
    AnalyticsCompare,
    /// Product-analytics path exploration command.
    AnalyticsPaths,
    /// Product-analytics ordered funnel command.
    AnalyticsFunnel,
    /// Product-analytics retention command.
    AnalyticsRetention,
    /// Product-analytics lifecycle command.
    AnalyticsLifecycle,
    /// Server-directed issue investigation command.
    Investigate,
    /// Apple native debug-artifact upload and lookup commands.
    NativeDebugArtifacts,
    /// State mutation command.
    Set,
    /// Support-ticket workflow.
    Support,
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
            Self::Deploy => "deploy",
            Self::Analytics => "analytics",
            Self::AnalyticsOverview => "analytics_overview",
            Self::AnalyticsProperties => "analytics_properties",
            Self::AnalyticsCompare => "analytics_compare",
            Self::AnalyticsPaths => "analytics_paths",
            Self::AnalyticsFunnel => "analytics_funnel",
            Self::AnalyticsRetention => "analytics_retention",
            Self::AnalyticsLifecycle => "analytics_lifecycle",
            Self::Investigate => "investigate",
            Self::NativeDebugArtifacts => "debug_artifacts",
            Self::Set => "set",
            Self::Support => "support",
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

/// Fields accepted by secure project creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectCreateOptions {
    /// Trimmed project name.
    pub name: String,
    /// Optional trimmed runtime.
    pub runtime: Option<String>,
    /// Optional trimmed environment.
    pub environment: Option<String>,
    /// Optional repository and component selection.
    pub repository: Option<ProjectRepositoryOptions>,
    /// Owner-selected destination for the one-time ingest key.
    pub ingest_key_file: String,
    /// Explicitly discard a mismatched pending retry before creating.
    pub abandon_retry: bool,
}

/// Provider repository selected for project creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRepositoryOptions {
    /// Source-control provider.
    pub provider: LoginProvider,
    /// Provider-owned repository identifier.
    pub id: String,
    /// Optional backend-issued component discovery snapshot.
    pub discovery_id: Option<String>,
    /// Selected snapshot-scoped component identifiers.
    pub component_ids: Vec<String>,
}

/// Read-only repository setup operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositorySetupTarget {
    /// Lists provider states and repositories not yet represented by a project.
    Catalog,
    /// Discovers bounded components for one selected repository.
    Discover(RepositoryDiscoveryOptions),
}

/// Exact repository component-discovery request fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryDiscoveryOptions {
    /// Source-control provider.
    pub provider: LoginProvider,
    /// Provider-owned repository identifier.
    pub repository_id: String,
}

/// Fields accepted by secure existing-project ingest-key creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectIngestKeyCreateOptions {
    /// Canonical lowercase account-owned project UUID.
    pub project_id: String,
    /// Trimmed operator-visible key label.
    pub label: String,
    /// Canonical credential kind.
    pub kind: String,
    /// Owner-selected destination for the one-time ingest key.
    pub ingest_key_file: String,
    /// Explicitly discard a mismatched pending retry before creating.
    pub abandon_retry: bool,
}

/// Apple native debug-artifact operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeDebugArtifactsTarget {
    /// Validate, upload, and verify every supported object identity.
    Upload(NativeDebugUploadOptions),
    /// Verify one exact uploaded object identity.
    Lookup(NativeDebugLookupOptions),
}

/// Shared exact native debug-artifact lookup scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDebugLookupOptions {
    /// Account-owned project UUID.
    pub project_id: String,
    /// Exact release identifier.
    pub release: String,
    /// Exact environment identifier.
    pub environment: String,
    /// Exact service identifier.
    pub service: String,
    /// Canonical lowercase Mach-O image UUID.
    pub image_uuid: String,
    /// Supported canonical architecture.
    pub architecture: String,
}

/// Apple native debug-artifact upload options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDebugUploadOptions {
    /// User-selected dSYM bundle, ZIP archive, or Mach-O debug object.
    pub path: String,
    /// Account-owned project UUID.
    pub project_id: String,
    /// Exact release identifier.
    pub release: String,
    /// Exact environment identifier.
    pub environment: String,
    /// Exact service identifier.
    pub service: String,
    /// Optional exact canonical image UUID gate for release automation.
    pub expected_image_uuids: Vec<String>,
    /// Validate locally without authentication or network mutation.
    pub dry_run: bool,
}

/// Support-ticket operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupportTarget {
    /// Create one support ticket.
    Create(Box<SupportTicketCreateOptions>),
    /// List support-ticket history.
    List(Box<SupportTicketListOptions>),
    /// Read one support ticket by public identifier.
    Detail(String),
    /// Read public context history for one support ticket.
    ContextHistory {
        /// Public ticket identifier.
        ticket_id: String,
    },
    /// Add requested context to one support ticket.
    ReplyContext(Box<SupportContextReplyOptions>),
    /// Update one support ticket's public lifecycle status.
    UpdateStatus {
        /// Public ticket identifier.
        ticket_id: String,
        /// User-owned lifecycle status.
        status: SupportTicketLifecycleStatus,
    },
}

/// Fields accepted when replying with requested support context.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupportContextReplyOptions {
    /// Public ticket identifier.
    pub ticket_id: String,
    /// User-provided support context.
    pub context: String,
    /// Required idempotency key used for exact retries.
    pub retry_key: String,
    /// Include bounded locally generated diagnostics.
    pub diagnostics: bool,
}

/// User-owned support-ticket lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportTicketLifecycleStatus {
    /// Reopen a ticket.
    Open,
    /// Close a ticket.
    Closed,
}

impl SupportTicketLifecycleStatus {
    /// Returns the canonical API status value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

/// Fields accepted when creating a support ticket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupportTicketCreateOptions {
    /// Public support category.
    pub category: String,
    /// Concise ticket title.
    pub title: String,
    /// Reproducible description supplied by the user.
    pub description: String,
    /// Optional project identifier.
    pub project_id: Option<String>,
    /// Optional deployment environment.
    pub environment: Option<String>,
    /// Optional runtime.
    pub runtime: Option<String>,
    /// Optional framework.
    pub framework: Option<String>,
    /// Optional SDK package.
    pub sdk_package: Option<String>,
    /// Optional SDK version.
    pub sdk_version: Option<String>,
    /// Optional release.
    pub release: Option<String>,
    /// Optional trace identifier.
    pub trace_id: Option<String>,
    /// Optional event identifier.
    pub event_id: Option<String>,
    /// Include bounded locally generated diagnostics.
    pub diagnostics: bool,
}

/// Filters for support-ticket history.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupportTicketListOptions {
    /// Optional project identifier.
    pub project_id: Option<String>,
    /// Optional ticket status.
    pub status: Option<String>,
    /// Optional ticket source.
    pub source: Option<String>,
    /// Optional support category.
    pub category: Option<String>,
    /// Optional release.
    pub release: Option<String>,
    /// Optional result limit.
    pub limit: Option<String>,
    /// Optional explicit pagination mode.
    pub pagination: Option<String>,
    /// Optional continuation timestamp.
    pub cursor_time: Option<String>,
    /// Optional continuation identifier.
    pub cursor_id: Option<String>,
}

/// Context target for `explain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplainTarget {
    /// One issue by ID.
    Issue {
        /// Grouped issue identifier.
        id: String,
        /// Retained occurrence selected for detailed evidence.
        occurrence: IssueOccurrenceSelection,
    },
    /// Evidence-only verification of one candidate issue correction.
    IssueCorrection(IssueCorrectionTarget),
    /// One structured log by ID.
    Log(String),
    /// One product action by ID.
    Action(String),
    /// One trace by ID.
    Trace(String),
    /// One exact span within an exact trace and deployment scope.
    Span(ExplainSpanTarget),
    /// One exact service release.
    Release(ExplainReleaseTarget),
    /// One bounded metric time series.
    Metric(ExplainMetricTarget),
}

/// Exact identity required by an exact-span investigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainSpanTarget {
    /// Non-zero W3C trace identifier.
    pub trace_id: String,
    /// Non-zero W3C span identifier.
    pub span_id: String,
    /// Owned project identifier.
    pub project_id: String,
    /// Exact deployment environment.
    pub environment: String,
    /// Exact application release.
    pub release: String,
}

/// Retained issue occurrence selected for investigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueOccurrenceSelection {
    /// Backend-recommended context-rich recent occurrence.
    Recommended,
    /// Earliest retained occurrence.
    First,
    /// Latest retained occurrence.
    Latest,
    /// One exact retained occurrence UUID.
    Exact(String),
}

/// Exact identities required to verify one candidate issue correction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueCorrectionTarget {
    /// Grouped issue identifier.
    pub issue_id: String,
    /// Retained failing occurrence selected from issue investigation.
    pub baseline_occurrence_id: String,
    /// Successful candidate deployment recorded by the caller.
    pub candidate_deployment_id: String,
}

/// Exact identity required by a release investigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainReleaseTarget {
    /// Owned project identifier.
    pub project_id: String,
    /// Exact application release.
    pub release: String,
    /// Exact deployment environment.
    pub environment: String,
    /// Exact logical service name.
    pub service_name: String,
}

/// Terminal result of one completed deployment attempt.
#[derive(Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    /// The deployment completed successfully.
    Succeeded,
    /// The deployment attempt completed unsuccessfully.
    Failed,
}

impl DeploymentStatus {
    /// Returns the canonical public wire value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

/// Exact completed deployment boundary sent by `logbrew deploy`.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct DeploymentRecordOptions {
    /// Caller-owned deployment identity used for idempotent retries.
    #[serde(skip_serializing)]
    pub deployment_id: String,
    /// Active account-owned project identifier.
    pub project_id: String,
    /// Exact application release used by runtime telemetry.
    pub release: String,
    /// Exact deployment environment used by runtime telemetry.
    pub environment: String,
    /// Exact logical service name used by runtime telemetry.
    pub service_name: String,
    /// Terminal deployment result.
    pub status: DeploymentStatus,
    /// RFC3339 deployment attempt start.
    pub started_at: String,
    /// RFC3339 deployment attempt finish.
    pub finished_at: String,
    /// Optional abbreviated or full source commit identity.
    pub commit_sha: Option<String>,
}

/// Bounded query required by a metric investigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainMetricTarget {
    /// Owned project identifier.
    pub project_id: String,
    /// Exact metric name.
    pub name: String,
    /// Inclusive compact duration or RFC 3339 lower bound.
    pub since: String,
    /// Optional exclusive RFC 3339 upper bound.
    pub until: Option<String>,
    /// Optional fixed or automatic bucket interval.
    pub interval: Option<String>,
    /// Optional fixed low-cardinality grouping dimension.
    pub group_by: Option<String>,
    /// Optional exact service filter.
    pub service_name: Option<String>,
    /// Optional exact release filter.
    pub release: Option<String>,
    /// Optional exact environment filter.
    pub environment: Option<String>,
    /// Optional maximum number of returned series.
    pub series_limit: Option<u8>,
}

/// Exact, bounded product-analytics overview request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsOverviewOptions {
    /// Account-owned project UUID.
    pub project_id: String,
    /// Inclusive compact duration or RFC 3339 lower bound.
    pub since: String,
    /// Optional exclusive RFC 3339 upper bound.
    pub until: Option<String>,
    /// Automatic or fixed activity-series interval.
    pub interval: String,
    /// Optional exact service filter.
    pub service_name: Option<String>,
    /// Optional exact release filter.
    pub release: Option<String>,
    /// Optional exact environment filter.
    pub environment: Option<String>,
    /// Maximum returned action, surface, and exact-event rankings.
    pub top_limit: u8,
}

/// Exact bounded product-analytics property catalog request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsPropertyOptions {
    /// Account-owned project UUID.
    pub project_id: String,
    /// Inclusive compact duration or RFC 3339 lower bound.
    pub since: String,
    /// Optional exclusive RFC 3339 upper bound.
    pub until: Option<String>,
    /// Optional exact service filter.
    pub service_name: Option<String>,
    /// Optional exact release filter.
    pub release: Option<String>,
    /// Optional exact environment filter.
    pub environment: Option<String>,
    /// Maximum safe property descriptors returned.
    pub limit: u8,
}

/// Supported version-1 classified target-event kind used in segment comparisons.
pub type AnalyticsSegmentEventKind = AnalyticsPathEventKind;

/// Identity boundary used to calculate eligibility and reach in segment comparisons.
pub type AnalyticsSegmentUnit = AnalyticsFunnelUnit;

/// One exact privacy-safe property predicate inside an analytics segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnalyticsSegmentPropertyFilter {
    /// Stable standard-context key or `tag.`-prefixed custom key.
    pub key: String,
    /// Exact bounded case-sensitive string value.
    pub value: String,
}

/// One exact privacy-safe property predicate applied to an analytics path anchor.
pub type AnalyticsPathPropertyFilter = AnalyticsSegmentPropertyFilter;

/// One named exact deployment and property segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsSegment {
    /// Stable machine-safe key unique inside the request.
    pub key: String,
    /// Short human-readable label.
    pub label: String,
    /// Optional exact service filter.
    pub service_name: Option<String>,
    /// Optional exact release filter.
    pub release: Option<String>,
    /// Optional exact environment filter.
    pub environment: Option<String>,
    /// Zero through four exact property predicates combined with logical AND.
    pub property_filters: Vec<AnalyticsSegmentPropertyFilter>,
}

/// Exact, bounded product-analytics segment-comparison request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsSegmentComparisonOptions {
    /// Account-owned project UUID.
    pub project_id: String,
    /// Inclusive compact duration or RFC 3339 lower bound.
    pub since: String,
    /// Optional exclusive RFC 3339 upper bound.
    pub until: Option<String>,
    /// Automatic or fixed comparison-series interval.
    pub interval: String,
    /// Session or typed opaque user counting boundary.
    pub analysis_unit: AnalyticsSegmentUnit,
    /// Exact classified target kind.
    pub target_kind: AnalyticsSegmentEventKind,
    /// Exact route template, screen name, or interaction name.
    pub target_event: String,
    /// Two through four ordered segments; the first is the descriptive baseline.
    pub segments: Vec<AnalyticsSegment>,
}

/// Direction explored from one exact product-event anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsPathDirection {
    /// Show the anchor followed by later named events in the same session.
    Following,
    /// Show earlier named events in chronological order followed by the anchor.
    Preceding,
}

impl AnalyticsPathDirection {
    /// Returns the stable public API token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Following => "following",
            Self::Preceding => "preceding",
        }
    }
}

/// Supported version-1 classified event kind used in product paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsPathEventKind {
    /// Browser route view.
    PageView,
    /// Application screen view.
    ScreenView,
    /// Explicit product interaction.
    Interaction,
}

impl AnalyticsPathEventKind {
    /// Returns the stable public API token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PageView => "page_view",
            Self::ScreenView => "screen_view",
            Self::Interaction => "interaction",
        }
    }
}

/// Exact, bounded product-analytics path request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsPathOptions {
    /// Account-owned project UUID.
    pub project_id: String,
    /// Inclusive compact duration or RFC 3339 lower bound.
    pub since: String,
    /// Optional exclusive RFC 3339 upper bound.
    pub until: Option<String>,
    /// Optional exact service filter.
    pub service_name: Option<String>,
    /// Optional exact release filter.
    pub release: Option<String>,
    /// Optional exact environment filter.
    pub environment: Option<String>,
    /// Direction explored from the anchor.
    pub direction: AnalyticsPathDirection,
    /// Exact classified anchor kind.
    pub anchor_kind: AnalyticsPathEventKind,
    /// Exact route template, screen name, or interaction name.
    pub anchor_event: String,
    /// Zero through four exact predicates applied only to the selected anchor occurrence.
    pub property_filters: Vec<AnalyticsPathPropertyFilter>,
    /// Maximum adjacent named events on the selected side.
    pub depth: u8,
    /// Whether consecutive identical events collapse before anchoring.
    pub collapse_repeated: bool,
    /// Maximum highest-volume aggregate paths returned.
    pub path_limit: u8,
}

/// Supported version-1 classified event kind used in funnel steps.
pub type AnalyticsFunnelEventKind = AnalyticsPathEventKind;

/// Identity boundary used to order and count one funnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsFunnelUnit {
    /// Count separate browser or application sessions.
    Session,
    /// Count explicit application-supplied opaque subject IDs.
    IdentifiedUser,
}

impl AnalyticsFunnelUnit {
    /// Returns the stable public API token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::IdentifiedUser => "identified_user",
        }
    }
}

/// One exact ordered product-analytics funnel step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsFunnelStep {
    /// Supported classified product-event kind.
    pub kind: AnalyticsFunnelEventKind,
    /// Exact route template, screen name, or interaction name.
    pub event_name: String,
}

/// Exact, bounded product-analytics funnel request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsFunnelOptions {
    /// Account-owned project UUID.
    pub project_id: String,
    /// Inclusive compact duration or RFC 3339 lower bound.
    pub since: String,
    /// Optional exclusive RFC 3339 upper bound.
    pub until: Option<String>,
    /// Optional exact service filter.
    pub service_name: Option<String>,
    /// Optional exact release filter.
    pub release: Option<String>,
    /// Optional exact environment filter.
    pub environment: Option<String>,
    /// Session or typed opaque user counting boundary.
    pub analysis_unit: AnalyticsFunnelUnit,
    /// Optional first-to-final step window in seconds.
    pub conversion_window_seconds: Option<u32>,
    /// Two through eight exact ordered event selectors.
    pub steps: Vec<AnalyticsFunnelStep>,
}

/// Supported version-1 classified event kind used in retention queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsRetentionEventKind {
    /// Browser route view.
    PageView,
    /// Application screen view.
    ScreenView,
    /// Explicit product interaction.
    Interaction,
}

impl AnalyticsRetentionEventKind {
    /// Returns the stable public API token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PageView => "page_view",
            Self::ScreenView => "screen_view",
            Self::Interaction => "interaction",
        }
    }
}

/// Fixed subject-relative period width for retention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsRetentionInterval {
    /// One fixed hour.
    Hour,
    /// One fixed 24-hour day.
    Day,
    /// One fixed seven-day week.
    Week,
    /// One fixed 30-day period, not a calendar month.
    ThirtyDay,
}

impl AnalyticsRetentionInterval {
    /// Returns the stable public API token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::ThirtyDay => "thirty_day",
        }
    }

    /// Returns the exact fixed interval width in seconds.
    #[must_use]
    pub const fn seconds(self) -> u64 {
        match self {
            Self::Hour => 3_600,
            Self::Day => 86_400,
            Self::Week => 604_800,
            Self::ThirtyDay => 2_592_000,
        }
    }
}

/// Return qualification semantics for each retention period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsRetentionMode {
    /// Count returns inside each exact subject-relative period.
    ReturnOn,
    /// Count returns from each period threshold through the query upper bound.
    ReturnOnOrAfter,
}

impl AnalyticsRetentionMode {
    /// Returns the stable public API token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReturnOn => "return_on",
            Self::ReturnOnOrAfter => "return_on_or_after",
        }
    }
}

/// Subject cohort-anchor semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsRetentionCohortMode {
    /// Anchor each subject at its first matching start inside the selected range.
    FirstInRange,
}

impl AnalyticsRetentionCohortMode {
    /// Returns the stable public API token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FirstInRange => "first_in_range",
        }
    }
}

/// Exact, bounded product-analytics retention request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsRetentionOptions {
    /// Account-owned project UUID.
    pub project_id: String,
    /// Inclusive compact duration or RFC 3339 lower bound.
    pub since: String,
    /// Optional exclusive RFC 3339 upper bound.
    pub until: Option<String>,
    /// Optional exact service filter.
    pub service_name: Option<String>,
    /// Optional exact release filter.
    pub release: Option<String>,
    /// Optional exact environment filter.
    pub environment: Option<String>,
    /// Exact event that anchors each subject.
    pub start_kind: AnalyticsRetentionEventKind,
    /// Exact start route, screen, or interaction name.
    pub start_event: String,
    /// Exact event that qualifies a return.
    pub return_kind: AnalyticsRetentionEventKind,
    /// Exact return route, screen, or interaction name.
    pub return_event: String,
    /// Fixed period and cohort width.
    pub interval: AnalyticsRetentionInterval,
    /// Number of zero-based periods to evaluate.
    pub interval_count: u8,
    /// Exact-period or rolling retention semantics.
    pub mode: AnalyticsRetentionMode,
    /// First-in-range cohort semantics.
    pub cohort_mode: AnalyticsRetentionCohortMode,
}

/// Supported version-1 classified event kind used in lifecycle queries.
pub type AnalyticsLifecycleEventKind = AnalyticsRetentionEventKind;

/// Fixed period width used in lifecycle queries.
pub type AnalyticsLifecycleInterval = AnalyticsRetentionInterval;

/// Exact, bounded product-analytics lifecycle request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsLifecycleOptions {
    /// Account-owned project UUID.
    pub project_id: String,
    /// Inclusive compact duration or RFC 3339 lower bound.
    pub since: String,
    /// Optional exclusive RFC 3339 upper bound.
    pub until: Option<String>,
    /// Optional exact service filter.
    pub service_name: Option<String>,
    /// Optional exact release filter.
    pub release: Option<String>,
    /// Optional exact environment filter.
    pub environment: Option<String>,
    /// Exact classified event kind.
    pub event_kind: AnalyticsLifecycleEventKind,
    /// Exact route template, screen name, or interaction name.
    pub event_name: String,
    /// Optional fixed period width; omission lets the backend choose by range.
    pub interval: Option<AnalyticsLifecycleInterval>,
    /// Complete fixed periods observed before the analysis lower bound.
    pub history_period_count: u8,
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
            Self::Deploy { options, .. } => Some(format!(
                "/api/telemetry/deployments/{}",
                options.deployment_id
            )),
            Self::AnalyticsOverview { options, .. } => {
                Some(analytics_overview::request_path(options))
            }
            Self::AnalyticsProperties { options, .. } => {
                Some(analytics_properties::request_path(options))
            }
            Self::AnalyticsCompare { .. } => {
                Some(String::from("/api/telemetry/analytics/segments/compare"))
            }
            Self::AnalyticsPaths { .. } => Some(String::from("/api/telemetry/analytics/paths")),
            Self::AnalyticsFunnel { .. } => Some(String::from("/api/telemetry/analytics/funnel")),
            Self::AnalyticsRetention { .. } => {
                Some(String::from("/api/telemetry/analytics/retention"))
            }
            Self::AnalyticsLifecycle { .. } => {
                Some(String::from("/api/telemetry/analytics/lifecycle"))
            }
            Self::Set { target, .. } => Some(set_path(target)),
            Self::ProjectSetupSeen { project_id, .. } => {
                Some(format!("/api/projects/{project_id}/setup/seen"))
            }
            Self::ProjectArchive { project_id, .. } => Some(format!("/api/projects/{project_id}")),
            Self::ProjectDeletion { .. } => Some(String::from("/api/support/tickets")),
            Self::ProjectRepositories { target, .. } => Some(String::from(match target {
                RepositorySetupTarget::Catalog => "/api/projects/repositories",
                RepositorySetupTarget::Discover(_) => {
                    "/api/projects/repositories/components/discover"
                }
            })),
            Self::ProjectIngestKeyCreate { options, .. } => {
                Some(format!("/api/projects/{}/ingest-keys", options.project_id))
            }
            Self::ProjectCreate { .. } | Self::Projects { .. } => {
                Some(String::from("/api/projects"))
            }
            Self::Support { target, .. } => Some(support::path(target)),
            Self::Help { .. }
            | Self::Login { .. }
            | Self::Logout { .. }
            | Self::Setup { .. }
            | Self::Status { .. }
            | Self::WhoAmI { .. }
            | Self::Doctor { .. }
            | Self::Usage { .. }
            | Self::Version { .. }
            | Self::InvestigateIssue { .. }
            | Self::NativeDebugArtifacts { .. }
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
            | Self::WhoAmI { json }
            | Self::Doctor { json, .. }
            | Self::ProjectCreate { json, .. }
            | Self::ProjectIngestKeyCreate { json, .. }
            | Self::ProjectArchive { json, .. }
            | Self::ProjectDeletion { json, .. }
            | Self::ProjectRepositories { json, .. }
            | Self::Projects { json }
            | Self::Usage { json }
            | Self::Version { json }
            | Self::Read { json, .. }
            | Self::Watch { json, .. }
            | Self::Explain { json, .. }
            | Self::Deploy { json, .. }
            | Self::AnalyticsOverview { json, .. }
            | Self::AnalyticsProperties { json, .. }
            | Self::AnalyticsCompare { json, .. }
            | Self::AnalyticsPaths { json, .. }
            | Self::AnalyticsFunnel { json, .. }
            | Self::AnalyticsRetention { json, .. }
            | Self::AnalyticsLifecycle { json, .. }
            | Self::InvestigateIssue { json, .. }
            | Self::NativeDebugArtifacts { json, .. }
            | Self::Set { json, .. }
            | Self::ProjectSetupSeen { json, .. }
            | Self::Support { json, .. }
            | Self::Setup { json, .. } => *json,
        }
    }

    /// Returns the HTTP method for commands backed by a REST request.
    #[must_use]
    pub const fn http_method(&self) -> Option<HttpMethod> {
        match self {
            Self::Deploy { .. } => Some(HttpMethod::Put),
            Self::ProjectCreate { .. }
            | Self::ProjectIngestKeyCreate { .. }
            | Self::ProjectRepositories {
                target: RepositorySetupTarget::Discover(_),
                ..
            }
            | Self::ProjectSetupSeen { .. }
            | Self::AnalyticsPaths { .. }
            | Self::AnalyticsCompare { .. }
            | Self::AnalyticsFunnel { .. }
            | Self::AnalyticsRetention { .. }
            | Self::AnalyticsLifecycle { .. }
            | Self::Support {
                target: SupportTarget::Create(_) | SupportTarget::ReplyContext(_),
                ..
            }
            | Self::ProjectDeletion { .. } => Some(HttpMethod::Post),
            Self::Support {
                target: SupportTarget::UpdateStatus { .. },
                ..
            }
            | Self::Set { .. } => Some(HttpMethod::Patch),
            Self::ProjectArchive { .. } => Some(HttpMethod::Delete),
            Self::Projects { .. }
            | Self::ProjectRepositories {
                target: RepositorySetupTarget::Catalog,
                ..
            }
            | Self::Read { .. }
            | Self::Explain { .. }
            | Self::AnalyticsOverview { .. }
            | Self::AnalyticsProperties { .. }
            | Self::Support { .. } => Some(HttpMethod::Get),
            Self::Help { .. }
            | Self::Login { .. }
            | Self::Logout { .. }
            | Self::Setup { .. }
            | Self::Status { .. }
            | Self::WhoAmI { .. }
            | Self::Doctor { .. }
            | Self::Usage { .. }
            | Self::Version { .. }
            | Self::InvestigateIssue { .. }
            | Self::NativeDebugArtifacts { .. }
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
            Self::Deploy { options, .. } => serde_json::to_value(options).ok(),
            Self::AnalyticsPaths { options, .. } => Some(analytics::request_body(options)),
            Self::AnalyticsCompare { options, .. } => {
                Some(analytics_segments::request_body(options))
            }
            Self::AnalyticsFunnel { options, .. } => Some(analytics_funnel::request_body(options)),
            Self::AnalyticsRetention { options, .. } => {
                Some(analytics_retention::request_body(options))
            }
            Self::AnalyticsLifecycle { options, .. } => {
                Some(analytics_lifecycle::request_body(options))
            }
            Self::ProjectSetupSeen { options, .. } => Some(project_setup_seen_body(options, token)),
            Self::ProjectCreate { options, .. } => Some(project_create_body(options)),
            Self::ProjectRepositories {
                target: RepositorySetupTarget::Discover(options),
                ..
            } => Some(serde_json::json!({
                "provider": options.provider.as_str(),
                "repository_id": options.repository_id,
            })),
            Self::ProjectIngestKeyCreate { options, .. } => {
                Some(project_ingest_key_create_body(options))
            }
            Self::ProjectDeletion { project_id, .. } => {
                Some(project_archive::deletion_body(project_id))
            }
            Self::Support {
                target: SupportTarget::Create(options),
                ..
            } => Some(support::create_body(options)),
            Self::Support {
                target: SupportTarget::ReplyContext(options),
                ..
            } => Some(support::context_body(options)),
            Self::Support {
                target: SupportTarget::UpdateStatus { status, .. },
                ..
            } => Some(serde_json::json!({"status": status.as_str()})),
            Self::Help { .. }
            | Self::Login { .. }
            | Self::Logout { .. }
            | Self::Setup { .. }
            | Self::Status { .. }
            | Self::WhoAmI { .. }
            | Self::Doctor { .. }
            | Self::ProjectArchive { .. }
            | Self::ProjectRepositories {
                target: RepositorySetupTarget::Catalog,
                ..
            }
            | Self::Projects { .. }
            | Self::Usage { .. }
            | Self::Version { .. }
            | Self::Read { .. }
            | Self::Watch { .. }
            | Self::Explain { .. }
            | Self::AnalyticsOverview { .. }
            | Self::AnalyticsProperties { .. }
            | Self::InvestigateIssue { .. }
            | Self::NativeDebugArtifacts { .. }
            | Self::Support { .. } => None,
        }
    }

    /// Returns an idempotency key for support context replies.
    fn idempotency_key(&self) -> Option<&str> {
        match self {
            Self::Support {
                target: SupportTarget::ReplyContext(options),
                ..
            } => Some(options.retry_key.as_str()),
            Self::Help { .. }
            | Self::Login { .. }
            | Self::Logout { .. }
            | Self::Setup { .. }
            | Self::Status { .. }
            | Self::WhoAmI { .. }
            | Self::Doctor { .. }
            | Self::Usage { .. }
            | Self::Version { .. }
            | Self::Read { .. }
            | Self::Watch { .. }
            | Self::Explain { .. }
            | Self::Deploy { .. }
            | Self::AnalyticsOverview { .. }
            | Self::AnalyticsProperties { .. }
            | Self::AnalyticsCompare { .. }
            | Self::AnalyticsPaths { .. }
            | Self::AnalyticsFunnel { .. }
            | Self::AnalyticsRetention { .. }
            | Self::AnalyticsLifecycle { .. }
            | Self::InvestigateIssue { .. }
            | Self::NativeDebugArtifacts { .. }
            | Self::Set { .. }
            | Self::ProjectSetupSeen { .. }
            | Self::ProjectCreate { .. }
            | Self::ProjectIngestKeyCreate { .. }
            | Self::ProjectArchive { .. }
            | Self::ProjectDeletion { .. }
            | Self::ProjectRepositories { .. }
            | Self::Projects { .. }
            | Self::Support { .. } => None,
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
    /// PUT request.
    Put,
    /// PATCH request.
    Patch,
    /// DELETE request.
    Delete,
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

/// Builds the byte-stable project creation request surface.
pub(crate) fn project_create_body(options: &ProjectCreateOptions) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    drop(body.insert(
        "name".to_owned(),
        serde_json::Value::String(options.name.clone()),
    ));
    if let Some(runtime) = options.runtime.as_ref() {
        drop(body.insert(
            "runtime".to_owned(),
            serde_json::Value::String(runtime.clone()),
        ));
    }
    if let Some(environment) = options.environment.as_ref() {
        drop(body.insert(
            "environment".to_owned(),
            serde_json::Value::String(environment.clone()),
        ));
    }
    if let Some(repository) = options.repository.as_ref() {
        drop(body.insert(
            "repository".to_owned(),
            serde_json::json!({
                "provider": repository.provider.as_str(),
                "id": repository.id,
                "discovery_id": repository.discovery_id,
                "component_ids": repository.component_ids,
            }),
        ));
    }
    drop(body.insert(
        "source".to_owned(),
        serde_json::Value::String(String::from("cli")),
    ));
    serde_json::Value::Object(body)
}

/// Builds the byte-stable existing-project ingest-key request surface.
fn project_ingest_key_create_body(options: &ProjectIngestKeyCreateOptions) -> serde_json::Value {
    serde_json::json!({
        "label": options.label,
        "kind": options.kind,
        "expires_at": null,
    })
}

/// Resolves setup source while preserving ingest-key identity derivation.
fn setup_seen_source(options: &ProjectSetupSeenOptions, token: Option<&str>) -> Option<String> {
    if token_is_project_ingest_key(token) {
        return None;
    }
    Some(options.source.as_deref().unwrap_or("cli").to_owned())
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
        Command::Login {
            provider,
            open_browser,
            json,
        } => execute_login(env, *provider, *open_browser, *json, output).await,
        Command::Logout { json } => execute_logout(env, *json, output).await,
        Command::Setup { auto, yes, json } => execute_setup(env, *auto, *yes, *json, output),
        Command::Status { json } => execute_status(env, *json, output).await,
        Command::WhoAmI { json } => execute_whoami(env, *json, output).await,
        Command::Doctor { project_id, json } => {
            doctor::execute(env, project_id.as_str(), *json, output).await
        }
        Command::ProjectCreate { options, json } => {
            project_create::execute(env, options, *json, output).await
        }
        Command::ProjectIngestKeyCreate { options, json } => {
            project_create::execute_ingest_key_create(env, options, *json, output).await
        }
        Command::ProjectArchive { project_id, json } => {
            project_archive::execute(env, project_id.as_str(), *json, output).await
        }
        Command::ProjectDeletion { project_id, json } => {
            project_archive::execute_deletion(env, project_id.as_str(), *json, output).await
        }
        Command::Projects { json } => projects::execute(env, *json, output).await,
        Command::Usage { json } => usage::execute(env, *json, output).await,
        Command::Version { json } => execute_version(*json, output),
        Command::InvestigateIssue {
            issue_id,
            occurrence,
            json,
        } => investigate::execute(env, issue_id.as_str(), occurrence, *json, output).await,
        Command::Explain { target, json } => explain::execute(env, target, *json, output).await,
        Command::Deploy { options, json } => deployment::execute(env, options, *json, output).await,
        Command::AnalyticsOverview { options, json } => {
            analytics_overview::execute(env, options, *json, output).await
        }
        Command::AnalyticsProperties { options, json } => {
            analytics_properties::execute(env, options, *json, output).await
        }
        Command::AnalyticsCompare { options, json } => {
            analytics_segments::execute(env, options, *json, output).await
        }
        Command::AnalyticsPaths { options, json } => {
            analytics::execute(env, options, *json, output).await
        }
        Command::AnalyticsFunnel { options, json } => {
            analytics_funnel::execute(env, options, *json, output).await
        }
        Command::AnalyticsRetention { options, json } => {
            analytics_retention::execute(env, options, *json, output).await
        }
        Command::AnalyticsLifecycle { options, json } => {
            analytics_lifecycle::execute(env, options, *json, output).await
        }
        Command::NativeDebugArtifacts { target, json } => {
            native_debug_artifacts::execute(env, target, *json, output).await
        }
        Command::Read { .. }
        | Command::Set { .. }
        | Command::ProjectRepositories { .. }
        | Command::ProjectSetupSeen { .. }
        | Command::Support { .. } => execute_http(command, env, output).await,
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
    let support_command = matches!(command, Command::Support { .. });
    let origin = http::normalized_origin(env.base_url.as_str()).ok_or_else(|| {
        if support_command {
            support_transport_error()
        } else {
            RuntimeError::Unavailable {
                message: "the configured API URL is invalid",
                next: "set LOGBREW_API_URL to an http or https API origin and retry",
            }
        }
    })?;
    let url = format!("{origin}{path}");
    let client = http::api_client()?;

    let response_result = send_authenticated_with_refresh(&client, env, |client, credential| {
        build_command_request(client, command, url.as_str(), credential)
    })
    .await;
    let (response, credential) = match response_result {
        Ok(response) => response,
        Err(RuntimeError::Http(_)) if support_command => return Err(support_transport_error()),
        Err(error) => return Err(error),
    };
    let status = response.status();
    let body = match http::bounded_body(response, 8 * 1024 * 1024).await {
        Ok(body) => body,
        Err(_) if support_command => return Err(support_transport_error()),
        Err(_) => {
            return Err(RuntimeError::Unavailable {
                message: "API response was invalid",
                next: "retry the command; if it repeats, report the public response contract",
            });
        }
    };

    if !status.is_success() {
        let body = if let Command::Support { target, .. } = command {
            support::safe_error_body(target, status.as_u16())
        } else if matches!(command, Command::ProjectRepositories { .. }) {
            return Err(repositories::validate_error(
                status.as_u16(),
                body.as_str(),
                &credential,
            )?);
        } else {
            credential.redact_response_body(body.as_str())
        };
        return Err(RuntimeError::Api {
            status: status.as_u16(),
            body,
            auth_source: credential.source(),
            auth_label: credential.label(),
        });
    }

    write_api_success(command, body.as_str(), output)?;
    Ok(())
}

/// Returns a fixed, path-free support transport failure.
const fn support_transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "support request could not be completed",
        next: "check network connectivity and retry the support command",
    }
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
        HttpMethod::Put => client.put(url),
        HttpMethod::Patch => client.patch(url),
        HttpMethod::Delete => client.delete(url),
    }
    .bearer_auth(credential.token());
    if let Some(body) = command.request_body_for_token(Some(credential.token())) {
        request = request.json(&body);
    }
    if let Some(key) = command.idempotency_key() {
        request = request.header("Idempotency-Key", key);
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
    let client = http::api_client()?;
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
                ("pagination", filters.pagination),
                ("cursor_time", filters.cursor_time),
                ("cursor_id", filters.cursor_id),
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
        ExplainTarget::Issue { id, occurrence } => issue_explain_path(id, occurrence),
        ExplainTarget::IssueCorrection(correction) => path_with_query(
            &format!(
                "/api/telemetry/issues/{}/correction-verification",
                encode_component(correction.issue_id.as_str())
            ),
            &[
                (
                    "baseline_occurrence_id",
                    Some(correction.baseline_occurrence_id.as_str()),
                ),
                (
                    "candidate_deployment_id",
                    Some(correction.candidate_deployment_id.as_str()),
                ),
            ],
        ),
        ExplainTarget::Log(id) => {
            format!("/api/logs/{}/investigation", encode_component(id))
        }
        ExplainTarget::Action(id) => {
            format!(
                "/api/telemetry/actions/{}/investigation",
                encode_component(id)
            )
        }
        ExplainTarget::Trace(id) => {
            format!(
                "/api/telemetry/traces/{}/investigation",
                encode_component(id)
            )
        }
        ExplainTarget::Span(span) => path_with_query(
            &format!(
                "/api/telemetry/traces/{}/spans/{}/investigation",
                encode_component(span.trace_id.as_str()),
                encode_component(span.span_id.as_str())
            ),
            &[
                ("project_id", Some(span.project_id.as_str())),
                ("environment", Some(span.environment.as_str())),
                ("release", Some(span.release.as_str())),
            ],
        ),
        ExplainTarget::Release(release) => path_with_query(
            "/api/telemetry/releases/investigation",
            &[
                ("project_id", Some(release.project_id.as_str())),
                ("release", Some(release.release.as_str())),
                ("environment", Some(release.environment.as_str())),
                ("service_name", Some(release.service_name.as_str())),
                ("response_version", Some("3")),
            ],
        ),
        ExplainTarget::Metric(metric) => metric_explain_path(metric),
    }
}

/// Builds one explicit version-10 issue investigation path.
fn issue_explain_path(id: &str, occurrence: &IssueOccurrenceSelection) -> String {
    let base = format!(
        "/api/telemetry/issues/{}/investigation",
        encode_component(id)
    );
    let (name, value) = match occurrence {
        IssueOccurrenceSelection::Recommended => ("selection", "recommended"),
        IssueOccurrenceSelection::First => ("selection", "first"),
        IssueOccurrenceSelection::Latest => ("selection", "latest"),
        IssueOccurrenceSelection::Exact(id) => ("occurrence_id", id.as_str()),
    };
    path_with_query(
        base.as_str(),
        &[("response_version", Some("10")), (name, Some(value))],
    )
}

/// Builds one bounded metric-investigation endpoint path.
fn metric_explain_path(metric: &ExplainMetricTarget) -> String {
    let series_limit = metric.series_limit.map(|value| value.to_string());
    path_with_query(
        "/api/telemetry/metrics/investigation",
        &[
            ("project_id", Some(metric.project_id.as_str())),
            ("name", Some(metric.name.as_str())),
            ("since", Some(metric.since.as_str())),
            ("until", metric.until.as_deref()),
            ("interval", metric.interval.as_deref()),
            ("group_by", metric.group_by.as_deref()),
            ("service_name", metric.service_name.as_deref()),
            ("release", metric.release.as_deref()),
            ("environment", metric.environment.as_deref()),
            ("series_limit", series_limit.as_deref()),
            ("response_version", Some("2")),
        ],
    )
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
