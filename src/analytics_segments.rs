//! Versioned, bounded product-analytics segment comparison reporting.

use serde::Deserialize;

use crate::auth::{AuthCredential, send_authenticated_with_refresh};
use crate::{
    AnalyticsSegment, AnalyticsSegmentComparisonOptions, AnalyticsSegmentEventKind,
    AnalyticsSegmentUnit, CliEnvironment, RuntimeError,
};

/// Public response version implemented by this CLI.
const SCHEMA_VERSION: u8 = 1;
/// Maximum accepted response body, matching the API result-byte bound.
const RESPONSE_LIMIT: usize = 10 * 1024 * 1024;
/// Server-side scan cap also bounds every returned count.
const COUNT_LIMIT: u64 = 10_000_000;
/// Maximum UTC-aligned buckets returned for one segment.
const BUCKET_LIMIT: usize = 500;
/// Automatic interval target used by the API.
const AUTO_BUCKET_TARGET: u64 = 300;
/// Maximum material limitations accepted from the bounded API.
const LIMITATION_LIMIT: usize = 8;
/// Maximum time-series points rendered per segment in human mode.
const HUMAN_POINT_LIMIT: usize = 12;
/// Nanoseconds in one second.
const NANOS_PER_SECOND: i128 = 1_000_000_000;

/// Builds the exact public POST body with explicit CLI defaults.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command model consumes this private-module helper"
)]
pub(super) fn request_body(options: &AnalyticsSegmentComparisonOptions) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    drop(body.insert(
        "project_id".to_owned(),
        serde_json::Value::String(options.project_id.clone()),
    ));
    drop(body.insert(
        "since".to_owned(),
        serde_json::Value::String(options.since.clone()),
    ));
    if let Some(until) = options.until.as_deref() {
        drop(body.insert(
            "until".to_owned(),
            serde_json::Value::String(until.to_owned()),
        ));
    }
    drop(body.insert(
        "interval".to_owned(),
        serde_json::Value::String(options.interval.clone()),
    ));
    drop(body.insert(
        "analysis_unit".to_owned(),
        serde_json::Value::String(options.analysis_unit.as_str().to_owned()),
    ));
    drop(body.insert(
        "target".to_owned(),
        serde_json::json!({
            "kind": options.target_kind.as_str(),
            "event_name": options.target_event,
        }),
    ));
    drop(body.insert(
        "segments".to_owned(),
        serde_json::Value::Array(options.segments.iter().map(segment_body).collect()),
    ));
    serde_json::Value::Object(body)
}

/// Builds one exact segment request without unrelated null fields.
fn segment_body(segment: &AnalyticsSegment) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    drop(body.insert(
        "key".to_owned(),
        serde_json::Value::String(segment.key.clone()),
    ));
    drop(body.insert(
        "label".to_owned(),
        serde_json::Value::String(segment.label.clone()),
    ));
    insert_optional(&mut body, "service_name", segment.service_name.as_deref());
    insert_optional(&mut body, "release", segment.release.as_deref());
    insert_optional(&mut body, "environment", segment.environment.as_deref());
    if !segment.property_filters.is_empty() {
        drop(
            body.insert(
                "property_filters".to_owned(),
                serde_json::Value::Array(
                    segment
                        .property_filters
                        .iter()
                        .map(|filter| {
                            serde_json::json!({
                                "key": filter.key,
                                "value": filter.value,
                            })
                        })
                        .collect(),
                ),
            ),
        );
    }
    serde_json::Value::Object(body)
}

/// Adds one optional exact context filter without sending null placeholders.
fn insert_optional(
    body: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        drop(body.insert(key.to_owned(), serde_json::Value::String(value.to_owned())));
    }
}

/// Executes one bounded, identity-safe segment comparison request.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    options: &AnalyticsSegmentComparisonOptions,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let origin = normalized_origin(env.base_url.as_str())?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_error| transport_error())?;
    let url = format!("{origin}/api/telemetry/analytics/segments/compare");
    let request = request_body(options);
    let response = send_authenticated_with_refresh(&client, env, |client, credential| {
        client
            .post(url.as_str())
            .bearer_auth(credential.token())
            .json(&request)
    })
    .await
    .map_err(request_error)?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(safe_api_error(status, &credential));
    }
    let body = bounded_body(response).await?;
    let response = validated_response(options, body.as_str())?;
    if json {
        writeln!(output, "{body}")?;
    } else {
        write!(output, "{}", render_response(&response))?;
    }
    Ok(())
}

/// Reads a successful response incrementally and rejects oversized content.
async fn bounded_body(mut response: reqwest::Response) -> Result<String, RuntimeError> {
    if response.content_length().is_some_and(|length| {
        usize::try_from(length).map_or(true, |length| length > RESPONSE_LIMIT)
    }) {
        return Err(invalid_response());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_error| transport_error())? {
        if body.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
            return Err(invalid_response());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_error| invalid_response())
}

/// Complete response with unknown fields rejected at every level.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComparisonResponse {
    schema_version: u8,
    query: ComparisonQuery,
    purpose: String,
    summary: ComparisonSummary,
    confidence: ComparisonConfidence,
    segments: Vec<SegmentResult>,
    next_action: NextAction,
}

/// Normalized effective query echoed by the backend.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComparisonQuery {
    project_id: String,
    since: String,
    until: String,
    interval: ComparisonInterval,
    interval_seconds: u64,
    analysis_unit: AnalyticsSegmentUnit,
    target: ComparisonTarget,
    segments: Vec<SegmentScope>,
}

/// One supported fixed UTC comparison interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
enum ComparisonInterval {
    /// One minute.
    #[serde(rename = "1m")]
    OneMinute,
    /// Five minutes.
    #[serde(rename = "5m")]
    FiveMinutes,
    /// Fifteen minutes.
    #[serde(rename = "15m")]
    FifteenMinutes,
    /// One hour.
    #[serde(rename = "1h")]
    OneHour,
    /// Six hours.
    #[serde(rename = "6h")]
    SixHours,
    /// One day.
    #[serde(rename = "1d")]
    OneDay,
}

impl ComparisonInterval {
    /// Returns the stable public API token.
    const fn as_str(self) -> &'static str {
        match self {
            Self::OneMinute => "1m",
            Self::FiveMinutes => "5m",
            Self::FifteenMinutes => "15m",
            Self::OneHour => "1h",
            Self::SixHours => "6h",
            Self::OneDay => "1d",
        }
    }

    /// Returns the fixed width in seconds.
    const fn seconds(self) -> u64 {
        match self {
            Self::OneMinute => 60,
            Self::FiveMinutes => 5 * 60,
            Self::FifteenMinutes => 15 * 60,
            Self::OneHour => 60 * 60,
            Self::SixHours => 6 * 60 * 60,
            Self::OneDay => 24 * 60 * 60,
        }
    }

    /// Returns supported intervals in ascending width order.
    const fn supported() -> [Self; 6] {
        [
            Self::OneMinute,
            Self::FiveMinutes,
            Self::FifteenMinutes,
            Self::OneHour,
            Self::SixHours,
            Self::OneDay,
        ]
    }
}

/// One exact classified outcome.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComparisonTarget {
    kind: AnalyticsSegmentEventKind,
    event_name: String,
}

/// One normalized exact context segment.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentScope {
    key: String,
    label: String,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    #[serde(default)]
    property_filters: Vec<PropertyFilter>,
}

/// One normalized exact property predicate echoed by the backend.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyFilter {
    key: String,
    value: String,
}

/// High-level comparison state.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComparisonSummary {
    segment_count: u8,
    baseline_segment_key: String,
    segments_with_eligible_units: u8,
    segments_with_target_events: u8,
    segments_with_reached_units: u8,
}

/// Accuracy and interpretation boundary.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComparisonConfidence {
    interpretation: String,
    unique_count_accuracy: UniqueCountAccuracy,
    estimation_method: String,
    causal_inference: CausalInference,
    statistical_significance: StatisticalSignificance,
    segment_overlap: SegmentOverlap,
    limitations: Vec<String>,
}

/// Unique-count accuracy class supported by schema version 1.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum UniqueCountAccuracy {
    /// Bounded approximate cardinality estimation.
    Approximate,
}

/// Causal interpretation supported by schema version 1.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CausalInference {
    /// No causal relationship is established.
    NotEstablished,
}

/// Statistical test state supported by schema version 1.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StatisticalSignificance {
    /// No significance test was performed.
    NotTested,
}

/// Membership relationship supported by schema version 1.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SegmentOverlap {
    /// The exact segments can overlap.
    Possible,
}

/// One complete segment outcome.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentResult {
    key: String,
    label: String,
    eligible_units: u64,
    reached_units: u64,
    reach_rate: Option<f64>,
    usable_target_events_per_reached_unit: Option<f64>,
    coverage: SegmentCoverage,
    series: Vec<SegmentPoint>,
    comparison_to_baseline: Option<BaselineComparison>,
}

/// Capture coverage qualifying one segment result.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentCoverage {
    classified_events: u64,
    unit_identified_events: u64,
    excluded_events: u64,
    unit_coverage_rate: Option<f64>,
    target_events: u64,
    usable_target_events: u64,
    excluded_target_events: u64,
    target_unit_coverage_rate: Option<f64>,
    traced_target_events: u64,
    target_trace_link_rate: Option<f64>,
    property_filters: Option<PropertyCoverage>,
}

/// Property-index readiness and exact-value match coverage for one segment.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyCoverage {
    context_events: u64,
    property_ready_events: u64,
    missing_property_events: u64,
    property_ready_rate: Option<f64>,
    matching_events: u64,
    nonmatching_value_events: u64,
    match_rate: Option<f64>,
}

/// One ordered non-empty comparison bucket.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentPoint {
    bucket_start: String,
    bucket_end: String,
    classified_events: u64,
    eligible_units: u64,
    target_events: u64,
    usable_target_events: u64,
    reached_units: u64,
    reach_rate: Option<f64>,
}

/// Descriptive difference from the first requested segment.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BaselineComparison {
    eligible_units_difference: i64,
    reached_units_difference: i64,
    target_events_difference: i64,
    reach_rate_difference: Option<f64>,
    relative_reach_rate_lift: Option<f64>,
}

/// Stable server-selected follow-up.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NextAction {
    code: String,
    target: String,
    reason: String,
}

/// Parsed canonical UTC timestamp used only for contract validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct UtcTimestamp {
    /// Whole seconds from the Unix epoch.
    epoch_seconds: i64,
    /// Fractional nanoseconds inside the second.
    nanoseconds: u32,
}

/// Parses and proves the complete schema-version-1 response.
fn validated_response(
    options: &AnalyticsSegmentComparisonOptions,
    body: &str,
) -> Result<ComparisonResponse, RuntimeError> {
    let response =
        serde_json::from_str::<ComparisonResponse>(body).map_err(|_error| invalid_response())?;
    if response.schema_version != SCHEMA_VERSION
        || !bounded_contract_text(response.purpose.as_str(), 4096)
        || !valid_query(options, &response.query)
        || !valid_confidence(&response.confidence)
        || !valid_summary_and_segments(options, &response)
        || !valid_next_action(&response)
    {
        return Err(invalid_response());
    }
    Ok(response)
}

/// Requires the backend echo to match every client-selected scope field.
fn valid_query(options: &AnalyticsSegmentComparisonOptions, query: &ComparisonQuery) -> bool {
    let Some((since, until)) = query_timestamps(query) else {
        return false;
    };
    let interval_seconds = query.interval.seconds();
    let Some(expected_buckets) = bucket_count(since, until, interval_seconds) else {
        return false;
    };
    query.project_id == options.project_id
        && query.interval_seconds == interval_seconds
        && interval_matches(options.interval.as_str(), query.interval, since, until)
        && expected_buckets <= u64::try_from(BUCKET_LIMIT).unwrap_or(u64::MAX)
        && query.analysis_unit == options.analysis_unit
        && query.target.kind == options.target_kind
        && query.target.event_name == options.target_event
        && valid_event_name(query.target.kind, query.target.event_name.as_str())
        && query.segments.len() == options.segments.len()
        && query
            .segments
            .iter()
            .zip(options.segments.iter())
            .all(|(actual, expected)| segment_scope_matches(actual, expected))
}

/// Parses the normalized window and enforces the positive 31-day range bound.
fn query_timestamps(query: &ComparisonQuery) -> Option<(UtcTimestamp, UtcTimestamp)> {
    let since = parse_utc_timestamp(query.since.as_str())?;
    let until = parse_utc_timestamp(query.until.as_str())?;
    let range = timestamp_nanos(until).checked_sub(timestamp_nanos(since))?;
    (since < until && range <= i128::from(31 * 24 * 60 * 60) * NANOS_PER_SECOND)
        .then_some((since, until))
}

/// Proves fixed or automatic interval selection against the exact aligned range.
fn interval_matches(
    requested: &str,
    actual: ComparisonInterval,
    since: UtcTimestamp,
    until: UtcTimestamp,
) -> bool {
    if requested != "auto" {
        return requested == actual.as_str();
    }
    ComparisonInterval::supported()
        .into_iter()
        .find(|interval| {
            bucket_count(since, until, interval.seconds())
                .is_some_and(|count| count <= AUTO_BUCKET_TARGET)
        })
        .unwrap_or(ComparisonInterval::OneDay)
        == actual
}

/// Requires a normalized segment echo to match the ordered request.
fn segment_scope_matches(actual: &SegmentScope, expected: &AnalyticsSegment) -> bool {
    actual.key == expected.key
        && actual.label == expected.label
        && actual.service_name == expected.service_name
        && actual.release == expected.release
        && actual.environment == expected.environment
        && property_filters_match(actual.property_filters.as_slice(), expected)
        && valid_segment_key(actual.key.as_str())
        && bounded_contract_text(actual.label.as_str(), 80)
        && [
            actual.service_name.as_deref(),
            actual.release.as_deref(),
            actual.environment.as_deref(),
        ]
        .into_iter()
        .flatten()
        .all(|value| bounded_contract_text(value, 256))
}

/// Requires canonical property-filter echoes to match the locally validated request.
fn property_filters_match(actual: &[PropertyFilter], expected: &AnalyticsSegment) -> bool {
    actual.len() == expected.property_filters.len()
        && actual
            .iter()
            .zip(expected.property_filters.iter())
            .all(|(actual, expected)| {
                actual.key == expected.key
                    && actual.value == expected.value
                    && crate::analytics_property_contract::is_safe_key(actual.key.as_str())
                    && bounded_contract_text(actual.value.as_str(), 256)
            })
        && actual
            .windows(2)
            .all(|pair| pair[0].key.as_str() < pair[1].key.as_str())
}

/// Applies the public machine-safe segment-key contract to response echoes.
fn valid_segment_key(value: &str) -> bool {
    let first_is_alphanumeric = value
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    !value.is_empty()
        && value.len() <= 32
        && first_is_alphanumeric
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

/// Applies the public target-name contract to response echoes.
fn valid_event_name(kind: AnalyticsSegmentEventKind, value: &str) -> bool {
    bounded_contract_text(value, 256)
        && (kind != AnalyticsSegmentEventKind::Interaction
            || (value.len() <= 64
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
                })))
}

/// Proves the fixed interpretation and bounded material limitations.
fn valid_confidence(confidence: &ComparisonConfidence) -> bool {
    confidence.interpretation == "descriptive_only"
        && confidence.unique_count_accuracy == UniqueCountAccuracy::Approximate
        && confidence.estimation_method == "clickhouse_uniq_combined64"
        && confidence.causal_inference == CausalInference::NotEstablished
        && confidence.statistical_significance == StatisticalSignificance::NotTested
        && confidence.segment_overlap == SegmentOverlap::Possible
        && (5..=LIMITATION_LIMIT).contains(&confidence.limitations.len())
        && confidence
            .limitations
            .iter()
            .all(|value| bounded_contract_text(value, 1024))
}

/// Proves summary totals, ordered segment results, series, and baseline differences.
fn valid_summary_and_segments(
    options: &AnalyticsSegmentComparisonOptions,
    response: &ComparisonResponse,
) -> bool {
    let summary = &response.summary;
    let count = options.segments.len();
    if response.segments.len() != count
        || usize::from(summary.segment_count) != count
        || summary.baseline_segment_key != options.segments[0].key
    {
        return false;
    }
    let Some(baseline) = response.segments.first() else {
        return false;
    };
    if !response
        .segments
        .iter()
        .zip(options.segments.iter())
        .enumerate()
        .all(|(index, (result, expected))| {
            valid_segment_result(response, result, expected, (index != 0).then_some(baseline))
        })
    {
        return false;
    }
    let eligible = response
        .segments
        .iter()
        .filter(|segment| segment.eligible_units > 0)
        .count();
    let targeted = response
        .segments
        .iter()
        .filter(|segment| segment.coverage.target_events > 0)
        .count();
    let reached = response
        .segments
        .iter()
        .filter(|segment| segment.reached_units > 0)
        .count();
    usize::from(summary.segments_with_eligible_units) == eligible
        && usize::from(summary.segments_with_target_events) == targeted
        && usize::from(summary.segments_with_reached_units) == reached
}

/// Proves one complete segment result against its exact request and baseline.
fn valid_segment_result(
    response: &ComparisonResponse,
    result: &SegmentResult,
    expected: &AnalyticsSegment,
    baseline: Option<&SegmentResult>,
) -> bool {
    result.key == expected.key
        && result.label == expected.label
        && valid_segment_coverage(result, expected)
        && ratio_matches(
            result.reach_rate,
            result.reached_units,
            result.eligible_units,
        )
        && average_matches(
            result.usable_target_events_per_reached_unit,
            result.coverage.usable_target_events,
            result.reached_units,
        )
        && valid_series(response, result)
        && baseline.map_or_else(
            || result.comparison_to_baseline.is_none(),
            |baseline| {
                result
                    .comparison_to_baseline
                    .as_ref()
                    .is_some_and(|comparison| {
                        valid_baseline_comparison(result, baseline, comparison)
                    })
            },
        )
}

/// Proves every derived capture count and ratio for one segment.
fn valid_segment_coverage(result: &SegmentResult, expected: &AnalyticsSegment) -> bool {
    let coverage = &result.coverage;
    let eligible_units_fit_identified_events =
        result.eligible_units <= coverage.unit_identified_events;
    let reached_units_fit_usable_target_events =
        result.reached_units <= coverage.usable_target_events;
    bounded_counts(&[
        result.eligible_units,
        result.reached_units,
        coverage.classified_events,
        coverage.unit_identified_events,
        coverage.target_events,
        coverage.usable_target_events,
        coverage.traced_target_events,
    ]) && coverage.unit_identified_events <= coverage.classified_events
        && eligible_units_fit_identified_events
        && coverage.target_events <= coverage.classified_events
        && coverage.usable_target_events <= coverage.target_events
        && coverage.usable_target_events <= coverage.unit_identified_events
        && result.reached_units <= result.eligible_units
        && reached_units_fit_usable_target_events
        && coverage.traced_target_events <= coverage.target_events
        && coverage.excluded_events == coverage.classified_events - coverage.unit_identified_events
        && coverage.excluded_target_events == coverage.target_events - coverage.usable_target_events
        && ratio_matches(
            coverage.unit_coverage_rate,
            coverage.unit_identified_events,
            coverage.classified_events,
        )
        && ratio_matches(
            coverage.target_unit_coverage_rate,
            coverage.usable_target_events,
            coverage.target_events,
        )
        && ratio_matches(
            coverage.target_trace_link_rate,
            coverage.traced_target_events,
            coverage.target_events,
        )
        && valid_property_coverage(
            coverage.property_filters.as_ref(),
            expected.property_filters.is_empty(),
            coverage.classified_events,
        )
}

/// Proves missing-key and nonmatching-value populations remain distinct and exhaustive.
fn valid_property_coverage(
    coverage: Option<&PropertyCoverage>,
    filters_empty: bool,
    matching_classified_events: u64,
) -> bool {
    let Some(coverage) = coverage else {
        return filters_empty;
    };
    if filters_empty {
        return false;
    }
    bounded_counts(&[
        coverage.context_events,
        coverage.property_ready_events,
        coverage.missing_property_events,
        coverage.matching_events,
        coverage.nonmatching_value_events,
    ]) && coverage.property_ready_events <= coverage.context_events
        && coverage.matching_events <= coverage.property_ready_events
        && coverage.matching_events == matching_classified_events
        && coverage.missing_property_events
            == coverage.context_events - coverage.property_ready_events
        && coverage.nonmatching_value_events
            == coverage.property_ready_events - coverage.matching_events
        && ratio_matches(
            coverage.property_ready_rate,
            coverage.property_ready_events,
            coverage.context_events,
        )
        && ratio_matches(
            coverage.match_rate,
            coverage.matching_events,
            coverage.property_ready_events,
        )
}

/// Proves ordered aligned buckets and exact event totals for one segment.
fn valid_series(response: &ComparisonResponse, result: &SegmentResult) -> bool {
    let Some((since, until)) = query_timestamps(&response.query) else {
        return false;
    };
    let Some(expected_buckets) = bucket_count(since, until, response.query.interval_seconds) else {
        return false;
    };
    if result.series.len() > usize::try_from(expected_buckets).unwrap_or(usize::MAX) {
        return false;
    }
    let mut previous_start = None;
    let mut classified_total = 0_u64;
    let mut target_total = 0_u64;
    let mut usable_target_total = 0_u64;
    for point in &result.series {
        if !valid_point(
            point,
            since,
            until,
            response.query.interval_seconds,
            previous_start,
        ) {
            return false;
        }
        let Some(start) = parse_utc_timestamp(point.bucket_start.as_str()) else {
            return false;
        };
        previous_start = Some(start);
        let Some(next_classified) = classified_total.checked_add(point.classified_events) else {
            return false;
        };
        let Some(next_target) = target_total.checked_add(point.target_events) else {
            return false;
        };
        let Some(next_usable) = usable_target_total.checked_add(point.usable_target_events) else {
            return false;
        };
        classified_total = next_classified;
        target_total = next_target;
        usable_target_total = next_usable;
    }
    classified_total == result.coverage.classified_events
        && target_total == result.coverage.target_events
        && usable_target_total == result.coverage.usable_target_events
}

/// Proves one non-empty UTC-aligned bucket and all of its count invariants.
fn valid_point(
    point: &SegmentPoint,
    since: UtcTimestamp,
    until: UtcTimestamp,
    interval_seconds: u64,
    previous_start: Option<UtcTimestamp>,
) -> bool {
    let Some(start) = parse_utc_timestamp(point.bucket_start.as_str()) else {
        return false;
    };
    let Some(end) = parse_utc_timestamp(point.bucket_end.as_str()) else {
        return false;
    };
    let Some(expected_end) = add_seconds(start, interval_seconds) else {
        return false;
    };
    let interval_nanos = i128::from(interval_seconds) * NANOS_PER_SECOND;
    point.classified_events > 0
        && end == expected_end
        && start < until
        && end > since
        && previous_start.is_none_or(|previous| start > previous)
        && timestamp_nanos(start).rem_euclid(interval_nanos) == 0
        && bounded_counts(&[
            point.classified_events,
            point.eligible_units,
            point.target_events,
            point.usable_target_events,
            point.reached_units,
        ])
        && point.eligible_units <= point.classified_events
        && point.target_events <= point.classified_events
        && point.usable_target_events <= point.target_events
        && point.reached_units <= point.eligible_units
        && point.reached_units <= point.usable_target_events
        && ratio_matches(point.reach_rate, point.reached_units, point.eligible_units)
}

/// Proves all signed and rate differences from the validated baseline.
fn valid_baseline_comparison(
    result: &SegmentResult,
    baseline: &SegmentResult,
    comparison: &BaselineComparison,
) -> bool {
    let eligible_difference = signed_difference(result.eligible_units, baseline.eligible_units);
    let reached_difference = signed_difference(result.reached_units, baseline.reached_units);
    let target_difference = signed_difference(
        result.coverage.target_events,
        baseline.coverage.target_events,
    );
    let reach_difference = result
        .reach_rate
        .zip(baseline.reach_rate)
        .map(|(current, baseline)| current - baseline);
    let relative_lift = result
        .reach_rate
        .zip(baseline.reach_rate)
        .and_then(|(current, baseline)| (baseline > 0.0).then_some(current / baseline - 1.0));
    eligible_difference == Some(comparison.eligible_units_difference)
        && reached_difference == Some(comparison.reached_units_difference)
        && target_difference == Some(comparison.target_events_difference)
        && optional_float_matches(comparison.reach_rate_difference, reach_difference)
        && optional_float_matches(comparison.relative_reach_rate_lift, relative_lift)
}

/// Computes one bounded signed difference.
fn signed_difference(current: u64, baseline: u64) -> Option<i64> {
    i64::try_from(current)
        .ok()?
        .checked_sub(i64::try_from(baseline).ok()?)
}

/// Requires the stable action code and target implied by validated response state.
fn valid_next_action(response: &ComparisonResponse) -> bool {
    if !bounded_contract_text(response.next_action.reason.as_str(), 768) {
        return false;
    }
    let expected = expected_next_action(response);
    response.next_action.code == expected.0 && response.next_action.target == expected.1
}

/// Derives the backend's stable next action from validated result state.
fn expected_next_action(response: &ComparisonResponse) -> (&'static str, &'static str) {
    if response
        .segments
        .iter()
        .all(|segment| segment_context_events(segment) == 0)
    {
        return ("capture_product_activity", "analyticsSchemaVersion=1");
    }
    for (scope, result) in response.query.segments.iter().zip(response.segments.iter()) {
        if scope.property_filters.is_empty() {
            continue;
        }
        let Some(coverage) = result.coverage.property_filters.as_ref() else {
            return ("invalid_property_coverage", "invalid_property_coverage");
        };
        if coverage.context_events == 0 {
            continue;
        }
        if coverage.property_ready_events == 0 {
            return (
                "capture_segment_properties",
                "context.resource or context.tags",
            );
        }
        if coverage.property_ready_events.saturating_mul(100)
            < coverage.context_events.saturating_mul(80)
        {
            return (
                "improve_property_coverage",
                "/api/telemetry/analytics/properties",
            );
        }
        if coverage.matching_events == 0 {
            return (
                "verify_property_values",
                "/api/telemetry/analytics/segments/compare",
            );
        }
    }
    if response
        .segments
        .iter()
        .all(|segment| segment.coverage.target_events == 0)
    {
        return (
            "choose_captured_target",
            "/api/telemetry/analytics/overview",
        );
    }
    if response
        .segments
        .iter()
        .all(|segment| segment.eligible_units == 0)
    {
        return match response.query.analysis_unit {
            AnalyticsSegmentUnit::Session => ("sessionize_product_activity", "context.session.id"),
            AnalyticsSegmentUnit::IdentifiedUser => {
                ("identify_product_users", "context.subject.kind=user")
            }
        };
    }
    if response
        .segments
        .iter()
        .filter(|segment| segment.eligible_units > 0)
        .count()
        < 2
    {
        return (
            "adjust_segment_filters",
            "/api/telemetry/analytics/segments/compare",
        );
    }
    if response.segments.iter().any(|segment| {
        segment.coverage.classified_events > 0
            && segment.coverage.unit_identified_events.saturating_mul(100)
                < segment.coverage.classified_events.saturating_mul(80)
    }) {
        return match response.query.analysis_unit {
            AnalyticsSegmentUnit::Session => ("improve_session_coverage", "context.session.id"),
            AnalyticsSegmentUnit::IdentifiedUser => {
                ("improve_identity_coverage", "context.subject.kind=user")
            }
        };
    }
    (
        "investigate_segment_paths",
        "/api/telemetry/analytics/paths",
    )
}

/// Returns pre-property context volume when filters are present.
fn segment_context_events(segment: &SegmentResult) -> u64 {
    segment
        .coverage
        .property_filters
        .as_ref()
        .map_or(segment.coverage.classified_events, |coverage| {
            coverage.context_events
        })
}

/// Returns whether every count stays inside the server's public scan bound.
fn bounded_counts(values: &[u64]) -> bool {
    values.iter().all(|value| *value <= COUNT_LIMIT)
}

/// Verifies one optional exact ratio bounded between zero and one.
fn ratio_matches(value: Option<f64>, numerator: u64, denominator: u64) -> bool {
    if denominator == 0 {
        return value.is_none();
    }
    if numerator > denominator {
        return false;
    }
    let (Ok(numerator), Ok(denominator)) = (u32::try_from(numerator), u32::try_from(denominator))
    else {
        return false;
    };
    let expected = f64::from(numerator) / f64::from(denominator);
    value.is_some_and(|value| {
        value.is_finite() && (0.0..=1.0).contains(&value) && floats_match(value, expected)
    })
}

/// Verifies optional usable-event volume per reached unit.
fn average_matches(value: Option<f64>, numerator: u64, denominator: u64) -> bool {
    if denominator == 0 {
        return value.is_none();
    }
    let (Ok(numerator), Ok(denominator)) = (u32::try_from(numerator), u32::try_from(denominator))
    else {
        return false;
    };
    let expected = f64::from(numerator) / f64::from(denominator);
    value.is_some_and(|value| value.is_finite() && value >= 1.0 && floats_match(value, expected))
}

/// Compares two optional derived floating-point values.
fn optional_float_matches(actual: Option<f64>, expected: Option<f64>) -> bool {
    match (actual, expected) {
        (None, None) => true,
        (Some(actual), Some(expected)) => actual.is_finite() && floats_match(actual, expected),
        _ => false,
    }
}

/// Applies one scale-aware tolerance to deterministic derived values.
fn floats_match(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= 1.0e-12 * expected.abs().max(1.0)
}

/// Validates one backend-authored, non-telemetry contract string.
fn bounded_contract_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= limit
        && !value.chars().any(char::is_control)
}

/// Returns the exact number of UTC-aligned buckets intersecting a half-open range.
fn bucket_count(since: UtcTimestamp, until: UtcTimestamp, interval_seconds: u64) -> Option<u64> {
    let width = i128::from(interval_seconds).checked_mul(NANOS_PER_SECOND)?;
    let first = timestamp_nanos(since).div_euclid(width);
    let last = timestamp_nanos(until).checked_sub(1)?.div_euclid(width);
    u64::try_from(last.checked_sub(first)?.checked_add(1)?).ok()
}

/// Parses the canonical UTC `Z` timestamp emitted by the API.
fn parse_utc_timestamp(value: &str) -> Option<UtcTimestamp> {
    let bytes = value.as_bytes();
    if !(20..=30).contains(&bytes.len())
        || !bytes.is_ascii()
        || bytes.last() != Some(&b'Z')
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let year = decimal(bytes.get(0..4)?)?;
    let month = decimal(bytes.get(5..7)?)?;
    let day = decimal(bytes.get(8..10)?)?;
    let hour = decimal(bytes.get(11..13)?)?;
    let minute = decimal(bytes.get(14..16)?)?;
    let second = decimal(bytes.get(17..19)?)?;
    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let nanoseconds = parse_fraction(bytes.get(19..bytes.len().saturating_sub(1))?)?;
    let days = days_from_civil(i64::from(year), i64::from(month), i64::from(day));
    let epoch_seconds = days
        .checked_mul(86_400)?
        .checked_add(i64::from(hour).checked_mul(3_600)?)?
        .checked_add(i64::from(minute).checked_mul(60)?)?
        .checked_add(i64::from(second))?;
    Some(UtcTimestamp {
        epoch_seconds,
        nanoseconds,
    })
}

/// Parses one fixed-width ASCII decimal field.
fn decimal(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value.checked_mul(10)?.checked_add(u32::from(*byte - b'0')))
            .flatten()
    })
}

/// Parses an absent or one-to-nine-digit timestamp fraction.
fn parse_fraction(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return Some(0);
    }
    let digits = bytes.strip_prefix(b".")?;
    if digits.is_empty() || digits.len() > 9 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let value = decimal(digits)?;
    value.checked_mul(10_u32.checked_pow(u32::try_from(9_usize.checked_sub(digits.len())?).ok()?)?)
}

/// Returns the valid day count for one Gregorian month.
const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Returns whether one Gregorian year is a leap year.
const fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// Converts one civil date to days relative to the Unix epoch.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Adds whole positive seconds without changing the fractional component.
fn add_seconds(timestamp: UtcTimestamp, seconds: u64) -> Option<UtcTimestamp> {
    Some(UtcTimestamp {
        epoch_seconds: timestamp
            .epoch_seconds
            .checked_add(i64::try_from(seconds).ok()?)?,
        nanoseconds: timestamp.nanoseconds,
    })
}

/// Returns one timestamp as a checked nanosecond offset.
fn timestamp_nanos(timestamp: UtcTimestamp) -> i128 {
    i128::from(timestamp.epoch_seconds) * NANOS_PER_SECOND + i128::from(timestamp.nanoseconds)
}

/// Renders a progressive human comparison without reflecting backend prose.
fn render_response(response: &ComparisonResponse) -> String {
    let mut output = String::new();
    output.push_str("Product segment comparison ");
    output.push_str(response.query.target.kind.as_str());
    output.push(' ');
    output.push_str(display_text(response.query.target.event_name.as_str()).as_str());
    output.push('\n');
    output.push_str(
        format!(
            "Window: {} to {}; interval: {} ({}s); unit: {}; baseline: {}\n",
            response.query.since,
            response.query.until,
            response.query.interval.as_str(),
            response.query.interval_seconds,
            unit_label(response.query.analysis_unit),
            response.summary.baseline_segment_key,
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Coverage summary: {}/{} segments eligible; {} contain the target; {} reached it.\n",
            response.summary.segments_with_eligible_units,
            response.summary.segment_count,
            response.summary.segments_with_target_events,
            response.summary.segments_with_reached_units,
        )
        .as_str(),
    );
    render_segments(response, &mut output);
    output.push_str(
        "Interpretation: descriptive only; unique-unit counts are approximate; exact event and coverage totals are fully evaluated; segments may overlap; no causality or statistical significance is established.\n",
    );
    output.push_str(
        "Series note: bucket-level unique counts are recalculated per bucket and must not be added across time.\n",
    );
    output.push_str("Next: ");
    output.push_str(next_step(response.next_action.code.as_str()));
    output.push('\n');
    output
}

/// Adds every segment headline, context, difference, coverage, and bounded series evidence.
fn render_segments(response: &ComparisonResponse, output: &mut String) {
    output.push_str("Segments:\n");
    for (index, (scope, result)) in response
        .query
        .segments
        .iter()
        .zip(response.segments.iter())
        .enumerate()
    {
        let baseline = if index == 0 { " [baseline]" } else { "" };
        output.push_str(
            format!(
                "  {} ({}){}\n",
                result.key,
                display_text(result.label.as_str()),
                baseline,
            )
            .as_str(),
        );
        output.push_str(
            format!(
                "    Context: service={}; release={}; environment={}\n",
                filter_label(scope.service_name.as_deref()),
                filter_label(scope.release.as_deref()),
                filter_label(scope.environment.as_deref()),
            )
            .as_str(),
        );
        render_property_context(scope, result, output);
        output.push_str(
            format!(
                "    Reach: {}/{} ({}) | target events: {} ({} usable) | usable events/reached unit: {}\n",
                result.reached_units,
                result.eligible_units,
                percentage(result.reach_rate),
                result.coverage.target_events,
                result.coverage.usable_target_events,
                decimal_ratio(result.usable_target_events_per_reached_unit),
            )
            .as_str(),
        );
        output.push_str(
            format!(
                "    Coverage: unit {}/{} ({}) | target unit {}/{} ({}) | trace-linked target {}/{} ({})\n",
                result.coverage.unit_identified_events,
                result.coverage.classified_events,
                percentage(result.coverage.unit_coverage_rate),
                result.coverage.usable_target_events,
                result.coverage.target_events,
                percentage(result.coverage.target_unit_coverage_rate),
                result.coverage.traced_target_events,
                result.coverage.target_events,
                percentage(result.coverage.target_trace_link_rate),
            )
            .as_str(),
        );
        render_baseline_difference(result, output);
        render_capture_gaps(response.query.analysis_unit, result, output);
        render_series(result, output);
    }
}

/// Adds key-only predicates and separates missing-key coverage from value mismatch.
fn render_property_context(scope: &SegmentScope, result: &SegmentResult, output: &mut String) {
    if scope.property_filters.is_empty() {
        return;
    }
    let keys = scope
        .property_filters
        .iter()
        .map(|filter| display_text(filter.key.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    output.push_str(
        format!(
            "    Property predicates: {} exact case-sensitive {} across {}; values hidden in human output.\n",
            scope.property_filters.len(),
            if scope.property_filters.len() == 1 {
                "value"
            } else {
                "values"
            },
            keys,
        )
        .as_str(),
    );
    let Some(coverage) = result.coverage.property_filters.as_ref() else {
        return;
    };
    output.push_str(
        format!(
            "    Property coverage: ready {}/{} ({}) | matched {}/{} ({}) | missing keys {} | nonmatching values {}\n",
            coverage.property_ready_events,
            coverage.context_events,
            percentage(coverage.property_ready_rate),
            coverage.matching_events,
            coverage.property_ready_events,
            percentage(coverage.match_rate),
            coverage.missing_property_events,
            coverage.nonmatching_value_events,
        )
        .as_str(),
    );
}

/// Adds one descriptive difference from the first segment.
fn render_baseline_difference(result: &SegmentResult, output: &mut String) {
    let Some(comparison) = result.comparison_to_baseline.as_ref() else {
        return;
    };
    output.push_str(
        format!(
            "    Versus baseline: eligible {:+}; reached {:+}; target events {:+}; reach {}; relative lift {}\n",
            comparison.eligible_units_difference,
            comparison.reached_units_difference,
            comparison.target_events_difference,
            percentage_points(comparison.reach_rate_difference),
            signed_percentage(comparison.relative_reach_rate_lift),
        )
        .as_str(),
    );
}

/// Adds selected-unit, target-unit, and trace-link gaps only when present.
fn render_capture_gaps(unit: AnalyticsSegmentUnit, result: &SegmentResult, output: &mut String) {
    if result.coverage.excluded_events > 0 {
        output.push_str(
            format!(
                "    Capture gap: {} classified events lacked {}.\n",
                result.coverage.excluded_events,
                unit_context(unit),
            )
            .as_str(),
        );
    }
    if result.coverage.excluded_target_events > 0 {
        output.push_str(
            format!(
                "    Target gap: {} target events lacked {} and were excluded from reached-unit analysis.\n",
                result.coverage.excluded_target_events,
                unit_context(unit),
            )
            .as_str(),
        );
    }
    let untraced = result
        .coverage
        .target_events
        .saturating_sub(result.coverage.traced_target_events);
    if untraced > 0 {
        output.push_str(
            format!("    Correlation gap: {untraced} target events lacked a trace ID.\n").as_str(),
        );
    }
}

/// Adds all small series or representative endpoints for a large bounded series.
fn render_series(result: &SegmentResult, output: &mut String) {
    if result.series.is_empty() {
        output.push_str("    Series: no classified activity in this segment.\n");
        return;
    }
    output.push_str(format!("    Series: {} non-empty buckets\n", result.series.len()).as_str());
    if result.series.len() <= HUMAN_POINT_LIMIT {
        for point in &result.series {
            render_point(point, output);
        }
        return;
    }
    let edge = HUMAN_POINT_LIMIT / 2;
    for point in &result.series[..edge] {
        render_point(point, output);
    }
    output.push_str(
        format!(
            "      ... {} middle buckets omitted in human mode; use --json for the full validated series ...\n",
            result.series.len() - HUMAN_POINT_LIMIT,
        )
        .as_str(),
    );
    for point in &result.series[result.series.len() - edge..] {
        render_point(point, output);
    }
}

/// Adds one compact comparison-series point.
fn render_point(point: &SegmentPoint, output: &mut String) {
    output.push_str(
        format!(
            "      {} to {}: eligible {} | reached {} ({}) | target events {} ({} usable) | classified {}\n",
            point.bucket_start,
            point.bucket_end,
            point.eligible_units,
            point.reached_units,
            percentage(point.reach_rate),
            point.target_events,
            point.usable_target_events,
            point.classified_events,
        )
        .as_str(),
    );
}

/// Returns a terminal-safe exact filter label or a wildcard marker.
fn filter_label(value: Option<&str>) -> String {
    value.map_or_else(|| "<any>".to_owned(), display_text)
}

/// Returns the human identity-boundary label.
const fn unit_label(unit: AnalyticsSegmentUnit) -> &'static str {
    match unit {
        AnalyticsSegmentUnit::Session => "session",
        AnalyticsSegmentUnit::IdentifiedUser => "typed opaque user",
    }
}

/// Returns the exact selected-unit context field.
const fn unit_context(unit: AnalyticsSegmentUnit) -> &'static str {
    match unit {
        AnalyticsSegmentUnit::Session => "context.session.id",
        AnalyticsSegmentUnit::IdentifiedUser => "context.subject.id + context.subject.kind=user",
    }
}

/// Formats one optional fraction as a percentage.
fn percentage(value: Option<f64>) -> String {
    value.map_or_else(
        || "n/a".to_owned(),
        |value| format!("{:.1}%", value * 100.0),
    )
}

/// Formats one optional signed fraction as a percentage-point difference.
fn percentage_points(value: Option<f64>) -> String {
    value.map_or_else(
        || "n/a".to_owned(),
        |value| format!("{:+.1} pp", value * 100.0),
    )
}

/// Formats one optional signed relative lift.
fn signed_percentage(value: Option<f64>) -> String {
    value.map_or_else(
        || "n/a".to_owned(),
        |value| format!("{:+.1}%", value * 100.0),
    )
}

/// Formats one optional non-rate ratio.
fn decimal_ratio(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |value| format!("{value:.2}"))
}

/// Maps validated stable action codes to local, value-free human guidance.
fn next_step(code: &str) -> &'static str {
    match code {
        "capture_product_activity" => {
            "capture version-1 page views, screen views, or interactions in these contexts"
        }
        "choose_captured_target" => "choose one exact event present in Product Analytics overview",
        "sessionize_product_activity" => {
            "attach one opaque context.session.id before comparing session reach"
        }
        "identify_product_users" => {
            "attach one stable opaque context.subject.id and set context.subject.kind=user before \
             comparing identified-user reach"
        }
        "adjust_segment_filters" => {
            "verify the exact segment contexts or widen the bounded time range"
        }
        "capture_segment_properties" => {
            "capture every requested safe property key in this segment before comparing reach"
        }
        "improve_property_coverage" => {
            "inspect analytics properties and improve requested-key coverage in the weaker segment"
        }
        "verify_property_values" => {
            "the keys are present but no event matches every value; verify exact case and spelling"
        }
        "improve_session_coverage" => {
            "improve context.session.id coverage in the weaker segment before interpreting reach"
        }
        "improve_identity_coverage" => {
            "improve context.subject.id plus context.subject.kind=user coverage in the weaker \
             segment before interpreting reach"
        }
        "investigate_segment_paths" => {
            "inspect paths around the target in the weakest segment, then follow correlated traces"
        }
        _ => "retry the bounded analytics segment comparison",
    }
}

/// Escapes terminal controls and bidirectional-display characters in echoed values.
fn display_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            output.extend(character.escape_default());
        } else if matches!(
            character,
            '\u{061c}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{feff}'
                | '\u{fff9}'..='\u{fffb}'
        ) {
            output.extend(character.escape_unicode());
        } else {
            output.push(character);
        }
    }
    output
}

/// Validates the configured API origin without retaining it in errors.
fn normalized_origin(base_url: &str) -> Result<String, RuntimeError> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_error| transport_error())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(transport_error());
    }
    url.set_path("");
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// Converts transport and refresh failures into fixed comparison-safe recovery.
fn request_error(error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::MissingToken | RuntimeError::Unavailable { .. } => error,
        RuntimeError::Cli(_)
        | RuntimeError::Io(_)
        | RuntimeError::Http(_)
        | RuntimeError::Api { .. }
        | RuntimeError::StatusUnavailable { .. }
        | RuntimeError::InvestigationResponseInvalid
        | RuntimeError::ExplainResponseInvalid
        | RuntimeError::AnalyticsOverviewResponseInvalid
        | RuntimeError::AnalyticsPropertiesResponseInvalid
        | RuntimeError::AnalyticsResponseInvalid
        | RuntimeError::AnalyticsFunnelResponseInvalid
        | RuntimeError::AnalyticsRetentionResponseInvalid
        | RuntimeError::AnalyticsLifecycleResponseInvalid
        | RuntimeError::AnalyticsSegmentResponseInvalid
        | RuntimeError::NativeDebugArtifactInvalid
        | RuntimeError::NativeDebugResponseInvalid
        | RuntimeError::NativeDebugVerificationFailed => transport_error(),
    }
}

/// Returns one fixed path-free transport failure.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "analytics segment comparison request could not be completed",
        next: "check network connectivity and retry the same analytics segment comparison",
    }
}

/// Returns one fixed response-contract failure.
const fn invalid_response() -> RuntimeError {
    RuntimeError::AnalyticsSegmentResponseInvalid
}

/// Converts a failed HTTP status into fixed guidance without reflecting its body.
fn safe_api_error(status: u16, credential: &AuthCredential) -> RuntimeError {
    let (error, code, next) = match status {
        400 | 422 => (
            "analytics segment comparison rejected",
            "validation_failed",
            "check the exact project, time scope, target, segments, property filters, interval, and analysis unit",
        ),
        401 => (
            "authentication required",
            "unauthorized",
            "run logbrew login",
        ),
        403 => (
            "analytics segment comparison forbidden",
            "forbidden",
            "confirm account access and retry the same analytics segment comparison",
        ),
        404 => (
            "analytics segment comparison resource not found",
            "not_found",
            "check the project and retry the same analytics segment comparison",
        ),
        405 => (
            "analytics segment comparison method is not supported",
            "method_not_allowed",
            "use the POST-backed logbrew analytics compare command",
        ),
        429 => (
            "analytics segment comparison rate limited",
            "rate_limited",
            "retry the same analytics segment comparison later",
        ),
        500..=599 => (
            "analytics segment comparison service unavailable",
            "service_unavailable",
            "retry the same analytics segment comparison later",
        ),
        _ => (
            "analytics segment comparison failed",
            "request_failed",
            "check account access and retry the same analytics segment comparison",
        ),
    };
    RuntimeError::Api {
        status,
        body: serde_json::json!({"error": error, "code": code, "next": next}).to_string(),
        auth_source: credential.source(),
        auth_label: credential.label(),
    }
}
