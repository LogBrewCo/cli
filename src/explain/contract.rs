//! Strict field accessors shared by versioned explanation responses.

use serde_json::{Map, Value};

use super::invalid_response;
use crate::ids::is_uuid;
use crate::{RuntimeError, time};

/// Largest integer accepted from JSON-number investigation contracts.
pub(super) const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;

/// Returns one required UUID without reflecting invalid server content.
pub(super) fn required_uuid_text<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, RuntimeError> {
    let id = require_string(value, name)?;
    if is_uuid(id) {
        Ok(id)
    } else {
        Err(invalid_response())
    }
}

/// Returns whether a string is one canonical non-zero lowercase W3C identifier.
pub(super) fn is_w3c_id(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

/// Validates and returns one explicitly nullable W3C identifier.
pub(super) fn nullable_w3c_id<'a>(
    value: &'a Map<String, Value>,
    name: &str,
    length: usize,
) -> Result<Option<&'a str>, RuntimeError> {
    let id = optional_string(value, name)?;
    if id.is_some_and(|id| !is_w3c_id(id, length)) {
        Err(invalid_response())
    } else {
        Ok(id)
    }
}

/// Returns one explicitly nullable object while rejecting omission and wrong types.
pub(super) const fn optional_object_value(
    value: Option<&Value>,
) -> Result<Option<&Map<String, Value>>, RuntimeError> {
    match value {
        Some(Value::Null) => Ok(None),
        Some(Value::Object(value)) => Ok(Some(value)),
        _ => Err(invalid_response()),
    }
}

/// Returns whether a normalized status represents an error outcome.
pub(super) fn is_error_status(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "error" | "cancelled" | "deadline_exceeded"
        )
    })
}

/// Validates one required SDK or runtime name/version object.
pub(super) fn validate_name_version(value: &Map<String, Value>) -> Result<(), RuntimeError> {
    validate_strings(value, &["name", "version"])
}

/// Validates one required nullable object field.
pub(super) fn validate_nullable_object(
    value: &Map<String, Value>,
    name: &str,
) -> Result<(), RuntimeError> {
    match value.get(name) {
        Some(Value::Null | Value::Object(_)) => Ok(()),
        _ => Err(invalid_response()),
    }
}

/// Validates one required bounded array of objects.
pub(super) fn validate_object_array<'a>(
    value: &'a Map<String, Value>,
    name: &str,
    limit: usize,
) -> Result<&'a [Value], RuntimeError> {
    value
        .get(name)
        .and_then(Value::as_array)
        .filter(|items| items.len() <= limit && items.iter().all(Value::is_object))
        .map(Vec::as_slice)
        .ok_or_else(invalid_response)
}

/// Validates one required bounded array of non-empty strings.
pub(super) fn validate_string_array(
    value: &Map<String, Value>,
    name: &str,
    limit: usize,
) -> Result<(), RuntimeError> {
    let items = value
        .get(name)
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    if items.len() > limit
        || items
            .iter()
            .any(|item| item.as_str().is_none_or(str::is_empty))
    {
        Err(invalid_response())
    } else {
        Ok(())
    }
}

/// Returns one required safe JSON-number unsigned integer.
pub(super) fn require_safe_u64(
    value: &Map<String, Value>,
    name: &str,
) -> Result<u64, RuntimeError> {
    require_u64(value, name).and_then(|value| {
        (value <= MAX_SAFE_JSON_INTEGER)
            .then_some(value)
            .ok_or_else(invalid_response)
    })
}

/// Returns one required positive safe JSON-number unsigned integer.
pub(super) fn require_safe_positive_u64(
    value: &Map<String, Value>,
    name: &str,
) -> Result<u64, RuntimeError> {
    require_safe_u64(value, name)
        .and_then(|value| (value > 0).then_some(value).ok_or_else(invalid_response))
}

/// Returns one required nullable safe JSON-number unsigned integer.
pub(super) fn optional_safe_u64(
    value: &Map<String, Value>,
    name: &str,
) -> Result<Option<u64>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|value| *value <= MAX_SAFE_JSON_INTEGER)
            .map(Some)
            .ok_or_else(invalid_response),
        _ => Err(invalid_response()),
    }
}

/// Returns a response object after checking its required additive fields.
pub(super) fn response_object<'a>(
    value: &'a Value,
    required: &[&str],
) -> Result<&'a Map<String, Value>, RuntimeError> {
    let object = value.as_object().ok_or_else(invalid_response)?;
    if required.iter().any(|name| !object.contains_key(*name)) {
        return Err(invalid_response());
    }
    Ok(object)
}

/// Returns a response object only when its top-level vocabulary is exact.
pub(super) fn exact_response_object<'a>(
    value: &'a Value,
    fields: &[&str],
) -> Result<&'a Map<String, Value>, RuntimeError> {
    let object = response_object(value, fields)?;
    require_exact_fields(object, fields).map(|()| object)
}

/// Requires one exact object vocabulary so privacy-sensitive contracts fail closed on additions.
pub(super) fn require_exact_fields(
    value: &Map<String, Value>,
    expected: &[&str],
) -> Result<(), RuntimeError> {
    if value.len() == expected.len() && expected.iter().all(|field| value.contains_key(*field)) {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Requires every mandatory field and rejects keys outside an explicit optional vocabulary.
pub(super) fn require_known_fields(
    value: &Map<String, Value>,
    required: &[&str],
    optional: &[&str],
) -> Result<(), RuntimeError> {
    if required.iter().any(|field| !value.contains_key(*field))
        || value
            .keys()
            .any(|field| !required.contains(&field.as_str()) && !optional.contains(&field.as_str()))
    {
        Err(invalid_response())
    } else {
        Ok(())
    }
}

/// Requires the current version of a versioned explanation response.
pub(super) fn validate_schema_version(value: &Map<String, Value>) -> Result<(), RuntimeError> {
    validate_schema_version_value(value, 1)
}

/// Requires one exact schema version selected by the request contract.
pub(super) fn validate_schema_version_value(
    value: &Map<String, Value>,
    expected: u64,
) -> Result<(), RuntimeError> {
    if value.get("schema_version").and_then(Value::as_u64) == Some(expected) {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Returns one required object field.
pub(super) fn required_object<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a Map<String, Value>, RuntimeError> {
    value
        .get(name)
        .and_then(Value::as_object)
        .ok_or_else(invalid_response)
}

/// Returns one required string field.
pub(super) fn require_string<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, RuntimeError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_response)
}

/// Requires every named field to contain a non-empty string.
pub(super) fn validate_strings(
    value: &Map<String, Value>,
    names: &[&str],
) -> Result<(), RuntimeError> {
    names
        .iter()
        .try_for_each(|name| require_string(value, name).map(|_| ()))
}

/// Returns one optional string field while rejecting other types.
pub(super) fn optional_string<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value.as_str())),
        Some(_) => Err(invalid_response()),
    }
}

/// Requires one exact string identity.
pub(super) fn require_string_equals(
    value: &Map<String, Value>,
    name: &str,
    expected: &str,
) -> Result<(), RuntimeError> {
    if require_string(value, name)? == expected {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Returns one required unsigned integer.
pub(super) fn require_u64(value: &Map<String, Value>, name: &str) -> Result<u64, RuntimeError> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(invalid_response)
}

/// Returns one required boolean.
pub(super) fn require_bool(value: &Map<String, Value>, name: &str) -> Result<bool, RuntimeError> {
    value
        .get(name)
        .and_then(Value::as_bool)
        .ok_or_else(invalid_response)
}

/// Requires one finite JSON number.
pub(super) fn require_finite_number(
    value: &Map<String, Value>,
    name: &str,
) -> Result<f64, RuntimeError> {
    value
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(invalid_response)
}

/// Validates one optional finite JSON number while rejecting other types.
pub(super) fn optional_finite_number(
    value: &Map<String, Value>,
    name: &str,
) -> Result<Option<f64>, RuntimeError> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(Some)
            .ok_or_else(invalid_response),
    }
}

/// Requires one UTC RFC 3339 timestamp.
pub(super) fn require_timestamp<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, RuntimeError> {
    let timestamp = require_string(value, name)?;
    time::is_rfc3339_utc(timestamp)
        .then_some(timestamp)
        .ok_or_else(invalid_response)
}

/// Returns one UTC timestamp normalized to the API's millisecond precision.
pub(super) fn require_timestamp_millis(
    value: &Map<String, Value>,
    name: &str,
) -> Result<i128, RuntimeError> {
    time::parse_utc_millis(require_timestamp(value, name)?).ok_or_else(invalid_response)
}
