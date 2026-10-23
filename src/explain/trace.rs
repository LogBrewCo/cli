//! Trace investigation validation and bounded human rendering.

use serde_json::{Map, Value};

use super::contract::{
    nullable_w3c_id, optional_object_value, optional_safe_u64, optional_string, require_bool,
    require_exact_fields, require_string, require_string_equals, require_timestamp, require_u64,
    required_object, required_uuid_text, response_object, validate_nullable_object,
    validate_object_array, validate_schema_version_value, validate_string_array, validate_strings,
};
use super::{
    RELATED_ISSUE_LIMIT, append_actions, append_evidence, append_labeled_bool,
    append_labeled_integer, append_labeled_text, append_named_text, append_related_collections,
    append_string_array, append_timeline, field_text, invalid_response,
    validate_correlated_collection, validate_correlated_signal, validate_evidence,
    validate_items_collection, validate_next_actions, validate_timeline,
};
use crate::RuntimeError;

/// Exact application-reported code-location vocabulary.
const REPORTED_LOCATION_FIELDS: [&str; 7] = [
    "component",
    "module",
    "function",
    "file",
    "line",
    "column",
    "in_app",
];

/// Validates one versioned trace investigation envelope.
pub(super) fn validate_trace_response(
    value: &Value,
    expected_id: &str,
) -> Result<(), RuntimeError> {
    let response = response_object(
        value,
        &[
            "schema_version",
            "subject",
            "analysis",
            "spans",
            "correlations",
            "timeline",
            "evidence",
            "next_actions",
        ],
    )?;
    validate_schema_version_value(response, 3)?;
    let subject = required_object(response, "subject")?;
    require_string_equals(subject, "kind", "trace")?;
    require_string_equals(subject, "trace_id", expected_id)?;
    let _analyzed_span_count = require_u64(subject, "analyzed_span_count")?;
    let _error_span_count = require_u64(subject, "error_span_count")?;
    let _service_count = require_u64(subject, "service_count")?;
    let _project_count = require_u64(subject, "project_count")?;
    let _started_at = require_timestamp(subject, "started_at")?;
    require_nonnegative_integer(subject, "duration_ms")?;
    validate_string_array(subject, "releases", 256)?;
    validate_string_array(subject, "environments", 256)?;

    let analysis = required_object(response, "analysis")?;
    validate_strings(analysis, &["status", "causality"])?;
    for name in ["root_span", "first_error_span", "bottleneck_span"] {
        validate_nullable_object(analysis, name)?;
    }
    let _ = validate_object_array(analysis, "first_error_path", 256)?;
    let _ = validate_object_array(analysis, "bottleneck_path", 256)?;

    let _ = validate_items_collection(required_object(response, "spans")?, false)?;
    validate_trace_correlations(required_object(response, "correlations")?)?;
    validate_timeline(required_object(response, "timeline")?)?;
    validate_evidence(required_object(response, "evidence")?)?;
    validate_next_actions(response.get("next_actions"))
}

/// Validates trace correlation scope and bounded related evidence.
fn validate_trace_correlations(value: &Map<String, Value>) -> Result<(), RuntimeError> {
    let window = required_object(value, "window")?;
    let _since = require_timestamp(window, "since")?;
    let _until = require_timestamp(window, "until")?;
    let scopes = validate_object_array(window, "scopes", 256)?;
    for scope in scopes {
        let scope = scope.as_object().ok_or_else(invalid_response)?;
        let _project_id = required_uuid_text(scope, "project_id")?;
        validate_strings(scope, &["environment", "release"])?;
    }
    let _truncated = require_bool(window, "truncated")?;
    let _issues = validate_correlated_collection(
        required_object(value, "issues")?,
        RELATED_ISSUE_LIMIT,
        true,
        validate_trace_issue,
    )?;
    for name in ["logs", "actions", "metrics"] {
        let collection = required_object(value, name)?;
        for item in validate_items_collection(collection, true)? {
            let item = item.as_object().ok_or_else(invalid_response)?;
            let _span_id = nullable_w3c_id(item, "span_id", 16)?;
        }
    }
    Ok(())
}

/// Validates one exact-trace issue and its application-reported assessment.
fn validate_trace_issue(issue: &Map<String, Value>) -> Result<(), RuntimeError> {
    let cause = required_object(issue, "cause")?;
    let fix = required_object(issue, "fix")?;
    let mut core = issue.clone();
    let _span_id = nullable_w3c_id(&core, "span_id", 16)?;
    core.retain(|field, _value| !matches!(field.as_str(), "cause" | "fix" | "span_id"));
    let _id = validate_correlated_signal(
        &core,
        require_string(issue, "project_id")?,
        require_string(issue, "environment")?,
        require_string(issue, "release")?,
        "issue",
        None,
    )?;
    validate_reported_issue_assessment(cause, fix)
}

/// Validates the only two honest states emitted for a related issue preview.
fn validate_reported_issue_assessment(
    cause: &Map<String, Value>,
    fix: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    require_exact_fields(cause, &["status", "summary", "provenance", "signals"])?;
    let signals = cause["signals"].as_array().ok_or_else(invalid_response)?;
    let cause_valid = match (
        require_string(cause, "status")?,
        optional_string(cause, "summary")?,
        optional_string(cause, "provenance")?,
        signals.as_slice(),
    ) {
        ("reported_hypothesis", Some(_), Some("application_reported"), [Value::String(signal)]) => {
            signal == "reported_root_cause"
        }
        ("unknown", None, None, []) => true,
        _ => false,
    };
    require_exact_fields(fix, &["status", "location", "provenance"])?;
    if !cause_valid {
        return Err(invalid_response());
    }
    match (
        require_string(fix, "status")?,
        optional_string(fix, "provenance")?,
        optional_object_value(fix.get("location"))?,
    ) {
        ("reported_location", Some("application_reported"), Some(location)) => {
            validate_reported_location(location)
        }
        ("unknown", None, None) => Ok(()),
        _ => Err(invalid_response()),
    }
}

/// Validates one normalized application-reported code location without hidden fields.
fn validate_reported_location(location: &Map<String, Value>) -> Result<(), RuntimeError> {
    require_exact_fields(location, &REPORTED_LOCATION_FIELDS)?;
    let identities = ["component", "module", "function", "file"]
        .map(|name| optional_string(location, name))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let positions = ["line", "column"]
        .map(|name| optional_safe_u64(location, name))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    if positions
        .iter()
        .flatten()
        .any(|value| *value == 0 || *value > u64::from(u32::MAX))
        || !matches!(location.get("in_app"), Some(Value::Null | Value::Bool(_)))
        || (!identities.iter().any(Option::is_some) && !positions.iter().any(Option::is_some))
    {
        return Err(invalid_response());
    }
    Ok(())
}

/// Requires one integer that is not negative.
fn require_nonnegative_integer(value: &Map<String, Value>, name: &str) -> Result<(), RuntimeError> {
    if value
        .get(name)
        .and_then(Value::as_i64)
        .is_some_and(|value| value >= 0)
    {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Appends the shared exact-trace receipt used by issue and log investigations.
pub(super) fn append_trace_receipt(output: &mut String, trace: &Value, include_services: bool) {
    output.push_str("Trace:");
    append_labeled_text(output, "status", trace, "status", 40);
    append_labeled_text(output, "trace", trace, "trace_id", 80);
    append_labeled_text(output, "span", trace, "span_id", 40);
    if let Some(summary) = trace.get("summary").filter(|value| !value.is_null()) {
        append_labeled_integer(output, "spans", summary, "span_count");
        append_labeled_integer(output, "errors", summary, "error_span_count");
        if include_services {
            append_labeled_integer(output, "services", summary, "service_count");
        }
        append_labeled_integer(output, "duration_ms", summary, "duration_ms");
    }
    append_labeled_bool(output, "truncated", trace, "truncated");
    output.push('\n');
}

/// Builds a detailed trace failure and latency investigation.
pub(super) fn render_trace(value: &Value) -> Option<String> {
    let subject = value.get("subject")?;
    let analysis = value.get("analysis")?;
    let mut output = String::new();
    output.push_str("Trace ");
    output.push_str(field_text(subject, "trace_id", 80)?.as_str());
    append_labeled_text(&mut output, "status", analysis, "status", 64);
    append_labeled_text(&mut output, "causality", analysis, "causality", 64);
    append_labeled_integer(&mut output, "spans", subject, "analyzed_span_count");
    append_labeled_integer(&mut output, "errors", subject, "error_span_count");
    append_labeled_integer(&mut output, "services", subject, "service_count");
    append_labeled_integer(&mut output, "projects", subject, "project_count");
    append_labeled_integer(&mut output, "duration_ms", subject, "duration_ms");
    output.push('\n');
    append_named_text(&mut output, "Started", subject, "started_at", 64);
    append_string_array(&mut output, "Releases", subject.get("releases"), 8);
    append_string_array(&mut output, "Environments", subject.get("environments"), 8);
    append_trace_focus(&mut output, "Root", analysis.get("root_span"));
    append_trace_focus(&mut output, "First error", analysis.get("first_error_span"));
    append_trace_path(
        &mut output,
        "First error path",
        analysis.get("first_error_path"),
    );
    append_trace_focus(&mut output, "Bottleneck", analysis.get("bottleneck_span"));
    append_trace_path(
        &mut output,
        "Bottleneck path",
        analysis.get("bottleneck_path"),
    );
    append_trace_correlations(&mut output, value.get("correlations"));
    append_timeline(&mut output, value.get("timeline"));
    append_evidence(&mut output, value.get("evidence"));
    append_actions(&mut output, value.get("next_actions"));
    Some(output)
}

/// Appends one actionable span focus.
fn append_trace_focus(output: &mut String, label: &str, span: Option<&Value>) {
    let Some(span) = span.filter(|span| !span.is_null()) else {
        return;
    };
    let Some(name) = field_text(span, "name", 240) else {
        return;
    };
    output.push_str(label);
    output.push_str(": ");
    output.push_str(name.as_str());
    append_labeled_text(output, "service", span, "service_name", 160);
    append_labeled_text(output, "operation", span, "operation", 120);
    append_labeled_text(output, "status", span, "status", 40);
    append_labeled_integer(output, "duration_ms", span, "duration_ms");
    append_labeled_text(output, "span", span, "span_id", 40);
    append_labeled_text(output, "parent", span, "parent_span_id", 40);
    output.push('\n');
}

/// Appends a bounded ordered span path.
fn append_trace_path(output: &mut String, label: &str, path: Option<&Value>) {
    let Some(path) = path
        .and_then(Value::as_array)
        .filter(|path| !path.is_empty())
    else {
        return;
    };
    output.push_str(label);
    output.push_str(": ");
    let names = path
        .iter()
        .take(8)
        .filter_map(|span| field_text(span, "name", 120))
        .collect::<Vec<_>>();
    output.push_str(names.join(" -> ").as_str());
    if path.len() > 8 {
        output.push_str(" -> ...");
    }
    output.push('\n');
}

/// Appends exact-trace issue, log, action, and metric evidence.
fn append_trace_correlations(output: &mut String, correlations: Option<&Value>) {
    let Some(correlations) = correlations else {
        return;
    };
    append_related_collections(
        output,
        correlations,
        &[
            ("issues", "Related issues"),
            ("logs", "Related logs"),
            ("actions", "Related actions"),
            ("metrics", "Related metrics"),
        ],
    );
}
