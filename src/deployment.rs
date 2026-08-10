//! Strict completed-deployment capture for release comparison.

use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::{
    CliEnvironment, CliError, Command, DeploymentRecordOptions, DeploymentStatus, RuntimeError,
};
use serde::Deserialize;

/// Public deployment-capture response version supported by this CLI.
const SCHEMA_VERSION: u8 = 1;
/// Maximum accepted deployment response body.
const RESPONSE_LIMIT: usize = 64 * 1024;
/// Maximum completed deployment duration accepted by the API.
const MAX_DURATION_MILLIS: i128 = 30 * 24 * 60 * 60 * 1_000;
/// Stable parser recovery for the closed deployment grammar.
#[expect(
    clippy::redundant_pub_crate,
    reason = "sibling parser and error modules consume this private-module contract"
)]
pub(crate) const DEPLOYMENT_NEXT_STEP: &str = "use logbrew deploy <deployment_id> --project \
    <project_id> --release <release> --environment <environment> --service <service_name> \
    --status <succeeded|failed> --started-at <rfc3339> --finished-at <rfc3339> with optional \
    --commit-sha <sha> and --json";

/// Parsed deployment flags before required-field validation.
#[derive(Default)]
struct DeploymentFlags {
    /// Caller-owned deployment identity.
    deployment_id: Option<String>,
    /// Account-owned project identity.
    project_id: Option<String>,
    /// Exact runtime release.
    release: Option<String>,
    /// Exact runtime environment.
    environment: Option<String>,
    /// Exact runtime service.
    service_name: Option<String>,
    /// Terminal deployment result.
    status: Option<String>,
    /// Deployment start timestamp.
    started_at: Option<String>,
    /// Deployment finish timestamp.
    finished_at: Option<String>,
    /// Optional source commit.
    commit_sha: Option<String>,
    /// Stable machine-readable output mode.
    json: bool,
}

/// Fully normalized deployment values plus comparable timestamps.
struct NormalizedDeployment {
    /// Caller-owned deployment identity.
    deployment_id: String,
    /// Canonical lowercase project UUID.
    project_id: String,
    /// Trimmed exact runtime release.
    release: String,
    /// Trimmed exact runtime environment.
    environment: String,
    /// Trimmed exact runtime service.
    service_name: String,
    /// Terminal deployment result.
    status: DeploymentStatus,
    /// Original valid RFC3339 start timestamp without surrounding whitespace.
    started_at: String,
    /// Start time normalized to the API's millisecond precision.
    started_at_millis: i128,
    /// Original valid RFC3339 finish timestamp without surrounding whitespace.
    finished_at: String,
    /// Finish time normalized to the API's millisecond precision.
    finished_at_millis: i128,
    /// Optional normalized lowercase source commit.
    commit_sha: Option<String>,
}

impl NormalizedDeployment {
    /// Converts validated parser state into the public command model.
    fn into_options(self) -> DeploymentRecordOptions {
        DeploymentRecordOptions {
            deployment_id: self.deployment_id,
            project_id: self.project_id,
            release: self.release,
            environment: self.environment,
            service_name: self.service_name,
            status: self.status,
            started_at: self.started_at,
            finished_at: self.finished_at,
            commit_sha: self.commit_sha,
        }
    }

    /// Builds the exact version-1 deployment request.
    fn request_body(&self) -> serde_json::Value {
        serde_json::json!({
            "project_id": self.project_id,
            "release": self.release,
            "environment": self.environment,
            "service_name": self.service_name,
            "status": self.status.as_str(),
            "started_at": self.started_at,
            "finished_at": self.finished_at,
            "commit_sha": self.commit_sha,
        })
    }
}

/// Strict version-1 deployment response.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeploymentResponse {
    schema_version: u8,
    id: String,
    deployment_id: String,
    project_id: String,
    release: String,
    environment: String,
    service_name: String,
    status: DeploymentStatus,
    started_at: String,
    finished_at: String,
    commit_sha: Option<String>,
    recorded_at: String,
}

/// Parsed RFC3339 instant at nanosecond precision.
#[derive(Clone, Copy)]
struct ParsedTimestamp {
    /// Whole UTC seconds since the Unix epoch.
    epoch_seconds: i64,
    /// Positive sub-second nanoseconds.
    nanoseconds: u32,
}

impl ParsedTimestamp {
    /// Returns the instant at the API's truncating millisecond precision.
    fn epoch_millis(self) -> i128 {
        i128::from(self.epoch_seconds) * 1_000 + i128::from(self.nanoseconds / 1_000_000)
    }

    /// Returns whether no precision below milliseconds is present.
    const fn is_millisecond_normalized(self) -> bool {
        self.nanoseconds.is_multiple_of(1_000_000)
    }
}

/// Parses the closed deployment command without reflecting invalid values.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the sibling parser module consumes this private-module helper"
)]
pub(crate) fn parse(args: &[String]) -> Result<Command, CliError> {
    let mut flags = DeploymentFlags::default();
    let mut index = 0;
    while let Some(raw) = args.get(index) {
        if raw == "--json" {
            if std::mem::replace(&mut flags.json, true) {
                return Err(CliError::InvalidDeploymentCommand);
            }
            index += 1;
            continue;
        }
        if raw.starts_with('-') {
            let (name, inline) = raw
                .split_once('=')
                .map_or((raw.as_str(), None), |(name, value)| (name, Some(value)));
            let slot = match name {
                "--project" | "--project-id" => &mut flags.project_id,
                "--release" => &mut flags.release,
                "--environment" | "--env" => &mut flags.environment,
                "--service" | "--service-name" => &mut flags.service_name,
                "--status" => &mut flags.status,
                "--started-at" => &mut flags.started_at,
                "--finished-at" => &mut flags.finished_at,
                "--commit-sha" | "--commit" => &mut flags.commit_sha,
                _ => return Err(CliError::InvalidDeploymentCommand),
            };
            let value = flag_value(args, &mut index, inline)?;
            if slot.replace(value).is_some() {
                return Err(CliError::InvalidDeploymentCommand);
            }
            index += 1;
            continue;
        }
        if flags.deployment_id.replace(raw.clone()).is_some() {
            return Err(CliError::InvalidDeploymentCommand);
        }
        index += 1;
    }

    let raw = DeploymentRecordOptions {
        deployment_id: required(flags.deployment_id)?,
        project_id: required(flags.project_id)?,
        release: required(flags.release)?,
        environment: required(flags.environment)?,
        service_name: required(flags.service_name)?,
        status: parse_status(required(flags.status)?.as_str())?,
        started_at: required(flags.started_at)?,
        finished_at: required(flags.finished_at)?,
        commit_sha: flags.commit_sha,
    };
    let normalized = normalize(&raw)?;
    Ok(Command::Deploy {
        options: normalized.into_options(),
        json: flags.json,
    })
}

/// Reads one separate or inline deployment flag value.
fn flag_value(
    args: &[String],
    index: &mut usize,
    inline: Option<&str>,
) -> Result<String, CliError> {
    let value = if let Some(value) = inline {
        value.to_owned()
    } else {
        *index += 1;
        args.get(*index)
            .filter(|value| !value.starts_with('-'))
            .cloned()
            .ok_or(CliError::InvalidDeploymentCommand)?
    };
    if value.is_empty() {
        return Err(CliError::InvalidDeploymentCommand);
    }
    Ok(value)
}

/// Requires one deployment argument without naming or reflecting its value.
fn required(value: Option<String>) -> Result<String, CliError> {
    value.ok_or(CliError::InvalidDeploymentCommand)
}

/// Parses one case-insensitive terminal result.
fn parse_status(value: &str) -> Result<DeploymentStatus, CliError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "succeeded" => Ok(DeploymentStatus::Succeeded),
        "failed" => Ok(DeploymentStatus::Failed),
        _ => Err(CliError::InvalidDeploymentCommand),
    }
}

/// Executes one strict account-authenticated deployment record.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(crate) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    options: &DeploymentRecordOptions,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let normalized = normalize(options)?;
    let origin = normalized_origin(env.base_url.as_str())?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_error| transport_error())?;
    let url = format!(
        "{origin}/api/telemetry/deployments/{}",
        normalized.deployment_id
    );
    let request_body = normalized.request_body();
    let response = send_account_authenticated_with_refresh(&client, env, |client, credential| {
        client
            .put(url.as_str())
            .bearer_auth(credential.token())
            .json(&request_body)
    })
    .await
    .map_err(request_error)?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(safe_api_error(status, &credential));
    }
    let body = bounded_body(response).await?;
    let response = validated_response(&normalized, body.as_str())?;
    if json {
        writeln!(output, "{body}")?;
    } else {
        write_human(&response, output)?;
    }
    Ok(())
}

/// Normalizes and proves every request value before network use.
fn normalize(options: &DeploymentRecordOptions) -> Result<NormalizedDeployment, CliError> {
    let deployment_id = options.deployment_id.trim();
    if deployment_id.is_empty()
        || deployment_id.len() > 128
        || !deployment_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(CliError::InvalidDeploymentCommand);
    }
    let project_id = options.project_id.trim().to_ascii_lowercase();
    if !crate::ids::is_uuid(project_id.as_str()) {
        return Err(CliError::InvalidDeploymentCommand);
    }
    let release = options.release.trim();
    if release.is_empty()
        || release.len() > 200
        || matches!(release, "." | "..")
        || release
            .bytes()
            .any(|byte| matches!(byte, b'\n' | b'\r' | b'\t' | b'/' | b'\\'))
        || !safe_text(release, 200)
    {
        return Err(CliError::InvalidDeploymentCommand);
    }
    let environment = options.environment.trim();
    if environment.is_empty()
        || environment.len() > 64
        || environment == "None"
        || environment
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || matches!(byte, b'/' | b'\\'))
        || !safe_text(environment, 64)
    {
        return Err(CliError::InvalidDeploymentCommand);
    }
    let service_name = options.service_name.trim();
    if service_name.is_empty()
        || service_name.chars().count() > 200
        || !safe_text(service_name, 200)
    {
        return Err(CliError::InvalidDeploymentCommand);
    }
    let started_at = options.started_at.trim();
    let finished_at = options.finished_at.trim();
    let started = parse_rfc3339(started_at).ok_or(CliError::InvalidDeploymentCommand)?;
    let finished = parse_rfc3339(finished_at).ok_or(CliError::InvalidDeploymentCommand)?;
    let started_at_millis = started.epoch_millis();
    let finished_at_millis = finished.epoch_millis();
    let duration = finished_at_millis
        .checked_sub(started_at_millis)
        .ok_or(CliError::InvalidDeploymentCommand)?;
    if !(0..=MAX_DURATION_MILLIS).contains(&duration) {
        return Err(CliError::InvalidDeploymentCommand);
    }
    let commit_sha = options
        .commit_sha
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_commit_sha)
        .transpose()?;
    Ok(NormalizedDeployment {
        deployment_id: deployment_id.to_owned(),
        project_id,
        release: release.to_owned(),
        environment: environment.to_owned(),
        service_name: service_name.to_owned(),
        status: options.status,
        started_at: started_at.to_owned(),
        started_at_millis,
        finished_at: finished_at.to_owned(),
        finished_at_millis,
        commit_sha,
    })
}

/// Validates and lowercases one optional Git commit identity.
fn normalize_commit_sha(value: &str) -> Result<String, CliError> {
    if !(7..=64).contains(&value.len()) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CliError::InvalidDeploymentCommand);
    }
    Ok(value.to_ascii_lowercase())
}

/// Parses and cross-checks the complete successful response.
fn validated_response(
    expected: &NormalizedDeployment,
    body: &str,
) -> Result<DeploymentResponse, RuntimeError> {
    let response =
        serde_json::from_str::<DeploymentResponse>(body).map_err(|_error| invalid_response())?;
    let started = parse_rfc3339(response.started_at.as_str()).ok_or_else(invalid_response)?;
    let finished = parse_rfc3339(response.finished_at.as_str()).ok_or_else(invalid_response)?;
    let recorded = parse_rfc3339(response.recorded_at.as_str()).ok_or_else(invalid_response)?;
    let valid = response.schema_version == SCHEMA_VERSION
        && crate::ids::is_uuid(response.id.as_str())
        && response.deployment_id == expected.deployment_id
        && response.project_id == expected.project_id
        && response.release == expected.release
        && response.environment == expected.environment
        && response.service_name == expected.service_name
        && response.status == expected.status
        && response.commit_sha == expected.commit_sha
        && response.started_at.ends_with('Z')
        && response.finished_at.ends_with('Z')
        && response.recorded_at.ends_with('Z')
        && started.is_millisecond_normalized()
        && finished.is_millisecond_normalized()
        && recorded.is_millisecond_normalized()
        && started.epoch_millis() == expected.started_at_millis
        && finished.epoch_millis() == expected.finished_at_millis
        && safe_text(response.deployment_id.as_str(), 128)
        && safe_text(response.release.as_str(), 200)
        && safe_text(response.environment.as_str(), 64)
        && safe_text(response.service_name.as_str(), 200);
    valid.then_some(response).ok_or_else(invalid_response)
}

/// Reads a successful response incrementally and rejects oversized content.
async fn bounded_body(mut response: reqwest::Response) -> Result<String, RuntimeError> {
    if response.content_length().is_some_and(|length| {
        usize::try_from(length).map_or(true, |length| length > RESPONSE_LIMIT)
    }) {
        return Err(invalid_response());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_error| transport_error())? {
        if body.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
            return Err(invalid_response());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_error| invalid_response())
}

/// Writes a deterministic human receipt from validated fields only.
fn write_human<W: std::io::Write>(
    response: &DeploymentResponse,
    output: &mut W,
) -> Result<(), RuntimeError> {
    writeln!(output, "Deployment recorded: {}", response.deployment_id)?;
    writeln!(output, "Status: {}", response.status.as_str())?;
    writeln!(output, "Project: {}", response.project_id)?;
    writeln!(output, "Release: {}", response.release)?;
    writeln!(output, "Environment: {}", response.environment)?;
    writeln!(output, "Service: {}", response.service_name)?;
    writeln!(output, "Started: {}", response.started_at)?;
    writeln!(output, "Finished: {}", response.finished_at)?;
    if let Some(commit_sha) = response.commit_sha.as_deref() {
        writeln!(output, "Commit: {commit_sha}")?;
    }
    writeln!(
        output,
        "Next: explain this release with the same project, environment, and service."
    )?;
    Ok(())
}

/// Converts request failures into fixed, host-free deployment recovery.
fn request_error(error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::MissingToken | RuntimeError::Unavailable { .. } => error,
        RuntimeError::Cli(_)
        | RuntimeError::Io(_)
        | RuntimeError::Http(_)
        | RuntimeError::Api { .. }
        | RuntimeError::StatusUnavailable { .. }
        | RuntimeError::InvestigationResponseInvalid
        | RuntimeError::ExplainResponseInvalid
        | RuntimeError::AnalyticsOverviewResponseInvalid
        | RuntimeError::AnalyticsPropertiesResponseInvalid
        | RuntimeError::AnalyticsResponseInvalid
        | RuntimeError::AnalyticsFunnelResponseInvalid
        | RuntimeError::AnalyticsRetentionResponseInvalid
        | RuntimeError::AnalyticsLifecycleResponseInvalid
        | RuntimeError::AnalyticsSegmentResponseInvalid
        | RuntimeError::NativeDebugArtifactInvalid
        | RuntimeError::NativeDebugResponseInvalid
        | RuntimeError::NativeDebugVerificationFailed => transport_error(),
    }
}

/// Produces a fixed API error without reflecting backend or request content.
fn safe_api_error(status: u16, credential: &AuthCredential) -> RuntimeError {
    RuntimeError::Api {
        status,
        body: safe_error_body(status).to_owned(),
        auth_source: credential.source(),
        auth_label: credential.label(),
    }
}

/// Returns an allowlisted deployment error envelope for public output.
const fn safe_error_body(status: u16) -> &'static str {
    match status {
        401 => {
            r#"{"error":"account authentication is invalid","code":"unauthorized","next":"run logbrew login"}"#
        }
        403 => {
            r#"{"error":"deployment access is forbidden","code":"forbidden","next":"use an account with project access"}"#
        }
        404 => {
            r#"{"error":"deployment project was not found","code":"not_found","next":"check the active project id and account access"}"#
        }
        409 => {
            r#"{"error":"deployment identity already has different content","code":"idempotency_conflict","next":"retry the original deployment record or use a new deployment_id"}"#
        }
        422 => {
            r#"{"error":"deployment record is invalid","code":"validation_failed","next":"check the exact release scope and completed timestamps"}"#
        }
        429 => {
            r#"{"error":"deployment capture is rate limited","code":"rate_limited","next":"retry the same deployment id later"}"#
        }
        500..=599 => {
            r#"{"error":"deployment capture is temporarily unavailable","code":"server_error","next":"retry the same deployment id later"}"#
        }
        _ => {
            r#"{"error":"deployment capture returned an unexpected status","code":"unexpected_response","next":"retry the same deployment id or check API compatibility"}"#
        }
    }
}

/// Normalizes an HTTP(S) API origin without credentials, query, or fragment.
fn normalized_origin(value: &str) -> Result<String, RuntimeError> {
    let mut parsed = reqwest::Url::parse(value).map_err(|_error| transport_error())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !matches!(parsed.path(), "" | "/")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(transport_error());
    }
    parsed.set_path("");
    Ok(parsed.to_string().trim_end_matches('/').to_owned())
}

/// Rejects controls and display-direction characters in bounded text.
fn safe_text(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= limit
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

/// Parses one RFC3339 timestamp with a required UTC designator or numeric offset.
fn parse_rfc3339(value: &str) -> Option<ParsedTimestamp> {
    let bytes = value.as_bytes();
    if !(20..=35).contains(&bytes.len())
        || !bytes.is_ascii()
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let year = decimal(bytes.get(0..4)?)?;
    let month = decimal(bytes.get(5..7)?)?;
    let day = decimal(bytes.get(8..10)?)?;
    let hour = decimal(bytes.get(11..13)?)?;
    let minute = decimal(bytes.get(14..16)?)?;
    let second = decimal(bytes.get(17..19)?)?;
    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let mut index = 19;
    let nanoseconds = if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        parse_fraction(bytes.get(start..index)?)?
    } else {
        0
    };
    let offset_seconds = match bytes.get(index) {
        Some(b'Z') if index + 1 == bytes.len() => 0_i64,
        Some(sign @ (b'+' | b'-')) if index + 6 == bytes.len() => {
            if bytes.get(index + 3) != Some(&b':') {
                return None;
            }
            let offset_hour = decimal(bytes.get(index + 1..index + 3)?)?;
            let offset_minute = decimal(bytes.get(index + 4..index + 6)?)?;
            if offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            let magnitude = i64::from(offset_hour) * 3_600 + i64::from(offset_minute) * 60;
            if *sign == b'+' { magnitude } else { -magnitude }
        }
        Some(_) | None => return None,
    };
    let days = days_from_civil(i64::from(year), i64::from(month), i64::from(day));
    let local_seconds = days
        .checked_mul(86_400)?
        .checked_add(i64::from(hour).checked_mul(3_600)?)?
        .checked_add(i64::from(minute).checked_mul(60)?)?
        .checked_add(i64::from(second))?;
    Some(ParsedTimestamp {
        epoch_seconds: local_seconds.checked_sub(offset_seconds)?,
        nanoseconds,
    })
}

/// Parses one fixed-width ASCII decimal field.
fn decimal(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value.checked_mul(10)?.checked_add(u32::from(*byte - b'0')))
            .flatten()
    })
}

/// Expands one one-to-nine-digit fractional second to nanoseconds.
fn parse_fraction(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || bytes.len() > 9 || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let value = decimal(bytes)?;
    value.checked_mul(10_u32.checked_pow(u32::try_from(9_usize.checked_sub(bytes.len())?).ok()?)?)
}

/// Returns the valid day count for one Gregorian month.
const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Returns whether one Gregorian year is a leap year.
const fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// Converts one civil date to days relative to the Unix epoch.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Returns a fixed transport failure without host or request details.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "deployment capture could not be completed",
        next: "check network connectivity and retry the same deployment id",
    }
}

/// Returns a stable successful-response contract failure.
const fn invalid_response() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "deployment capture returned an invalid response",
        next: "retry the same deployment id; if it repeats, report the public response contract",
    }
}
