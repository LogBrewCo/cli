//! Strict schema-version-4 issue occurrence-analysis validation and bounded rendering.

mod render;
mod time;

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::{
    invalid_response, optional_safe_u64, require_exact_fields, require_safe_positive_u64,
    require_safe_u64, require_string, require_timestamp, required_object,
};
use crate::RuntimeError;
use time::parse_utc_millis;

/// Appends the bounded human-readable occurrence-analysis projection when available.
pub(super) fn render(output: &mut String, value: Option<&Value>) {
    render::render(output, value);
}

/// Exact fixed UTC bucket cap published by issue-investigation schema version 4.
const TREND_BUCKET_LIMIT: usize = 30;
/// Exact number of supported occurrence distributions.
const DISTRIBUTION_COUNT: usize = 4;
/// Exact named-value cap for each occurrence distribution.
const DISTRIBUTION_VALUE_LIMIT: usize = 10;
/// Milliseconds in one second.
const MILLIS_PER_SECOND: i128 = 1_000;
/// Stable distribution order and evidence/limitation vocabulary.
const DIMENSIONS: [DimensionContract; DISTRIBUTION_COUNT] = [
    DimensionContract {
        name: "release",
        field: "occurrence.distribution.release",
        limitation: "release_distribution_unavailable",
    },
    DimensionContract {
        name: "environment",
        field: "occurrence.distribution.environment",
        limitation: "environment_distribution_unavailable",
    },
    DimensionContract {
        name: "service",
        field: "occurrence.distribution.service",
        limitation: "service_distribution_unavailable",
    },
    DimensionContract {
        name: "sdk",
        field: "occurrence.distribution.sdk",
        limitation: "sdk_distribution_unavailable",
    },
];

/// One fixed distribution contract.
#[derive(Clone, Copy)]
struct DimensionContract {
    /// Stable public dimension.
    name: &'static str,
    /// Shared evidence field.
    field: &'static str,
    /// Stable limitation when the dimension is unavailable.
    limitation: &'static str,
}

/// Validated distribution availability and named-value truncation.
#[derive(Clone, Copy)]
struct DistributionFacts {
    /// Index in [`DIMENSIONS`].
    index: usize,
    /// Whether values outside the named projection exist.
    truncated: bool,
}

/// Validates exact occurrence volume, fixed buckets, distributions, limitations, and receipts.
pub(super) fn validate(
    analysis: &Map<String, Value>,
    retained_occurrences: u64,
    first_seen: &str,
    last_seen: &str,
    evidence: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        analysis,
        &[
            "status",
            "coverage",
            "trend",
            "distributions",
            "limitations",
        ],
    )?;
    let status = require_string(analysis, "status")?;
    if !matches!(status, "complete" | "partial" | "unavailable") {
        return Err(invalid_response());
    }
    let coverage = required_object(analysis, "coverage")?;
    validate_coverage_constants(coverage, retained_occurrences)?;

    let trend_available = match analysis.get("trend") {
        Some(Value::Null) => false,
        Some(Value::Object(trend)) => {
            validate_trend(trend, retained_occurrences, first_seen, last_seen)?;
            true
        }
        _ => return Err(invalid_response()),
    };
    let trend_occurrences = optional_safe_u64(coverage, "trend_occurrences")?;
    if trend_occurrences != trend_available.then_some(retained_occurrences) {
        return Err(invalid_response());
    }

    let distributions = validate_distributions(
        analysis.get("distributions").ok_or_else(invalid_response)?,
        retained_occurrences,
    )?;
    if require_safe_u64(coverage, "available_distribution_count")?
        != u64::try_from(distributions.len()).map_err(|_error| invalid_response())?
    {
        return Err(invalid_response());
    }

    let component_count = usize::from(trend_available) + distributions.len();
    let expected_status = if component_count == 0 {
        "unavailable"
    } else if component_count == DISTRIBUTION_COUNT + 1 {
        "complete"
    } else {
        "partial"
    };
    if status != expected_status {
        return Err(invalid_response());
    }

    let limitations = expected_limitations(trend_available, distributions.as_slice());
    validate_limitations(analysis, limitations.as_slice())?;
    validate_evidence_receipts(
        evidence,
        trend_available,
        distributions.as_slice(),
        !limitations.is_empty(),
    )
}

/// Proves fixed response caps and the grouped occurrence receipt.
fn validate_coverage_constants(
    coverage: &Map<String, Value>,
    retained_occurrences: u64,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        coverage,
        &[
            "retained_occurrences",
            "trend_occurrences",
            "available_distribution_count",
            "expected_distribution_count",
            "max_buckets",
            "max_values_per_dimension",
        ],
    )?;
    if require_safe_positive_u64(coverage, "retained_occurrences")? == retained_occurrences
        && require_safe_u64(coverage, "expected_distribution_count")?
            == u64::try_from(DISTRIBUTION_COUNT).map_err(|_error| invalid_response())?
        && require_safe_u64(coverage, "max_buckets")?
            == u64::try_from(TREND_BUCKET_LIMIT).map_err(|_error| invalid_response())?
        && require_safe_u64(coverage, "max_values_per_dimension")?
            == u64::try_from(DISTRIBUTION_VALUE_LIMIT).map_err(|_error| invalid_response())?
    {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Validates exact scope, contiguous epoch-aligned buckets, interval width, and total volume.
fn validate_trend(
    trend: &Map<String, Value>,
    retained_occurrences: u64,
    first_seen: &str,
    last_seen: &str,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        trend,
        &["scope_start", "scope_end", "interval_seconds", "buckets"],
    )?;
    if require_timestamp(trend, "scope_start")? != first_seen
        || require_timestamp(trend, "scope_end")? != last_seen
    {
        return Err(invalid_response());
    }
    let interval_seconds = require_safe_positive_u64(trend, "interval_seconds")?;
    let interval_millis = i128::from(interval_seconds)
        .checked_mul(MILLIS_PER_SECOND)
        .ok_or_else(invalid_response)?;
    let buckets = trend
        .get("buckets")
        .and_then(Value::as_array)
        .filter(|buckets| !buckets.is_empty() && buckets.len() <= TREND_BUCKET_LIMIT)
        .ok_or_else(invalid_response)?;
    let scope_start = parse_utc_millis(first_seen).ok_or_else(invalid_response)?;
    let scope_end = parse_utc_millis(last_seen).ok_or_else(invalid_response)?;
    let mut previous_end = None;
    let mut first_bounds = None;
    let mut last_bounds = None;
    let mut occurrence_sum = 0_u64;
    for bucket in buckets {
        let bucket = bucket.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(bucket, &["bucket_start", "bucket_end", "occurrence_count"])?;
        let start = parse_utc_millis(require_timestamp(bucket, "bucket_start")?)
            .ok_or_else(invalid_response)?;
        let end = parse_utc_millis(require_timestamp(bucket, "bucket_end")?)
            .ok_or_else(invalid_response)?;
        let count = require_safe_u64(bucket, "occurrence_count")?;
        if end.checked_sub(start) != Some(interval_millis)
            || start.rem_euclid(interval_millis) != 0
            || previous_end.is_some_and(|previous| previous != start)
        {
            return Err(invalid_response());
        }
        occurrence_sum = occurrence_sum
            .checked_add(count)
            .ok_or_else(invalid_response)?;
        let _ = first_bounds.get_or_insert((start, end));
        last_bounds = Some((start, end));
        previous_end = Some(end);
    }
    let Some((first_start, first_end)) = first_bounds else {
        return Err(invalid_response());
    };
    let Some((last_start, last_end)) = last_bounds else {
        return Err(invalid_response());
    };
    if occurrence_sum == retained_occurrences
        && (first_start..first_end).contains(&scope_start)
        && (last_start..last_end).contains(&scope_end)
    {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Validates a stable-order subset of the four supported distributions.
fn validate_distributions(
    value: &Value,
    retained_occurrences: u64,
) -> Result<Vec<DistributionFacts>, RuntimeError> {
    let values = value.as_array().ok_or_else(invalid_response)?;
    if values.len() > DISTRIBUTION_COUNT {
        return Err(invalid_response());
    }
    let mut facts = Vec::with_capacity(values.len());
    let mut previous_index = None;
    for value in values {
        let distribution = value.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(
            distribution,
            &[
                "dimension",
                "distinct_value_count",
                "values",
                "other_occurrence_count",
            ],
        )?;
        let dimension = require_string(distribution, "dimension")?;
        let index = DIMENSIONS
            .iter()
            .position(|contract| contract.name == dimension)
            .ok_or_else(invalid_response)?;
        if previous_index.is_some_and(|previous| previous >= index) {
            return Err(invalid_response());
        }
        let truncated = validate_distribution(distribution, index, retained_occurrences)?;
        facts.push(DistributionFacts { index, truncated });
        previous_index = Some(index);
    }
    Ok(facts)
}

/// Validates exact named-value ordering, shares, cardinality, and remainder arithmetic.
fn validate_distribution(
    distribution: &Map<String, Value>,
    dimension_index: usize,
    retained_occurrences: u64,
) -> Result<bool, RuntimeError> {
    let distinct = require_safe_positive_u64(distribution, "distinct_value_count")?;
    let values = distribution
        .get("values")
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty() && values.len() <= DISTRIBUTION_VALUE_LIMIT)
        .ok_or_else(invalid_response)?;
    if distinct < u64::try_from(values.len()).map_err(|_error| invalid_response())?
        || distinct > retained_occurrences
    {
        return Err(invalid_response());
    }
    let sdk_dimension = DIMENSIONS[dimension_index].name == "sdk";
    let mut seen = BTreeSet::new();
    let mut previous: Option<(u64, &str, Option<&str>)> = None;
    let mut named_occurrences = 0_u64;
    for value in values {
        let value = value.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(
            value,
            &["value", "version", "occurrence_count", "share_basis_points"],
        )?;
        let name = require_string(value, "value")?;
        if name.is_empty() {
            return Err(invalid_response());
        }
        let version = match value.get("version") {
            Some(Value::String(version)) if sdk_dimension && !version.is_empty() => {
                Some(version.as_str())
            }
            Some(Value::Null) if !sdk_dimension => None,
            _ => return Err(invalid_response()),
        };
        let count = require_safe_positive_u64(value, "occurrence_count")?;
        let share = require_safe_u64(value, "share_basis_points")?;
        let expected_share = u128::from(count)
            .checked_mul(10_000)
            .and_then(|value| value.checked_div(u128::from(retained_occurrences)))
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(invalid_response)?;
        let key = (name, version);
        if count > retained_occurrences
            || share != expected_share
            || share > 10_000
            || !seen.insert(key)
            || previous.is_some_and(|(prior_count, prior_name, prior_version)| {
                let prior_key = (prior_name, prior_version);
                let current_key = (name, version);
                prior_count < count || (prior_count == count && prior_key > current_key)
            })
        {
            return Err(invalid_response());
        }
        named_occurrences = named_occurrences
            .checked_add(count)
            .ok_or_else(invalid_response)?;
        previous = Some((count, name, version));
    }
    let other = require_safe_u64(distribution, "other_occurrence_count")?;
    let truncated = other > 0;
    if named_occurrences.checked_add(other) == Some(retained_occurrences)
        && (distinct > u64::try_from(values.len()).map_err(|_error| invalid_response())?)
            == truncated
    {
        Ok(truncated)
    } else {
        Err(invalid_response())
    }
}

/// Derives the exact canonical limitation list from unavailable components.
fn expected_limitations(
    trend_available: bool,
    distributions: &[DistributionFacts],
) -> Vec<&'static str> {
    let mut limitations = Vec::new();
    if !trend_available {
        limitations.push("trend_read_unavailable");
    }
    for (index, dimension) in DIMENSIONS.iter().enumerate() {
        if !distributions.iter().any(|facts| facts.index == index) {
            limitations.push(dimension.limitation);
        }
    }
    limitations
}

/// Requires exactly the derived, unique, stable-order limitation vocabulary.
fn validate_limitations(
    analysis: &Map<String, Value>,
    expected: &[&str],
) -> Result<(), RuntimeError> {
    let limitations = analysis
        .get("limitations")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    if limitations.len() == expected.len()
        && limitations
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.as_str() == Some(*expected))
    {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Binds availability and bounded named-value omissions to shared evidence receipts.
fn validate_evidence_receipts(
    evidence: &Map<String, Value>,
    trend_available: bool,
    distributions: &[DistributionFacts],
    incomplete: bool,
) -> Result<(), RuntimeError> {
    validate_occurrence_receipt_vocabulary(evidence)?;
    validate_component_receipt(evidence, "occurrence.trend", trend_available, false)?;
    let mut truncated = false;
    for (index, dimension) in DIMENSIONS.iter().enumerate() {
        let facts = distributions.iter().find(|facts| facts.index == index);
        let value_truncated = facts.is_some_and(|facts| facts.truncated);
        validate_component_receipt(evidence, dimension.field, facts.is_some(), value_truncated)?;
        truncated |= value_truncated;
    }
    if (incomplete || truncated) && require_string(evidence, "status")? != "partial" {
        Err(invalid_response())
    } else {
        Ok(())
    }
}

/// Requires one component in exactly one availability category and an exact truncation receipt.
fn validate_component_receipt(
    evidence: &Map<String, Value>,
    field: &str,
    available: bool,
    truncated: bool,
) -> Result<(), RuntimeError> {
    let captured = evidence_count(evidence, "captured_fields", field)?;
    let missing = evidence_count(evidence, "missing_fields", field)?;
    let redacted = evidence_count(evidence, "redacted_fields", field)?;
    let truncated_field = format!("{field}.values");
    let base_truncated = evidence_count(evidence, "truncated_fields", field)?;
    let truncated_captured = evidence_count(evidence, "captured_fields", truncated_field.as_str())?;
    let truncated_missing = evidence_count(evidence, "missing_fields", truncated_field.as_str())?;
    let truncated_redacted = evidence_count(evidence, "redacted_fields", truncated_field.as_str())?;
    let truncated_count = evidence_count(evidence, "truncated_fields", truncated_field.as_str())?;
    if captured == usize::from(available)
        && missing == usize::from(!available)
        && redacted == 0
        && base_truncated == 0
        && truncated_captured == 0
        && truncated_missing == 0
        && truncated_redacted == 0
        && truncated_count == usize::from(truncated)
    {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Rejects additive occurrence receipt names unless a future schema version declares them.
fn validate_occurrence_receipt_vocabulary(
    evidence: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    let allowed = [
        "occurrence.boundaries",
        "occurrence.recommendation",
        "occurrence.selection",
        "occurrence.trend",
        "occurrence.distribution.release",
        "occurrence.distribution.release.values",
        "occurrence.distribution.environment",
        "occurrence.distribution.environment.values",
        "occurrence.distribution.service",
        "occurrence.distribution.service.values",
        "occurrence.distribution.sdk",
        "occurrence.distribution.sdk.values",
    ];
    for category in [
        "captured_fields",
        "missing_fields",
        "redacted_fields",
        "truncated_fields",
    ] {
        let fields = evidence
            .get(category)
            .and_then(Value::as_array)
            .ok_or_else(invalid_response)?;
        if fields
            .iter()
            .filter_map(Value::as_str)
            .any(|field| field.starts_with("occurrence.") && !allowed.contains(&field))
        {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Counts exact shared evidence receipts so duplicate entries fail closed.
fn evidence_count(
    evidence: &Map<String, Value>,
    category: &str,
    field: &str,
) -> Result<usize, RuntimeError> {
    evidence
        .get(category)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter(|value| value.as_str() == Some(field))
                .count()
        })
        .ok_or_else(invalid_response)
}
