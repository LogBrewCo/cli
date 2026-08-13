//! Strict semantic validation for exact-span investigation responses.

mod correlations;
mod timeline;
mod topology;

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use self::correlations::CorrelationFacts;
use self::topology::{BaselineFacts, TopologyFacts};
use super::super::projection::{
    count_scalar_leaves, sensitive_context_tag_key, sensitive_key, sensitive_string,
    validate_projection,
};
use super::super::{
    MAX_SAFE_JSON_INTEGER, invalid_response, is_error_status, is_w3c_id, nullable_w3c_id,
    require_bool, require_exact_fields, require_safe_u64, require_string, require_timestamp,
    require_u64, required_object, response_object, validate_schema_version,
};
use crate::ids::is_uuid;
use crate::{ExplainSpanTarget, RuntimeError};

/// Maximum retained scalar leaves across one span payload.
const PAYLOAD_LEAF_LIMIT: u64 = 64;
/// Maximum application events returned on one exact span.
const EVENT_LIMIT: usize = 8;
/// Maximum causal links returned on one exact span.
const LINK_LIMIT: usize = 8;
/// Maximum low-cardinality typed-context tags.
const CONTEXT_TAG_LIMIT: usize = 32;
/// Maximum typed-context tag-key length.
const CONTEXT_TAG_KEY_LIMIT: usize = 64;
/// Maximum typed-context string or tag-value length.
const CONTEXT_TEXT_LIMIT: usize = 256;
/// Maximum privacy-safe opaque context-identifier length.
const CONTEXT_ID_LIMIT: usize = 200;

/// Validated exact subject fields needed by every later invariant.
#[derive(Debug, Clone, Copy)]
pub(super) struct SubjectFacts<'a> {
    /// Stored project identity.
    pub(super) project_id: &'a str,
    /// Exact distributed-trace identity.
    pub(super) trace_id: &'a str,
    /// Exact selected span identity.
    pub(super) span_id: &'a str,
    /// Captured parent span identity.
    pub(super) parent_span_id: Option<&'a str>,
    /// Application-controlled span name.
    pub(super) name: &'a str,
    /// Low-cardinality operation.
    pub(super) operation: &'a str,
    /// Application-reported status.
    pub(super) status: Option<&'a str>,
    /// UTC start timestamp.
    pub(super) started_at: &'a str,
    /// Non-negative duration.
    pub(super) duration_ms: u64,
    /// Logical service identity.
    pub(super) service_name: &'a str,
    /// Exact environment.
    pub(super) environment: &'a str,
    /// Exact release.
    pub(super) release: &'a str,
    /// Whether both SDK identity fields were captured.
    pub(super) sdk_captured: bool,
}

/// Validated payload receipt needed by evidence checks.
#[derive(Debug, Clone, Copy)]
pub(super) struct PayloadFacts {
    /// Whether privacy filtering removed payload fields.
    pub(super) redacted: bool,
    /// Whether response bounds clipped payload fields.
    pub(super) truncated: bool,
}

/// Validates a complete exact-span v1 response and all cross-field invariants.
pub(super) fn validate_response(
    value: &Value,
    expected: &ExplainSpanTarget,
) -> Result<(), RuntimeError> {
    let response = response_object(
        value,
        &[
            "schema_version",
            "subject",
            "context",
            "payload",
            "analysis",
            "topology",
            "baseline",
            "correlations",
            "timeline",
            "evidence",
            "next_actions",
        ],
    )?;
    require_exact_fields(
        response,
        &[
            "schema_version",
            "subject",
            "context",
            "payload",
            "analysis",
            "topology",
            "baseline",
            "correlations",
            "timeline",
            "evidence",
            "next_actions",
        ],
    )?;
    validate_schema_version(response)?;
    let subject = validate_subject(required_object(response, "subject")?, expected)?;
    validate_context(response.get("context"), &subject)?;
    let payload = validate_payload(required_object(response, "payload")?)?;
    let topology = topology::validate_topology(required_object(response, "topology")?, &subject)?;
    let baseline = topology::validate_baseline(required_object(response, "baseline")?, &subject)?;
    let correlations = correlations::validate_correlations(
        required_object(response, "correlations")?,
        &subject,
        &topology,
    )?;
    validate_analysis(
        required_object(response, "analysis")?,
        &subject,
        &topology,
        &baseline,
        &correlations,
    )?;
    timeline::validate_timeline(
        required_object(response, "timeline")?,
        required_object(response, "payload")?,
        required_object(response, "correlations")?,
        &subject,
    )?;
    let evidence = timeline::validate_evidence(
        required_object(response, "evidence")?,
        response.get("context"),
        &subject,
        payload,
        &topology,
        &baseline,
        &correlations,
        required_object(response, "timeline")?,
    )?;
    timeline::validate_next_actions(
        response.get("next_actions"),
        &subject,
        &topology,
        &baseline,
        &correlations,
        evidence,
    )
}

/// Validates the selected row and binds it to every explicit command scope field.
fn validate_subject<'a>(
    subject: &'a Map<String, Value>,
    expected: &ExplainSpanTarget,
) -> Result<SubjectFacts<'a>, RuntimeError> {
    require_exact_fields(
        subject,
        &[
            "kind",
            "id",
            "project_id",
            "trace_id",
            "span_id",
            "parent_span_id",
            "name",
            "operation",
            "status",
            "started_at",
            "duration_ms",
            "service_name",
            "environment",
            "release",
            "sdk",
            "content_trust",
        ],
    )?;
    if require_string(subject, "kind")? != "span"
        || require_string(subject, "content_trust")? != "untrusted_telemetry"
        || !is_uuid(require_string(subject, "id")?)
    {
        return Err(invalid_response());
    }
    let project_id = require_string(subject, "project_id")?;
    let trace_id = require_string(subject, "trace_id")?;
    let span_id = require_string(subject, "span_id")?;
    let parent_span_id = nullable_w3c_id(subject, "parent_span_id", 16)?;
    let environment = require_string(subject, "environment")?;
    let release = require_string(subject, "release")?;
    if project_id != expected.project_id
        || trace_id != expected.trace_id
        || span_id != expected.span_id
        || environment != expected.environment
        || release != expected.release
        || !is_uuid(project_id)
        || !is_w3c_id(trace_id, 32)
        || !is_w3c_id(span_id, 16)
    {
        return Err(invalid_response());
    }
    let sdk = required_object(subject, "sdk")?;
    require_exact_fields(sdk, &["name", "version"])?;
    let sdk_name = string_allow_empty(sdk, "name")?;
    let sdk_version = string_allow_empty(sdk, "version")?;
    if sdk_name.is_empty() != sdk_version.is_empty() {
        return Err(invalid_response());
    }
    let duration_ms = require_safe_u64(subject, "duration_ms")?;
    Ok(SubjectFacts {
        project_id,
        trace_id,
        span_id,
        parent_span_id,
        name: require_string(subject, "name")?,
        operation: require_string(subject, "operation")?,
        status: nullable_string(subject, "status")?,
        started_at: require_timestamp(subject, "started_at")?,
        duration_ms,
        service_name: require_string(subject, "service_name")?,
        environment,
        release,
        sdk_captured: !sdk_name.is_empty(),
    })
}

/// Validates strict shared telemetry context and exact trace/deployment bindings.
fn validate_context(value: Option<&Value>, subject: &SubjectFacts<'_>) -> Result<(), RuntimeError> {
    let Some(value) = value else {
        return Err(invalid_response());
    };
    let Value::Object(context) = value else {
        return if value.is_null() {
            Ok(())
        } else {
            Err(invalid_response())
        };
    };
    require_exact_fields(
        context,
        &[
            "schema_version",
            "resource",
            "trace",
            "session",
            "subject",
            "tags",
        ],
    )?;
    if require_u64(context, "schema_version")? != 1 {
        return Err(invalid_response());
    }
    let resource_present = validate_resource_context(context.get("resource"), subject)?;
    let trace_present = validate_trace_context(context.get("trace"), subject)?;
    let session_present = validate_session_context(context.get("session"))?;
    let subject_present = validate_subject_context(context.get("subject"))?;
    let tags = required_object(context, "tags")?;
    validate_context_tags(tags)?;
    if !resource_present
        && !trace_present
        && !session_present
        && !subject_present
        && tags.is_empty()
    {
        return Err(invalid_response());
    }
    Ok(())
}

/// Validates optional typed resource context and returns whether it was present.
fn validate_resource_context(
    value: Option<&Value>,
    subject: &SubjectFacts<'_>,
) -> Result<bool, RuntimeError> {
    let Some(resource) = required_nullable_object(value)? else {
        return Ok(false);
    };
    require_exact_fields(
        resource,
        &[
            "service",
            "deployment",
            "runtime",
            "framework",
            "operating_system",
            "device",
            "application",
        ],
    )?;
    for name in ["service", "runtime", "framework"] {
        if let Some(identity) = required_nullable_object(resource.get(name))? {
            require_exact_fields(identity, &["name", "version"])?;
            let identity_name = require_string(identity, "name")?;
            let _version = nullable_bounded_text(identity, "version")?;
            if name == "service" && identity_name != subject.service_name {
                return Err(invalid_response());
            }
        }
    }
    if let Some(deployment) = required_nullable_object(resource.get("deployment"))? {
        require_exact_fields(deployment, &["environment", "release"])?;
        if nullable_bounded_text(deployment, "environment")?
            .is_some_and(|value| value != subject.environment)
            || nullable_bounded_text(deployment, "release")?
                .is_some_and(|value| value != subject.release)
        {
            return Err(invalid_response());
        }
    }
    if let Some(os) = required_nullable_object(resource.get("operating_system"))? {
        require_exact_fields(os, &["name", "version", "build"])?;
        let _name = require_string(os, "name")?;
        let _version = nullable_bounded_text(os, "version")?;
        let _build = nullable_bounded_text(os, "build")?;
    }
    if let Some(device) = required_nullable_object(resource.get("device"))? {
        require_exact_fields(device, &["family", "model", "architecture"])?;
        for name in ["family", "model", "architecture"] {
            let _value = nullable_bounded_text(device, name)?;
        }
    }
    if let Some(application) = required_nullable_object(resource.get("application"))? {
        require_exact_fields(application, &["name", "version", "build"])?;
        for name in ["name", "version", "build"] {
            let _value = nullable_bounded_text(application, name)?;
        }
    }
    Ok(true)
}

/// Validates optional W3C context and returns whether it was present.
fn validate_trace_context(
    value: Option<&Value>,
    subject: &SubjectFacts<'_>,
) -> Result<bool, RuntimeError> {
    let Some(trace) = required_nullable_object(value)? else {
        return Ok(false);
    };
    require_exact_fields(trace, &["trace_id", "span_id", "parent_span_id", "sampled"])?;
    if require_string(trace, "trace_id")? != subject.trace_id
        || nullable_w3c_id(trace, "span_id", 16)? != Some(subject.span_id)
        || nullable_w3c_id(trace, "parent_span_id", 16)? != subject.parent_span_id
        || !matches!(trace.get("sampled"), Some(Value::Null | Value::Bool(_)))
    {
        return Err(invalid_response());
    }
    Ok(true)
}

/// Validates optional privacy-bounded session context.
fn validate_session_context(value: Option<&Value>) -> Result<bool, RuntimeError> {
    let Some(session) = required_nullable_object(value)? else {
        return Ok(false);
    };
    require_exact_fields(session, &["id", "previous_id"])?;
    for name in ["id", "previous_id"] {
        if let Some(id) = nullable_string(session, name)? {
            validate_opaque_context_id(id)?;
        }
    }
    Ok(true)
}

/// Validates optional privacy-bounded user or anonymous-subject context.
fn validate_subject_context(value: Option<&Value>) -> Result<bool, RuntimeError> {
    let Some(context_subject) = required_nullable_object(value)? else {
        return Ok(false);
    };
    require_exact_fields(context_subject, &["id", "kind"])?;
    if let Some(id) = nullable_string(context_subject, "id")? {
        validate_opaque_context_id(id)?;
    }
    if !matches!(
        require_string(context_subject, "kind")?,
        "user" | "anonymous"
    ) {
        return Err(invalid_response());
    }
    Ok(true)
}

/// Validates deterministic low-cardinality context tags without direct identifiers.
fn validate_context_tags(tags: &Map<String, Value>) -> Result<(), RuntimeError> {
    if tags.len() > CONTEXT_TAG_LIMIT {
        return Err(invalid_response());
    }
    let mut previous = None;
    for (key, value) in tags {
        if previous.is_some_and(|previous: &str| previous >= key.as_str()) {
            return Err(invalid_response());
        }
        previous = Some(key.as_str());
        let text = value
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(invalid_response)?;
        let mut characters = key.chars();
        if !characters
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
            || key.chars().count() > CONTEXT_TAG_KEY_LIMIT
            || !characters
                .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character))
            || text.chars().count() > CONTEXT_TEXT_LIMIT
            || text.chars().any(char::is_control)
            || sensitive_key(key)
            || sensitive_context_tag_key(key)
            || sensitive_string(text, key)
        {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Validates bounded metadata, application events, causal links, and their leaf receipt.
fn validate_payload(payload: &Map<String, Value>) -> Result<PayloadFacts, RuntimeError> {
    require_exact_fields(
        payload,
        &[
            "metadata",
            "events",
            "links",
            "included_leaf_count",
            "redacted",
            "truncated",
        ],
    )?;
    let metadata = required_object(payload, "metadata")?;
    require_exact_fields(
        metadata,
        &["values", "included_leaf_count", "redacted", "truncated"],
    )?;
    let metadata_values = metadata.get("values").ok_or_else(invalid_response)?;
    if !metadata_values.is_object() {
        return Err(invalid_response());
    }
    validate_projection(metadata_values)?;
    let metadata_leaves = require_safe_u64(metadata, "included_leaf_count")?;
    if metadata_leaves != count_scalar_leaves(metadata_values)
        || metadata_leaves > PAYLOAD_LEAF_LIMIT
    {
        return Err(invalid_response());
    }
    let metadata_redacted = require_bool(metadata, "redacted")?;
    let metadata_truncated = require_bool(metadata, "truncated")?;
    let events = required_array(payload, "events", EVENT_LIMIT)?;
    let links = required_array(payload, "links", LINK_LIMIT)?;
    let mut leaves = metadata_leaves;
    for event in events {
        leaves = leaves.saturating_add(validate_event(event)?);
    }
    for link in links {
        leaves = leaves.saturating_add(validate_link(link)?);
    }
    let included_leaf_count = require_safe_u64(payload, "included_leaf_count")?;
    let redacted = require_bool(payload, "redacted")?;
    let truncated = require_bool(payload, "truncated")?;
    if included_leaf_count != leaves
        || included_leaf_count > PAYLOAD_LEAF_LIMIT
        || metadata_redacted && !redacted
        || metadata_truncated && !truncated
    {
        return Err(invalid_response());
    }
    Ok(PayloadFacts {
        redacted,
        truncated,
    })
}

/// Validates one application-reported event and returns its scalar-leaf contribution.
fn validate_event(value: &Value) -> Result<u64, RuntimeError> {
    let event = value.as_object().ok_or_else(invalid_response)?;
    require_exact_fields(event, &["name", "timestamp", "offset_ms", "metadata"])?;
    let _name = require_string(event, "name")?;
    let timestamp = nullable_timestamp(event, "timestamp")?;
    let offset = nullable_safe_i64(event, "offset_ms")?;
    if timestamp.is_some() != offset.is_some() {
        return Err(invalid_response());
    }
    let metadata = event.get("metadata").ok_or_else(invalid_response)?;
    validate_projection(metadata)?;
    Ok(1_u64
        .saturating_add(u64::from(timestamp.is_some()))
        .saturating_add(count_scalar_leaves(metadata)))
}

/// Validates one causal link and returns its scalar-leaf contribution.
fn validate_link(value: &Value) -> Result<u64, RuntimeError> {
    let link = value.as_object().ok_or_else(invalid_response)?;
    require_exact_fields(link, &["trace_id", "span_id", "sampled", "metadata"])?;
    if !is_w3c_id(require_string(link, "trace_id")?, 32)
        || !is_w3c_id(require_string(link, "span_id")?, 16)
        || !matches!(link.get("sampled"), Some(Value::Null | Value::Bool(_)))
    {
        return Err(invalid_response());
    }
    let metadata = link.get("metadata").ok_or_else(invalid_response)?;
    validate_projection(metadata)?;
    Ok(2_u64
        .saturating_add(u64::from(
            link.get("sampled").is_some_and(Value::is_boolean),
        ))
        .saturating_add(count_scalar_leaves(metadata)))
}

/// Validates analysis as an exact deterministic projection of returned evidence.
fn validate_analysis(
    analysis: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
    topology: &TopologyFacts<'_>,
    baseline: &BaselineFacts<'_>,
    correlations: &CorrelationFacts<'_>,
) -> Result<(), RuntimeError> {
    require_exact_fields(analysis, &["status", "causality", "observations"])?;
    if require_string(analysis, "causality")? != "evidence_only" {
        return Err(invalid_response());
    }
    let mut expected = Vec::new();
    let subject_error = is_error_status(subject.status);
    if subject_error {
        expected.push("subject_error");
    }
    if topology.error_child_span_id.is_some() {
        expected.push("child_error");
    }
    if correlations.exact_error_log {
        expected.push("exact_span_error_log");
    }
    if !correlations.issues.is_empty() {
        expected.push("related_issue");
    }
    if baseline.at_or_above_p95 {
        expected.push("subject_at_or_above_peer_p95");
    }
    if baseline.at_or_above_p99 {
        expected.push("subject_at_or_above_peer_p99");
    }
    let observations = analysis
        .get("observations")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    if observations.len() != expected.len()
        || observations
            .iter()
            .zip(expected)
            .any(|(actual, expected)| actual.as_str() != Some(expected))
    {
        return Err(invalid_response());
    }
    let expected_status = if subject_error
        || topology.error_child_span_id.is_some()
        || correlations.exact_error_log
        || !correlations.issues.is_empty()
    {
        "error_evidence"
    } else if baseline.at_or_above_p95 {
        "latency_focus"
    } else {
        "no_failure_observed"
    };
    if require_string(analysis, "status")? != expected_status {
        return Err(invalid_response());
    }
    Ok(())
}

/// Returns a required bounded array field.
pub(super) fn required_array<'a>(
    value: &'a Map<String, Value>,
    name: &str,
    limit: usize,
) -> Result<&'a [Value], RuntimeError> {
    value
        .get(name)
        .and_then(Value::as_array)
        .filter(|items| items.len() <= limit)
        .map(Vec::as_slice)
        .ok_or_else(invalid_response)
}

/// Returns an explicitly nullable object while rejecting omission and other types.
pub(super) const fn required_nullable_object(
    value: Option<&Value>,
) -> Result<Option<&Map<String, Value>>, RuntimeError> {
    match value {
        Some(Value::Null) => Ok(None),
        Some(Value::Object(value)) => Ok(Some(value)),
        _ => Err(invalid_response()),
    }
}

/// Returns an explicitly nullable non-empty string.
pub(super) fn nullable_string<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value.as_str())),
        _ => Err(invalid_response()),
    }
}

/// Returns a required string while permitting an explicit empty capture receipt.
fn string_allow_empty<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, RuntimeError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(invalid_response)
}

/// Returns an explicitly nullable bounded context string.
fn nullable_bounded_text<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    let text = nullable_string(value, name)?;
    if text.is_some_and(|text| {
        text.chars().count() > CONTEXT_TEXT_LIMIT || text.chars().any(char::is_control)
    }) {
        Err(invalid_response())
    } else {
        Ok(text)
    }
}

/// Returns an explicitly nullable UTC RFC3339 timestamp.
pub(super) fn nullable_timestamp<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(timestamp)) if crate::render::is_rfc3339_utc(timestamp) => {
            Ok(Some(timestamp.as_str()))
        }
        _ => Err(invalid_response()),
    }
}

/// Returns an explicitly nullable safe signed JSON integer.
pub(super) fn nullable_safe_i64(
    value: &Map<String, Value>,
    name: &str,
) -> Result<Option<i64>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .filter(|number| number.unsigned_abs() <= MAX_SAFE_JSON_INTEGER)
            .map(Some)
            .ok_or_else(invalid_response),
        None => Err(invalid_response()),
    }
}

/// Returns an explicitly nullable safe unsigned JSON integer.
pub(super) fn nullable_safe_u64(
    value: &Map<String, Value>,
    name: &str,
) -> Result<Option<u64>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(_) => require_safe_u64(value, name).map(Some),
        None => Err(invalid_response()),
    }
}

/// Returns an explicitly nullable finite non-negative number.
pub(super) fn nullable_nonnegative_number(
    value: &Map<String, Value>,
    name: &str,
) -> Result<Option<f64>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|number| number.is_finite() && *number >= 0.0)
            .map(Some)
            .ok_or_else(invalid_response),
        None => Err(invalid_response()),
    }
}

/// Returns one allowed optional-evidence status without accepting `not_linked` here.
pub(super) fn availability<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, RuntimeError> {
    let status = require_string(value, name)?;
    if matches!(status, "available" | "not_found" | "unavailable") {
        Ok(status)
    } else {
        Err(invalid_response())
    }
}

/// Validates one opaque context identity without rendering or accepting obvious private values.
fn validate_opaque_context_id(value: &str) -> Result<(), RuntimeError> {
    if value.chars().count() <= CONTEXT_ID_LIMIT
        && !value.chars().any(char::is_control)
        && !sensitive_string(value, "context_id")
    {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Validates a sorted, unique, non-empty bounded string list.
pub(super) fn validate_sorted_strings<'a>(
    value: &'a Map<String, Value>,
    name: &str,
    limit: usize,
) -> Result<Vec<&'a str>, RuntimeError> {
    let items = required_array(value, name, limit)?;
    let mut output = Vec::with_capacity(items.len());
    let mut seen = BTreeSet::new();
    for item in items {
        let text = item
            .as_str()
            .filter(|text| !text.is_empty())
            .ok_or_else(invalid_response)?;
        if !seen.insert(text) {
            return Err(invalid_response());
        }
        output.push(text);
    }
    if output.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid_response());
    }
    Ok(output)
}
