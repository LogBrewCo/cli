//! CLI error types and output rendering.

use crate::ISSUE_STATUS_VALUES_NEXT_STEP;

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
    /// Log level is unsupported.
    #[error("unknown log level: {0}")]
    UnknownLogLevel(String),
    /// Row limit is malformed.
    #[error("invalid limit: {0}")]
    InvalidLimit(String),
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
    if !json {
        if let RuntimeError::StatusUnavailable {
            api_url,
            status_code,
            body,
            auth_label,
            message,
            ..
        } = error
        {
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
            return Ok(());
        }
        writeln!(output, "{error}")?;
        if let RuntimeError::Api { auth_label, .. } = error {
            writeln!(output, "Auth: {auth_label}")?;
        }
        writeln!(output, "Next: {}", runtime_error_next_step(error))?;
        return Ok(());
    }

    let body = match error {
        RuntimeError::MissingToken
        | RuntimeError::Unavailable { .. }
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
        } => serde_json::json!({
            "ok": false,
            "error": runtime_error_code(error),
            "message": error.to_string(),
            "status": status,
            "body": body,
            "auth_source": auth_source,
            "next": runtime_error_next_step(error),
        }),
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
    };
    writeln!(output, "{body}")
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
        CliError::UnknownLogLevel(_) => "unknown_log_level",
        CliError::InvalidLimit(_) => "invalid_limit",
    }
}

/// Returns the next step for a parse error.
const fn cli_error_next_step(error: &CliError) -> &'static str {
    match error {
        CliError::InvalidLimit(_) => "use --limit with a positive whole number",
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
        CliError::UnknownLogLevel(_) => "use one of trace, debug, info, warn, error, fatal",
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
    }
}

/// Next step for failed `status` reachability checks.
const STATUS_UNAVAILABLE_NEXT_STEP: &str = "check LOGBREW_API_URL or network";

/// Returns a useful next step for runtime errors when one is known.
const fn runtime_error_next_step(error: &RuntimeError) -> &'static str {
    match error {
        RuntimeError::Cli(error) => cli_error_next_step(error),
        RuntimeError::MissingToken
        | RuntimeError::Api {
            status: 401 | 403, ..
        } => "run logbrew login",
        RuntimeError::Api {
            status: 400 | 422, ..
        } => "check command arguments or filters",
        RuntimeError::Api { status: 404, .. } => "check the resource id or filters",
        RuntimeError::Api { status: 429, .. } => "retry later",
        RuntimeError::Api {
            status: 500..=599, ..
        } => "check LOGBREW_API_URL or retry later",
        RuntimeError::Api { .. } => "check command arguments or retry later",
        RuntimeError::StatusUnavailable { .. } | RuntimeError::Http(_) => {
            STATUS_UNAVAILABLE_NEXT_STEP
        }
        RuntimeError::Unavailable { next, .. } => next,
        RuntimeError::Io(_) => "check local files and permissions",
    }
}
