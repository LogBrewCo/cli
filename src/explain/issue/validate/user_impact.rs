//! Affected-user coverage validation for issue investigations.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::RuntimeError;
use crate::explain::{
    invalid_response, optional_safe_u64, require_safe_u64, require_string, required_object,
};

/// Exact occurrence-level capture receipt used to validate affected-user semantics.
#[derive(Clone, Copy)]
struct IssueUserImpactCoverage {
    /// Grouped occurrences retained for this issue and investigation window.
    retained: u64,
    /// Retained occurrences written with a supported subject-index version.
    indexed: u64,
    /// Retained occurrences predating subject indexing.
    historical: u64,
    /// Indexed occurrences carrying a privacy-safe identified-subject key.
    identified: u64,
    /// Indexed occurrences explicitly captured without an identified subject.
    anonymous: u64,
    /// Indexed occurrences missing the expected subject context.
    missing: u64,
    /// Indexed occurrences whose subject context was excluded by privacy policy.
    privacy_filtered: u64,
}

/// Validates affected-user status, method, coverage, legacy alias, and limitation invariants.
pub(super) fn validate_issue_user_impact(
    impact: &Map<String, Value>,
    occurrence_count: u64,
) -> Result<(), RuntimeError> {
    let user_impact = required_object(impact, "user_impact")?;
    let status = require_string(user_impact, "status")?;
    if !matches!(
        status,
        "complete" | "partial" | "not_captured" | "unavailable"
    ) {
        return Err(invalid_response());
    }
    let known = optional_safe_u64(user_impact, "known_affected_users")?;
    let method = require_string(user_impact, "count_method")?;
    if !matches!(method, "approximate_distinct" | "unavailable") {
        return Err(invalid_response());
    }
    let coverage = match user_impact.get("coverage") {
        Some(Value::Null) => None,
        Some(Value::Object(value)) => Some(validate_issue_user_impact_coverage(value)?),
        _ => return Err(invalid_response()),
    };
    let limitations = issue_user_impact_limitations(user_impact)?;
    let legacy = optional_safe_u64(impact, "affected_users")?;

    if status == "unavailable" {
        let expected = BTreeSet::from([String::from("user_impact_read_unavailable")]);
        return if known.is_none()
            && method == "unavailable"
            && coverage.is_none()
            && legacy.is_none()
            && limitations == expected
        {
            Ok(())
        } else {
            Err(invalid_response())
        };
    }
    let coverage = coverage.ok_or_else(invalid_response)?;
    if coverage.retained != occurrence_count {
        return Err(invalid_response());
    }
    validate_available_issue_user_impact(status, method, known, legacy, coverage, &limitations)
}

/// Validates exact coverage arithmetic and basis-point receipts.
fn validate_issue_user_impact_coverage(
    value: &Map<String, Value>,
) -> Result<IssueUserImpactCoverage, RuntimeError> {
    let coverage = IssueUserImpactCoverage {
        retained: require_safe_u64(value, "retained_occurrences")?,
        indexed: require_safe_u64(value, "indexed_occurrences")?,
        historical: require_safe_u64(value, "historical_unindexed_occurrences")?,
        identified: require_safe_u64(value, "identified_user_occurrences")?,
        anonymous: require_safe_u64(value, "anonymous_subject_occurrences")?,
        missing: require_safe_u64(value, "missing_subject_occurrences")?,
        privacy_filtered: require_safe_u64(value, "privacy_filtered_subject_occurrences")?,
    };
    let indexed_and_historical = coverage
        .indexed
        .checked_add(coverage.historical)
        .ok_or_else(invalid_response)?;
    let classified = coverage
        .identified
        .checked_add(coverage.anonymous)
        .and_then(|value| value.checked_add(coverage.missing))
        .and_then(|value| value.checked_add(coverage.privacy_filtered))
        .ok_or_else(invalid_response)?;
    let index_basis_points = require_safe_u64(value, "index_coverage_basis_points")?;
    let identified_basis_points =
        optional_safe_u64(value, "identified_user_coverage_basis_points")?;
    if coverage.retained == 0
        || coverage.retained != indexed_and_historical
        || coverage.indexed != classified
        || index_basis_points != exact_basis_points(coverage.indexed, coverage.retained)
        || identified_basis_points
            != (coverage.indexed > 0)
                .then(|| exact_basis_points(coverage.identified, coverage.indexed))
    {
        return Err(invalid_response());
    }
    Ok(coverage)
}

/// Validates status-specific known counts and derives the exact limitation set.
fn validate_available_issue_user_impact(
    status: &str,
    method: &str,
    known: Option<u64>,
    legacy: Option<u64>,
    coverage: IssueUserImpactCoverage,
    limitations: &BTreeSet<String>,
) -> Result<(), RuntimeError> {
    let complete = coverage.indexed == coverage.retained
        && coverage.identified == coverage.indexed
        && coverage.historical == 0
        && coverage.anonymous == 0
        && coverage.missing == 0
        && coverage.privacy_filtered == 0;
    let has_valid_known = known.is_some_and(|value| value > 0 && value <= coverage.identified);
    let valid_shape = match status {
        "complete" => {
            complete && has_valid_known && method == "approximate_distinct" && legacy == known
        }
        "partial" => {
            !complete
                && coverage.identified > 0
                && has_valid_known
                && method == "approximate_distinct"
                && legacy.is_none()
        }
        "not_captured" => {
            coverage.identified == 0
                && known.is_none()
                && method == "unavailable"
                && legacy.is_none()
        }
        _ => false,
    };
    if !valid_shape || *limitations != expected_issue_user_impact_limitations(known, coverage) {
        return Err(invalid_response());
    }
    Ok(())
}

/// Parses a unique bounded set of supported user-impact limitation codes.
fn issue_user_impact_limitations(
    value: &Map<String, Value>,
) -> Result<BTreeSet<String>, RuntimeError> {
    let values = value
        .get("limitations")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    if values.len() > 6 {
        return Err(invalid_response());
    }
    let mut limitations = BTreeSet::new();
    for value in values {
        let code = value.as_str().ok_or_else(invalid_response)?;
        if !matches!(
            code,
            "approximate_distinct_count"
                | "historical_occurrences_unindexed"
                | "anonymous_subjects_excluded"
                | "missing_subject_context"
                | "privacy_filtered_subject_context"
                | "user_impact_read_unavailable"
        ) || !limitations.insert(code.to_owned())
        {
            return Err(invalid_response());
        }
    }
    Ok(limitations)
}

/// Derives every limitation implied by exact coverage; order is intentionally irrelevant.
fn expected_issue_user_impact_limitations(
    known: Option<u64>,
    coverage: IssueUserImpactCoverage,
) -> BTreeSet<String> {
    let mut limitations = BTreeSet::new();
    if known.is_some() {
        let _ = limitations.insert(String::from("approximate_distinct_count"));
    }
    for (count, code) in [
        (coverage.historical, "historical_occurrences_unindexed"),
        (coverage.anonymous, "anonymous_subjects_excluded"),
        (coverage.missing, "missing_subject_context"),
        (
            coverage.privacy_filtered,
            "privacy_filtered_subject_context",
        ),
    ] {
        if count > 0 {
            let _ = limitations.insert(String::from(code));
        }
    }
    limitations
}

/// Computes an exact floor-rounded percentage in basis points.
fn exact_basis_points(numerator: u64, denominator: u64) -> u64 {
    debug_assert!(
        denominator > 0,
        "validated coverage denominator must be positive"
    );
    u64::try_from(u128::from(numerator) * 10_000 / u128::from(denominator)).unwrap_or(10_000)
}
