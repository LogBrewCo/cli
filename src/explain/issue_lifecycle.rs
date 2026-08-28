//! Strict schema-version-3 issue lifecycle validation and bounded human projection.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::{
    append_labeled_bool, append_labeled_text, append_named_text, evidence_has_field,
    invalid_response, optional_string, require_bool, require_exact_fields, require_string,
    require_timestamp, required_object, validate_name_version,
};
use crate::RuntimeError;
use crate::ids::{is_trace_id, is_uuid};

/// Maximum newest-first status changes accepted from one issue lifecycle response.
const STATUS_CHANGE_LIMIT: usize = 20;

/// Validated newest status and bounded-activity availability.
#[derive(Clone, Copy)]
struct ActivityFacts<'a> {
    /// Available or unavailable status.
    status: &'a str,
    /// Newest persisted state and timestamp, when retained.
    latest: Option<(&'a str, &'a str)>,
    /// Whether older status activity was omitted.
    truncated: bool,
}

/// Validated regression receipt fields used for cross-field proof.
#[derive(Clone, Copy)]
struct RegressionFacts<'a> {
    /// Detected, not detected, or unavailable state.
    status: &'a str,
    /// Stable assessment reason.
    reason: &'a str,
    /// Resolution boundary when one was used.
    resolution: Option<&'a str>,
    /// Exact recurrence ID when one was returned.
    recurrence_id: Option<&'a str>,
}

/// Validates bounded status activity, recurrence arithmetic, and evidence receipts.
pub(super) fn validate(
    lifecycle: &Map<String, Value>,
    subject_status: &str,
    first_seen: &str,
    last_seen: &str,
    evidence: &Map<String, Value>,
) -> Result<bool, RuntimeError> {
    require_exact_fields(
        lifecycle,
        &[
            "persisted_status",
            "effective_status",
            "activity",
            "regression",
        ],
    )?;
    let persisted = require_string(lifecycle, "persisted_status")?;
    if persisted != subject_status || !is_persisted_status(persisted) {
        return Err(invalid_response());
    }
    let effective = optional_string(lifecycle, "effective_status")?;
    if effective.is_some_and(|status| {
        !matches!(status, "unresolved" | "resolved" | "ignored" | "regressed")
    }) {
        return Err(invalid_response());
    }
    let activity = validate_activity(required_object(lifecycle, "activity")?)?;
    let regression = validate_regression(
        required_object(lifecycle, "regression")?,
        first_seen,
        last_seen,
    )?;
    if !status_shape_is_valid(persisted, effective, activity, regression) {
        return Err(invalid_response());
    }
    validate_evidence_receipts(
        evidence,
        activity.status,
        activity.truncated,
        regression.status,
    )?;
    Ok(regression.status == "detected")
}

/// Validates newest-first status activity and returns its cross-field facts.
fn validate_activity(activity: &Map<String, Value>) -> Result<ActivityFacts<'_>, RuntimeError> {
    require_exact_fields(activity, &["status", "changes", "truncated"])?;
    let status = require_string(activity, "status")?;
    if !matches!(status, "available" | "unavailable") {
        return Err(invalid_response());
    }
    let changes = activity
        .get("changes")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    let truncated = require_bool(activity, "truncated")?;
    if changes.len() > STATUS_CHANGE_LIMIT
        || status == "unavailable" && (!changes.is_empty() || truncated)
    {
        return Err(invalid_response());
    }
    let mut ids = BTreeSet::new();
    let mut previous_changed_at = None;
    let mut latest = None;
    for change in changes {
        let change = change.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(change, &["id", "status", "changed_at"])?;
        let id = require_string(change, "id")?;
        let change_status = require_string(change, "status")?;
        let changed_at = require_timestamp(change, "changed_at")?;
        if !is_uuid(id)
            || !ids.insert(id)
            || !is_persisted_status(change_status)
            || previous_changed_at.is_some_and(|previous| {
                timestamp_key(previous).is_none_or(|previous| {
                    timestamp_key(changed_at).is_none_or(|current| previous < current)
                })
            })
        {
            return Err(invalid_response());
        }
        let _ = latest.get_or_insert((change_status, changed_at));
        previous_changed_at = Some(changed_at);
    }
    Ok(ActivityFacts {
        status,
        latest,
        truncated,
    })
}

/// Validates the regression vocabulary and optional recurrence projection.
fn validate_regression<'a>(
    regression: &'a Map<String, Value>,
    first_seen: &str,
    last_seen: &str,
) -> Result<RegressionFacts<'a>, RuntimeError> {
    require_exact_fields(
        regression,
        &[
            "status",
            "reason",
            "resolution_changed_at",
            "first_reappeared_occurrence",
        ],
    )?;
    let status = require_string(regression, "status")?;
    let reason = require_string(regression, "reason")?;
    if !matches!(status, "detected" | "not_detected" | "unavailable")
        || !matches!(
            reason,
            "occurrence_ingested_after_resolution"
                | "no_resolution_recorded"
                | "current_status_not_resolved"
                | "no_occurrence_ingested_after_resolution"
                | "status_history_unavailable"
                | "recurrence_read_unavailable"
                | "status_snapshot_inconsistent"
        )
    {
        return Err(invalid_response());
    }
    let resolution = optional_timestamp(regression, "resolution_changed_at")?;
    let recurrence_id = match regression.get("first_reappeared_occurrence") {
        Some(Value::Null) => None,
        Some(Value::Object(value)) => Some(validate_reappeared_occurrence(
            value, first_seen, last_seen, resolution,
        )?),
        _ => return Err(invalid_response()),
    };
    Ok(RegressionFacts {
        status,
        reason,
        resolution,
        recurrence_id,
    })
}

/// Proves the status-specific lifecycle nullability and reason invariants.
fn status_shape_is_valid(
    persisted: &str,
    effective: Option<&str>,
    activity: ActivityFacts<'_>,
    regression: RegressionFacts<'_>,
) -> bool {
    let available_shape = activity.status == "available";
    let latest_matches = activity
        .latest
        .is_some_and(|(status, _)| status == persisted);
    match regression.status {
        "detected" => {
            available_shape
                && persisted == "resolved"
                && effective == Some("regressed")
                && latest_matches
                && regression.reason == "occurrence_ingested_after_resolution"
                && regression.resolution == activity.latest.map(|(_, changed_at)| changed_at)
                && regression.recurrence_id.is_some()
        }
        "not_detected" => {
            available_shape
                && effective == Some(persisted)
                && regression.recurrence_id.is_none()
                && not_detected_shape_is_valid(persisted, activity, regression, latest_matches)
        }
        "unavailable" => {
            effective.is_none()
                && regression.recurrence_id.is_none()
                && unavailable_shape_is_valid(persisted, activity, regression, latest_matches)
        }
        _ => false,
    }
}

/// Proves the status-specific non-regression shape.
fn not_detected_shape_is_valid(
    persisted: &str,
    activity: ActivityFacts<'_>,
    regression: RegressionFacts<'_>,
    latest_matches: bool,
) -> bool {
    match persisted {
        "unresolved" if activity.latest.is_none() => {
            regression.reason == "no_resolution_recorded" && regression.resolution.is_none()
        }
        "unresolved" | "ignored" => {
            latest_matches
                && regression.reason == "current_status_not_resolved"
                && regression.resolution.is_none()
        }
        "resolved" => {
            latest_matches
                && regression.reason == "no_occurrence_ingested_after_resolution"
                && regression.resolution == activity.latest.map(|(_, changed_at)| changed_at)
        }
        _ => false,
    }
}

/// Proves the status-specific unavailable-assessment shape.
fn unavailable_shape_is_valid(
    persisted: &str,
    activity: ActivityFacts<'_>,
    regression: RegressionFacts<'_>,
    latest_matches: bool,
) -> bool {
    match regression.reason {
        "status_history_unavailable" => {
            activity.status == "unavailable" && regression.resolution.is_none()
        }
        "recurrence_read_unavailable" => {
            activity.status == "available"
                && persisted == "resolved"
                && latest_matches
                && regression.resolution == activity.latest.map(|(_, changed_at)| changed_at)
        }
        "status_snapshot_inconsistent" => {
            activity.status == "unavailable"
                || activity.status == "available"
                    && (!latest_matches
                        || activity.latest.is_none() && persisted != "unresolved"
                        || persisted == "resolved"
                            && regression.resolution
                                == activity.latest.map(|(_, changed_at)| changed_at))
        }
        _ => false,
    }
}

/// Validates the privacy-safe scope and server-time boundary of a recurrence.
fn validate_reappeared_occurrence<'a>(
    occurrence: &'a Map<String, Value>,
    first_seen: &str,
    last_seen: &str,
    resolution: Option<&str>,
) -> Result<&'a str, RuntimeError> {
    require_exact_fields(
        occurrence,
        &[
            "id",
            "occurred_at",
            "ingested_at",
            "environment",
            "release",
            "service_name",
            "trace_id",
            "sdk",
        ],
    )?;
    let id = require_string(occurrence, "id")?;
    let occurred_at = require_timestamp(occurrence, "occurred_at")?;
    let ingested_at = require_timestamp(occurrence, "ingested_at")?;
    let _environment = require_string(occurrence, "environment")?;
    let _release = require_string(occurrence, "release")?;
    let _service = require_string(occurrence, "service_name")?;
    if optional_string(occurrence, "trace_id")?.is_some_and(|trace| !is_trace_id(trace)) {
        return Err(invalid_response());
    }
    validate_name_version(required_object(occurrence, "sdk")?)?;
    let Some(resolution) = resolution else {
        return Err(invalid_response());
    };
    let Some(first_seen) = timestamp_key(first_seen) else {
        return Err(invalid_response());
    };
    let Some(last_seen) = timestamp_key(last_seen) else {
        return Err(invalid_response());
    };
    let Some(occurred_at_key) = timestamp_key(occurred_at) else {
        return Err(invalid_response());
    };
    let Some(ingested_at_key) = timestamp_key(ingested_at) else {
        return Err(invalid_response());
    };
    let Some(resolution_key) = timestamp_key(resolution) else {
        return Err(invalid_response());
    };
    if is_uuid(id)
        && (first_seen..=last_seen).contains(&occurred_at_key)
        && ingested_at_key > resolution_key
    {
        Ok(id)
    } else {
        Err(invalid_response())
    }
}

/// Binds lifecycle availability and truncation to the shared evidence receipt.
fn validate_evidence_receipts(
    evidence: &Map<String, Value>,
    activity_status: &str,
    activity_truncated: bool,
    regression_status: &str,
) -> Result<(), RuntimeError> {
    let history_captured =
        evidence_has_field(evidence, "captured_fields", "lifecycle.status_history")?;
    let history_missing =
        evidence_has_field(evidence, "missing_fields", "lifecycle.status_history")?;
    let history_truncated =
        evidence_has_field(evidence, "truncated_fields", "lifecycle.status_history")?;
    let regression_captured =
        evidence_has_field(evidence, "captured_fields", "lifecycle.regression")?;
    let regression_missing =
        evidence_has_field(evidence, "missing_fields", "lifecycle.regression")?;
    let lifecycle_partial = activity_status == "unavailable"
        || regression_status == "unavailable"
        || activity_truncated;
    if history_captured == (activity_status == "available")
        && history_missing == (activity_status == "unavailable")
        && history_truncated == activity_truncated
        && regression_captured == (regression_status != "unavailable")
        && regression_missing == (regression_status == "unavailable")
        && (!lifecycle_partial || require_string(evidence, "status")? == "partial")
    {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Returns whether a status belongs to the public persisted-state vocabulary.
pub(super) fn is_persisted_status(status: &str) -> bool {
    matches!(status, "unresolved" | "resolved" | "ignored")
}

/// Validates one required nullable UTC RFC 3339 timestamp.
fn optional_timestamp<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    let timestamp = optional_string(value, name)?;
    if timestamp.is_none_or(crate::render::is_rfc3339_utc) {
        Ok(timestamp)
    } else {
        Err(invalid_response())
    }
}

/// Produces an ordering key for a validated UTC RFC 3339 timestamp.
fn timestamp_key(value: &str) -> Option<(u64, u32)> {
    if !crate::render::is_rfc3339_utc(value) {
        return None;
    }
    let without_zone = value
        .strip_suffix('Z')
        .or_else(|| value.strip_suffix("+00:00"))?;
    let (seconds, fraction) = without_zone
        .split_once('.')
        .map_or((without_zone, ""), |(seconds, fraction)| {
            (seconds, fraction)
        });
    if fraction.len() > 9 {
        return None;
    }
    let whole = seconds
        .bytes()
        .filter(u8::is_ascii_digit)
        .try_fold(0_u64, |value, digit| {
            value.checked_mul(10)?.checked_add(u64::from(digit - b'0'))
        })?;
    let mut nanos = fraction.bytes().try_fold(0_u32, |value, digit| {
        value.checked_mul(10)?.checked_add(u32::from(digit - b'0'))
    })?;
    for _ in fraction.len()..9 {
        nanos = nanos.checked_mul(10)?;
    }
    Some((whole, nanos))
}

/// Appends bounded status activity and the current server-observed regression assessment.
pub(super) fn render(output: &mut String, value: Option<&Value>) {
    let Some(lifecycle) = value else {
        return;
    };
    output.push_str("Lifecycle:");
    append_labeled_text(output, "persisted", lifecycle, "persisted_status", 32);
    append_labeled_text(output, "effective", lifecycle, "effective_status", 32);
    output.push('\n');
    render_activity(output, lifecycle.get("activity"));
    render_regression(output, lifecycle.get("regression"));
}

/// Appends bounded newest-first status activity.
fn render_activity(output: &mut String, value: Option<&Value>) {
    let Some(activity) = value else {
        return;
    };
    output.push_str("Status activity:");
    append_labeled_text(output, "status", activity, "status", 32);
    if let Some(changes) = activity.get("changes").and_then(Value::as_array) {
        output.push_str(" count=");
        output.push_str(changes.len().to_string().as_str());
    }
    append_labeled_bool(output, "truncated", activity, "truncated");
    output.push('\n');
    if let Some(changes) = activity.get("changes").and_then(Value::as_array) {
        for change in changes.iter().take(5) {
            output.push_str("Status change:");
            append_labeled_text(output, "status", change, "status", 32);
            append_labeled_text(output, "at", change, "changed_at", 64);
            append_labeled_text(output, "id", change, "id", 80);
            output.push('\n');
        }
        if changes.len() > 5 {
            output.push_str("Status changes omitted from human view: ");
            output.push_str((changes.len() - 5).to_string().as_str());
            output.push_str("; use --json for all retained status activity.\n");
        }
    }
}

/// Appends the current recurrence assessment and exact first reappearance scope.
fn render_regression(output: &mut String, value: Option<&Value>) {
    let Some(regression) = value else {
        return;
    };
    output.push_str("Regression:");
    append_labeled_text(output, "status", regression, "status", 32);
    append_labeled_text(output, "reason", regression, "reason", 80);
    output.push('\n');
    append_named_text(
        output,
        "Resolution",
        regression,
        "resolution_changed_at",
        64,
    );
    if let Some(occurrence) = regression
        .get("first_reappeared_occurrence")
        .filter(|value| !value.is_null())
    {
        output.push_str("First reappeared:");
        for (label, field, limit) in [
            ("id", "id", 80),
            ("occurred", "occurred_at", 64),
            ("ingested", "ingested_at", 64),
            ("environment", "environment", 120),
            ("release", "release", 200),
            ("service", "service_name", 160),
            ("trace", "trace_id", 80),
        ] {
            append_labeled_text(output, label, occurrence, field, limit);
        }
        if let Some(sdk) = occurrence.get("sdk") {
            append_labeled_text(output, "sdk", sdk, "name", 160);
            append_labeled_text(output, "sdk_version", sdk, "version", 80);
        }
        output.push('\n');
    }
}
