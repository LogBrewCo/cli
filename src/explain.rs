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
mod text;
mod trace;

use serde::de::{Error as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};

use crate::api_path::explain_path;
use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::{CliEnvironment, ExplainReleaseTarget, ExplainTarget, RuntimeError};

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
