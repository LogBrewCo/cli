//! Versioned, bounded telemetry investigation reads.

mod action;
mod context;
mod contract;
mod correction;
mod correlation;
mod evidence;
mod issue;
mod issue_exception_chain;
mod issue_lifecycle;
mod issue_occurrence_analysis;
mod log;
mod metric;
mod projection;
mod release;
mod span;
mod trace;

use serde::de::{Error as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};

use crate::api_path::explain_path;
use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::http::terminal_safe;
use crate::{CliEnvironment, ExplainReleaseTarget, ExplainTarget, RuntimeError};
use issue::render::{append_issue_cause, append_issue_fix};

/// Maximum accepted explanation response body.
const RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
/// Maximum nested JSON depth accepted from a successful response.
const JSON_DEPTH_LIMIT: usize = 16;
/// Maximum JSON values accepted after parsing a bounded response.
const JSON_NODE_LIMIT: usize = 250_000;
/// Maximum elements accepted in one response array.
const JSON_ARRAY_LIMIT: usize = 10_000;
/// Maximum fields accepted in one response object.
const JSON_OBJECT_LIMIT: usize = 256;
/// Maximum characters accepted in one response string.
const JSON_STRING_LIMIT: usize = 16_384;
/// Maximum backend-generated next actions rendered or accepted.
const NEXT_ACTION_LIMIT: usize = 16;
/// Maximum related evidence items expanded in human output.
const RELATED_PREVIEW_LIMIT: usize = 3;
/// Maximum sibling issues retained by one investigation response.
const RELATED_ISSUE_LIMIT: usize = 20;
/// Maximum metric series returned by the public API.
const METRIC_SERIES_LIMIT: usize = 20;
/// Maximum points returned for one metric series.
const METRIC_POINT_LIMIT: usize = 500;
/// Duplicate-aware JSON value.
struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueValueVisitor)
    }
}

/// Serde visitor that rejects duplicate fields in every response object.
struct UniqueValueVisitor;

impl<'de> Visitor<'de> for UniqueValueVisitor {
    type Value = UniqueValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("valid JSON without duplicate object fields")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(UniqueValue(Value::Bool(v)))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(UniqueValue(Value::Number(v.into())))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(UniqueValue(Value::Number(v.into())))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .map(UniqueValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(UniqueValue(Value::String(v.to_owned())))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(UniqueValue(Value::String(v)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(UniqueValue(value)) = seq.next_element()? {
            values.push(value);
        }
        Ok(UniqueValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut fields = Map::new();
        while let Some((key, UniqueValue(value))) = map.next_entry::<String, UniqueValue>()? {
            if fields.insert(key, value).is_some() {
                return Err(A::Error::custom("duplicate response field"));
            }
        }
        Ok(UniqueValue(Value::Object(fields)))
    }
}

/// Executes one versioned read-only explanation.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    target: &ExplainTarget,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let origin =
        crate::http::normalized_origin(env.base_url.as_str()).ok_or_else(transport_error)?;
    let client = crate::http::api_client().map_err(|_error| transport_error())?;
    let path = explain_path(target);
    let url = format!("{origin}{path}");
    let response = send_account_authenticated_with_refresh(&client, env, |client, credential| {
        client.get(url.as_str()).bearer_auth(credential.token())
    })
    .await
    .map_err(request_error)?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(safe_api_error(status, &credential));
    }
    let body = crate::http::bounded_body(response, RESPONSE_LIMIT)
        .await
        .map_err(|error| match error {
            crate::http::BodyError::Invalid => invalid_response(),
            crate::http::BodyError::Transport => transport_error(),
        })?;
    let value = validated_response(target, body.as_str())?;
    if json {
        writeln!(output, "{body}")?;
    } else {
        let rendered = render_response(target, &value).ok_or_else(invalid_response)?;
        write!(output, "{rendered}")?;
    }
    Ok(())
}

/// Parses and validates one duplicate-free versioned response.
fn validated_response(target: &ExplainTarget, body: &str) -> Result<Value, RuntimeError> {
    let UniqueValue(value) =
        serde_json::from_str::<UniqueValue>(body).map_err(|_error| invalid_response())?;
    let mut nodes = 0;
    if !validate_tree(&value, 0, &mut nodes) {
        return Err(invalid_response());
    }
    match target {
        ExplainTarget::Issue { id, occurrence } => {
            issue::validate::validate_issue_response(&value, id, occurrence)
        }
        ExplainTarget::IssueCorrection(target) => correction::validate(&value, target),
        ExplainTarget::Log(id) => log::validate_log_response(&value, id),
        ExplainTarget::Action(id) => action::validate_response(&value, id),
        ExplainTarget::Span(target) => span::validate_response(&value, target),
        ExplainTarget::Trace(id) => trace::validate_trace_response(&value, id),
        ExplainTarget::Release(release) => validate_release_response(&value, release),
        ExplainTarget::Metric(target) => metric::validate_response(&value, target),
    }?;
    Ok(value)
}

/// Applies structural memory and nesting bounds to parsed response data.
fn validate_tree(value: &Value, depth: usize, nodes: &mut usize) -> bool {
    if depth > JSON_DEPTH_LIMIT || *nodes >= JSON_NODE_LIMIT {
        return false;
    }
    *nodes = (*nodes).saturating_add(1);
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
        Value::String(value) => value.chars().count() <= JSON_STRING_LIMIT,
        Value::Array(values) => {
            values.len() <= JSON_ARRAY_LIMIT
                && values
                    .iter()
                    .all(|value| validate_tree(value, depth.saturating_add(1), nodes))
        }
        Value::Object(values) => {
            values.len() <= JSON_OBJECT_LIMIT
                && values.iter().all(|(key, value)| {
                    key.chars().count() <= 256
                        && validate_tree(value, depth.saturating_add(1), nodes)
                })
        }
    }
}

/// Validates one versioned release investigation envelope and exact query identity.
fn validate_release_response(
    value: &Value,
    expected: &ExplainReleaseTarget,
) -> Result<(), RuntimeError> {
    let response = value.as_object().ok_or_else(invalid_response)?;
    release::validate::response(response, expected)?;
    release::next_actions::validate_release_next_actions(response)
}

/// Builds one bounded human projection after contract validation.
fn render_response(target: &ExplainTarget, value: &Value) -> Option<String> {
    match target {
        ExplainTarget::Issue { .. } => issue::render::render_issue(value),
        ExplainTarget::IssueCorrection(_) => correction::render(value),
        ExplainTarget::Log(_) => log::render_log(value),
        ExplainTarget::Action(_) => action::render(value),
        ExplainTarget::Span(_) => span::render(value),
        ExplainTarget::Trace(_) => trace::render_trace(value),
        ExplainTarget::Release(_) => release::render::render_release(value),
        ExplainTarget::Metric(_) => metric::render(value),
    }
}

/// Appends strict shared runtime context when available.
fn append_runtime_context(output: &mut String, context: Option<&Value>) {
    let Some(context) = context.filter(|value| !value.is_null()) else {
        return;
    };
    if let Some(resource) = context.get("resource").filter(|value| !value.is_null()) {
        let mut runtime = String::from("Runtime:");
        for (label, name) in [
            ("service", "service"),
            ("runtime", "runtime"),
            ("framework", "framework"),
            ("os", "operating_system"),
            ("app", "application"),
        ] {
            if let Some(identity) = resource.get(name).filter(|value| !value.is_null()) {
                append_named_version_label(&mut runtime, label, identity);
            }
        }
        if let Some(deployment) = resource.get("deployment").filter(|value| !value.is_null()) {
            append_labeled_text(&mut runtime, "environment", deployment, "environment", 120);
            append_labeled_text(&mut runtime, "release", deployment, "release", 200);
        }
        if let Some(device) = resource.get("device").filter(|value| !value.is_null()) {
            append_labeled_text(&mut runtime, "device_family", device, "family", 120);
            append_labeled_text(&mut runtime, "device_model", device, "model", 120);
            append_labeled_text(&mut runtime, "architecture", device, "architecture", 80);
        }
        if runtime != "Runtime:" {
            runtime.push('\n');
            output.push_str(runtime.as_str());
        }
    }
    if let Some(trace) = context.get("trace").filter(|value| !value.is_null()) {
        output.push_str("Captured correlation:");
        append_labeled_text(output, "trace", trace, "trace_id", 80);
        append_labeled_text(output, "span", trace, "span_id", 40);
        append_labeled_text(output, "parent", trace, "parent_span_id", 40);
        append_labeled_bool(output, "sampled", trace, "sampled");
        output.push('\n');
    }
    if let Some(session) = context
        .get("session")
        .filter(|value| !value.is_null())
        .filter(|session| {
            session.get("id").and_then(Value::as_str).is_some()
                || session.get("previous_id").and_then(Value::as_str).is_some()
        })
    {
        output.push_str("Session correlation:");
        append_labeled_text(output, "id", session, "id", 160);
        append_labeled_text(output, "previous", session, "previous_id", 160);
        output.push('\n');
    }
    if let Some(subject) = context.get("subject").filter(|value| !value.is_null()) {
        output.push_str("Subject correlation:");
        append_labeled_text(output, "kind", subject, "kind", 40);
        append_labeled_text(output, "id", subject, "id", 160);
        output.push('\n');
    }
    if let Some(tags) = context.get("tags").and_then(Value::as_object) {
        for (key, value) in tags.iter().take(8) {
            let Some(value) = value.as_str() else {
                continue;
            };
            output.push_str("Tag: ");
            output.push_str(display_text(key, 120).as_str());
            output.push('=');
            output.push_str(display_text(value, 200).as_str());
            output.push('\n');
        }
        if tags.len() > 8 {
            output.push_str("Tags omitted from human view: ");
            output.push_str((tags.len() - 8).to_string().as_str());
            output.push_str("; use --json for all retained tags.\n");
        }
    }
}

/// Appends one named/versioned runtime identity to a compact line.
fn append_named_version_label(output: &mut String, label: &str, value: &Value) {
    let Some(name) = field_text(value, "name", 120) else {
        return;
    };
    output.push(' ');
    output.push_str(label);
    output.push('=');
    output.push_str(name.as_str());
    if let Some(version) = field_text(value, "version", 120) {
        output.push('@');
        output.push_str(version.as_str());
    }
}

/// Collects safe scalar leaves from the server-bounded attribute projection.
fn collect_scalar_fields(value: &Value, prefix: &str, fields: &mut Vec<(String, String)>) {
    if fields.len() >= 8 {
        return;
    }
    match value {
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|(key, _)| *key);
            for (key, child) in entries {
                let path = if prefix.is_empty() {
                    terminal_safe(key)
                } else {
                    format!("{prefix}.{}", terminal_safe(key))
                };
                collect_scalar_fields(child, path.as_str(), fields);
                if fields.len() >= 8 {
                    break;
                }
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                let path = format!("{prefix}[{index}]");
                collect_scalar_fields(child, path.as_str(), fields);
                if fields.len() >= 8 {
                    break;
                }
            }
        }
        Value::Null => fields.push((prefix.to_owned(), String::from("null"))),
        Value::Bool(value) => fields.push((prefix.to_owned(), value.to_string())),
        Value::Number(value) => fields.push((prefix.to_owned(), value.to_string())),
        Value::String(value) => fields.push((prefix.to_owned(), display_text(value, 300))),
    }
}

/// Appends one mixed-signal timeline receipt and representative ordered items.
fn append_timeline(output: &mut String, timeline: Option<&Value>) {
    let Some(timeline) = timeline else {
        return;
    };
    let items = timeline.get("items").and_then(Value::as_array);
    output.push_str("Timeline: count=");
    output.push_str(items.map_or(0, Vec::len).to_string().as_str());
    append_labeled_bool(output, "truncated", timeline, "truncated");
    output.push('\n');
    for item in items.into_iter().flatten().take(5) {
        output.push_str("Timeline item:");
        append_labeled_text(output, "at", item, "occurred_at", 64);
        append_labeled_text(output, "kind", item, "kind", 64);
        append_labeled_text(output, "relationship", item, "relationship", 80);
        append_labeled_text(output, "service", item, "service_name", 160);
        append_labeled_text(output, "name", item, "name", 220);
        append_labeled_text(output, "summary", item, "summary", 300);
        append_labeled_text(output, "message", item, "message", 300);
        append_labeled_text(output, "severity", item, "severity", 32);
        append_labeled_text(output, "status", item, "status", 40);
        append_labeled_integer(output, "duration_ms", item, "duration_ms");
        append_labeled_text(output, "issue", item, "issue_id", 80);
        append_labeled_text(output, "trace", item, "trace_id", 80);
        append_labeled_text(output, "span", item, "span_id", 40);
        output.push('\n');
    }
}

/// Appends availability, count, and truncation for one related collection.
fn append_collection<'a>(
    output: &mut String,
    label: &str,
    collection: &'a Value,
) -> Option<&'a [Value]> {
    output.push_str(label);
    output.push(':');
    append_labeled_text(output, "status", collection, "status", 40);
    let items = collection.get("items").and_then(Value::as_array);
    output.push_str(" count=");
    output.push_str(items.map_or(0, Vec::len).to_string().as_str());
    append_labeled_integer(output, "total", collection, "total");
    append_labeled_bool(output, "truncated", collection, "truncated");
    output.push('\n');
    items.map(Vec::as_slice)
}

/// Appends named correlation receipts and their type-specific safe previews.
fn append_related_collections(
    output: &mut String,
    correlations: &Value,
    collections: &[(&str, &str)],
) {
    for (name, label) in collections {
        let Some(collection) = correlations.get(name) else {
            continue;
        };
        let items = append_collection(output, label, collection);
        match *name {
            "issues" | "related_issues" => append_issue_previews(output, items),
            "logs" | "trace_logs" | "nearby_logs" => append_log_previews(output, items),
            "actions" => append_action_previews(output, items),
            "metrics" => append_metric_previews(output, items),
            _ => {}
        }
    }
}

/// Appends issue evidence previews without raw attributes.
fn append_issue_previews(output: &mut String, items: Option<&[Value]>) {
    for issue in items.into_iter().flatten().take(RELATED_PREVIEW_LIMIT) {
        output.push_str("Issue:");
        append_labeled_text(output, "title", issue, "title", 260);
        append_labeled_text(output, "severity", issue, "severity", 32);
        append_labeled_text(output, "status", issue, "status", 32);
        append_labeled_integer(output, "occurrences", issue, "occurrence_count");
        append_labeled_text(output, "issue", issue, "issue_id", 80);
        append_labeled_text(output, "trace", issue, "trace_id", 80);
        append_labeled_text(output, "span", issue, "span_id", 40);
        append_labeled_text(output, "at", issue, "occurred_at", 64);
        output.push('\n');
        append_issue_cause(output, issue.get("cause"));
        append_issue_fix(output, issue.get("fix"));
    }
}

/// Appends structured-log evidence previews without arbitrary attributes.
fn append_log_previews(output: &mut String, items: Option<&[Value]>) {
    for log in items.into_iter().flatten().take(RELATED_PREVIEW_LIMIT) {
        output.push_str("Log:");
        append_labeled_text(output, "message", log, "message", 300);
        append_labeled_text(output, "severity", log, "severity", 32);
        append_labeled_text(output, "level", log, "level", 32);
        append_labeled_text(output, "source", log, "source", 100);
        append_labeled_text(output, "service", log, "service_name", 160);
        append_labeled_text(output, "span", log, "span_id", 40);
        append_labeled_text(output, "at", log, "occurred_at", 64);
        output.push('\n');
    }
}

/// Appends product-action evidence previews.
fn append_action_previews(output: &mut String, items: Option<&[Value]>) {
    for action in items.into_iter().flatten().take(RELATED_PREVIEW_LIMIT) {
        output.push_str("Action:");
        append_labeled_text(output, "name", action, "name", 180);
        append_labeled_text(output, "service", action, "service_name", 160);
        append_labeled_integer(output, "events", action, "event_count");
        append_labeled_integer(output, "users", action, "identified_user_count");
        append_labeled_integer(output, "sessions", action, "session_count");
        append_labeled_text(output, "span", action, "span_id", 40);
        append_labeled_text(output, "at", action, "occurred_at", 64);
        output.push('\n');
    }
}

/// Appends metric exemplar or release metric evidence without anomaly claims.
fn append_metric_previews(output: &mut String, items: Option<&[Value]>) {
    for metric in items.into_iter().flatten().take(RELATED_PREVIEW_LIMIT) {
        output.push_str("Metric:");
        append_labeled_text(output, "name", metric, "name", 200);
        append_labeled_text(output, "kind", metric, "kind", 80);
        append_labeled_text(output, "temporality", metric, "temporality", 40);
        append_labeled_number(output, "value", metric, "value");
        append_labeled_number(output, "latest", metric, "latest_value");
        append_labeled_number(output, "min", metric, "minimum_value");
        append_labeled_number(output, "max", metric, "maximum_value");
        append_labeled_number(output, "average", metric, "average_value");
        append_labeled_integer(output, "events", metric, "event_count");
        append_labeled_text(output, "unit", metric, "unit", 80);
        append_labeled_text(output, "service", metric, "service_name", 160);
        append_labeled_text(output, "at", metric, "occurred_at", 64);
        append_labeled_text(output, "latest_at", metric, "latest_at", 64);
        append_labeled_text(output, "trace", metric, "trace_id", 80);
        append_labeled_text(output, "span", metric, "span_id", 40);
        output.push('\n');
    }
}

/// Appends explicit evidence completeness and omission identifiers.
fn append_evidence(output: &mut String, evidence: Option<&Value>) {
    let Some(evidence) = evidence else {
        return;
    };
    output.push_str("Evidence:");
    append_labeled_text(output, "status", evidence, "status", 32);
    for (label, name) in [
        ("captured", "captured_fields"),
        ("missing", "missing_fields"),
        ("redacted", "redacted_fields"),
        ("truncated", "truncated_fields"),
    ] {
        if let Some(fields) = evidence.get(name).and_then(Value::as_array) {
            output.push(' ');
            output.push_str(label);
            output.push('=');
            output.push_str(fields.len().to_string().as_str());
        }
    }
    output.push('\n');
    append_string_array(output, "Missing", evidence.get("missing_fields"), 8);
    append_string_array(output, "Redacted", evidence.get("redacted_fields"), 8);
    append_string_array(output, "Truncated", evidence.get("truncated_fields"), 8);
}

/// Appends prioritized backend-generated next actions and reasons.
fn append_actions(output: &mut String, actions: Option<&Value>) {
    let Some(actions) = actions.and_then(Value::as_array) else {
        return;
    };
    for (index, action) in actions.iter().take(NEXT_ACTION_LIMIT).enumerate() {
        output.push_str("Next");
        output.push(' ');
        output.push_str(
            integer_text(action, "priority")
                .unwrap_or_else(|| index.saturating_add(1).to_string())
                .as_str(),
        );
        output.push(':');
        append_labeled_text(output, "code", action, "code", 100);
        append_labeled_text(output, "target", action, "target", 100);
        append_labeled_text(output, "reason", action, "reason", 500);
        output.push('\n');
    }
}

/// Appends one bounded string array on a named line.
fn append_string_array(output: &mut String, label: &str, value: Option<&Value>, limit: usize) {
    let Some(values) = value
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty())
    else {
        return;
    };
    let values = values
        .iter()
        .filter_map(Value::as_str)
        .take(limit)
        .map(|value| display_text(value, 120))
        .collect::<Vec<_>>();
    output.push_str(label);
    output.push_str(": ");
    output.push_str(values.join(", ").as_str());
    if value
        .and_then(Value::as_array)
        .is_some_and(|all| all.len() > limit)
    {
        output.push_str(", ...");
    }
    output.push('\n');
}

/// Appends one named string field on its own line.
fn append_named_text(output: &mut String, label: &str, value: &Value, name: &str, limit: usize) {
    append_named(output, label, field_text(value, name, limit));
}

/// Appends one named integer field on its own line.
fn append_named_integer(output: &mut String, label: &str, value: &Value, name: &str) {
    append_named(output, label, integer_text(value, name));
}

/// Appends a name/version pair using one separator.
fn append_named_pair(
    output: &mut String,
    label: &str,
    value: &Value,
    left: &str,
    right: &str,
    separator: &str,
) {
    let (Some(left), Some(right)) = (field_text(value, left, 160), field_text(value, right, 120))
    else {
        return;
    };
    output.push_str(label);
    output.push_str(": ");
    output.push_str(left.as_str());
    output.push_str(separator);
    output.push_str(right.as_str());
    output.push('\n');
}

/// Appends one compact bounded string field.
fn append_labeled_text(output: &mut String, label: &str, value: &Value, name: &str, limit: usize) {
    append_labeled(output, label, field_text(value, name, limit), false);
}

/// Appends one compact integer field.
fn append_labeled_integer(output: &mut String, label: &str, value: &Value, name: &str) {
    append_labeled(output, label, integer_text(value, name), false);
}

/// Appends one compact approximate integer field with an explicit approximation marker.
fn append_labeled_approximate_integer(output: &mut String, label: &str, value: &Value, name: &str) {
    append_labeled(output, label, integer_text(value, name), true);
}

/// Appends one compact basis-point field as a two-decimal percentage.
fn append_labeled_basis_points(output: &mut String, label: &str, value: &Value, name: &str) {
    append_labeled(
        output,
        label,
        value
            .get(name)
            .and_then(Value::as_u64)
            .map(|value| format!("{}.{:02}%", value / 100, value % 100)),
        false,
    );
}

/// Appends one compact finite numeric field.
fn append_labeled_number(output: &mut String, label: &str, value: &Value, name: &str) {
    append_labeled(
        output,
        label,
        value
            .get(name)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .map(|value| value.to_string()),
        false,
    );
}

/// Appends one compact boolean field.
fn append_labeled_bool(output: &mut String, label: &str, value: &Value, name: &str) {
    append_labeled(
        output,
        label,
        value
            .get(name)
            .and_then(Value::as_bool)
            .map(|value| value.to_string()),
        false,
    );
}

/// Appends one optional value on its own named line.
fn append_named(output: &mut String, label: &str, value: Option<String>) {
    let Some(value) = value else { return };
    output.push_str(label);
    output.push_str(": ");
    output.push_str(value.as_str());
    output.push('\n');
}

/// Appends one optional compact value with an exact or approximate marker.
fn append_labeled(output: &mut String, label: &str, value: Option<String>, approximate: bool) {
    let Some(value) = value else { return };
    output.push(' ');
    output.push_str(label);
    output.push_str(if approximate { "=~" } else { "=" });
    output.push_str(value.as_str());
}

/// Returns a terminal-safe bounded string field.
fn field_text(value: &Value, name: &str, limit: usize) -> Option<String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .map(|value| display_text(value, limit))
}

/// Returns a signed or unsigned integer field as text.
fn integer_text(value: &Value, name: &str) -> Option<String> {
    value.get(name).and_then(|value| {
        value
            .as_i64()
            .map(|value| value.to_string())
            .or_else(|| value.as_u64().map(|value| value.to_string()))
    })
}

/// Escapes terminal controls and truncates one human-facing value.
fn display_text(value: &str, limit: usize) -> String {
    let safe = terminal_safe(value);
    let mut characters = safe.chars();
    let mut output = characters.by_ref().take(limit).collect::<String>();
    if characters.next().is_some() {
        output.push_str("...");
    }
    output
}

/// Converts transport and refresh failures into fixed path-free recovery.
fn request_error(error: RuntimeError) -> RuntimeError {
    error.auth_or(transport_error())
}

/// Returns one fixed path-free transport failure.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "explanation request could not be completed",
        next: "check network connectivity and retry the same explanation",
    }
}

/// Returns one fixed contract failure without reflecting server content.
const fn invalid_response() -> RuntimeError {
    RuntimeError::ExplainResponseInvalid
}

/// Converts a failed status into fixed value-safe guidance.
fn safe_api_error(status: u16, credential: &AuthCredential) -> RuntimeError {
    let (error, code, next, action_code, action_target) = match status {
        400 | 422 => (
            "explanation request rejected",
            "validation_failed",
            "check the exact identifier and bounded query scope, then retry",
            "fix_request",
            "request",
        ),
        401 => (
            "authentication required",
            "unauthorized",
            "run logbrew login",
            "sign_in",
            "auth",
        ),
        403 => (
            "explanation request forbidden",
            "forbidden",
            "confirm account access and retry the same explanation",
            "check_access",
            "auth",
        ),
        404 => (
            "explanation data not found",
            "not_found",
            "refresh the subject identity and retry the same explanation",
            "check_resource",
            "resource",
        ),
        405 => (
            "explanation method is not supported",
            "method_not_allowed",
            "retry the read-only explanation command",
            "use_supported_method",
            "api_method",
        ),
        429 => (
            "explanation request rate limited",
            "rate_limited",
            "retry the same explanation later",
            "retry_later",
            "investigation",
        ),
        500..=599 => (
            "explanation service unavailable",
            "service_unavailable",
            "retry the same explanation later",
            "retry_later",
            "investigation",
        ),
        _ => (
            "explanation request failed",
            "request_failed",
            "check account access and retry the same explanation",
            "retry_explanation",
            "investigation",
        ),
    };
    RuntimeError::Api {
        status,
        body: serde_json::json!({
            "error": error,
            "code": code,
            "next": next,
            "next_action": {"code": action_code, "target": action_target}
        })
        .to_string(),
        auth_source: credential.source(),
        auth_label: credential.label(),
    }
}
