//! Selected-occurrence validation for issue investigations.

use serde_json::{Map, Value};

use super::{ISSUE_OCCURRENCE_CANDIDATE_LIMIT, ISSUE_OCCURRENCE_FRAME_LIMIT};
use crate::explain::contract::{
    optional_string, require_bool, require_safe_positive_u64, require_safe_u64, require_string,
    require_string_equals, require_timestamp, require_u64, required_object, validate_name_version,
};
use crate::explain::{context, invalid_response};
use crate::ids::{is_trace_id, is_uuid};
use crate::{IssueOccurrenceSelection, RuntimeError};

/// Validated occurrence facts used to bind detailed evidence and correlations.
#[derive(Clone, Copy)]
pub(super) struct IssueOccurrenceFacts<'a> {
    /// Exact retained occurrence identity.
    id: &'a str,
    /// Exact client occurrence time.
    occurred_at: &'a str,
    /// Selected deployment environment.
    environment: &'a str,
    /// Selected application release.
    release: &'a str,
    /// Selected logical service.
    service_name: &'a str,
    /// Whether the selected occurrence captured an exact trace identifier.
    trace_linked: bool,
    /// Whether the selected occurrence produced typed context.
    context_captured: bool,
}

/// Exact summary objects participating in one selector decision.
#[derive(Clone, Copy)]
struct IssueOccurrenceViews<'a> {
    /// Detailed selected occurrence summary.
    selected: &'a Map<String, Value>,
    /// Earliest retained occurrence summary.
    first: &'a Map<String, Value>,
    /// Latest retained occurrence summary.
    latest: &'a Map<String, Value>,
    /// Bounded deterministic recommendation summary.
    recommended: &'a Map<String, Value>,
}

/// Validates selector semantics, boundaries, recommendation coverage, and detailed event identity.
pub(super) fn validate_issue_occurrence_selection<'a>(
    value: &'a Map<String, Value>,
    event: &Map<String, Value>,
    expected: &IssueOccurrenceSelection,
    occurrence_count: u64,
    first_seen: &str,
    last_seen: &str,
    stack_projection_receipted: bool,
) -> Result<IssueOccurrenceFacts<'a>, RuntimeError> {
    let requested = require_string(value, "requested")?;
    let reason = require_string(value, "reason")?;
    let selected_value = required_object(value, "selected")?;
    let first_value = required_object(value, "first")?;
    let latest_value = required_object(value, "latest")?;
    let recommended_value = required_object(value, "recommended")?;
    let selected = validate_issue_occurrence_summary(selected_value)?;
    let first = validate_issue_occurrence_summary(first_value)?;
    let latest = validate_issue_occurrence_summary(latest_value)?;
    let _recommended = validate_issue_occurrence_summary(recommended_value)?;
    let views = IssueOccurrenceViews {
        selected: selected_value,
        first: first_value,
        latest: latest_value,
        recommended: recommended_value,
    };

    if first.occurred_at != first_seen || latest.occurred_at != last_seen {
        return Err(invalid_response());
    }
    validate_issue_occurrence_request(
        expected,
        requested,
        reason,
        occurrence_count,
        views,
        selected.id,
    )?;
    validate_issue_occurrence_recommendation(
        required_object(value, "recommendation")?,
        occurrence_count,
    )?;
    validate_selected_occurrence_event(
        selected_value,
        selected,
        event,
        stack_projection_receipted,
    )?;
    Ok(selected)
}

/// Validates one privacy-safe occurrence comparison summary.
fn validate_issue_occurrence_summary(
    value: &Map<String, Value>,
) -> Result<IssueOccurrenceFacts<'_>, RuntimeError> {
    let id = require_string(value, "id")?;
    if !is_uuid(id) || !value.contains_key("exception_type") {
        return Err(invalid_response());
    }
    let occurred_at = require_timestamp(value, "occurred_at")?;
    let _severity = require_string(value, "severity")?;
    let environment = require_string(value, "environment")?;
    let release = require_string(value, "release")?;
    let service_name = require_string(value, "service_name")?;
    validate_name_version(required_object(value, "sdk")?)?;
    let _exception_type = optional_string(value, "exception_type")?;
    let trace_linked = require_bool(value, "trace_linked")?;
    let stack = required_object(value, "stack")?;
    let frame_count = require_safe_u64(stack, "frame_count")?;
    let _stack_truncated = require_bool(stack, "truncated")?;
    let breadcrumbs = required_object(value, "breadcrumbs")?;
    let breadcrumb_count = require_safe_u64(breadcrumbs, "count")?;
    let _breadcrumbs_truncated = require_bool(breadcrumbs, "truncated")?;
    let context_captured = require_bool(value, "context_captured")?;
    if frame_count > ISSUE_OCCURRENCE_FRAME_LIMIT || breadcrumb_count > 256 {
        return Err(invalid_response());
    }
    Ok(IssueOccurrenceFacts {
        id,
        occurred_at,
        environment,
        release,
        service_name,
        trace_linked,
        context_captured,
    })
}

/// Validates the requested selector, stable reason, and selected summary identity.
fn validate_issue_occurrence_request(
    expected: &IssueOccurrenceSelection,
    requested: &str,
    reason: &str,
    occurrence_count: u64,
    views: IssueOccurrenceViews<'_>,
    selected_id: &str,
) -> Result<(), RuntimeError> {
    let selector_matches = match expected {
        IssueOccurrenceSelection::Recommended => requested == "recommended",
        IssueOccurrenceSelection::First => requested == "first",
        IssueOccurrenceSelection::Latest => requested == "latest",
        IssueOccurrenceSelection::Exact(id) => requested == "exact" && selected_id == id,
    };
    let selection_matches = match requested {
        "recommended" => views.selected == views.recommended,
        "first" => views.selected == views.first,
        "latest" => views.selected == views.latest,
        "exact" => matches!(expected, IssueOccurrenceSelection::Exact(_)),
        _ => false,
    };
    let reason_matches = match requested {
        "recommended" if occurrence_count == 1 => reason == "only_retained_occurrence",
        "recommended" => {
            reason == "context_rich_recent_occurrence"
                || reason == "latest_occurrence_fallback" && views.recommended == views.latest
        }
        "first" => reason == "first_occurrence_requested",
        "latest" => reason == "latest_occurrence_requested",
        "exact" => reason == "exact_occurrence_requested",
        _ => false,
    };
    let single_occurrence_matches = occurrence_count != 1
        || views.first == views.latest
            && views.latest == views.recommended
            && views.recommended == views.selected;
    if selector_matches && selection_matches && reason_matches && single_occurrence_matches {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Validates deterministic recommendation algorithm and candidate-window arithmetic.
fn validate_issue_occurrence_recommendation(
    value: &Map<String, Value>,
    occurrence_count: u64,
) -> Result<(), RuntimeError> {
    let algorithm_version = require_u64(value, "algorithm_version")?;
    let candidate_count = require_safe_positive_u64(value, "candidate_count")?;
    let candidate_limit = require_safe_positive_u64(value, "candidate_limit")?;
    let truncated = require_bool(value, "candidate_window_truncated")?;
    if algorithm_version == 1
        && candidate_limit == ISSUE_OCCURRENCE_CANDIDATE_LIMIT
        && candidate_count == occurrence_count.min(ISSUE_OCCURRENCE_CANDIDATE_LIMIT)
        && truncated == (occurrence_count > ISSUE_OCCURRENCE_CANDIDATE_LIMIT)
    {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Binds the selected comparison summary to the full detailed event.
fn validate_selected_occurrence_event(
    selected: &Map<String, Value>,
    facts: IssueOccurrenceFacts<'_>,
    event: &Map<String, Value>,
    stack_projection_receipted: bool,
) -> Result<(), RuntimeError> {
    require_string_equals(event, "id", facts.id)?;
    require_string_equals(event, "occurred_at", facts.occurred_at)?;
    let selected_sdk = required_object(selected, "sdk")?;
    let event_sdk = required_object(event, "sdk")?;
    for field in ["name", "version"] {
        require_string_equals(event_sdk, field, require_string(selected_sdk, field)?)?;
    }
    let selected_exception = optional_string(selected, "exception_type")?;
    let event_exception = match event.get("exception") {
        Some(Value::Null) => None,
        Some(Value::Object(exception)) => Some(require_string(exception, "type")?),
        _ => return Err(invalid_response()),
    };
    let stack_count = event
        .get("stack_frames")
        .and_then(Value::as_array)
        .map(Vec::len)
        .and_then(|count| u64::try_from(count).ok())
        .ok_or_else(invalid_response)?;
    let breadcrumb_count = event
        .get("breadcrumbs")
        .and_then(Value::as_array)
        .map(Vec::len)
        .and_then(|count| u64::try_from(count).ok())
        .ok_or_else(invalid_response)?;
    let stack = required_object(selected, "stack")?;
    let breadcrumbs = required_object(selected, "breadcrumbs")?;
    let context_captured = context::validate(
        event.get("context"),
        context::Expected::issue(facts.service_name, facts.environment, facts.release),
    )?;
    let captured_frame_count = require_safe_u64(stack, "frame_count")?;
    let captured_stack_truncated = require_bool(stack, "truncated")?;
    let projected_frames_fit_capture = stack_count <= captured_frame_count;
    let projection_is_complete = stack_count == captured_frame_count && !captured_stack_truncated;
    let stack_projection_matches =
        projected_frames_fit_capture && (projection_is_complete || stack_projection_receipted);
    if selected_exception == event_exception
        && stack_projection_matches
        && require_safe_u64(breadcrumbs, "count")? == breadcrumb_count
        && require_bool(breadcrumbs, "truncated")? == require_bool(event, "breadcrumbs_truncated")?
        && facts.context_captured == context_captured
    {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Reads the exact selected-event trace identity from typed context.
fn selected_event_trace_id(event: &Map<String, Value>) -> Result<Option<&str>, RuntimeError> {
    let Some(context) = event.get("context").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(trace) = context.get("trace").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let trace = trace.as_object().ok_or_else(invalid_response)?;
    let trace_id = require_string(trace, "trace_id")?;
    if is_trace_id(trace_id) {
        Ok(Some(trace_id))
    } else {
        Err(invalid_response())
    }
}

/// Binds selected deployment and exact trace facts to every selected-scope correlation query.
pub(super) fn validate_selected_occurrence_correlations(
    selected: IssueOccurrenceFacts<'_>,
    event: &Map<String, Value>,
    correlations: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    let release = required_object(correlations, "release")?;
    let trace = required_object(correlations, "trace")?;
    let event_trace_id = selected_event_trace_id(event)?;
    let correlated_trace_id = optional_string(trace, "trace_id")?;
    let trace_status = require_string(trace, "status")?;
    let trace_status_matches = if selected.trace_linked {
        trace_status != "not_linked"
    } else {
        trace_status == "not_linked"
    };
    let summary_matches = match trace.get("summary") {
        None | Some(Value::Null) => true,
        Some(Value::Object(summary)) => correlated_trace_id.is_some_and(|trace_id| {
            summary.get("trace_id").and_then(Value::as_str) == Some(trace_id)
        }),
        Some(_) => false,
    };
    if require_string(release, "release")? == selected.release
        && require_string(release, "environment")? == selected.environment
        && require_string(release, "service_name")? == selected.service_name
        && correlated_trace_id.is_some() == selected.trace_linked
        && event_trace_id.is_none_or(|trace_id| correlated_trace_id == Some(trace_id))
        && event_trace_id.is_none_or(|_| selected.trace_linked)
        && trace_status_matches
        && summary_matches
    {
        Ok(())
    } else {
        Err(invalid_response())
    }
}
