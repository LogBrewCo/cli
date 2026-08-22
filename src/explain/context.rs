//! Shared strict validation for typed telemetry context.

use serde_json::{Map, Value};

use super::projection::{sensitive_context_tag_key, sensitive_key, sensitive_string};
use super::{
    invalid_response, is_w3c_id, nullable_w3c_id, require_exact_fields, require_string,
    require_u64, required_object,
};
use crate::RuntimeError;

/// Maximum typed-context string or tag-value length.
const TEXT_LIMIT: usize = 256;
/// Maximum privacy-safe opaque context-identifier length.
const ID_LIMIT: usize = 200;
/// Maximum low-cardinality typed-context tags.
const TAG_LIMIT: usize = 32;
/// Maximum typed-context tag-key length.
const TAG_KEY_LIMIT: usize = 64;

/// Exact envelope identities that a captured context must preserve.
#[derive(Clone, Copy)]
pub(super) struct Expected<'a> {
    /// Logical service identity.
    service: &'a str,
    /// Exact deployment environment.
    environment: &'a str,
    /// Exact deployment release.
    release: &'a str,
    /// Signal-specific trace and identity policy.
    mode: Mode<'a>,
}

/// Signal-specific context invariants beyond the shared resource grammar.
#[derive(Clone, Copy)]
enum Mode<'a> {
    /// Binds captured trace identities to their enclosing signal.
    Captured {
        /// Optional distributed-trace identity.
        trace_id: Option<&'a str>,
        /// Optional exact span identity.
        span_id: Option<&'a str>,
        /// Optional captured parent identity.
        parent_span_id: Option<&'a str>,
        /// Whether the selected envelope knows the exact parent.
        bind_parent: bool,
    },
    /// Withholds actor/session IDs while preserving action classification.
    Action {
        /// Privacy-safe action subject classification.
        classification: &'a str,
    },
}

impl<'a> Expected<'a> {
    /// Binds every trace identity for one selected span.
    pub(super) const fn span(
        service: &'a str,
        environment: &'a str,
        release: &'a str,
        trace_id: &'a str,
        span_id: &'a str,
        parent_span_id: Option<&'a str>,
    ) -> Self {
        Self {
            service,
            environment,
            release,
            mode: Mode::Captured {
                trace_id: Some(trace_id),
                span_id: Some(span_id),
                parent_span_id,
                bind_parent: true,
            },
        }
    }

    /// Binds the optional trace identities carried by one metric sample.
    pub(super) const fn metric(
        service: &'a str,
        environment: &'a str,
        release: &'a str,
        trace_id: Option<&'a str>,
        span_id: Option<&'a str>,
    ) -> Self {
        Self {
            service,
            environment,
            release,
            mode: Mode::Captured {
                trace_id,
                span_id,
                parent_span_id: None,
                bind_parent: false,
            },
        }
    }

    /// Withholds action identities while accepting separately bound trace context.
    pub(super) const fn action(
        service: &'a str,
        environment: &'a str,
        release: &'a str,
        classification: &'a str,
    ) -> Self {
        Self {
            service,
            environment,
            release,
            mode: Mode::Action { classification },
        }
    }
}

/// Validates one explicitly nullable typed telemetry context.
pub(super) fn validate(value: Option<&Value>, expected: Expected<'_>) -> Result<(), RuntimeError> {
    let Some(context) = nullable_object(value)? else {
        return Ok(());
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
    let populated = validate_resource(context.get("resource"), expected)?
        | validate_trace(context.get("trace"), expected)?
        | validate_session(context.get("session"), expected)?
        | validate_subject(context.get("subject"), expected)?;
    let tags = required_object(context, "tags")?;
    validate_tags(tags, expected)?;
    if populated || !tags.is_empty() || matches!(expected.mode, Mode::Action { .. }) {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Validates typed resource identity and deployment scope.
fn validate_resource(value: Option<&Value>, expected: Expected<'_>) -> Result<bool, RuntimeError> {
    let Some(resource) = nullable_object(value)? else {
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
    for field in ["service", "runtime", "framework"] {
        let Some(identity) = nullable_object(resource.get(field))? else {
            continue;
        };
        require_exact_fields(identity, &["name", "version"])?;
        let name = require_string(identity, "name")?;
        validate_text(name)?;
        let _version = nullable_text(identity, "version")?;
        if field == "service" && name != expected.service {
            return Err(invalid_response());
        }
    }
    if let Some(deployment) = nullable_object(resource.get("deployment"))? {
        require_exact_fields(deployment, &["environment", "release"])?;
        let environment = nullable_text(deployment, "environment")?;
        let release = nullable_text(deployment, "release")?;
        if matches!(expected.mode, Mode::Captured { .. })
            && environment.is_none()
            && release.is_none()
            || environment.is_some_and(|value| value != expected.environment)
            || release.is_some_and(|value| value != expected.release)
        {
            return Err(invalid_response());
        }
    }
    if let Some(os) = nullable_object(resource.get("operating_system"))? {
        require_exact_fields(os, &["name", "version", "build"])?;
        validate_text(require_string(os, "name")?)?;
        let _version = nullable_text(os, "version")?;
        let _build = nullable_text(os, "build")?;
    }
    let allow_empty = matches!(expected.mode, Mode::Action { .. });
    validate_optional_group(
        resource.get("device"),
        &["family", "model", "architecture"],
        allow_empty,
    )?;
    validate_optional_group(
        resource.get("application"),
        &["name", "version", "build"],
        allow_empty,
    )?;
    Ok(true)
}

/// Requires one optional structured identity to contain at least one value.
fn validate_optional_group(
    value: Option<&Value>,
    fields: &[&str],
    allow_empty: bool,
) -> Result<(), RuntimeError> {
    let Some(group) = nullable_object(value)? else {
        return Ok(());
    };
    require_exact_fields(group, fields)?;
    let mut populated = false;
    for field in fields {
        populated |= nullable_text(group, field)?.is_some();
    }
    if populated || allow_empty {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Validates optional W3C context against the selected envelope.
fn validate_trace(value: Option<&Value>, expected: Expected<'_>) -> Result<bool, RuntimeError> {
    let Some(trace) = nullable_object(value)? else {
        return Ok(false);
    };
    require_exact_fields(trace, &["trace_id", "span_id", "parent_span_id", "sampled"])?;
    let trace_id = require_string(trace, "trace_id")?;
    let span_id = nullable_w3c_id(trace, "span_id", 16)?;
    let parent_span_id = nullable_w3c_id(trace, "parent_span_id", 16)?;
    let identity_mismatch = match expected.mode {
        Mode::Captured {
            trace_id: expected_trace,
            span_id: expected_span,
            parent_span_id: expected_parent,
            bind_parent,
        } => {
            Some(trace_id) != expected_trace
                || span_id != expected_span
                || bind_parent && parent_span_id != expected_parent
        }
        Mode::Action { .. } => false,
    };
    if !is_w3c_id(trace_id, 32)
        || identity_mismatch
        || !matches!(trace.get("sampled"), Some(Value::Null | Value::Bool(_)))
    {
        return Err(invalid_response());
    }
    Ok(true)
}

/// Validates optional privacy-bounded session context.
fn validate_session(value: Option<&Value>, expected: Expected<'_>) -> Result<bool, RuntimeError> {
    let Some(session) = nullable_object(value)? else {
        return Ok(false);
    };
    require_exact_fields(session, &["id", "previous_id"])?;
    let id = nullable_id(session, "id")?;
    let previous = nullable_id(session, "previous_id")?;
    if matches!(expected.mode, Mode::Action { .. }) && (id.is_some() || previous.is_some())
        || id.is_some() && id == previous
    {
        return Err(invalid_response());
    }
    Ok(true)
}

/// Validates optional privacy-bounded subject context.
fn validate_subject(value: Option<&Value>, expected: Expected<'_>) -> Result<bool, RuntimeError> {
    let Some(subject) = nullable_object(value)? else {
        return Ok(false);
    };
    require_exact_fields(subject, &["id", "kind"])?;
    let id = nullable_id(subject, "id")?;
    let kind = require_string(subject, "kind")?;
    let classification_mismatch = match expected.mode {
        Mode::Action { classification } => {
            id.is_some() || matches!(classification, "anonymous" | "user") && classification != kind
        }
        Mode::Captured { .. } => false,
    };
    if matches!(kind, "anonymous" | "user") && !classification_mismatch {
        Ok(true)
    } else {
        Err(invalid_response())
    }
}

/// Validates sorted low-cardinality context tags.
fn validate_tags(tags: &Map<String, Value>, expected: Expected<'_>) -> Result<(), RuntimeError> {
    if tags.len() > TAG_LIMIT {
        return Err(invalid_response());
    }
    let mut previous = None;
    for (key, value) in tags {
        let text = value
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(invalid_response)?;
        let mut characters = key.chars();
        if previous.is_some_and(|previous: &str| previous >= key.as_str())
            || !characters
                .next()
                .is_some_and(|character| character.is_ascii_alphabetic())
            || key.chars().count() > TAG_KEY_LIMIT
            || !characters
                .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character))
            || text.chars().count() > TEXT_LIMIT
            || text.chars().any(char::is_control)
            || sensitive_key(key)
            || sensitive_context_tag_key(key)
            || sensitive_string(
                text,
                if matches!(expected.mode, Mode::Action { .. }) {
                    key
                } else {
                    "context_value"
                },
            )
        {
            return Err(invalid_response());
        }
        previous = Some(key.as_str());
    }
    Ok(())
}

/// Returns one explicitly nullable object.
const fn nullable_object(
    value: Option<&Value>,
) -> Result<Option<&Map<String, Value>>, RuntimeError> {
    match value {
        Some(Value::Null) => Ok(None),
        Some(Value::Object(value)) => Ok(Some(value)),
        _ => Err(invalid_response()),
    }
}

/// Returns one explicitly nullable privacy-bounded string.
fn nullable_text<'a>(
    value: &'a Map<String, Value>,
    field: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    match value.get(field) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => validate_text(text).map(|()| Some(text.as_str())),
        _ => Err(invalid_response()),
    }
}

/// Rejects empty, private, credential-like, or oversized context text.
fn validate_text(value: &str) -> Result<(), RuntimeError> {
    if value.is_empty()
        || value.chars().count() > TEXT_LIMIT
        || value.chars().any(char::is_control)
        || sensitive_string(value, "context_value")
    {
        Err(invalid_response())
    } else {
        Ok(())
    }
}

/// Returns one explicitly nullable machine-like opaque identifier.
fn nullable_id<'a>(
    value: &'a Map<String, Value>,
    field: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    match value.get(field) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(id))
            if id.chars().count() <= ID_LIMIT
                && id
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_alphanumeric())
                && id.chars().all(|character| {
                    character.is_ascii_alphanumeric() || "._:-".contains(character)
                })
                && !sensitive_string(id, "context_id") =>
        {
            Ok(Some(id.as_str()))
        }
        _ => Err(invalid_response()),
    }
}
