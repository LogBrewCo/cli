//! Local CLI identifier shape helpers.

use crate::{ExplainTarget, IssueOccurrenceSelection};

/// Infers an explain target from obvious pasted IDs.
pub(crate) fn infer_explain_target(value: &str) -> Option<ExplainTarget> {
    if is_trace_id(value) {
        Some(ExplainTarget::Trace(value.to_owned()))
    } else if is_issue_id(value) {
        Some(ExplainTarget::Issue {
            id: value.to_owned(),
            occurrence: IssueOccurrenceSelection::Recommended,
        })
    } else {
        None
    }
}

/// Returns whether a word is an obvious copied detail id.
pub(crate) fn is_pasted_detail_id(value: &str) -> bool {
    is_trace_id(value) || is_issue_id(value)
}

/// Returns whether a value is shaped like a W3C trace id.
pub(crate) fn is_trace_id(value: &str) -> bool {
    value
        .strip_prefix("trace_")
        .or_else(|| value.strip_prefix("trace-"))
        .is_some()
        || (value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// Returns whether a value is shaped like a grouped issue id.
pub(crate) fn is_issue_id(value: &str) -> bool {
    value
        .strip_prefix("issue_")
        .or_else(|| value.strip_prefix("issue-"))
        .is_some()
        || is_uuid(value)
}

/// Returns whether a value is a dashed UUID.
pub(crate) fn is_uuid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    value.bytes().enumerate().all(|(index, byte)| {
        matches!(index, 8 | 13 | 18 | 23) && byte == b'-'
            || !matches!(index, 8 | 13 | 18 | 23) && byte.is_ascii_hexdigit()
    })
}

/// Returns whether a value is a non-nil dashed UUID.
pub(crate) fn is_non_nil_uuid(value: &str) -> bool {
    is_uuid(value) && value.bytes().any(|byte| !matches!(byte, b'0' | b'-'))
}

/// Returns whether a value is a canonical public support-ticket identifier.
pub(crate) fn is_support_ticket_id(value: &str) -> bool {
    value.strip_prefix("sup_").is_some_and(|raw| {
        raw.len() == 32
            && raw
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

/// Returns whether a value is a canonical public support-context identifier.
pub(crate) fn is_support_context_id(value: &str) -> bool {
    value.strip_prefix("ctx_").is_some_and(|raw| {
        raw.len() == 32
            && raw
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}
