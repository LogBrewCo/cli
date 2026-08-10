//! Strict schema-version-5 exception-tree validation and bounded human projection.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::{
    append_labeled_bool, append_labeled_integer, append_labeled_text, evidence_has_field,
    invalid_response, optional_string, require_bool, require_exact_fields, require_string,
    require_u64, required_object,
};
use crate::RuntimeError;

/// Maximum parent-first exception nodes accepted from one selected occurrence.
const EXCEPTION_LIMIT: usize = 8;
/// Maximum structured frames accepted for one exception node.
const FRAME_LIMIT: usize = 32;

/// Validated exception-node facts used to bind the derived fix location.
struct EntryFacts<'a> {
    /// Contiguous backend-assigned node identifier.
    id: u64,
    /// Validated frames retained for exact fix-location binding.
    frames: Vec<&'a Map<String, Value>>,
}

/// Legacy exception identity used to bind the additive reported node.
struct LegacyException<'a> {
    /// Validated legacy exception type.
    exception_type: &'a str,
    /// Optional validated legacy capture mechanism.
    mechanism: Option<&'a Map<String, Value>>,
}

/// Aggregate chain facts used for evidence, cause, and fix cross-field checks.
struct ChainFacts<'a> {
    /// Backend capture status for the whole exception graph.
    status: &'a str,
    /// Validated graph nodes in parent-first order.
    entries: Vec<EntryFacts<'a>>,
    /// Whether the SDK or backend reported an incomplete graph.
    truncated: bool,
    /// Distinct message evidence states present across the graph.
    message_states: BTreeSet<&'a str>,
    /// Distinct stack evidence states present across the graph.
    stack_states: BTreeSet<&'a str>,
}

/// Validates the exact version-5 event vocabulary and all exception-tree cross-field receipts.
pub(super) fn validate(
    event: &Map<String, Value>,
    evidence: &Map<String, Value>,
    cause: &Map<String, Value>,
    fix: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        event,
        &[
            "id",
            "occurred_at",
            "sdk",
            "context",
            "exception",
            "exception_chain",
            "stack_frames",
            "breadcrumbs",
            "breadcrumbs_truncated",
        ],
    )?;
    let legacy = validate_legacy_exception(event.get("exception"))?;
    let facts = validate_chain(required_object(event, "exception_chain")?, legacy)?;
    validate_receipts(evidence, &facts)?;
    validate_cause(cause, &facts)?;
    validate_fix(fix, &facts)
}

/// Validates the legacy top-level exception used for version compatibility.
fn validate_legacy_exception(
    value: Option<&Value>,
) -> Result<Option<LegacyException<'_>>, RuntimeError> {
    match value {
        Some(Value::Null) => Ok(None),
        Some(Value::Object(exception)) => {
            let mechanism = match exception.get("mechanism") {
                None => None,
                Some(Value::Object(mechanism)) => {
                    validate_mechanism(mechanism)?;
                    Some(mechanism)
                }
                Some(_) => return Err(invalid_response()),
            };
            if exception.len() != 1 + usize::from(mechanism.is_some())
                || exception
                    .keys()
                    .any(|key| !matches!(key.as_str(), "type" | "mechanism"))
            {
                return Err(invalid_response());
            }
            Ok(Some(LegacyException {
                exception_type: bounded_text(exception, "type", 256, true)?,
                mechanism,
            }))
        }
        _ => Err(invalid_response()),
    }
}

/// Validates the status-specific chain shape and every parent-first node.
fn validate_chain<'a>(
    chain: &'a Map<String, Value>,
    legacy: Option<LegacyException<'_>>,
) -> Result<ChainFacts<'a>, RuntimeError> {
    require_exact_fields(chain, &["status", "entries", "truncated"])?;
    let status = require_string(chain, "status")?;
    let entries = chain
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    let truncated = require_bool(chain, "truncated")?;
    match status {
        "captured" if !entries.is_empty() && entries.len() <= EXCEPTION_LIMIT => {}
        "not_captured" if entries.is_empty() && !truncated => {}
        "invalid" if entries.is_empty() && truncated => {}
        _ => return Err(invalid_response()),
    }

    let mut facts = ChainFacts {
        status,
        entries: Vec::with_capacity(entries.len()),
        truncated,
        message_states: BTreeSet::new(),
        stack_states: BTreeSet::new(),
    };
    for (index, value) in entries.iter().enumerate() {
        let entry = value.as_object().ok_or_else(invalid_response)?;
        let (entry_facts, message_state, stack_state) = validate_entry(entry, index)?;
        let _inserted_message_state = facts.message_states.insert(message_state);
        let _inserted_stack_state = facts.stack_states.insert(stack_state);
        facts.entries.push(entry_facts);
    }
    if status == "captured" {
        validate_reported_legacy(facts.entries.first(), entries.first(), legacy)?;
    }
    Ok(facts)
}

/// Validates one exact exception node and returns its frame references.
fn validate_entry(
    entry: &Map<String, Value>,
    index: usize,
) -> Result<(EntryFacts<'_>, &str, &str), RuntimeError> {
    require_exact_fields(
        entry,
        &[
            "id",
            "parent_id",
            "relationship",
            "type",
            "message",
            "message_state",
            "module",
            "mechanism",
            "stack_frames",
            "stack_frames_state",
        ],
    )?;
    let id = require_u64(entry, "id")?;
    if usize::try_from(id).ok() != Some(index) {
        return Err(invalid_response());
    }
    let relationship = require_string(entry, "relationship")?;
    let parent_id = optional_u64(entry, "parent_id")?;
    if index == 0 {
        if relationship != "reported" || parent_id.is_some() {
            return Err(invalid_response());
        }
    } else if !matches!(
        relationship,
        "cause" | "context" | "aggregate_member" | "suppressed"
    ) || parent_id.is_none_or(|parent| parent >= id)
    {
        return Err(invalid_response());
    }
    let _exception_type = bounded_text(entry, "type", 256, true)?;
    let message_state = require_string(entry, "message_state")?;
    match (
        message_state,
        optional_bounded_text(entry, "message", 1_024, false)?,
    ) {
        ("captured" | "truncated", Some(_)) | ("redacted" | "not_captured", None) => {}
        _ => return Err(invalid_response()),
    }
    let _module = optional_bounded_text(entry, "module", 512, true)?;
    validate_nullable_mechanism(entry.get("mechanism"))?;

    let stack_state = require_string(entry, "stack_frames_state")?;
    let frames = entry
        .get("stack_frames")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    match stack_state {
        "captured" | "truncated" if !frames.is_empty() && frames.len() <= FRAME_LIMIT => {}
        "not_captured" if frames.is_empty() => {}
        _ => return Err(invalid_response()),
    }
    let mut frame_facts = Vec::with_capacity(frames.len());
    for (frame_index, value) in frames.iter().enumerate() {
        let frame = value.as_object().ok_or_else(invalid_response)?;
        validate_frame(frame, frame_index)?;
        frame_facts.push(frame);
    }
    Ok((
        EntryFacts {
            id,
            frames: frame_facts,
        },
        message_state,
        stack_state,
    ))
}

/// Validates one normalized captured frame from the backend projection.
fn validate_frame(frame: &Map<String, Value>, index: usize) -> Result<(), RuntimeError> {
    require_exact_fields(
        frame,
        &[
            "index", "module", "function", "file", "line", "column", "in_app", "source",
        ],
    )?;
    if usize::try_from(require_u64(frame, "index")?).ok() != Some(index)
        || require_string(frame, "source")? != "captured"
    {
        return Err(invalid_response());
    }
    let _module = optional_bounded_text(frame, "module", 512, true)?;
    let _function = optional_bounded_text(frame, "function", 256, false)?;
    let _file = optional_bounded_text(frame, "file", 2_048, true)?;
    let _line = optional_positive_u32(frame, "line")?;
    let _column = optional_positive_u32(frame, "column")?;
    let _in_app = optional_bool(frame, "in_app")?;
    Ok(())
}

/// Binds the first chain node to the legacy exception without comparing additive messages.
fn validate_reported_legacy(
    first: Option<&EntryFacts<'_>>,
    first_value: Option<&Value>,
    legacy: Option<LegacyException<'_>>,
) -> Result<(), RuntimeError> {
    let first = first.ok_or_else(invalid_response)?;
    let first_value = first_value
        .and_then(Value::as_object)
        .ok_or_else(invalid_response)?;
    let legacy = legacy.ok_or_else(invalid_response)?;
    if first.id != 0 || require_string(first_value, "type")? != legacy.exception_type {
        return Err(invalid_response());
    }
    let chain_mechanism = match first_value.get("mechanism") {
        Some(Value::Null) => None,
        Some(Value::Object(value)) => Some(value),
        _ => return Err(invalid_response()),
    };
    if chain_mechanism == legacy.mechanism {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Proves that every chain-specific evidence path is in exactly the expected partitions.
fn validate_receipts(
    evidence: &Map<String, Value>,
    facts: &ChainFacts<'_>,
) -> Result<(), RuntimeError> {
    let captured = facts.status == "captured";
    validate_field_receipts(
        evidence,
        "exception_chain",
        [captured, !captured, false, facts.status == "invalid"],
    )?;
    validate_field_receipts(
        evidence,
        "exception_chain.messages",
        [
            facts.message_states.contains("captured") || facts.message_states.contains("truncated"),
            facts.message_states.contains("not_captured"),
            facts.message_states.contains("redacted"),
            facts.message_states.contains("truncated"),
        ],
    )?;
    validate_field_receipts(
        evidence,
        "exception_chain.stack_frames",
        [
            facts.stack_states.contains("captured") || facts.stack_states.contains("truncated"),
            facts.stack_states.contains("not_captured"),
            false,
            facts.stack_states.contains("truncated"),
        ],
    )?;
    validate_field_receipts(
        evidence,
        "exception_chain.entries",
        [false, false, false, captured && facts.truncated],
    )?;
    let chain_is_partial = facts.status != "captured"
        || facts.truncated
        || facts.message_states.contains("not_captured")
        || facts.message_states.contains("redacted")
        || facts.message_states.contains("truncated")
        || facts.stack_states.contains("not_captured")
        || facts.stack_states.contains("truncated");
    if chain_is_partial && require_string(evidence, "status")? != "partial" {
        Err(invalid_response())
    } else {
        Ok(())
    }
}

/// Requires one field to occur once or zero times in each standard receipt partition.
fn validate_field_receipts(
    evidence: &Map<String, Value>,
    field: &str,
    expected: [bool; 4],
) -> Result<(), RuntimeError> {
    for (index, category) in [
        "captured_fields",
        "missing_fields",
        "redacted_fields",
        "truncated_fields",
    ]
    .iter()
    .enumerate()
    {
        let values = evidence
            .get(*category)
            .and_then(Value::as_array)
            .ok_or_else(invalid_response)?;
        let count = values
            .iter()
            .filter(|value| value.as_str() == Some(field))
            .count();
        if count != usize::from(expected[index])
            || evidence_has_field(evidence, category, field)? != expected[index]
        {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Binds the runtime-chain cause signal to the presence of an underlying node.
fn validate_cause(cause: &Map<String, Value>, facts: &ChainFacts<'_>) -> Result<(), RuntimeError> {
    let signals = cause
        .get("signals")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    let count = signals
        .iter()
        .filter(|value| value.as_str() == Some("runtime_exception_chain"))
        .count();
    let expected = facts.status == "captured" && facts.entries.len() > 1;
    if count == usize::from(expected) {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Binds the derived underlying-fix location to an exact returned exception frame.
fn validate_fix(fix: &Map<String, Value>, facts: &ChainFacts<'_>) -> Result<(), RuntimeError> {
    let status = require_string(fix, "status")?;
    if !matches!(
        status,
        "reported_location"
            | "observed_application_frame"
            | "observed_frame"
            | "observed_underlying_exception_frame"
            | "unknown"
    ) {
        return Err(invalid_response());
    }
    let location = match fix.get("location") {
        Some(Value::Null) => None,
        Some(Value::Object(value)) => Some(value),
        _ => return Err(invalid_response()),
    };
    if status != "observed_underlying_exception_frame" {
        if location.is_some_and(|value| value.contains_key("source_exception_id")) {
            return Err(invalid_response());
        }
        return Ok(());
    }
    if facts.status != "captured" || optional_string(fix, "provenance")? != Some("backend_observed")
    {
        return Err(invalid_response());
    }
    let location = location.ok_or_else(invalid_response)?;
    require_exact_fields(
        location,
        &[
            "component",
            "module",
            "function",
            "file",
            "line",
            "column",
            "in_app",
            "source_exception_id",
        ],
    )?;
    if !location.get("component").is_some_and(Value::is_null) {
        return Err(invalid_response());
    }
    let source_id = require_u64(location, "source_exception_id")?;
    let source = facts
        .entries
        .iter()
        .find(|entry| entry.id == source_id && entry.id > 0)
        .ok_or_else(invalid_response)?;
    if source
        .frames
        .iter()
        .any(|frame| location_matches_frame(location, frame))
    {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Returns whether a derived code location matches one normalized chain frame exactly.
fn location_matches_frame(location: &Map<String, Value>, frame: &Map<String, Value>) -> bool {
    ["module", "function", "file", "line", "column", "in_app"]
        .iter()
        .all(|field| location.get(*field) == frame.get(*field))
}

/// Validates an optional mechanism object with the public low-cardinality grammar.
fn validate_nullable_mechanism(value: Option<&Value>) -> Result<(), RuntimeError> {
    match value {
        Some(Value::Null) => Ok(()),
        Some(Value::Object(value)) => validate_mechanism(value),
        _ => Err(invalid_response()),
    }
}

/// Validates one exact exception capture mechanism.
fn validate_mechanism(value: &Map<String, Value>) -> Result<(), RuntimeError> {
    require_exact_fields(value, &["type", "handled"])?;
    let mechanism = require_string(value, "type")?;
    let mut characters = mechanism.chars();
    if mechanism.chars().count() > 64
        || characters
            .next()
            .is_none_or(|first| !first.is_ascii_alphabetic())
        || characters.any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '_' | '.' | ':' | '-')
        })
    {
        return Err(invalid_response());
    }
    let _handled = require_bool(value, "handled")?;
    Ok(())
}

/// Returns one required bounded string after enforcing backend normalization invariants.
fn bounded_text<'a>(
    value: &'a Map<String, Value>,
    name: &str,
    limit: usize,
    reject_location_markers: bool,
) -> Result<&'a str, RuntimeError> {
    let text = require_string(value, name)?;
    if text == text.trim()
        && text.chars().count() <= limit
        && !text.chars().any(char::is_control)
        && (!reject_location_markers || !text.contains(['?', '#']))
    {
        Ok(text)
    } else {
        Err(invalid_response())
    }
}

/// Returns one required nullable bounded string.
fn optional_bounded_text<'a>(
    value: &'a Map<String, Value>,
    name: &str,
    limit: usize,
    reject_location_markers: bool,
) -> Result<Option<&'a str>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(_)) => {
            bounded_text(value, name, limit, reject_location_markers).map(Some)
        }
        _ => Err(invalid_response()),
    }
}

/// Returns one required nullable unsigned integer.
fn optional_u64(value: &Map<String, Value>, name: &str) -> Result<Option<u64>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::Number(_)) => require_u64(value, name).map(Some),
        _ => Err(invalid_response()),
    }
}

/// Returns one required nullable positive 32-bit integer.
fn optional_positive_u32(
    value: &Map<String, Value>,
    name: &str,
) -> Result<Option<u32>, RuntimeError> {
    optional_u64(value, name)?.map_or_else(
        || Ok(None),
        |value| {
            u32::try_from(value)
                .ok()
                .filter(|value| *value > 0)
                .map(Some)
                .ok_or_else(invalid_response)
        },
    )
}

/// Returns one required nullable boolean.
fn optional_bool(value: &Map<String, Value>, name: &str) -> Result<Option<bool>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        _ => Err(invalid_response()),
    }
}

/// Appends explicit chain state, every bounded node, and a short per-node stack preview.
pub(super) fn render(output: &mut String, value: Option<&Value>) {
    let Some(chain) = value else {
        return;
    };
    output.push_str("Exception chain:");
    append_labeled_text(output, "status", chain, "status", 32);
    let entry_count = chain
        .get("entries")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    output.push_str(" entries=");
    output.push_str(entry_count.to_string().as_str());
    append_labeled_bool(output, "truncated", chain, "truncated");
    output.push('\n');
    let Some(entries) = chain.get("entries").and_then(Value::as_array) else {
        return;
    };
    for entry in entries {
        output.push_str("Exception node");
        append_labeled_integer(output, "id", entry, "id");
        append_labeled_integer(output, "parent", entry, "parent_id");
        append_labeled_text(output, "relationship", entry, "relationship", 32);
        append_labeled_text(output, "type", entry, "type", 256);
        append_labeled_text(output, "module", entry, "module", 160);
        append_labeled_text(output, "message_state", entry, "message_state", 32);
        if matches!(
            entry.get("message_state").and_then(Value::as_str),
            Some("captured" | "truncated")
        ) {
            append_labeled_text(output, "message", entry, "message", 300);
        }
        if let Some(mechanism) = entry.get("mechanism").filter(|value| !value.is_null()) {
            append_labeled_text(output, "mechanism", mechanism, "type", 80);
            append_labeled_bool(output, "handled", mechanism, "handled");
        }
        output.push('\n');
        output.push_str("Exception stack");
        append_labeled_integer(output, "node", entry, "id");
        append_labeled_text(output, "state", entry, "stack_frames_state", 32);
        let frames = entry
            .get("stack_frames")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        output.push_str(" frames=");
        output.push_str(frames.len().to_string().as_str());
        output.push('\n');
        for frame in frames.iter().take(3) {
            output.push_str("Exception frame");
            append_labeled_integer(output, "node", entry, "id");
            append_labeled_integer(output, "index", frame, "index");
            append_labeled_text(output, "module", frame, "module", 160);
            append_labeled_text(output, "function", frame, "function", 200);
            append_labeled_text(output, "file", frame, "file", 240);
            append_labeled_integer(output, "line", frame, "line");
            append_labeled_integer(output, "column", frame, "column");
            append_labeled_bool(output, "in_app", frame, "in_app");
            output.push('\n');
        }
        if frames.len() > 3 {
            output.push_str("Exception frames omitted from human view:");
            append_labeled_integer(output, "node", entry, "id");
            output.push(' ');
            output.push_str((frames.len() - 3).to_string().as_str());
            output.push_str("; use --json for all retained frames.\n");
        }
    }
}
