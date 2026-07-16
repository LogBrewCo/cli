//! Bounded, read-only project setup diagnostics.

use crate::auth::send_authenticated_without_refresh;
use crate::{CliEnvironment, RuntimeError, path_with_query};

/// Maximum accepted response size for one diagnostic read.
const RESPONSE_LIMIT: u64 = 256 * 1024;

/// One fixed diagnostic check rendered for humans and agents.
#[derive(Debug, Clone, Copy)]
struct DoctorCheck {
    /// Stable check identifier.
    check: &'static str,
    /// Stable check result.
    status: &'static str,
    /// Fixed next step after this check.
    next: &'static str,
}

/// Complete CLI-owned project diagnostic report.
#[derive(Debug, Clone, Copy)]
struct DoctorReport {
    /// Stable overall state.
    status: &'static str,
    /// API reachability check.
    api: DoctorCheck,
    /// Effective auth validation check.
    auth: DoctorCheck,
    /// Selected project ownership/usability check.
    project: DoctorCheck,
    /// Backend-owned setup progress check.
    setup: DoctorCheck,
    /// Cheap recent-log check.
    telemetry: DoctorCheck,
    /// One prioritized recovery or follow-up.
    next: &'static str,
}

impl DoctorReport {
    /// Returns the checks in their stable execution order.
    const fn checks(&self) -> [DoctorCheck; 5] {
        [
            self.api,
            self.auth,
            self.project,
            self.setup,
            self.telemetry,
        ]
    }

    /// Replaces the overall state and prioritized next action.
    const fn mark(&mut self, status: &'static str, next: &'static str) {
        self.status = status;
        self.next = next;
    }
}

/// Valid backend setup progress after exact response validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupProgress {
    /// Project exists but setup has not started.
    NotStarted,
    /// A setup path was selected but explicit acknowledgement is not proven.
    PathSelected,
    /// The setup-seen acknowledgement is recorded without telemetry.
    Acknowledged,
    /// Real telemetry proves setup is operational.
    Operational,
}

/// Safe outcome from one authenticated diagnostic GET.
#[derive(Debug)]
enum AuthenticatedGet {
    /// Server returned a response.
    Response(reqwest::Response),
    /// No usable account credential is available.
    Unauthorized,
    /// The read failed without a safe server response.
    Failed,
}

/// Result of validating the account-auth read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccountOutcome {
    /// Current effective account auth is valid.
    Valid,
    /// Current credentials are missing or rejected.
    Invalid,
    /// The account read or response contract failed.
    Failed,
    /// The server returned a malformed or unrecognized response.
    InvalidResponse,
}

/// Result of validating the selected project setup read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupOutcome {
    /// Project is usable with validated setup progress.
    Usable(SetupProgress),
    /// Credentials were rejected during the setup read.
    AuthInvalid,
    /// Project is missing or unavailable to this account.
    Missing,
    /// The setup read or response contract failed.
    Failed,
    /// The server returned a malformed or unrecognized response.
    InvalidResponse,
}

/// Result of validating the recent-log probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelemetryOutcome {
    /// At least one retained log exists for the project.
    Visible,
    /// No retained log exists for the project.
    Empty,
    /// Credentials were rejected during the log read.
    AuthInvalid,
    /// The log read or response contract failed.
    Failed,
    /// The server returned a malformed or unrecognized response.
    InvalidResponse,
}

/// Executes the bounded project diagnostic and writes one deterministic report.
pub async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    project_id: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
    else {
        return write_report(&api_unreachable(), json, output);
    };
    let report = run_checks(&client, env, project_id).await;
    write_report(&report, json, output)
}

/// Runs the ordered diagnostic and stops after the first blocker.
async fn run_checks(
    client: &reqwest::Client,
    env: &CliEnvironment,
    project_id: &str,
) -> DoctorReport {
    let mut report = initial_report();
    if !api_reachable(client, env).await {
        return api_unreachable();
    }
    report.api = check("api", "reachable", "validate persisted auth");
    match account_outcome(client, env).await {
        AccountOutcome::Valid => {
            report.auth = check("auth", "valid", "check the selected project");
        }
        AccountOutcome::Invalid => {
            report.auth = check("auth", "invalid", "run logbrew login");
            report.mark("auth_invalid", "run logbrew login");
            return report;
        }
        AccountOutcome::Failed => {
            report.auth = check(
                "auth",
                "error",
                "retry the project doctor without changing local credentials",
            );
            mark_check_failed(&mut report);
            return report;
        }
        AccountOutcome::InvalidResponse => {
            report.auth = invalid_response_check("auth");
            mark_check_failed(&mut report);
            return report;
        }
    }
    let progress = match setup_outcome(client, env, project_id).await {
        SetupOutcome::Usable(SetupProgress::NotStarted) => {
            report.project = check("project", "usable", "inspect project setup state");
            report.setup = check(
                "setup",
                "not_started",
                "choose an SDK or CLI setup path for this project",
            );
            SetupProgress::NotStarted
        }
        SetupOutcome::Usable(SetupProgress::PathSelected) => {
            report.project = check("project", "usable", "inspect project setup state");
            report.setup = check(
                "setup",
                "path_selected",
                "send the first telemetry event for this project",
            );
            SetupProgress::PathSelected
        }
        SetupOutcome::Usable(SetupProgress::Acknowledged) => {
            report.project = check("project", "usable", "inspect project setup state");
            report.setup = check("setup", "acknowledged", "check recent telemetry");
            SetupProgress::Acknowledged
        }
        SetupOutcome::Usable(SetupProgress::Operational) => {
            report.project = check("project", "usable", "inspect project setup state");
            report.setup = check("setup", "operational", "check recent telemetry");
            SetupProgress::Operational
        }
        SetupOutcome::AuthInvalid => {
            report.auth = check("auth", "invalid", "run logbrew login");
            report.project = check("project", "not_checked", "run logbrew login");
            report.mark("auth_invalid", "run logbrew login");
            return report;
        }
        SetupOutcome::Missing => {
            report.project = check(
                "project",
                "missing",
                "use a project_id returned by logbrew projects",
            );
            report.mark(
                "project_missing",
                "use a project_id returned by logbrew projects",
            );
            return report;
        }
        SetupOutcome::Failed => {
            report.project = check(
                "project",
                "error",
                "retry the project doctor without changing project state",
            );
            mark_check_failed(&mut report);
            return report;
        }
        SetupOutcome::InvalidResponse => {
            report.project = invalid_response_check("project");
            mark_check_failed(&mut report);
            return report;
        }
    };
    finish_with_telemetry(client, env, project_id, progress, &mut report).await;
    report
}

/// Applies the final recent-log probe to an otherwise usable project.
async fn finish_with_telemetry(
    client: &reqwest::Client,
    env: &CliEnvironment,
    project_id: &str,
    progress: SetupProgress,
    report: &mut DoctorReport,
) {
    match telemetry_outcome(client, env, project_id).await {
        TelemetryOutcome::Visible => {
            report.telemetry = check("telemetry", "visible", "inspect the newest visible log");
            match progress {
                SetupProgress::NotStarted => report.mark(
                    "setup_incomplete",
                    "choose an SDK or CLI setup path for this project",
                ),
                SetupProgress::PathSelected => report.mark(
                    "setup_incomplete",
                    "send the first telemetry event for this project",
                ),
                SetupProgress::Acknowledged | SetupProgress::Operational => {
                    report.mark("ready", "run logbrew logs --project <project_id>");
                }
            }
        }
        TelemetryOutcome::Empty => match progress {
            SetupProgress::NotStarted => {
                report.telemetry = check(
                    "telemetry",
                    "empty",
                    "send a log or inspect another telemetry stream",
                );
                report.mark(
                    "setup_incomplete",
                    "choose an SDK or CLI setup path for this project",
                );
            }
            SetupProgress::PathSelected => {
                report.telemetry = check(
                    "telemetry",
                    "empty",
                    "send a log or inspect another telemetry stream",
                );
                report.mark(
                    "setup_incomplete",
                    "send the first telemetry event for this project",
                );
            }
            SetupProgress::Acknowledged => {
                report.telemetry = check(
                    "telemetry",
                    "empty",
                    "send a log or inspect another telemetry stream",
                );
                report.mark(
                        "telemetry_empty",
                        "send a log or inspect a wider window with logbrew logs --project <project_id> --since 7d",
                    );
            }
            SetupProgress::Operational => {
                report.telemetry = check(
                    "telemetry",
                    "cross_signal",
                    "inspect project issues, actions, releases, or traces",
                );
                report.mark(
                    "ready",
                    "inspect project issues, actions, releases, or traces",
                );
            }
        },
        TelemetryOutcome::AuthInvalid => {
            report.auth = check("auth", "invalid", "run logbrew login");
            report.telemetry = check("telemetry", "not_checked", "run logbrew login");
            report.mark("auth_invalid", "run logbrew login");
        }
        TelemetryOutcome::Failed => {
            report.telemetry = check(
                "telemetry",
                "error",
                "retry the project doctor without changing project state",
            );
            mark_check_failed(report);
        }
        TelemetryOutcome::InvalidResponse => {
            report.telemetry = invalid_response_check("telemetry");
            mark_check_failed(report);
        }
    }
}

/// Checks the health route without retaining its URL or body.
async fn api_reachable(client: &reqwest::Client, env: &CliEnvironment) -> bool {
    let health_url = format!("{}/health", env.base_url.trim_end_matches('/'));
    client
        .get(health_url)
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

/// Validates current effective account auth through the account route.
async fn account_outcome(client: &reqwest::Client, env: &CliEnvironment) -> AccountOutcome {
    let response = match authenticated_get(client, env, "/api/auth/account").await {
        AuthenticatedGet::Response(response) => response,
        AuthenticatedGet::Unauthorized => return AccountOutcome::Invalid,
        AuthenticatedGet::Failed => return AccountOutcome::Failed,
    };
    let status = response.status().as_u16();
    let Some(value) = response_json(response).await else {
        return AccountOutcome::InvalidResponse;
    };
    if (200..300).contains(&status) {
        return if valid_account(&value) {
            AccountOutcome::Valid
        } else {
            AccountOutcome::InvalidResponse
        };
    }
    if !valid_error_envelope(&value) {
        return AccountOutcome::InvalidResponse;
    }
    if status == 401 {
        if is_unauthorized_error(&value) {
            AccountOutcome::Invalid
        } else {
            AccountOutcome::InvalidResponse
        }
    } else {
        AccountOutcome::Failed
    }
}

/// Validates project ownership/usability and exact setup state.
async fn setup_outcome(
    client: &reqwest::Client,
    env: &CliEnvironment,
    project_id: &str,
) -> SetupOutcome {
    let setup_path = format!("/api/projects/{project_id}/setup");
    let response = match authenticated_get(client, env, setup_path.as_str()).await {
        AuthenticatedGet::Response(response) => response,
        AuthenticatedGet::Unauthorized => return SetupOutcome::AuthInvalid,
        AuthenticatedGet::Failed => return SetupOutcome::Failed,
    };
    let status = response.status().as_u16();
    let Some(value) = response_json(response).await else {
        return SetupOutcome::InvalidResponse;
    };
    if (200..300).contains(&status) {
        return validate_setup(&value, project_id)
            .map_or(SetupOutcome::InvalidResponse, SetupOutcome::Usable);
    }
    if !valid_error_envelope(&value) {
        return SetupOutcome::InvalidResponse;
    }
    match status {
        401 if is_unauthorized_error(&value) => SetupOutcome::AuthInvalid,
        404 if is_project_not_found_error(&value) => SetupOutcome::Missing,
        401 | 404 => SetupOutcome::InvalidResponse,
        _ => SetupOutcome::Failed,
    }
}

/// Probes the newest retained log without retaining any telemetry object.
async fn telemetry_outcome(
    client: &reqwest::Client,
    env: &CliEnvironment,
    project_id: &str,
) -> TelemetryOutcome {
    let logs_path = path_with_query(
        "/api/logs",
        &[("project_id", Some(project_id)), ("limit", Some("1"))],
    );
    let response = match authenticated_get(client, env, logs_path.as_str()).await {
        AuthenticatedGet::Response(response) => response,
        AuthenticatedGet::Unauthorized => return TelemetryOutcome::AuthInvalid,
        AuthenticatedGet::Failed => return TelemetryOutcome::Failed,
    };
    let status = response.status().as_u16();
    let Some(value) = response_json(response).await else {
        return TelemetryOutcome::InvalidResponse;
    };
    if (200..300).contains(&status) {
        return match validate_logs(&value) {
            Some(true) => TelemetryOutcome::Visible,
            Some(false) => TelemetryOutcome::Empty,
            None => TelemetryOutcome::InvalidResponse,
        };
    }
    if !valid_error_envelope(&value) {
        return TelemetryOutcome::InvalidResponse;
    }
    if status == 401 {
        if is_unauthorized_error(&value) {
            TelemetryOutcome::AuthInvalid
        } else {
            TelemetryOutcome::InvalidResponse
        }
    } else {
        TelemetryOutcome::Failed
    }
}

/// Sends one authenticated GET while collapsing unsafe local errors.
async fn authenticated_get(
    client: &reqwest::Client,
    env: &CliEnvironment,
    path: &str,
) -> AuthenticatedGet {
    let url = format!("{}{}", env.base_url.trim_end_matches('/'), path);
    match send_authenticated_without_refresh(client, env, |client, credential| {
        client.get(url.as_str()).bearer_auth(credential.token())
    })
    .await
    {
        Ok((response, _credential)) => AuthenticatedGet::Response(response),
        Err(RuntimeError::MissingToken | RuntimeError::Api { status: 401, .. }) => {
            AuthenticatedGet::Unauthorized
        }
        Err(_) => AuthenticatedGet::Failed,
    }
}

/// Reads one bounded JSON response without retaining its text on failure.
async fn response_json(mut response: reqwest::Response) -> Option<serde_json::Value> {
    if response
        .content_length()
        .is_some_and(|length| length > RESPONSE_LIMIT)
    {
        return None;
    }
    let limit = usize::try_from(RESPONSE_LIMIT).ok()?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len().saturating_add(chunk.len()) > limit {
            return None;
        }
        body.extend_from_slice(chunk.as_ref());
    }
    serde_json::from_slice(body.as_slice()).ok()
}

/// Validates enough account shape to prove authenticated account access.
fn valid_account(value: &serde_json::Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get("id"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(crate::ids::is_uuid)
}

/// Validates the exact public project setup response.
fn validate_setup(value: &serde_json::Value, requested_project_id: &str) -> Option<SetupProgress> {
    const KEYS: [&str; 13] = [
        "project_id",
        "status",
        "runtime",
        "source",
        "created_at",
        "setup_started_at",
        "first_telemetry_seen_at",
        "last_seen_at",
        "last_release",
        "last_environment",
        "last_signal",
        "next",
        "next_action",
    ];
    let object = value
        .as_object()
        .filter(|object| exact_keys(object, &KEYS))?;
    if safe_string(object, "project_id")? != requested_project_id {
        return None;
    }
    let status = safe_string(object, "status")?;
    if !valid_optional_safe_string(object, "runtime") || !valid_optional_source(object) {
        return None;
    }
    required_timestamp(object, "created_at")?;
    optional_timestamp(object, "setup_started_at")?;
    optional_timestamp(object, "first_telemetry_seen_at")?;
    optional_timestamp(object, "last_seen_at")?;
    if !valid_optional_safe_string(object, "last_release")
        || !valid_optional_safe_string(object, "last_environment")
    {
        return None;
    }
    validate_last_signal(object.get("last_signal")?)?;

    let next = safe_string(object, "next")?;
    let action = object.get("next_action")?.as_object()?;
    if !exact_keys(action, &["code", "target"]) {
        return None;
    }
    let pair = (safe_string(action, "code")?, safe_string(action, "target")?);
    match (status, next, pair) {
        (
            "created",
            "choose an SDK or CLI setup path for this project",
            ("choose_setup_path", "project_setup"),
        ) => Some(SetupProgress::NotStarted),
        (
            "setup_started",
            "send the first telemetry event for this project",
            ("send_first_telemetry", "telemetry_ingest"),
        ) => Some(SetupProgress::PathSelected),
        (
            "sdk_seen",
            "send the first telemetry event for this project",
            ("send_first_telemetry", "telemetry_ingest"),
        ) => Some(SetupProgress::Acknowledged),
        (
            "first_telemetry_seen" | "active",
            "open the project dashboard or inspect recent telemetry",
            ("review_project_dashboard", "project_dashboard"),
        ) => Some(SetupProgress::Operational),
        _ => None,
    }
}

/// Validates the optional exact last-signal object.
fn validate_last_signal(value: &serde_json::Value) -> Option<()> {
    if value.is_null() {
        return Some(());
    }
    let object = value.as_object()?;
    if !exact_keys(object, &["kind", "id", "message", "occurred_at"])
        || !matches!(
            safe_string(object, "kind")?,
            "action" | "issue" | "log" | "release" | "trace"
        )
        || !valid_optional_safe_string(object, "id")
        || !valid_optional_safe_string(object, "message")
    {
        return None;
    }
    required_timestamp(object, "occurred_at")?;
    Some(())
}

/// Validates the legacy bare-array log response and reports whether it has rows.
fn validate_logs(value: &serde_json::Value) -> Option<bool> {
    let logs = value.as_array()?;
    if !logs.iter().all(|log| {
        log.as_object()
            .and_then(|object| safe_string(object, "service_name"))
            .is_some()
    }) {
        return None;
    }
    Some(!logs.is_empty())
}

/// Validates the shared exact error envelope without retaining its text.
fn valid_error_envelope(value: &serde_json::Value) -> bool {
    error_fields(value).is_some()
}

/// Returns whether a response is the canonical rejected-auth envelope.
fn is_unauthorized_error(value: &serde_json::Value) -> bool {
    matches!(
        error_fields(value),
        Some((
            "Invalid or expired token",
            "unauthorized",
            "send Authorization: Bearer <token>, include the logbrew_session cookie, or sign in again",
            "sign_in",
            "auth"
        ))
    )
}

/// Returns whether a response is the canonical hidden-ownership 404 envelope.
fn is_project_not_found_error(value: &serde_json::Value) -> bool {
    matches!(
        error_fields(value),
        Some((
            "project not found",
            "not_found",
            "check project_id or create a project with POST /api/projects",
            "check_resource",
            "resource"
        ))
    )
}

/// Extracts the bounded fields from the shared exact error envelope.
fn error_fields(value: &serde_json::Value) -> Option<(&str, &str, &str, &str, &str)> {
    let object = value.as_object()?;
    if !exact_keys(object, &["error", "code", "next", "next_action"]) {
        return None;
    }
    let action = object.get("next_action")?.as_object()?;
    if !exact_keys(action, &["code", "target"]) {
        return None;
    }
    Some((
        safe_string(object, "error")?,
        safe_string(object, "code")?,
        safe_string(object, "next")?,
        safe_string(action, "code")?,
        safe_string(action, "target")?,
    ))
}

/// Returns whether an object has exactly the required key set.
fn exact_keys(object: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> bool {
    object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
}

/// Extracts one bounded, nonblank, control-safe string.
fn safe_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    object
        .get(key)?
        .as_str()
        .filter(|value| !value.trim().is_empty() && safe_text(value))
}

/// Returns whether one required key is a null or bounded safe string.
fn valid_optional_safe_string(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> bool {
    match object.get(key) {
        Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(value)) => safe_text(value),
        Some(
            serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::Array(_)
            | serde_json::Value::Object(_),
        )
        | None => false,
    }
}

/// Returns whether setup source is null or one canonical public value.
fn valid_optional_source(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    match object.get("source") {
        Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(value)) => {
            matches!(value.as_str(), "api" | "cli" | "sdk")
        }
        Some(
            serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::Array(_)
            | serde_json::Value::Object(_),
        )
        | None => false,
    }
}

/// Returns whether a server string is bounded and free of display controls.
fn safe_text(value: &str) -> bool {
    value.chars().count() <= 512
        && !value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '\u{061c}'
                        | '\u{200b}'..='\u{200f}'
                        | '\u{2028}'..='\u{202e}'
                        | '\u{2060}'..='\u{206f}'
                        | '\u{feff}'
                        | '\u{fff9}'..='\u{fffb}'
                )
        })
}

/// Validates one required RFC3339 timestamp.
fn required_timestamp(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<()> {
    is_rfc3339(safe_string(object, key)?).then_some(())
}

/// Validates one optional RFC3339 timestamp.
fn optional_timestamp(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<()> {
    match object.get(key)? {
        serde_json::Value::Null => Some(()),
        serde_json::Value::String(value) if safe_text(value) && is_rfc3339(value) => Some(()),
        serde_json::Value::String(_)
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::Array(_)
        | serde_json::Value::Object(_) => None,
    }
}

/// Validates RFC3339 calendar, time, fraction, and offset syntax.
fn is_rfc3339(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return false;
    }
    let Some(year) = bytes.get(0..4).and_then(digits_u32) else {
        return false;
    };
    let Some(month) = bytes.get(5..7).and_then(digits_u32) else {
        return false;
    };
    let Some(day) = bytes.get(8..10).and_then(digits_u32) else {
        return false;
    };
    let Some(hour) = bytes.get(11..13).and_then(digits_u32) else {
        return false;
    };
    let Some(minute) = bytes.get(14..16).and_then(digits_u32) else {
        return false;
    };
    let Some(second) = bytes.get(17..19).and_then(digits_u32) else {
        return false;
    };
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return false;
    }

    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == fraction_start {
            return false;
        }
    }
    match bytes.get(index) {
        Some(b'Z') => index + 1 == bytes.len(),
        Some(b'+' | b'-') => {
            let Some(offset_hour) = bytes.get(index + 1..index + 3).and_then(digits_u32) else {
                return false;
            };
            let Some(offset_minute) = bytes.get(index + 4..index + 6).and_then(digits_u32) else {
                return false;
            };
            bytes.get(index + 3) == Some(&b':')
                && index + 6 == bytes.len()
                && offset_hour <= 23
                && offset_minute <= 59
        }
        Some(_) | None => false,
    }
}

/// Parses an all-digit ASCII byte slice.
fn digits_u32(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value * 10 + u32::from(*byte - b'0'))
    })
}

/// Returns the number of days in a Gregorian calendar month.
const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

/// Builds one fixed check.
const fn check(check: &'static str, status: &'static str, next: &'static str) -> DoctorCheck {
    DoctorCheck {
        check,
        status,
        next,
    }
}

/// Builds one fixed malformed-response check.
const fn invalid_response_check(check_name: &'static str) -> DoctorCheck {
    check(
        check_name,
        "invalid_response",
        "retry without changing local or project state",
    )
}

/// Builds the initial all-not-checked report.
const fn initial_report() -> DoctorReport {
    DoctorReport {
        status: "check_failed",
        api: check("api", "not_checked", "run the API reachability check first"),
        auth: check("auth", "not_checked", "resolve the prior check first"),
        project: check("project", "not_checked", "resolve the prior check first"),
        setup: check("setup", "not_checked", "resolve the prior check first"),
        telemetry: check("telemetry", "not_checked", "resolve the prior check first"),
        next: "retry logbrew doctor --project <project_id>; if it repeats, report the public response contract",
    }
}

/// Builds an API-unreachable report.
const fn api_unreachable() -> DoctorReport {
    let mut report = initial_report();
    report.status = "api_unreachable";
    report.api = check(
        "api",
        "unreachable",
        "check network access and retry the project doctor",
    );
    report.next = "check network access, then retry logbrew doctor --project <project_id>";
    report
}

/// Marks a report as an upstream-contract check failure.
const fn mark_check_failed(report: &mut DoctorReport) {
    report.mark(
        "check_failed",
        "retry logbrew doctor --project <project_id>; if it repeats, report the public response contract",
    );
}

/// Writes stable machine JSON or one bounded human report.
fn write_report<W: std::io::Write>(
    report: &DoctorReport,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if json {
        let checks = report
            .checks()
            .into_iter()
            .map(|check| {
                serde_json::json!({
                    "check": check.check,
                    "status": check.status,
                    "next": check.next,
                })
            })
            .collect::<Vec<_>>();
        let body = serde_json::json!({
            "status": report.status,
            "checks": checks,
            "next": report.next,
        });
        writeln!(output, "{body}")?;
        return Ok(());
    }

    writeln!(output, "LogBrew project doctor")?;
    for check in report.checks() {
        let marker = match check.status {
            "reachable" | "valid" | "usable" | "acknowledged" | "operational" | "visible"
            | "cross_signal" => "ok",
            "not_checked" => " ",
            _ => "!",
        };
        writeln!(
            output,
            "[{marker}] {}: {}",
            check_title(check.check),
            check.status
        )?;
    }
    writeln!(output, "Status: {}", report.status)?;
    writeln!(output, "Next: {}", report.next)?;
    Ok(())
}

/// Returns one stable human check title.
fn check_title(check: &str) -> &'static str {
    match check {
        "api" => "API",
        "auth" => "Auth",
        "project" => "Project",
        "setup" => "Setup",
        "telemetry" => "Telemetry",
        _ => "Check",
    }
}
