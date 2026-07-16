//! CLI error types and output rendering.

use crate::ISSUE_STATUS_VALUES_NEXT_STEP;
use std::borrow::Cow;

/// CLI parsing error.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CliError {
    /// Command is missing or unsupported.
    #[error("unknown or missing command")]
    UnknownCommand,
    /// Command name is unsupported.
    #[error("unknown command: {command}")]
    UnknownCommandName {
        /// Unsupported command name.
        command: String,
        /// Suggested next step.
        next: &'static str,
    },
    /// Required command argument is missing.
    #[error("missing argument: {argument}")]
    MissingArgument {
        /// Missing argument name.
        argument: &'static str,
        /// Argument-specific next step.
        next: &'static str,
    },
    /// Required flag value is missing.
    #[error("missing value for {flag}")]
    MissingFlagValue {
        /// Flag missing a value.
        flag: &'static str,
        /// Flag-specific next step.
        next: &'static str,
    },
    /// Flag is present more than once.
    #[error("duplicate flag: {flag}")]
    DuplicateFlag {
        /// Duplicate flag value.
        flag: &'static str,
        /// Flag-specific next step.
        next: &'static str,
    },
    /// Positional argument is unsupported for the selected command.
    #[error("unexpected argument for {command}: {argument}")]
    UnexpectedArgument {
        /// Unexpected argument value.
        argument: String,
        /// Command name.
        command: &'static str,
        /// Command-specific next step.
        next: &'static str,
    },
    /// Flag is unknown for the selected command.
    #[error("unknown flag: {flag}")]
    UnknownFlag {
        /// Unknown flag value.
        flag: String,
        /// Command-specific next step.
        next: &'static str,
    },
    /// Flag is known globally but unsupported for the selected command.
    #[error("unsupported flag for {command}: {flag}")]
    UnsupportedFlag {
        /// Unsupported flag value.
        flag: String,
        /// Command name.
        command: &'static str,
        /// Command-specific next step.
        next: &'static str,
    },
    /// Resource is unsupported for the selected command.
    #[error("unknown resource: {resource}")]
    UnknownResource {
        /// Unsupported resource value.
        resource: String,
        /// Command-specific next step.
        next: &'static str,
    },
    /// Issue status is unsupported.
    #[error("unknown issue status: {0}")]
    UnknownStatus(String),
    /// Trace status is unsupported.
    #[error("unknown trace status: {0}")]
    UnknownTraceStatus(String),
    /// Log level is unsupported.
    #[error("unknown log level: {0}")]
    UnknownLogLevel(String),
    /// Row limit is malformed.
    #[error("invalid limit: {0}")]
    InvalidLimit(String),
    /// Minimum trace duration is malformed.
    #[error("invalid minimum duration: {0}")]
    InvalidMinDuration(String),
    /// Pagination mode is unsupported.
    #[error("unknown pagination mode")]
    UnknownPagination,
    /// Action cursor fields are inconsistent.
    #[error("invalid action cursor: {0}")]
    InvalidActionCursor(String),
    /// Log cursor fields are inconsistent.
    #[error("invalid log cursor: {0}")]
    InvalidLogCursor(String),
    /// Issue cursor fields are inconsistent.
    #[error("invalid issue cursor: {0}")]
    InvalidIssueCursor(String),
    /// Support-ticket cursor fields are inconsistent.
    #[error("invalid support cursor: {0}")]
    InvalidSupportCursor(String),
    /// Support-ticket category is unsupported.
    #[error("unknown support category")]
    UnknownSupportCategory,
    /// Support-ticket identifier is not in the public `sup_` form.
    #[error("invalid support ticket id")]
    InvalidSupportTicketId,
    /// Support context retry key cannot be sent as an HTTP header value.
    #[error("invalid support retry key")]
    InvalidSupportRetryKey,
    /// Support context reply syntax is malformed.
    #[error("invalid support context reply")]
    InvalidSupportContextReply,
    /// Support context history syntax is malformed.
    #[error("invalid support context command")]
    InvalidSupportContextCommand,
    /// Support context text is blank or exceeds the public limit.
    #[error("invalid support context")]
    InvalidSupportContext,
    /// Issue investigation syntax is malformed.
    #[error("invalid issue investigation command")]
    InvalidInvestigationCommand,
    /// Project setup source is malformed.
    #[error("invalid setup source: {0}")]
    InvalidSetupSource(String),
}

/// Runtime error for command execution.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// Command-line parsing failed.
    #[error(transparent)]
    Cli(#[from] CliError),
    /// Filesystem or process I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// HTTP request failed.
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// Auth token was missing for an authenticated API call.
    #[error("not logged in: run logbrew login")]
    MissingToken,
    /// API returned a non-success status.
    #[error("api returned status {status}: {body}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Response body.
        body: String,
        /// Stable machine-readable auth source key.
        auth_source: &'static str,
        /// Concise human auth label.
        auth_label: &'static str,
    },
    /// Status check could not prove API reachability.
    #[error("LogBrew API unreachable: {message}")]
    StatusUnavailable {
        /// Base API URL checked by the CLI.
        api_url: String,
        /// Optional HTTP status code returned by `/health`.
        status_code: Option<u16>,
        /// Optional response body returned by `/health`.
        body: Option<String>,
        /// Whether a local credential is configured.
        authenticated: bool,
        /// Stable machine-readable auth source key.
        auth_source: &'static str,
        /// Concise human auth label.
        auth_label: &'static str,
        /// Human-readable reachability reason.
        message: String,
    },
    /// Command is recognized but not available yet.
    #[error("{message}")]
    Unavailable {
        /// Human-readable unavailable reason.
        message: &'static str,
        /// Suggested fallback action.
        next: &'static str,
    },
    /// A successful investigation response violated the public contract.
    #[error("issue investigation returned an invalid response")]
    InvestigationResponseInvalid,
}

/// Writes a command-line parsing error for humans or agents.
///
/// # Errors
///
/// Returns an I/O error if writing to the output stream fails.
pub fn write_cli_error<W: std::io::Write>(
    error: &CliError,
    json: bool,
    output: &mut W,
) -> Result<(), std::io::Error> {
    if json {
        let body = serde_json::json!({
            "ok": false,
            "error": cli_error_code(error),
            "message": error.to_string(),
            "next": cli_error_next_step(error),
        });
        writeln!(output, "{body}")
    } else {
        writeln!(output, "{error}")?;
        writeln!(output, "Next: {}", cli_error_next_step(error))
    }
}

/// Writes a runtime error for humans or agents.
///
/// # Errors
///
/// Returns an I/O error if writing to the output stream fails.
pub fn write_runtime_error<W: std::io::Write>(
    error: &RuntimeError,
    json: bool,
    output: &mut W,
) -> Result<(), std::io::Error> {
    if json {
        let body = runtime_error_json(error);
        writeln!(output, "{body}")
    } else {
        write_human_runtime_error(error, output)
    }
}

/// Writes a runtime error for human readers.
fn write_human_runtime_error<W: std::io::Write>(
    error: &RuntimeError,
    output: &mut W,
) -> Result<(), std::io::Error> {
    match error {
        RuntimeError::StatusUnavailable {
            api_url,
            status_code,
            body,
            auth_label,
            message,
            ..
        } => {
            writeln!(output, "LogBrew API unreachable.")?;
            writeln!(output, "API: {api_url}")?;
            writeln!(output, "Auth: {auth_label}")?;
            if let Some(status_code) = status_code {
                writeln!(output, "Status: {status_code}")?;
            }
            if let Some(body) = body.as_ref().filter(|body| !body.is_empty()) {
                writeln!(output, "Body: {body}")?;
            } else {
                writeln!(output, "Reason: {message}")?;
            }
            writeln!(output, "Next: {STATUS_UNAVAILABLE_NEXT_STEP}")?;
            Ok(())
        }
        RuntimeError::Api {
            status,
            body,
            auth_label,
            ..
        } => {
            let api_details = ApiErrorDetails::parse(body);
            writeln!(output, "{error}")?;
            if let Some(code) = api_details.code.as_deref() {
                writeln!(output, "Code: {code}")?;
            }
            writeln!(output, "Auth: {auth_label}")?;
            writeln!(output, "Next: {}", api_next_step(*status, &api_details))
        }
        RuntimeError::Cli(_)
        | RuntimeError::Io(_)
        | RuntimeError::Http(_)
        | RuntimeError::MissingToken
        | RuntimeError::InvestigationResponseInvalid
        | RuntimeError::Unavailable { .. } => {
            writeln!(output, "{error}")?;
            writeln!(output, "Next: {}", runtime_error_next_step(error))?;
            Ok(())
        }
    }
}

/// Builds a JSON runtime error body for agents.
fn runtime_error_json(error: &RuntimeError) -> serde_json::Value {
    match error {
        RuntimeError::MissingToken
        | RuntimeError::Unavailable { .. }
        | RuntimeError::InvestigationResponseInvalid
        | RuntimeError::Io(_)
        | RuntimeError::Http(_) => serde_json::json!({
            "ok": false,
            "error": runtime_error_code(error),
            "message": error.to_string(),
            "next": runtime_error_next_step(error),
        }),
        RuntimeError::Api {
            status,
            body,
            auth_source,
            ..
        } => {
            let api_details = ApiErrorDetails::parse(body);
            let next = api_next_step(*status, &api_details);
            serde_json::json!({
                "ok": false,
                "error": runtime_error_code(error),
                "message": error.to_string(),
                "status": status,
                "body": body,
                "api_error": api_details.error.as_deref(),
                "api_code": api_details.code.as_deref(),
                "api_next": api_details.next.as_deref(),
                "auth_source": auth_source,
                "next": next,
            })
        }
        RuntimeError::StatusUnavailable {
            api_url,
            status_code,
            body,
            authenticated,
            auth_source,
            message,
            ..
        } => serde_json::json!({
            "ok": false,
            "error": runtime_error_code(error),
            "status": "unreachable",
            "status_code": status_code,
            "body": body,
            "api_url": api_url,
            "authenticated": authenticated,
            "auth_source": auth_source,
            "message": message,
            "next": runtime_error_next_step(error),
        }),
        RuntimeError::Cli(error) => serde_json::json!({
            "ok": false,
            "error": cli_error_code(error),
            "message": error.to_string(),
            "next": cli_error_next_step(error),
        }),
    }
}

/// Returns a stable machine-readable parse error code.
const fn cli_error_code(error: &CliError) -> &'static str {
    match error {
        CliError::UnknownCommand | CliError::UnknownCommandName { .. } => "unknown_command",
        CliError::MissingArgument { .. } => "missing_argument",
        CliError::MissingFlagValue { .. } => "missing_flag_value",
        CliError::DuplicateFlag { .. } => "duplicate_flag",
        CliError::UnexpectedArgument { .. } => "unexpected_argument",
        CliError::UnknownFlag { .. } => "unknown_flag",
        CliError::UnsupportedFlag { .. } => "unsupported_flag",
        CliError::UnknownResource { .. } => "unknown_resource",
        CliError::UnknownStatus(_) => "unknown_status",
        CliError::UnknownTraceStatus(_) => "unknown_trace_status",
        CliError::UnknownLogLevel(_) => "unknown_log_level",
        CliError::InvalidLimit(_) => "invalid_limit",
        CliError::InvalidMinDuration(_) => "invalid_min_duration",
        CliError::UnknownPagination => "unknown_pagination",
        CliError::InvalidActionCursor(_) => "invalid_action_cursor",
        CliError::InvalidLogCursor(_) => "invalid_log_cursor",
        CliError::InvalidIssueCursor(_) => "invalid_issue_cursor",
        CliError::InvalidSupportCursor(_) => "invalid_support_cursor",
        CliError::UnknownSupportCategory => "unknown_support_category",
        CliError::InvalidSupportTicketId => "invalid_support_ticket_id",
        CliError::InvalidSupportRetryKey => "invalid_support_retry_key",
        CliError::InvalidSupportContextReply => "invalid_support_context_reply",
        CliError::InvalidSupportContextCommand => "invalid_support_context_command",
        CliError::InvalidSupportContext => "invalid_support_context",
        CliError::InvalidInvestigationCommand => "invalid_investigation_command",
        CliError::InvalidSetupSource(_) => "invalid_setup_source",
    }
}

/// Returns the next step for a parse error.
const fn cli_error_next_step(error: &CliError) -> &'static str {
    match error {
        CliError::InvalidLimit(_) => "use --limit with a positive whole number",
        CliError::InvalidMinDuration(_) => "use --min-duration-ms with a non-negative whole number",
        CliError::UnknownPagination
        | CliError::InvalidActionCursor(_)
        | CliError::InvalidLogCursor(_)
        | CliError::InvalidIssueCursor(_)
        | CliError::InvalidSupportCursor(_) => {
            "use --pagination cursor alone for the first page, then use --cursor-time and --cursor-id together from next_cursor"
        }
        CliError::UnknownSupportCategory => {
            "use sdk_install_failure, ingest_failure, auth_failure, project_setup, dashboard_issue, docs_confusion, cli_issue, mobile_issue, billing_question, or other"
        }
        CliError::InvalidSupportTicketId => {
            "use the ticket_id returned by logbrew support create or list"
        }
        CliError::InvalidSupportRetryKey => {
            "use --retry-key with 1 to 128 visible ASCII characters and reuse it only for an exact retry"
        }
        CliError::InvalidSupportContextReply => {
            "use support reply <ticket_id> --context <text> --retry-key <key>"
        }
        CliError::InvalidSupportContextCommand => {
            "use support context <ticket_id> with optional --json"
        }
        CliError::InvalidSupportContext => {
            "use --context with 1 to 4000 characters after trimming whitespace"
        }
        CliError::InvalidInvestigationCommand => {
            "use logbrew investigate issue <issue_id> with optional --json"
        }
        CliError::InvalidSetupSource(_) => "use --source api, cli, or sdk",
        CliError::MissingArgument { next, .. }
        | CliError::MissingFlagValue { next, .. }
        | CliError::DuplicateFlag { next, .. }
        | CliError::UnexpectedArgument { next, .. }
        | CliError::UnsupportedFlag { next, .. }
        | CliError::UnknownFlag { next, .. }
        | CliError::UnknownResource { next, .. }
        | CliError::UnknownCommandName { next, .. } => next,
        CliError::UnknownCommand => "run logbrew --help",
        CliError::UnknownStatus(_) => ISSUE_STATUS_VALUES_NEXT_STEP,
        CliError::UnknownTraceStatus(_) => "use --status error or --status ok",
        CliError::UnknownLogLevel(_) => "use one of info, warning, error, critical",
    }
}

/// Returns a stable machine-readable runtime error code.
const fn runtime_error_code(error: &RuntimeError) -> &'static str {
    match error {
        RuntimeError::Cli(error) => cli_error_code(error),
        RuntimeError::Io(_) => "io_error",
        RuntimeError::Http(_) => "http_error",
        RuntimeError::MissingToken => "not_logged_in",
        RuntimeError::Api { .. } => "api_error",
        RuntimeError::StatusUnavailable { .. } => "status_unreachable",
        RuntimeError::Unavailable { .. } => "unavailable",
        RuntimeError::InvestigationResponseInvalid => "investigation_response_invalid",
    }
}

/// Next step for failed `status` reachability checks.
const STATUS_UNAVAILABLE_NEXT_STEP: &str = "check LOGBREW_API_URL or network";

/// Parsed public API error details.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ApiErrorDetails {
    /// Human-readable backend error message.
    error: Option<String>,
    /// Stable backend error code.
    code: Option<String>,
    /// Backend-provided recovery step.
    next: Option<String>,
}

impl ApiErrorDetails {
    /// Parses additive backend error fields from an API response body.
    fn parse(body: &str) -> Self {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
            return Self::default();
        };

        Self {
            error: json_string_field(&value, "error"),
            code: json_string_field(&value, "code"),
            next: json_string_field(&value, "next"),
        }
    }
}

/// Extracts a non-empty string field from a JSON object.
fn json_string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// Returns a useful next step for runtime errors when one is known.
fn runtime_error_next_step(error: &RuntimeError) -> Cow<'static, str> {
    match error {
        RuntimeError::Api { status, body, .. } => {
            let api_details = ApiErrorDetails::parse(body);
            api_next_step(*status, &api_details)
        }
        RuntimeError::Cli(_)
        | RuntimeError::Io(_)
        | RuntimeError::Http(_)
        | RuntimeError::MissingToken
        | RuntimeError::InvestigationResponseInvalid
        | RuntimeError::StatusUnavailable { .. }
        | RuntimeError::Unavailable { .. } => {
            Cow::Borrowed(fallback_runtime_error_next_step(error))
        }
    }
}

/// Returns the API next step, preferring backend guidance when available.
fn api_next_step(status: u16, api_details: &ApiErrorDetails) -> Cow<'static, str> {
    api_details.next.as_ref().map_or_else(
        || Cow::Borrowed(fallback_api_next_step(status)),
        |next| Cow::Owned(next.clone()),
    )
}

/// Returns the CLI fallback next step for an API status.
const fn fallback_api_next_step(status: u16) -> &'static str {
    match status {
        401 | 403 => "run logbrew login",
        400 | 422 => "check command arguments or filters",
        404 => "check the resource id or filters",
        429 => "retry later",
        500..=599 => "check LOGBREW_API_URL or retry later",
        _ => "check command arguments or retry later",
    }
}

/// Returns a useful fallback next step for runtime errors when one is known.
const fn fallback_runtime_error_next_step(error: &RuntimeError) -> &'static str {
    match error {
        RuntimeError::Cli(error) => cli_error_next_step(error),
        RuntimeError::MissingToken => "run logbrew login",
        RuntimeError::Api { status, .. } => fallback_api_next_step(*status),
        RuntimeError::StatusUnavailable { .. } | RuntimeError::Http(_) => {
            STATUS_UNAVAILABLE_NEXT_STEP
        }
        RuntimeError::Unavailable { next, .. } => next,
        RuntimeError::InvestigationResponseInvalid => {
            "retry the issue investigation; if it repeats, report the public response contract"
        }
        RuntimeError::Io(_) => "check local files and permissions",
    }
}
