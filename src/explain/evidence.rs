//! Shared validation for evidence receipts, timelines, and next actions.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::contract::{
    nullable_w3c_id, require_known_fields, require_string, require_timestamp_millis, require_u64,
    validate_strings,
};
use super::correlation::validate_items_collection;
use super::{NEXT_ACTION_LIMIT, invalid_response};
use crate::RuntimeError;

/// Shared evidence receipt partitions in their wire order.
pub(super) const EVIDENCE_CATEGORIES: [&str; 4] = [
    "captured_fields",
    "missing_fields",
    "redacted_fields",
    "truncated_fields",
];

/// Validates one bounded causal timeline shared by investigation responses.
pub(super) fn validate_timeline(value: &Map<String, Value>) -> Result<(), RuntimeError> {
    let items = validate_items_collection(value, false)?;
    let mut previous_time = None;
    for item in items {
        let item = item.as_object().ok_or_else(invalid_response)?;
        let _kind = require_string(item, "kind")?;
        let _span_id = nullable_w3c_id(item, "span_id", 16)?;
        let occurred_at = require_timestamp_millis(item, "occurred_at")?;
        if previous_time
            .replace(occurred_at)
            .is_some_and(|previous| previous > occurred_at)
        {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Validates common evidence coverage.
pub(super) fn validate_evidence(evidence: &Map<String, Value>) -> Result<(), RuntimeError> {
    require_known_fields(evidence, &["status"], &EVIDENCE_CATEGORIES)?;
    if !matches!(require_string(evidence, "status")?, "complete" | "partial") {
        return Err(invalid_response());
    }
    for name in EVIDENCE_CATEGORIES {
        let fields = evidence
            .get(name)
            .and_then(Value::as_array)
            .ok_or_else(invalid_response)?;
        if fields.len() > 256
            || fields.iter().any(|field| field.as_str().is_none())
            || fields
                .iter()
                .filter_map(Value::as_str)
                .collect::<BTreeSet<_>>()
                .len()
                != fields.len()
        {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Rejects additions inside one privacy-sensitive evidence namespace.
pub(super) fn validate_evidence_vocabulary(
    evidence: &Map<String, Value>,
    prefix: &str,
    known: &[&str],
) -> Result<(), RuntimeError> {
    let namespace = format!("{prefix}.");
    if EVIDENCE_CATEGORIES.iter().any(|category| {
        evidence[*category].as_array().is_none_or(|fields| {
            fields.iter().filter_map(Value::as_str).any(|field| {
                (field == prefix || field.starts_with(namespace.as_str()))
                    && !known.contains(&field)
            })
        })
    }) {
        Err(invalid_response())
    } else {
        Ok(())
    }
}

/// Returns whether one validated evidence array contains an exact field receipt.
pub(super) fn evidence_has_field(
    evidence: &Map<String, Value>,
    category: &str,
    field: &str,
) -> Result<bool, RuntimeError> {
    let fields = evidence
        .get(category)
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    Ok(fields.iter().any(|value| value.as_str() == Some(field)))
}

/// Requires one evidence field to occur exactly in its expected receipt partitions.
pub(super) fn validate_field_receipts(
    evidence: &Map<String, Value>,
    field: &str,
    expected: [bool; 4],
) -> Result<(), RuntimeError> {
    for (category, expected) in EVIDENCE_CATEGORIES.into_iter().zip(expected) {
        let count = evidence
            .get(category)
            .and_then(Value::as_array)
            .ok_or_else(invalid_response)?
            .iter()
            .filter(|value| value.as_str() == Some(field))
            .count();
        if count != usize::from(expected) {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Validates prioritized backend-generated actions.
pub(super) fn validate_next_actions(value: Option<&Value>) -> Result<(), RuntimeError> {
    let actions = value
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    if actions.is_empty() || actions.len() > NEXT_ACTION_LIMIT {
        return Err(invalid_response());
    }
    for action in actions {
        let action = action.as_object().ok_or_else(invalid_response)?;
        let priority = require_u64(action, "priority")?;
        if !(1..=u64::try_from(NEXT_ACTION_LIMIT).map_err(|_error| invalid_response())?)
            .contains(&priority)
        {
            return Err(invalid_response());
        }
        validate_strings(action, &["code", "target", "reason"])?;
    }
    Ok(())
}
