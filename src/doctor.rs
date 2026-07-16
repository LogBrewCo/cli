//! Bounded, read-only project diagnostics.

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
    /// Active project ingest-key check.
    ingest_key: DoctorCheck,
    /// Backend-owned setup progress check.
    setup: DoctorCheck,
    /// Cross-stream telemetry progress check.
    telemetry: DoctorCheck,
    /// Optional newest-log visibility check.
    logs: DoctorCheck,
    /// One prioritized recovery or follow-up.
    next: &'static str,
}

impl DoctorReport {
    /// Returns the checks in their stable display order.
    const fn checks(&self) -> [DoctorCheck; 7] {
        [
            self.api,
            self.auth,
            self.project,
            self.ingest_key,
            self.setup,
            self.telemetry,
            self.logs,
        ]
    }

    /// Replaces the overall state and prioritized next action.
    const fn mark(&mut self, status: &'static str, next: &'static str) {
        self.status = status;
        self.next = next;
    }
}

/// Canonical backend-owned project readiness state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectState {
    /// The project needs an active ingest key.
    NeedsIngestKey,
    /// The project needs a setup path or acknowledgement.
    NeedsSetup,
    /// Setup is acknowledged but no telemetry has arrived.
    NeedsTelemetry,
    /// The project is ready for telemetry investigation.
    Ready,
}

impl ProjectState {
    /// Stable machine value exposed by the CLI report.
    const fn key(self) -> &'static str {
        match self {
            Self::NeedsIngestKey => "needs_ingest_key",
            Self::NeedsSetup => "needs_setup",
            Self::NeedsTelemetry => "needs_telemetry",
            Self::Ready => "ready",
        }
    }

    /// Fixed CLI-owned prioritized recovery or follow-up.
    const fn next(self) -> &'static str {
        match self {
            Self::NeedsIngestKey => {
                "create an ingest key for this project, then rerun logbrew doctor --project <project_id>"
            }
            Self::NeedsSetup => "choose an SDK or CLI setup path for this project",
            Self::NeedsTelemetry => "send the first telemetry event for this project",
            Self::Ready => "inspect recent project logs, issues, actions, releases, or traces",
        }
    }

    /// Exact deployed next-action pair for this state.
    const fn action(self) -> (&'static str, &'static str) {
        match self {
            Self::NeedsIngestKey => ("create_ingest_key", "project_ingest_keys"),
            Self::NeedsSetup => ("choose_setup_path", "project_setup"),
            Self::NeedsTelemetry => ("send_first_telemetry", "telemetry_ingest"),
            Self::Ready => ("inspect_recent_telemetry", "telemetry_reads"),
        }
    }
}

/// Valid setup progress after exact doctor-response validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupProgress {
    /// Project exists but setup has not started.
    NotStarted,
    /// A setup path was selected without explicit acknowledgement.
    PathSelected,
    /// Setup acknowledgement is recorded without telemetry.
    Acknowledged,
    /// Real telemetry proves setup is operational.
    Operational,
}

/// Safe fields retained from the exact doctor response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DoctorSnapshot {
    /// Canonical readiness state.
    state: ProjectState,
    /// Whether an active ingest key exists.
    has_active_ingest_key: bool,
    /// Validated setup progress.
    setup: SetupProgress,
    /// Whether first telemetry has been observed.
    telemetry_seen: bool,
}

/// Result of the authoritative doctor read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoctorOutcome {
    /// Exact 200 response was accepted.
    Success(DoctorSnapshot),
    /// No local or environment account credential is available.
    MissingAuth,
    /// The account credential was rejected.
    AuthInvalid,
    /// The project is absent or owner-hidden.
    ProjectMissing,
    /// The server returned a valid typed failure.
    Failed,
    /// A success or error response violated its public contract.
    InvalidResponse,
    /// No safe HTTP response was received.
    TransportFailed,
}

/// Result of the optional newest-log visibility probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogsOutcome {
    /// At least one retained log is visible.
    Visible,
    /// No retained log is visible.
    Empty,
    /// The credential was rejected after the doctor read.
    AuthInvalid,
    /// The server returned a valid typed failure.
    Failed,
    /// The response violated the public logs contract.
    InvalidResponse,
    /// No safe HTTP response was received.
    TransportFailed,
}

/// Safe result of sending one authenticated GET without refresh.
#[derive(Debug)]
enum AuthenticatedGet {
    /// Server returned an HTTP response.
    Response(reqwest::Response),
    /// No usable account credential is available.
    MissingAuth,
    /// The request failed without a safe response.
    Failed,
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

/// Runs the authoritative read, then the non-authoritative log visibility probe.
async fn run_checks(
    client: &reqwest::Client,
    env: &CliEnvironment,
    project_id: &str,
) -> DoctorReport {
    let mut report = initial_report();
    let snapshot = match doctor_outcome(client, env, project_id).await {
        DoctorOutcome::Success(snapshot) => snapshot,
        DoctorOutcome::MissingAuth => {
            report.auth = check("auth", "missing", "run logbrew login");
            report.mark("auth_invalid", "run logbrew login");
            return report;
        }
        DoctorOutcome::AuthInvalid => {
            report.api = check("api", "reachable", "validate persisted auth");
            report.auth = check("auth", "invalid", "run logbrew login");
            report.mark("auth_invalid", "run logbrew login");
            return report;
        }
        DoctorOutcome::ProjectMissing => {
            report.api = check("api", "reachable", "validate persisted auth");
            report.auth = check("auth", "valid", "inspect the selected project");
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
        DoctorOutcome::Failed => {
            report.api = check("api", "reachable", "validate persisted auth");
            report.project = check(
                "project",
                "error",
                "retry the project doctor without changing project state",
            );
            mark_check_failed(&mut report);
            return report;
        }
        DoctorOutcome::InvalidResponse => {
            report.api = check("api", "reachable", "validate persisted auth");
            report.project = invalid_response_check("project");
            mark_check_failed(&mut report);
            return report;
        }
        DoctorOutcome::TransportFailed => return api_unreachable(),
    };

    apply_snapshot(&mut report, snapshot);
    apply_logs_outcome(&mut report, logs_outcome(client, env, project_id).await);
    report
}

/// Applies one validated canonical state without retaining server text or identifiers.
const fn apply_snapshot(report: &mut DoctorReport, snapshot: DoctorSnapshot) {
    report.api = check("api", "reachable", "validate persisted auth");
    report.auth = check("auth", "valid", "inspect the selected project");
    report.project = check("project", "usable", "inspect project readiness");
    report.ingest_key = if snapshot.has_active_ingest_key {
        check("ingest_key", "active", "inspect setup acknowledgement")
    } else {
        check(
            "ingest_key",
            "missing",
            "create an ingest key for this project",
        )
    };
    report.setup = match snapshot.setup {
        SetupProgress::NotStarted => check(
            "setup",
            "not_started",
            "choose an SDK or CLI setup path for this project",
        ),
        SetupProgress::PathSelected => check(
            "setup",
            "path_selected",
            "send the first telemetry event for this project",
        ),
        SetupProgress::Acknowledged => check("setup", "acknowledged", "inspect telemetry state"),
        SetupProgress::Operational => check("setup", "operational", "inspect telemetry state"),
    };
    report.telemetry = if snapshot.telemetry_seen {
        check("telemetry", "seen", "inspect recent telemetry")
    } else {
        check(
            "telemetry",
            "not_seen",
            "send the first telemetry event for this project",
        )
    };
    report.mark(snapshot.state.key(), snapshot.state.next());
}

/// Applies log visibility without reconstructing or overriding canonical readiness.
const fn apply_logs_outcome(report: &mut DoctorReport, outcome: LogsOutcome) {
    match outcome {
        LogsOutcome::Visible => {
            report.logs = check("logs", "visible", "inspect the newest visible log");
        }
        LogsOutcome::Empty => {
            report.logs = check("logs", "empty", "inspect another telemetry stream");
        }
        LogsOutcome::AuthInvalid => {
            report.logs = check(
                "logs",
                "unavailable",
                "run logbrew login, then retry the log visibility check",
            );
        }
        LogsOutcome::Failed | LogsOutcome::TransportFailed => {
            report.logs = check("logs", "error", "retry the recent-log visibility check");
        }
        LogsOutcome::InvalidResponse => {
            report.logs = invalid_response_check("logs");
        }
    }
}

/// Reads and validates the canonical owner-scoped doctor response.
async fn doctor_outcome(
    client: &reqwest::Client,
    env: &CliEnvironment,
    project_id: &str,
) -> DoctorOutcome {
    let path = format!("/api/projects/{project_id}/doctor");
    let response = match authenticated_get(client, env, path.as_str()).await {
        AuthenticatedGet::Response(response) => response,
        AuthenticatedGet::MissingAuth => return DoctorOutcome::MissingAuth,
        AuthenticatedGet::Failed => return DoctorOutcome::TransportFailed,
    };
    let status = response.status().as_u16();
    let Some(value) = response_json(response).await else {
        return DoctorOutcome::InvalidResponse;
    };
    if status == 200 {
        return validate_doctor(&value, project_id)
            .map_or(DoctorOutcome::InvalidResponse, DoctorOutcome::Success);
    }
    if !valid_error_envelope(&value) {
        return DoctorOutcome::InvalidResponse;
    }
    match status {
        401 if is_unauthorized_error(&value) => DoctorOutcome::AuthInvalid,
        404 if is_project_not_found_error(&value) => DoctorOutcome::ProjectMissing,
        401 | 404 => DoctorOutcome::InvalidResponse,
        _ => DoctorOutcome::Failed,
    }
}

/// Reads only newest-log visibility after the canonical project preflight succeeds.
async fn logs_outcome(
    client: &reqwest::Client,
    env: &CliEnvironment,
    project_id: &str,
) -> LogsOutcome {
    let path = path_with_query(
        "/api/logs",
        &[("project_id", Some(project_id)), ("limit", Some("1"))],
    );
    let response = match authenticated_get(client, env, path.as_str()).await {
        AuthenticatedGet::Response(response) => response,
        AuthenticatedGet::MissingAuth => return LogsOutcome::AuthInvalid,
        AuthenticatedGet::Failed => return LogsOutcome::TransportFailed,
    };
    let status = response.status().as_u16();
    let Some(value) = response_json(response).await else {
        return LogsOutcome::InvalidResponse;
    };
    if status == 200 {
        return match validate_logs(&value) {
            Some(true) => LogsOutcome::Visible,
            Some(false) => LogsOutcome::Empty,
            None => LogsOutcome::InvalidResponse,
        };
    }
    if !valid_error_envelope(&value) {
        return LogsOutcome::InvalidResponse;
    }
    if status == 401 {
        if is_unauthorized_error(&value) {
            LogsOutcome::AuthInvalid
        } else {
            LogsOutcome::InvalidResponse
        }
    } else {
        LogsOutcome::Failed
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
        Err(RuntimeError::MissingToken) => AuthenticatedGet::MissingAuth,
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

/// Validates the exact public project doctor response.
fn validate_doctor(
    value: &serde_json::Value,
    requested_project_id: &str,
) -> Option<DoctorSnapshot> {
    const KEYS: [&str; 10] = [
        "project_id",
        "state",
        "setup_status",
        "setup_acknowledged",
        "has_active_ingest_key",
        "first_telemetry_seen_at",
        "last_seen_at",
        "last_signal",
        "next",
        "next_action",
    ];
    let object = value
        .as_object()
        .filter(|object| exact_keys(object, &KEYS))?;
    let response_project_id = safe_string(object, "project_id")?;
    if !crate::ids::is_uuid(response_project_id)
        || !response_project_id.eq_ignore_ascii_case(requested_project_id)
    {
        return None;
    }
    let state = match safe_string(object, "state")? {
        "needs_ingest_key" => ProjectState::NeedsIngestKey,
        "needs_setup" => ProjectState::NeedsSetup,
        "needs_telemetry" => ProjectState::NeedsTelemetry,
        "ready" => ProjectState::Ready,
        _ => return None,
    };
    let setup = match safe_string(object, "setup_status")? {
        "created" => SetupProgress::NotStarted,
        "setup_started" => SetupProgress::PathSelected,
        "sdk_seen" => SetupProgress::Acknowledged,
        "first_telemetry_seen" | "active" => SetupProgress::Operational,
        _ => return None,
    };
    let setup_acknowledged = object.get("setup_acknowledged")?.as_bool()?;
    if setup_acknowledged
        != matches!(
            setup,
            SetupProgress::Acknowledged | SetupProgress::Operational
        )
    {
        return None;
    }
    let has_active_ingest_key = object.get("has_active_ingest_key")?.as_bool()?;
    let _first_telemetry_seen_at_present = optional_timestamp(object, "first_telemetry_seen_at")?;
    let _last_seen_at_present = optional_timestamp(object, "last_seen_at")?;
    validate_last_signal(object.get("last_signal")?)?;
    let _next = safe_string(object, "next")?;

    let action = object.get("next_action")?.as_object()?;
    if !exact_keys(action, &["code", "target"])
        || (safe_string(action, "code")?, safe_string(action, "target")?) != state.action()
    {
        return None;
    }
    let telemetry_seen = matches!(setup, SetupProgress::Operational);
    let expected_state = if has_active_ingest_key {
        match setup {
            SetupProgress::NotStarted => ProjectState::NeedsSetup,
            SetupProgress::PathSelected | SetupProgress::Acknowledged => {
                ProjectState::NeedsTelemetry
            }
            SetupProgress::Operational => ProjectState::Ready,
        }
    } else {
        ProjectState::NeedsIngestKey
    };
    (state == expected_state).then_some(DoctorSnapshot {
        state,
        has_active_ingest_key,
        setup,
        telemetry_seen,
    })
}

/// Validates the existing display-safe last-signal surface without retaining it.
fn validate_last_signal(value: &serde_json::Value) -> Option<()> {
    if value.is_null() {
        return Some(());
    }
    let object = value.as_object()?;
    if !exact_keys(object, &["kind", "id", "message", "occurred_at"])
        || safe_string(object, "kind").is_none()
        || !valid_optional_safe_string(object, "id")
        || !valid_optional_safe_string(object, "message")
    {
        return None;
    }
    is_rfc3339(safe_string(object, "occurred_at")?).then_some(())
}

/// Validates the bare legacy logs array enough to determine visibility safely.
fn validate_logs(value: &serde_json::Value) -> Option<bool> {
    let logs = value.as_array()?;
    logs.iter()
        .all(|log| {
            log.as_object()
                .and_then(|object| safe_string(object, "service_name"))
                .is_some()
        })
        .then_some(!logs.is_empty())
}

/// Validates the shared exact error envelope without retaining its text.
fn valid_error_envelope(value: &serde_json::Value) -> bool {
    error_fields(value).is_some()
}

/// Returns whether a response is the canonical rejected-auth envelope.
fn is_unauthorized_error(value: &serde_json::Value) -> bool {
    matches!(
        error_fields(value),
        Some((_error, "unauthorized", _next, "sign_in", "auth"))
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

/// Extracts bounded fields from the shared exact error envelope.
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

/// Returns whether one required key is null or a bounded control-safe string.
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

/// Validates one optional RFC3339 timestamp and reports whether it is present.
fn optional_timestamp(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<bool> {
    match object.get(key)? {
        serde_json::Value::Null => Some(false),
        serde_json::Value::String(value) if safe_text(value) && is_rfc3339(value) => Some(true),
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
        ingest_key: check("ingest_key", "not_checked", "resolve the prior check first"),
        setup: check("setup", "not_checked", "resolve the prior check first"),
        telemetry: check("telemetry", "not_checked", "resolve the prior check first"),
        logs: check("logs", "not_checked", "resolve the prior check first"),
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
            "reachable" | "valid" | "usable" | "active" | "acknowledged" | "operational"
            | "seen" | "visible" => "ok",
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
        "ingest_key" => "Ingest key",
        "setup" => "Setup",
        "telemetry" => "Telemetry",
        "logs" => "Logs",
        _ => "Check",
    }
}
