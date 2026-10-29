//! Fail-closed validation for the issue investigation response.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::explain::contract::{
    MAX_SAFE_JSON_INTEGER, exact_response_object, optional_object_value, optional_safe_u64,
    optional_string, require_bool, require_exact_fields, require_safe_positive_u64,
    require_safe_u64, require_string, require_string_equals, require_timestamp,
    require_timestamp_millis, required_object, required_uuid_text, validate_name_version,
    validate_nullable_object, validate_object_array, validate_schema_version_value,
    validate_string_array, validate_strings,
};
use crate::explain::correlation::{
    DeploymentExpectation, validate_availability, validate_correlated_collection,
    validate_correlated_signal, validate_deployment_boundary, validate_items_collection,
    validate_release_scope, validate_shared_trace_summary, validate_trace_link,
};
use crate::explain::evidence::{
    EVIDENCE_CATEGORIES, evidence_has_field, validate_evidence, validate_evidence_vocabulary,
    validate_field_receipts, validate_next_actions,
};
use crate::explain::{
    RELATED_ISSUE_LIMIT, invalid_response, issue_exception_chain, issue_lifecycle,
    issue_occurrence_analysis,
};
use crate::{IssueOccurrenceSelection, RuntimeError};
use occurrence::{validate_issue_occurrence_selection, validate_selected_occurrence_correlations};
use user_impact::validate_issue_user_impact;

mod occurrence;
mod source;
mod user_impact;

/// Fixed backend candidate cap for algorithm-version-1 issue occurrence recommendations.
const ISSUE_OCCURRENCE_CANDIDATE_LIMIT: u64 = 50;
/// Maximum structured frames projected into one issue occurrence summary.
const ISSUE_OCCURRENCE_FRAME_LIMIT: u64 = 32;

/// Exact top-level vocabulary for the schema-version-17 issue response.
const ISSUE_RESPONSE_FIELDS: &[&str] = &[
    "schema_version",
    "subject",
    "event",
    "occurrence_selection",
    "lifecycle",
    "occurrence_analysis",
    "grouping",
    "cause",
    "fix",
    "source_locator",
    "suspect_commits",
    "reproduction",
    "impact",
    "correlations",
    "evidence",
    "next_actions",
];

/// Exact schema-version-6-and-later request receipt vocabulary.
const REQUEST_EVIDENCE_FIELDS: [&str; 5] = [
    "request",
    "request.body",
    "request.method",
    "request.route_template",
    "request.response_status_code",
];
/// Exact schema-version-7 grouping receipt vocabulary.
const GROUPING_EVIDENCE_FIELDS: [&str; 6] = [
    "grouping",
    "grouping.strategy",
    "grouping.components",
    "grouping.strategy_details",
    "grouping.stack",
    "grouping.stack_frames",
];
/// Exact schema-version-8 deployment receipt vocabulary.
const DEPLOYMENT_EVIDENCE_FIELDS: [&str; 4] = [
    "deployment",
    "deployment.commit_sha",
    "deployment.lookup",
    "deployment.timing",
];
/// Exact receipts emitted while resolving a customer-source locator.
const SOURCE_LOCATOR_EVIDENCE_FIELDS: [&str; 10] = [
    "source.service",
    "source.lookup",
    "source.component",
    "source.component.unique",
    "source.repository",
    "source.repository.unique",
    "source.deployment_revision",
    "source.setup_snapshot_revision",
    "source.revision",
    "source.repository_relative_file",
];

/// Validates one versioned issue investigation envelope.
pub(in crate::explain) fn validate_issue_response(
    value: &Value,
    expected_id: &str,
    expected_occurrence: &IssueOccurrenceSelection,
) -> Result<(), RuntimeError> {
    let response = exact_response_object(value, ISSUE_RESPONSE_FIELDS)?;
    validate_schema_version_value(response, 17)?;
    let subject = required_object(response, "subject")?;
    require_string_equals(subject, "kind", "issue")?;
    require_string_equals(subject, "id", expected_id)?;
    let project_id = required_uuid_text(subject, "project_id")?;
    let _fingerprint = require_string(subject, "fingerprint")?;
    let subject_status = require_string(subject, "status")?;
    if !issue_lifecycle::is_persisted_status(subject_status) {
        return Err(invalid_response());
    }
    validate_strings(subject, &["severity", "title", "message"])?;
    let occurrence_count = require_safe_positive_u64(subject, "occurrence_count")?;
    let first_seen = require_timestamp(subject, "first_seen_at")?;
    let last_seen = require_timestamp(subject, "last_seen_at")?;

    let event = required_object(response, "event")?;
    let _event_id = required_uuid_text(event, "id")?;
    let _occurred_at = require_timestamp(event, "occurred_at")?;
    validate_name_version(required_object(event, "sdk")?)?;
    validate_nullable_object(event, "context")?;
    validate_nullable_object(event, "exception")?;
    let _ = validate_object_array(event, "stack_frames", 256)?;
    validate_issue_breadcrumbs(validate_object_array(event, "breadcrumbs", 256)?)?;
    let _breadcrumbs_truncated = require_bool(event, "breadcrumbs_truncated")?;
    let evidence = required_object(response, "evidence")?;
    validate_evidence(evidence)?;
    validate_issue_request(event, evidence)?;
    validate_issue_grouping(required_object(response, "grouping")?, evidence)?;
    let stack_projection_receipted =
        evidence_has_field(evidence, "truncated_fields", "stack_frames")?;
    let selected_occurrence = validate_issue_occurrence_selection(
        required_object(response, "occurrence_selection")?,
        event,
        expected_occurrence,
        occurrence_count,
        first_seen,
        last_seen,
        stack_projection_receipted,
    )?;
    let candidates_truncated = occurrence_count > ISSUE_OCCURRENCE_CANDIDATE_LIMIT;
    validate_field_receipts(
        evidence,
        "occurrence.recommendation_candidates",
        [false, false, false, candidates_truncated],
    )?;
    if candidates_truncated && require_string(evidence, "status")? != "partial" {
        return Err(invalid_response());
    }
    let regression_detected = issue_lifecycle::validate(
        required_object(response, "lifecycle")?,
        subject_status,
        first_seen,
        last_seen,
        evidence,
    )?;
    issue_occurrence_analysis::validate(
        required_object(response, "occurrence_analysis")?,
        required_object(response, "occurrence_selection")?,
        occurrence_count,
        first_seen,
        last_seen,
        evidence,
    )?;

    let fix = validate_issue_cause_and_source(response, event, evidence)?;

    let impact = required_object(response, "impact")?;
    let impact_occurrence_count = require_safe_positive_u64(impact, "occurrence_count")?;
    let impact_first = require_timestamp(impact, "first_seen_at")?;
    let impact_last = require_timestamp(impact, "last_seen_at")?;
    if impact_occurrence_count != occurrence_count
        || impact_first != first_seen
        || impact_last != last_seen
    {
        return Err(invalid_response());
    }
    validate_issue_user_impact(impact, occurrence_count)?;
    validate_nullable_object(impact, "reported")?;
    let reproduction_ready = validate_issue_reproduction(
        required_object(response, "reproduction")?,
        event,
        impact,
        fix,
    )?;

    let correlations = required_object(response, "correlations")?;
    validate_issue_correlations(correlations, event, evidence, expected_id, project_id)?;
    validate_selected_occurrence_correlations(selected_occurrence, event, correlations)?;
    validate_issue_actions(
        response.get("next_actions"),
        reproduction_ready,
        regression_detected,
    )
}

/// Validates cause, fix, source locator, frame links, and suspect commits; returns the fix.
fn validate_issue_cause_and_source<'a>(
    response: &'a Map<String, Value>,
    event: &Map<String, Value>,
    evidence: &Map<String, Value>,
) -> Result<&'a Map<String, Value>, RuntimeError> {
    let cause = required_object(response, "cause")?;
    let _cause_status = require_string(cause, "status")?;
    let _cause_summary = optional_string(cause, "summary")?;
    let _cause_provenance = optional_string(cause, "provenance")?;
    validate_string_array(cause, "signals", 32)?;

    let fix = required_object(response, "fix")?;
    let _fix_status = require_string(fix, "status")?;
    validate_nullable_object(fix, "location")?;
    let _fix_provenance = optional_string(fix, "provenance")?;
    issue_exception_chain::validate(event, evidence, cause, fix)?;
    let locator = required_object(response, "source_locator")?;
    validate_issue_source_locator(locator, evidence, fix)?;
    source::validate_frame_links(locator, event)?;
    let suspects = required_object(response, "suspect_commits")?;
    source::validate_suspect_commits(suspects, event, evidence)?;
    Ok(fix)
}

/// Validates exact reproducer readiness and its phased evidence receipt.
fn validate_issue_reproduction(
    reproduction: &Map<String, Value>,
    event: &Map<String, Value>,
    impact: &Map<String, Value>,
    fix: &Map<String, Value>,
) -> Result<bool, RuntimeError> {
    require_exact_fields(
        reproduction,
        &["status", "baseline_occurrence_id", "evidence"],
    )?;
    require_string_equals(
        reproduction,
        "baseline_occurrence_id",
        require_string(event, "id")?,
    )?;
    let evidence = required_object(reproduction, "evidence")?;
    validate_evidence(evidence)?;
    let fields = [
        "reproduction.baseline_occurrence",
        "reproduction.trigger",
        "reproduction.expected_failure",
        "reproduction.code_location",
    ];
    validate_evidence_vocabulary(evidence, "reproduction", &fields)?;
    let request = event.get("request").filter(|value| !value.is_null());
    let captured = [
        true,
        request.is_some()
            || impact
                .get("reported")
                .and_then(|value| value.get("failed_action"))
                .and_then(Value::as_str)
                .is_some(),
        event.get("exception").is_some_and(|value| !value.is_null())
            || request
                .and_then(|value| value.get("response_status_code"))
                .is_some_and(|value| !value.is_null()),
        fix.get("location").is_some_and(|value| !value.is_null()),
    ];
    for (field, is_captured) in fields.into_iter().zip(captured) {
        validate_field_receipts(evidence, field, [is_captured, !is_captured, false, false])?;
    }
    let status = match (captured[1] && captured[2], captured[3]) {
        (true, true) => "ready",
        (true, false) => "partial",
        (false, _) => "insufficient_evidence",
    };
    require_string_equals(reproduction, "status", status)?;
    require_string_equals(
        evidence,
        "status",
        if captured.into_iter().all(|set| set) {
            "complete"
        } else {
            "partial"
        },
    )?;
    Ok(status == "ready")
}

/// Validates exact repository, component, revision, and source-path evidence.
fn validate_issue_source_locator(
    locator: &Map<String, Value>,
    overall: &Map<String, Value>,
    fix: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        locator,
        &[
            "status",
            "repository_provider",
            "repository_full_name",
            "component_path",
            "revision",
            "revision_source",
            "repository_relative_file",
            "frame_links",
            "evidence",
        ],
    )?;
    let status = validate_availability(locator, "status")?;
    let provider = optional_string(locator, "repository_provider")?;
    let repository = optional_string(locator, "repository_full_name")?;
    let component = optional_string(locator, "component_path")?;
    let revision = optional_string(locator, "revision")?;
    let revision_source = optional_string(locator, "revision_source")?;
    let file = optional_string(locator, "repository_relative_file")?;
    let evidence = required_object(locator, "evidence")?;
    validate_evidence(evidence)?;
    if provider.is_some() != repository.is_some()
        || (status == "available") != repository.is_some()
        || provider.is_some() && component.is_none()
        || status == "not_linked" && component.is_some()
        || component.is_none() && (revision.is_some() || file.is_some())
        || provider.is_some_and(|value| !matches!(value, "github" | "gitlab" | "bitbucket"))
        || repository.is_some_and(|value| !safe_repository_name(value))
        || component.is_some_and(|value| !safe_source_path(value, true))
        || revision.is_some() != revision_source.is_some()
        || revision.is_some_and(|value| !safe_revision(value))
        || revision_source
            .is_some_and(|value| !matches!(value, "deployment_commit" | "setup_snapshot"))
        || file.is_some_and(|value| !value.contains('/') || !safe_source_path(value, false))
        || file.is_some_and(|value| {
            component.is_none_or(|root| {
                root != "."
                    && !value
                        .strip_prefix(root)
                        .is_some_and(|tail| tail.starts_with('/'))
            })
        })
        || file.is_some_and(|value| {
            fix.get("location")
                .and_then(|location| location.get("file"))
                .and_then(Value::as_str)
                != Some(value)
        })
    {
        return Err(invalid_response());
    }
    let incomplete = validate_source_locator_evidence(locator, evidence, status)?;
    validate_evidence_vocabulary(
        overall,
        "source_locator",
        &["source_locator", "source_locator.complete"],
    )?;
    validate_field_receipts(overall, "source_locator", [true, false, false, false])?;
    validate_field_receipts(
        overall,
        "source_locator.complete",
        [false, incomplete, false, false],
    )
}

/// Validates one exact phased source-linkage receipt and returns whether it is partial.
fn validate_source_locator_evidence(
    locator: &Map<String, Value>,
    evidence: &Map<String, Value>,
    status: &str,
) -> Result<bool, RuntimeError> {
    let provider = optional_string(locator, "repository_provider")?;
    let component = optional_string(locator, "component_path")?;
    let revision_source = optional_string(locator, "revision_source")?;
    let file = optional_string(locator, "repository_relative_file")?;
    let (mut captured, mut missing) = match (status, component) {
        ("not_linked", None) => (vec![], vec!["source.service"]),
        ("not_found", None) => (vec!["source.service"], vec!["source.component"]),
        ("unavailable", None)
            if evidence_has_field(evidence, "missing_fields", "source.lookup")? =>
        {
            (vec!["source.service"], vec!["source.lookup"])
        }
        ("unavailable", None) => (vec!["source.service"], vec!["source.component.unique"]),
        (_, Some(_)) => (vec!["source.service", "source.component"], vec![]),
        _ => return Err(invalid_response()),
    };
    if component.is_some() {
        if provider.is_some() {
            captured.push("source.repository");
        } else if status == "unavailable"
            && evidence_has_field(evidence, "missing_fields", "source.repository.unique")?
        {
            missing.push("source.repository.unique");
        } else {
            missing.push("source.repository");
        }
        match revision_source {
            Some("deployment_commit") => captured.push("source.deployment_revision"),
            Some("setup_snapshot") => {
                captured.push("source.setup_snapshot_revision");
                missing.push("source.deployment_revision");
            }
            None => missing.push("source.revision"),
            _ => return Err(invalid_response()),
        }
        if file.is_some() {
            captured.push("source.repository_relative_file");
        } else {
            missing.push("source.repository_relative_file");
        }
    }
    if EVIDENCE_CATEGORIES.iter().any(|category| {
        evidence[*category].as_array().is_none_or(|fields| {
            fields.iter().any(|field| {
                field
                    .as_str()
                    .is_none_or(|field| !SOURCE_LOCATOR_EVIDENCE_FIELDS.contains(&field))
            })
        })
    }) || (require_string(evidence, "status")? == "complete") != missing.is_empty()
    {
        return Err(invalid_response());
    }
    for field in SOURCE_LOCATOR_EVIDENCE_FIELDS {
        validate_field_receipts(
            evidence,
            field,
            [
                captured.contains(&field),
                missing.contains(&field),
                false,
                false,
            ],
        )?;
    }
    Ok(!missing.is_empty())
}

/// Accepts one bounded provider revision.
fn safe_revision(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._".contains(&byte))
}

/// Accepts one bounded owner-qualified repository name.
fn safe_repository_name(value: &str) -> bool {
    value.len() <= 255 && value.split('/').count() >= 2 && safe_source_path(value, false)
}

/// Accepts a normalized repository-relative path, optionally including the root marker.
fn safe_source_path(value: &str, allow_root: bool) -> bool {
    (allow_root && value == ".")
        || value.len() <= 512
            && !value.starts_with('/')
            && !value.contains('\\')
            && value.split('/').all(|part| {
                !part.is_empty()
                    && !matches!(part, "." | "..")
                    && !part.chars().any(char::is_control)
            })
}

/// Validates only normalized, low-cardinality request evidence and its omission receipts.
fn validate_issue_request(
    event: &Map<String, Value>,
    evidence: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    validate_evidence_vocabulary(evidence, "request", &REQUEST_EVIDENCE_FIELDS)?;
    let body_receipts = EVIDENCE_CATEGORIES
        .map(|category| evidence_has_field(evidence, category, "request.body"))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    if body_receipts[0] || body_receipts.iter().filter(|present| **present).count() > 1 {
        return Err(invalid_response());
    }
    let Some(value) = event.get("request") else {
        let redacted = evidence_has_field(evidence, "redacted_fields", "request")?;
        let truncated = evidence_has_field(evidence, "truncated_fields", "request.route_template")?;
        if redacted && truncated {
            return Err(invalid_response());
        }
        for (field, expected) in [
            ("request", [false, true, redacted, false]),
            ("request.method", [false; 4]),
            ("request.route_template", [false, false, false, truncated]),
            ("request.response_status_code", [false; 4]),
        ] {
            validate_field_receipts(evidence, field, expected)?;
        }
        return require_string_equals(evidence, "status", "partial");
    };
    let request = value.as_object().ok_or_else(invalid_response)?;
    require_exact_fields(
        request,
        &["method", "route_template", "response_status_code"],
    )?;
    if !matches!(
        require_string(request, "method")?,
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "CONNECT"
    ) || !safe_route_template(require_string(request, "route_template")?)
    {
        return Err(invalid_response());
    }
    let status = optional_safe_u64(request, "response_status_code")?;
    if status.is_some_and(|value| !(100..=599).contains(&value)) {
        return Err(invalid_response());
    }
    for field in ["request", "request.method", "request.route_template"] {
        validate_field_receipts(evidence, field, [true, false, false, false])?;
    }
    let redacted = evidence_has_field(evidence, "redacted_fields", "request.response_status_code")?;
    validate_field_receipts(
        evidence,
        "request.response_status_code",
        [
            status.is_some(),
            status.is_none() && !redacted,
            redacted,
            false,
        ],
    )?;
    if status.is_none() {
        require_string_equals(evidence, "status", "partial")
    } else {
        Ok(())
    }
}

/// Validates one value-free grouping explanation and its exact evidence receipts.
fn validate_issue_grouping(
    grouping: &Map<String, Value>,
    evidence: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    require_exact_fields(grouping, &["strategy", "components", "stack"])?;
    validate_evidence_vocabulary(evidence, "grouping", &GROUPING_EVIDENCE_FIELDS)?;
    let strategy = require_string(grouping, "strategy")?;
    let expected_components: &[&str] = match strategy {
        "sdk_fingerprint" => &["sdk_fingerprint"],
        "default_exception_title_message_v1" => &["exception_type_or_title", "title", "message"],
        "default_exception_stack_v1" => &[
            "exception_type_or_title",
            "frame_module",
            "frame_function",
            "frame_filename",
        ],
        "custom_or_legacy" => &[],
        _ => return Err(invalid_response()),
    };
    let expects_stack = strategy == "default_exception_stack_v1";
    let components = grouping
        .get("components")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    if components.len() != expected_components.len()
        || components
            .iter()
            .zip(expected_components)
            .any(|(actual, expected)| actual.as_str() != Some(expected))
    {
        return Err(invalid_response());
    }
    let mut ignored = false;
    if expects_stack {
        let stack = grouping
            .get("stack")
            .and_then(Value::as_object)
            .ok_or_else(invalid_response)?;
        require_exact_fields(
            stack,
            &[
                "considered_frame_count",
                "frame_limit",
                "additional_frames_ignored",
            ],
        )?;
        ignored = require_bool(stack, "additional_frames_ignored")?;
        if !(1..=8).contains(&require_safe_u64(stack, "considered_frame_count")?)
            || require_safe_u64(stack, "frame_limit")? != 8
        {
            return Err(invalid_response());
        }
    } else if !grouping.get("stack").is_some_and(Value::is_null) {
        return Err(invalid_response());
    }
    for field in ["grouping", "grouping.strategy", "grouping.components"] {
        validate_field_receipts(evidence, field, [true, false, false, false])?;
    }
    let custom = strategy == "custom_or_legacy";
    validate_field_receipts(
        evidence,
        "grouping.strategy_details",
        [!custom, custom, false, false],
    )?;
    validate_field_receipts(
        evidence,
        "grouping.stack",
        [expects_stack, false, false, false],
    )?;
    validate_field_receipts(
        evidence,
        "grouping.stack_frames",
        [false, false, false, ignored],
    )?;
    if custom || ignored {
        require_string_equals(evidence, "status", "partial")
    } else {
        Ok(())
    }
}

/// Accepts parameterized route templates without concrete identifier segments.
fn safe_route_template(route: &str) -> bool {
    route == "<unmatched>"
        || route.starts_with('/')
            && route.chars().count() <= 256
            && !route.contains("//")
            && route.split('/').all(|segment| {
                let identifier = !segment.is_empty()
                    && (segment.bytes().all(|byte| byte.is_ascii_digit())
                        || segment.len() >= 16
                            && segment
                                .bytes()
                                .all(|byte| byte.is_ascii_hexdigit() || byte == b'-'));
                !identifier
                    && segment.len() <= 64
                    && segment.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || b"-._~:{}<>*[]()+".contains(&byte)
                    })
            })
}

/// Typed nullable `data` fields of every version-14-and-later issue breadcrumb.
const ISSUE_BREADCRUMB_DATA_FIELDS: &[&str] =
    &["method", "url", "status_code", "from", "to", "logger"];

/// Requires each breadcrumb's exact typed `data`: text fields or an HTTP status, each nullable.
fn validate_issue_breadcrumbs(breadcrumbs: &[Value]) -> Result<(), RuntimeError> {
    for breadcrumb in breadcrumbs {
        let data = breadcrumb
            .get("data")
            .and_then(Value::as_object)
            .ok_or_else(invalid_response)?;
        require_exact_fields(data, ISSUE_BREADCRUMB_DATA_FIELDS)?;
        let valid = data
            .iter()
            .all(|(name, value)| match (name.as_str(), value) {
                (_, Value::Null) => true,
                ("status_code", value) => value
                    .as_u64()
                    .is_some_and(|code| (100..=599).contains(&code)),
                (_, Value::String(text)) => !text.is_empty(),
                _ => false,
            });
        if !valid {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Validates issue correlation containers and exact release/deployment identity.
fn validate_issue_correlations(
    value: &Map<String, Value>,
    event: &Map<String, Value>,
    evidence: &Map<String, Value>,
    issue_id: &str,
    project_id: &str,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        value,
        &[
            "trace",
            "logs",
            "actions",
            "metrics",
            "related_issues",
            "release",
        ],
    )?;
    let trace = required_object(value, "trace")?;
    validate_trace_link(trace, false)?;
    let summary = optional_object_value(trace.get("summary"))?;
    if (require_string(trace, "status")? == "available") != summary.is_some() {
        return Err(invalid_response());
    }
    if let Some(summary) = summary {
        validate_shared_trace_summary(summary, None)?;
    }
    for name in ["logs", "actions", "metrics"] {
        let _ = validate_items_collection(required_object(value, name)?, true)?;
    }
    let release = required_object(value, "release")?;
    validate_release_scope(release)?;
    validate_related_issues(
        required_object(value, "related_issues")?,
        evidence,
        issue_id,
        project_id,
        require_string(release, "environment")?,
        require_string(release, "release")?,
        require_string(trace, "status")?,
    )?;
    validate_issue_deployment(release, event, evidence)
}

/// Validates bounded, distinct sibling issues from the selected occurrence's exact trace.
fn validate_related_issues(
    value: &Map<String, Value>,
    evidence: &Map<String, Value>,
    current_issue_id: &str,
    project_id: &str,
    environment: &str,
    release: &str,
    trace_status: &str,
) -> Result<(), RuntimeError> {
    validate_evidence_vocabulary(evidence, "related_issues", &["related_issues"])?;
    let mut issue_ids = BTreeSet::new();
    let (status, truncated, _) =
        validate_correlated_collection(value, RELATED_ISSUE_LIMIT, true, |item| {
            let signal = related_issue_signal(item)?;
            let issue_id = validate_correlated_signal(
                &signal,
                project_id,
                environment,
                release,
                "issue",
                Some(current_issue_id),
            )?;
            issue_ids
                .insert(issue_id.ok_or_else(invalid_response)?.to_owned())
                .then_some(())
                .ok_or_else(invalid_response)
        })?;
    // Same-frame relations need no trace, so only an unlinked read requires an unlinked trace.
    if status == "not_linked" && trace_status != "not_linked" {
        return Err(invalid_response());
    }
    validate_field_receipts(
        evidence,
        "related_issues",
        [
            status == "available",
            status != "available",
            false,
            truncated,
        ],
    )
}

/// Validates the relation fields of one related issue and returns the remaining signal fields.
fn related_issue_signal(item: &Map<String, Value>) -> Result<Map<String, Value>, RuntimeError> {
    let basis = item
        .get("basis")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    if basis.is_empty()
        || basis.len() > 2
        || basis
            .iter()
            .any(|value| !matches!(value.as_str(), Some("first_in_app_frame" | "same_trace")))
        || basis.first() == basis.get(1)
    {
        return Err(invalid_response());
    }
    let _count = require_safe_positive_u64(item, "occurrence_count")?;
    let first_seen = require_timestamp_millis(item, "first_seen_at")?;
    if first_seen > require_timestamp_millis(item, "last_seen_at")? {
        return Err(invalid_response());
    }
    let mut signal = item.clone();
    for name in ["basis", "occurrence_count", "first_seen_at", "last_seen_at"] {
        let _removed = signal.remove(name);
    }
    Ok(signal)
}

/// Validates the exact deployment preceding the selected occurrence and its evidence receipts.
fn validate_issue_deployment(
    release: &Map<String, Value>,
    event: &Map<String, Value>,
    evidence: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    validate_evidence_vocabulary(evidence, "deployment", &DEPLOYMENT_EVIDENCE_FIELDS)?;
    let status = require_string(release, "deployment_status")?;
    if !matches!(status, "available" | "not_found" | "unavailable") {
        return Err(invalid_response());
    }
    let available = status == "available";
    let not_found = status == "not_found";
    let mut fields = vec![
        "release",
        "environment",
        "service_name",
        "deployment_status",
    ];
    if available {
        fields.extend([
            "deployment",
            "time_since_deployment_ms",
            "deployment_causality",
        ]);
    }
    require_exact_fields(release, fields.as_slice())?;
    let commit = if available {
        let deployment = required_object(release, "deployment")?;
        let boundary = validate_deployment_boundary(
            deployment,
            DeploymentExpectation {
                release: Some(require_string(release, "release")?),
                environment: Some(require_string(release, "environment")?),
                service_name: Some(require_string(release, "service_name")?),
                ..DeploymentExpectation::default()
            },
        )?;
        let occurred = require_timestamp_millis(event, "occurred_at")?;
        let elapsed = occurred
            .checked_sub(boundary.finished_millis)
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value <= MAX_SAFE_JSON_INTEGER)
            .ok_or_else(invalid_response)?;
        if require_safe_u64(release, "time_since_deployment_ms")? != elapsed
            || require_string(release, "deployment_causality")? != "evidence_only"
        {
            return Err(invalid_response());
        }
        Some(optional_string(deployment, "commit_sha")?.is_some())
    } else {
        None
    };
    if !available || commit == Some(false) {
        require_string_equals(evidence, "status", "partial")?;
    }
    for (field, expected) in [
        ("deployment", [available, not_found, false, false]),
        (
            "deployment.commit_sha",
            [commit == Some(true), commit == Some(false), false, false],
        ),
        (
            "deployment.lookup",
            [not_found, !available && !not_found, false, false],
        ),
        ("deployment.timing", [available, false, false, false]),
    ] {
        validate_field_receipts(evidence, field, expected)?;
    }
    Ok(())
}

/// Validates evidence-bound issue actions after their common structure.
fn validate_issue_actions(
    value: Option<&Value>,
    reproduction_ready: bool,
    regression_detected: bool,
) -> Result<(), RuntimeError> {
    validate_next_actions(value)?;
    let actions = value
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    for (expected, code, target, reason) in [
        (
            reproduction_ready,
            "verify_correction",
            "issue_correction_verification",
            "reproduction_ready",
        ),
        (
            regression_detected,
            "compare_release",
            "release_summary",
            "regression_detected",
        ),
    ] {
        let matching = actions
            .iter()
            .filter_map(Value::as_object)
            .filter(|action| action.get("reason").and_then(Value::as_str) == Some(reason))
            .collect::<Vec<_>>();
        if matching.len() != usize::from(expected)
            || matching.iter().any(|action| {
                action.get("code").and_then(Value::as_str) != Some(code)
                    || action.get("target").and_then(Value::as_str) != Some(target)
            })
        {
            return Err(invalid_response());
        }
    }
    Ok(())
}
