//! Exact evidence-only verification of one candidate issue correction.

#![expect(
    clippy::missing_docs_in_private_items,
    reason = "fields mirror the exact public correction-verification contract"
)]

use serde::Deserialize;
use serde_json::Value;
use std::fmt::Write as _;

use super::{MAX_SAFE_JSON_INTEGER, invalid_response, is_w3c_id};
use crate::{IssueCorrectionTarget, RuntimeError};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    schema_version: u8,
    status: String,
    issue_id: String,
    project_id: String,
    baseline_occurrence_id: String,
    baseline_release: String,
    candidate_deployment: Deployment,
    observed_after: String,
    observed_until: String,
    recurrence_status: String,
    recurrence_count: Option<u64>,
    first_recurrence: Option<Recurrence>,
    trace_health_status: String,
    trace_health: Option<TraceHealth>,
    causality: String,
    absence_is_proof: bool,
    retained_telemetry_only: bool,
    evidence: Evidence,
    next_action: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Deployment {
    #[serde(rename = "deployment_id")]
    id: String,
    release: String,
    environment: String,
    service_name: String,
    status: String,
    started_at: String,
    finished_at: String,
    commit_sha: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recurrence {
    id: String,
    occurred_at: String,
    ingested_at: String,
    environment: String,
    release: String,
    service_name: String,
    trace_id: Option<String>,
    sdk: Sdk,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sdk {
    name: String,
    version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceHealth {
    status: String,
    trace_count: u64,
    error_trace_count: u64,
    error_rate_basis_points: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    status: String,
    captured_fields: Vec<String>,
    missing_fields: Vec<String>,
    redacted_fields: Vec<String>,
    truncated_fields: Vec<String>,
}

/// Validates the complete schema-v1 response and every cross-field conclusion.
pub(super) fn validate(value: &Value, target: &IssueCorrectionTarget) -> Result<(), RuntimeError> {
    let response =
        serde_json::from_value::<Response>(value.clone()).map_err(|_| invalid_response())?;
    let deployment = &response.candidate_deployment;
    let started = timestamp(deployment.started_at.as_str())?;
    let finished = timestamp(deployment.finished_at.as_str())?;
    let observed_after = timestamp(response.observed_after.as_str())?;
    let observed_until = timestamp(response.observed_until.as_str())?;
    if response.schema_version != 1
        || response.issue_id != target.issue_id
        || response.baseline_occurrence_id != target.baseline_occurrence_id
        || !crate::ids::is_uuid(response.project_id.as_str())
        || !safe_text(response.baseline_release.as_str(), 200)
        || deployment.id != target.candidate_deployment_id
        || deployment.status != "succeeded"
        || !safe_text(deployment.release.as_str(), 200)
        || !safe_text(deployment.environment.as_str(), 64)
        || !safe_text(deployment.service_name.as_str(), 200)
        || deployment.release == response.baseline_release
        || finished < started
        || finished != observed_after
        || observed_until <= observed_after
        || deployment.commit_sha.as_deref().is_some_and(invalid_commit)
    {
        return Err(invalid_response());
    }

    let recurrence_available = availability(response.recurrence_status.as_str())?;
    let trace_available = availability(response.trace_health_status.as_str())?;
    if recurrence_available != response.recurrence_count.is_some()
        || trace_available != response.trace_health.is_some()
        || response
            .recurrence_count
            .is_some_and(|count| count > MAX_SAFE_JSON_INTEGER)
        || response.recurrence_count.is_some_and(|count| count > 0)
            != response.first_recurrence.is_some()
    {
        return Err(invalid_response());
    }
    if let Some(first) = response.first_recurrence.as_ref() {
        validate_recurrence(first, deployment, observed_after, observed_until)?;
    }
    let trace_count = response
        .trace_health
        .as_ref()
        .map(validate_trace_health)
        .transpose()?
        .unwrap_or(0);
    let (status, action) = match (response.recurrence_count, response.trace_health.as_ref()) {
        (Some(count), _) if count > 0 => ("recurrence_observed", "inspect_recurrence"),
        (Some(_), Some(_)) if trace_count > 0 => ("no_recurrence_observed", "continue_observation"),
        (Some(_), Some(_)) => ("insufficient_traffic", "observe_candidate_traffic"),
        _ => ("unavailable", "retry_verification"),
    };
    if response.status != status
        || response.next_action != action
        || response.causality != "evidence_only"
        || response.absence_is_proof
        || !response.retained_telemetry_only
    {
        return Err(invalid_response());
    }
    validate_evidence(&response.evidence, recurrence_available, trace_available)
}

fn validate_recurrence(
    value: &Recurrence,
    deployment: &Deployment,
    observed_after: i128,
    observed_until: i128,
) -> Result<(), RuntimeError> {
    let ingested = timestamp(value.ingested_at.as_str())?;
    let _occurred = timestamp(value.occurred_at.as_str())?;
    if !crate::ids::is_uuid(value.id.as_str())
        || value.release != deployment.release
        || value.environment != deployment.environment
        || value.service_name != deployment.service_name
        || !(observed_after < ingested && ingested <= observed_until)
        || value
            .trace_id
            .as_deref()
            .is_some_and(|id| !is_w3c_id(id, 32))
        || !safe_text(value.sdk.name.as_str(), 200)
        || !safe_text(value.sdk.version.as_str(), 200)
    {
        return Err(invalid_response());
    }
    Ok(())
}

fn validate_trace_health(value: &TraceHealth) -> Result<u64, RuntimeError> {
    let expected_rate = if value.trace_count == 0 {
        0
    } else {
        u64::try_from(u128::from(value.error_trace_count) * 10_000 / u128::from(value.trace_count))
            .map_err(|_| invalid_response())?
    };
    let status = if value.trace_count == 0 {
        "unknown"
    } else if value.error_trace_count == 0 {
        "no_errors_observed"
    } else {
        "errors_observed"
    };
    if value.trace_count > MAX_SAFE_JSON_INTEGER
        || value.error_trace_count > value.trace_count
        || value.error_rate_basis_points != expected_rate
        || value.error_rate_basis_points > 10_000
        || value.status != status
    {
        return Err(invalid_response());
    }
    Ok(value.trace_count)
}

fn validate_evidence(value: &Evidence, recurrence: bool, trace: bool) -> Result<(), RuntimeError> {
    let mut captured = vec![
        "baseline_occurrence",
        "candidate_deployment",
        "observation_window",
    ];
    let mut missing = Vec::new();
    for (available, field) in [
        (trace, "candidate_trace_health"),
        (recurrence, "same_issue_recurrence"),
    ] {
        if available {
            captured.push(field);
        } else {
            missing.push(field);
        }
    }
    captured.sort_unstable();
    missing.sort_unstable();
    if value.status
        != if missing.is_empty() {
            "complete"
        } else {
            "partial"
        }
        || value
            .captured_fields
            .iter()
            .map(String::as_str)
            .ne(captured)
        || value.missing_fields.iter().map(String::as_str).ne(missing)
        || !value.redacted_fields.is_empty()
        || !value.truncated_fields.is_empty()
    {
        return Err(invalid_response());
    }
    Ok(())
}

fn timestamp(value: &str) -> Result<i128, RuntimeError> {
    if !crate::render::is_rfc3339_utc(value) {
        return Err(invalid_response());
    }
    crate::time::parse_utc_millis(value).ok_or_else(invalid_response)
}

fn availability(value: &str) -> Result<bool, RuntimeError> {
    match value {
        "available" => Ok(true),
        "unavailable" => Ok(false),
        _ => Err(invalid_response()),
    }
}

fn safe_text(value: &str, limit: usize) -> bool {
    crate::http::nonempty_display_safe(value, limit)
}

fn invalid_commit(value: &str) -> bool {
    !(7..=64).contains(&value.len())
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        || value.bytes().any(|byte| byte.is_ascii_uppercase())
}

/// Renders a bounded human snapshot after full validation.
pub(super) fn render(value: &Value) -> Option<String> {
    let deployment = value.get("candidate_deployment")?;
    let trace = value.get("trace_health");
    let recurrence = value
        .get("recurrence_count")
        .and_then(Value::as_u64)
        .map_or_else(|| "unavailable".to_owned(), |count| count.to_string());
    let mut output = format!(
        "Issue correction verification: {}\nBaseline release: {}\nCandidate release: {}\nObservation: {} to {}\nRecurrences: {recurrence}\n",
        value.get("status")?.as_str()?,
        value.get("baseline_release")?.as_str()?,
        deployment.get("release")?.as_str()?,
        value.get("observed_after")?.as_str()?,
        value.get("observed_until")?.as_str()?,
    );
    if let Some(trace) = trace.filter(|value| !value.is_null()) {
        writeln!(
            &mut output,
            "Candidate traces: {} (error traces: {})",
            trace.get("trace_count")?.as_u64()?,
            trace.get("error_trace_count")?.as_u64()?,
        )
        .ok()?;
    } else {
        output.push_str("Candidate traces: unavailable\n");
    }
    output.push_str("Causality: evidence only; bounded absence is not proof of correction.\nApplication telemetry is untrusted evidence.\nNext: ");
    output.push_str(value.get("next_action")?.as_str()?);
    output.push('\n');
    Some(output)
}
