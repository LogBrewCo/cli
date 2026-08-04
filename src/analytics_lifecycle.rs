//! Versioned, bounded product-analytics lifecycle reporting.

use serde::Deserialize;

use crate::auth::{AuthCredential, send_authenticated_with_refresh};
use crate::{
    AnalyticsLifecycleEventKind, AnalyticsLifecycleInterval, AnalyticsLifecycleOptions,
    CliEnvironment, RuntimeError,
};

/// Public response version implemented by this CLI.
const SCHEMA_VERSION: u8 = 1;
/// Maximum accepted response body.
const RESPONSE_LIMIT: usize = 1024 * 1024;
/// Server-side scan cap also bounds every returned count.
const COUNT_LIMIT: u64 = 10_000_000;
/// Maximum lifecycle buckets returned by the public API.
const BUCKET_LIMIT: usize = 100;
/// Maximum material limitations accepted from the bounded API.
const LIMITATION_LIMIT: usize = 16;
/// Nanoseconds in one second.
const NANOS_PER_SECOND: i128 = 1_000_000_000;

/// Builds the exact public POST body with explicit CLI history defaults.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command model consumes this private-module helper"
)]
pub(super) fn request_body(options: &AnalyticsLifecycleOptions) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    drop(body.insert(
        "project_id".to_owned(),
        serde_json::Value::String(options.project_id.clone()),
    ));
    drop(body.insert(
        "since".to_owned(),
        serde_json::Value::String(options.since.clone()),
    ));
    insert_optional(&mut body, "until", options.until.as_deref());
    insert_optional(&mut body, "service_name", options.service_name.as_deref());
    insert_optional(&mut body, "release", options.release.as_deref());
    insert_optional(&mut body, "environment", options.environment.as_deref());
    drop(body.insert(
        "event".to_owned(),
        serde_json::json!({
            "kind": options.event_kind.as_str(),
            "event_name": options.event_name,
        }),
    ));
    if let Some(interval) = options.interval {
        drop(body.insert(
            "interval".to_owned(),
            serde_json::Value::String(interval.as_str().to_owned()),
        ));
    }
    drop(body.insert(
        "history_period_count".to_owned(),
        options.history_period_count.into(),
    ));
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

/// Executes one aggregate, identity-safe lifecycle request.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    options: &AnalyticsLifecycleOptions,
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
    let url = format!("{origin}/api/telemetry/analytics/lifecycle");
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
struct LifecycleResponse {
    schema_version: u8,
    query: LifecycleQuery,
    purpose: String,
    summary: LifecycleSummary,
    coverage: LifecycleCoverage,
    buckets: Vec<LifecycleBucket>,
    next_action: NextAction,
}

/// Normalized effective query echoed by the backend.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleQuery {
    project_id: String,
    since: String,
    until: String,
    history_since: String,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    event: LifecycleEvent,
    interval: AnalyticsLifecycleInterval,
    interval_seconds: u64,
    history_period_count: u8,
    expected_buckets: u16,
}

/// One exact classified event selector.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleEvent {
    kind: AnalyticsLifecycleEventKind,
    event_name: String,
}

/// Aggregate identified-subject lifecycle outcome.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleSummary {
    observed_subjects: u64,
    analysis_active_subjects: u64,
    history_only_subjects: u64,
    returned_buckets: u16,
    fully_observed_buckets: u16,
    latest_fully_observed_period: Option<u16>,
    buckets_with_resurrection: u16,
    buckets_with_dormancy: u16,
}

/// Capture coverage qualifying the lifecycle result.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleCoverage {
    analysis_classified_events: u64,
    analysis_named_events: u64,
    analysis_unnamed_events: u64,
    analysis_identified_events: u64,
    selected_events: u64,
    usable_selected_events: u64,
    usable_selected_sessionized_events: u64,
    usable_selected_trace_linked_events: u64,
    excluded_selected_events: u64,
    history_selected_events: u64,
    usable_history_selected_events: u64,
    excluded_history_selected_events: u64,
    event_name_rate: Option<f64>,
    selected_identity_coverage_rate: Option<f64>,
    selected_sessionization_rate: Option<f64>,
    selected_trace_link_rate: Option<f64>,
    history_identity_coverage_rate: Option<f64>,
    limitations: Vec<String>,
}

/// One fixed lifecycle bucket.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleBucket {
    period: u16,
    started_at: String,
    ended_at: String,
    fully_observed: bool,
    active_subjects: u64,
    new_within_observed_history_subjects: u64,
    returning_subjects: u64,
    resurrected_subjects: u64,
    dormant_subjects: u64,
    previous_active_subjects: u64,
    new_share_of_active: Option<f64>,
    returning_share_of_active: Option<f64>,
    resurrected_share_of_active: Option<f64>,
    dormant_share_of_previous_active: Option<f64>,
    net_active_change: i64,
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
    options: &AnalyticsLifecycleOptions,
    body: &str,
) -> Result<LifecycleResponse, RuntimeError> {
    let response =
        serde_json::from_str::<LifecycleResponse>(body).map_err(|_error| invalid_response())?;
    if response.schema_version != SCHEMA_VERSION
        || !bounded_contract_text(response.purpose.as_str(), 4096)
        || !valid_query(options, &response.query)
        || !valid_coverage(&response.coverage)
        || !valid_summary_and_buckets(&response)
        || !valid_next_action(&response)
    {
        return Err(invalid_response());
    }
    Ok(response)
}

/// Requires the backend echo to match every exact client-selected scope field.
fn valid_query(options: &AnalyticsLifecycleOptions, query: &LifecycleQuery) -> bool {
    let Some((history_since, since, until)) = query_timestamps(query) else {
        return false;
    };
    let interval_seconds = query.interval.seconds();
    let Some(history_seconds) = interval_seconds.checked_mul(u64::from(query.history_period_count))
    else {
        return false;
    };
    let Some(expected_history_since) = subtract_seconds(since, history_seconds) else {
        return false;
    };
    let Some(expected_buckets) = expected_bucket_count(since, until, interval_seconds) else {
        return false;
    };
    query.project_id == options.project_id
        && query.service_name == options.service_name
        && query.release == options.release
        && query.environment == options.environment
        && query.event.kind == options.event_kind
        && query.event.event_name == options.event_name
        && valid_event_name(query.event.kind, query.event.event_name.as_str())
        && options
            .interval
            .is_none_or(|interval| interval == query.interval)
        && query.interval_seconds == interval_seconds
        && query.history_period_count == options.history_period_count
        && (2..=31).contains(&query.history_period_count)
        && history_seconds <= 62 * 24 * 60 * 60
        && history_since == expected_history_since
        && expected_buckets == u64::from(query.expected_buckets)
        && (1..=u64::try_from(BUCKET_LIMIT).unwrap_or(u64::MAX)).contains(&expected_buckets)
}

/// Parses all query timestamps and enforces a positive bounded analysis range.
fn query_timestamps(query: &LifecycleQuery) -> Option<(UtcTimestamp, UtcTimestamp, UtcTimestamp)> {
    let history_since = parse_utc_timestamp(query.history_since.as_str())?;
    let since = parse_utc_timestamp(query.since.as_str())?;
    let until = parse_utc_timestamp(query.until.as_str())?;
    let range = timestamp_nanos(until).checked_sub(timestamp_nanos(since))?;
    (history_since < since
        && since < until
        && range <= i128::from(31 * 24 * 60 * 60) * NANOS_PER_SECOND)
        .then_some((history_since, since, until))
}

/// Returns the ceiling number of fixed buckets for one positive range.
fn expected_bucket_count(
    since: UtcTimestamp,
    until: UtcTimestamp,
    interval_seconds: u64,
) -> Option<u64> {
    let range = timestamp_nanos(until).checked_sub(timestamp_nanos(since))?;
    let interval = i128::from(interval_seconds).checked_mul(NANOS_PER_SECOND)?;
    let buckets = range
        .checked_add(interval.checked_sub(1)?)?
        .checked_div(interval)?;
    u64::try_from(buckets).ok()
}

/// Applies the server's exact event-name contract to response echoes.
fn valid_event_name(kind: AnalyticsLifecycleEventKind, value: &str) -> bool {
    let common = bounded_contract_text(value, 256);
    common
        && (kind != AnalyticsLifecycleEventKind::Interaction
            || (value.len() <= 64
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
                })))
}

/// Proves every derived coverage count, ratio, and limitation bound.
fn valid_coverage(coverage: &LifecycleCoverage) -> bool {
    if !bounded_counts(&[
        coverage.analysis_classified_events,
        coverage.analysis_named_events,
        coverage.analysis_unnamed_events,
        coverage.analysis_identified_events,
        coverage.selected_events,
        coverage.usable_selected_events,
        coverage.usable_selected_sessionized_events,
        coverage.usable_selected_trace_linked_events,
        coverage.excluded_selected_events,
        coverage.history_selected_events,
        coverage.usable_history_selected_events,
        coverage.excluded_history_selected_events,
    ]) || coverage.analysis_named_events > coverage.analysis_classified_events
        || coverage.analysis_identified_events > coverage.analysis_classified_events
        || coverage.selected_events > coverage.analysis_named_events
        || coverage.usable_selected_events > coverage.selected_events
        || coverage.usable_selected_events > coverage.analysis_identified_events
        || coverage.usable_selected_sessionized_events > coverage.usable_selected_events
        || coverage.usable_selected_trace_linked_events > coverage.usable_selected_events
        || coverage.usable_history_selected_events > coverage.history_selected_events
        || coverage.analysis_unnamed_events
            != coverage.analysis_classified_events - coverage.analysis_named_events
        || coverage.excluded_selected_events
            != coverage.selected_events - coverage.usable_selected_events
        || coverage.excluded_history_selected_events
            != coverage.history_selected_events - coverage.usable_history_selected_events
        || coverage.limitations.is_empty()
        || coverage.limitations.len() > LIMITATION_LIMIT
        || !coverage
            .limitations
            .iter()
            .all(|value| bounded_contract_text(value, 768))
    {
        return false;
    }
    ratio_matches(
        coverage.event_name_rate,
        coverage.analysis_named_events,
        coverage.analysis_classified_events,
    ) && ratio_matches(
        coverage.selected_identity_coverage_rate,
        coverage.usable_selected_events,
        coverage.selected_events,
    ) && ratio_matches(
        coverage.selected_sessionization_rate,
        coverage.usable_selected_sessionized_events,
        coverage.usable_selected_events,
    ) && ratio_matches(
        coverage.selected_trace_link_rate,
        coverage.usable_selected_trace_linked_events,
        coverage.usable_selected_events,
    ) && ratio_matches(
        coverage.history_identity_coverage_rate,
        coverage.usable_history_selected_events,
        coverage.history_selected_events,
    )
}

/// Proves summary totals and every ordered fixed lifecycle bucket.
fn valid_summary_and_buckets(response: &LifecycleResponse) -> bool {
    let summary = &response.summary;
    if !bounded_counts(&[
        summary.observed_subjects,
        summary.analysis_active_subjects,
        summary.history_only_subjects,
    ]) || summary
        .analysis_active_subjects
        .checked_add(summary.history_only_subjects)
        != Some(summary.observed_subjects)
        || usize::from(summary.returned_buckets) != response.buckets.len()
        || response.buckets.len() > BUCKET_LIMIT
        || summary.analysis_active_subjects > response.coverage.usable_selected_events
        || summary.observed_subjects
            > usable_observed_events(&response.coverage).unwrap_or(u64::MAX)
    {
        return false;
    }
    let usable_observed = usable_observed_events(&response.coverage);
    if usable_observed.is_none_or(|count| {
        (count == 0 && !response.buckets.is_empty())
            || (count > 0 && response.buckets.len() != usize::from(response.query.expected_buckets))
    }) {
        return false;
    }
    if !response
        .buckets
        .iter()
        .enumerate()
        .all(|(index, bucket)| valid_bucket(response, index, bucket))
    {
        return false;
    }
    let fully_observed = response
        .buckets
        .iter()
        .filter(|bucket| bucket.fully_observed)
        .count();
    let latest_fully_observed = response
        .buckets
        .iter()
        .rev()
        .find(|bucket| bucket.fully_observed)
        .map(|bucket| bucket.period);
    let with_resurrection = response
        .buckets
        .iter()
        .filter(|bucket| bucket.resurrected_subjects > 0)
        .count();
    let with_dormancy = response
        .buckets
        .iter()
        .filter(|bucket| bucket.dormant_subjects > 0)
        .count();
    usize::from(summary.fully_observed_buckets) == fully_observed
        && summary.latest_fully_observed_period == latest_fully_observed
        && usize::from(summary.buckets_with_resurrection) == with_resurrection
        && usize::from(summary.buckets_with_dormancy) == with_dormancy
}

/// Proves one lifecycle bucket's window, classifications, ratios, and signed change.
fn valid_bucket(response: &LifecycleResponse, index: usize, bucket: &LifecycleBucket) -> bool {
    let Ok(period) = u16::try_from(index) else {
        return false;
    };
    let Some((_, since, until)) = query_timestamps(&response.query) else {
        return false;
    };
    let interval_seconds = response.query.interval_seconds;
    let Some(offset) = u64::from(period).checked_mul(interval_seconds) else {
        return false;
    };
    let Some(expected_start) = add_seconds(since, offset) else {
        return false;
    };
    let Some(expected_full_end) = add_seconds(expected_start, interval_seconds) else {
        return false;
    };
    let expected_end = expected_full_end.min(until);
    let Some(started_at) = parse_utc_timestamp(bucket.started_at.as_str()) else {
        return false;
    };
    let Some(ended_at) = parse_utc_timestamp(bucket.ended_at.as_str()) else {
        return false;
    };
    let current_classified = bucket
        .new_within_observed_history_subjects
        .checked_add(bucket.returning_subjects)
        .and_then(|count| count.checked_add(bucket.resurrected_subjects));
    let previous_classified = bucket
        .returning_subjects
        .checked_add(bucket.dormant_subjects);
    let net_change = i64::try_from(bucket.active_subjects)
        .ok()
        .and_then(|active| {
            i64::try_from(bucket.previous_active_subjects)
                .ok()
                .and_then(|previous| active.checked_sub(previous))
        });
    bucket.period == period
        && started_at == expected_start
        && ended_at == expected_end
        && bucket.fully_observed == (expected_full_end <= until)
        && bounded_counts(&[
            bucket.active_subjects,
            bucket.new_within_observed_history_subjects,
            bucket.returning_subjects,
            bucket.resurrected_subjects,
            bucket.dormant_subjects,
            bucket.previous_active_subjects,
        ])
        && bucket.active_subjects <= response.summary.analysis_active_subjects
        && bucket.previous_active_subjects <= response.summary.observed_subjects
        && current_classified == Some(bucket.active_subjects)
        && previous_classified == Some(bucket.previous_active_subjects)
        && net_change == Some(bucket.net_active_change)
        && ratio_matches(
            bucket.new_share_of_active,
            bucket.new_within_observed_history_subjects,
            bucket.active_subjects,
        )
        && ratio_matches(
            bucket.returning_share_of_active,
            bucket.returning_subjects,
            bucket.active_subjects,
        )
        && ratio_matches(
            bucket.resurrected_share_of_active,
            bucket.resurrected_subjects,
            bucket.active_subjects,
        )
        && ratio_matches(
            bucket.dormant_share_of_previous_active,
            bucket.dormant_subjects,
            bucket.previous_active_subjects,
        )
}

/// Requires the stable action code and target implied by validated response state.
fn valid_next_action(response: &LifecycleResponse) -> bool {
    if !bounded_contract_text(response.next_action.reason.as_str(), 768) {
        return false;
    }
    let expected = expected_next_action(response);
    response.next_action.code == expected.0 && response.next_action.target == expected.1
}

/// Derives the backend's stable next action from validated result state.
fn expected_next_action(response: &LifecycleResponse) -> (&'static str, &'static str) {
    let coverage = &response.coverage;
    if coverage.analysis_classified_events == 0 && coverage.history_selected_events == 0 {
        return ("capture_product_activity", "analyticsSchemaVersion=1");
    }
    if coverage.selected_events == 0 && coverage.history_selected_events == 0 {
        return (
            "choose_captured_lifecycle_event",
            "/api/telemetry/analytics/overview",
        );
    }
    if usable_observed_events(coverage) == Some(0) {
        return ("identify_product_users", "context.subject.id");
    }
    if coverage.usable_selected_events == 0 && coverage.usable_history_selected_events > 0 {
        return (
            "investigate_lifecycle_inactivity",
            "/api/telemetry/analytics/paths",
        );
    }
    if coverage.usable_history_selected_events == 0 {
        return (
            "extend_lifecycle_history",
            "/api/telemetry/analytics/lifecycle",
        );
    }
    if response.summary.fully_observed_buckets == 0 {
        return (
            "complete_lifecycle_observation",
            "/api/telemetry/analytics/lifecycle",
        );
    }
    if coverage.usable_selected_sessionized_events == 0 {
        return ("sessionize_product_activity", "context.session.id");
    }
    if response
        .buckets
        .iter()
        .rev()
        .find(|bucket| bucket.fully_observed)
        .is_some_and(|bucket| {
            bucket.dormant_subjects
                > bucket
                    .new_within_observed_history_subjects
                    .saturating_add(bucket.resurrected_subjects)
        })
    {
        return (
            "investigate_lifecycle_loss",
            "/api/telemetry/analytics/paths",
        );
    }
    if coverage.usable_selected_trace_linked_events == 0 {
        return ("link_product_activity_to_traces", "context.trace.trace_id");
    }
    (
        "compare_lifecycle_contexts",
        "/api/telemetry/analytics/lifecycle",
    )
}

/// Returns usable selected events across bounded history and analysis.
const fn usable_observed_events(coverage: &LifecycleCoverage) -> Option<u64> {
    coverage
        .usable_selected_events
        .checked_add(coverage.usable_history_selected_events)
}

/// Returns whether every count stays inside the server's public scan bound.
fn bounded_counts(values: &[u64]) -> bool {
    values.iter().all(|value| *value <= COUNT_LIMIT)
}

/// Verifies one optional exact aggregate ratio.
fn ratio_matches(value: Option<f64>, numerator: u64, denominator: u64) -> bool {
    if denominator == 0 {
        return value.is_none();
    }
    let (Ok(numerator), Ok(denominator)) = (u32::try_from(numerator), u32::try_from(denominator))
    else {
        return false;
    };
    value.is_some_and(|value| {
        value.is_finite()
            && (0.0..=1.0).contains(&value)
            && (value - f64::from(numerator) / f64::from(denominator)).abs() <= 1.0e-12
    })
}

/// Validates one backend-authored, non-telemetry contract string.
fn bounded_contract_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= limit
        && !value.chars().any(char::is_control)
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

/// Subtracts whole positive seconds without changing the fractional component.
fn subtract_seconds(timestamp: UtcTimestamp, seconds: u64) -> Option<UtcTimestamp> {
    Some(UtcTimestamp {
        epoch_seconds: timestamp
            .epoch_seconds
            .checked_sub(i64::try_from(seconds).ok()?)?,
        nanoseconds: timestamp.nanoseconds,
    })
}

/// Returns one timestamp as a checked nanosecond offset.
fn timestamp_nanos(timestamp: UtcTimestamp) -> i128 {
    i128::from(timestamp.epoch_seconds) * NANOS_PER_SECOND + i128::from(timestamp.nanoseconds)
}

/// Renders a useful human interpretation without reflecting backend prose.
fn render_response(response: &LifecycleResponse) -> String {
    let mut output = String::new();
    output.push_str("Product lifecycle ");
    output.push_str(response.query.event.kind.as_str());
    output.push(' ');
    output.push_str(display_text(response.query.event.event_name.as_str()).as_str());
    output.push('\n');
    output.push_str(
        format!(
            "Window: {} to {}; observed history since {}; interval: {} ({}s); history: {} periods\n",
            response.query.since,
            response.query.until,
            response.query.history_since,
            response.query.interval.as_str(),
            response.query.interval_seconds,
            response.query.history_period_count,
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Subjects: {} active in analysis; {} history-only; {} observed\n",
            response.summary.analysis_active_subjects,
            response.summary.history_only_subjects,
            response.summary.observed_subjects,
        )
        .as_str(),
    );
    render_buckets(response, &mut output);
    render_coverage(response, &mut output);
    output.push_str("Next: ");
    output.push_str(next_step(response.next_action.code.as_str()));
    output.push('\n');
    output
}

/// Adds every fixed lifecycle bucket to human output.
fn render_buckets(response: &LifecycleResponse, output: &mut String) {
    if response.buckets.is_empty() {
        output.push_str("Lifecycle buckets: no identified subject activity was observable.\n");
        return;
    }
    output.push_str("Lifecycle buckets:\n");
    for bucket in &response.buckets {
        let maturity = if bucket.fully_observed {
            ""
        } else {
            " (partial)"
        };
        output.push_str(
            format!(
                "  P{} {} to {}{}: {} active | {} new in observed history | {} returning | {} resurrected | {} dormant | net {:+}\n",
                bucket.period,
                bucket.started_at,
                bucket.ended_at,
                maturity,
                bucket.active_subjects,
                bucket.new_within_observed_history_subjects,
                bucket.returning_subjects,
                bucket.resurrected_subjects,
                bucket.dormant_subjects,
                bucket.net_active_change,
            )
            .as_str(),
        );
    }
}

/// Adds capture quality, identity gaps, and provisional-period status.
fn render_coverage(response: &LifecycleResponse, output: &mut String) {
    let coverage = &response.coverage;
    output.push_str(
        format!(
            "Coverage: named {}/{}; selected identity {}/{}; sessionized {}/{}; trace-linked {}/{}; history identity {}/{}\n",
            coverage.analysis_named_events,
            coverage.analysis_classified_events,
            coverage.usable_selected_events,
            coverage.selected_events,
            coverage.usable_selected_sessionized_events,
            coverage.usable_selected_events,
            coverage.usable_selected_trace_linked_events,
            coverage.usable_selected_events,
            coverage.usable_history_selected_events,
            coverage.history_selected_events,
        )
        .as_str(),
    );
    if coverage.analysis_unnamed_events > 0 {
        output.push_str(
            format!(
                "Capture gap: {} classified analysis events lacked an exact derived event name.\n",
                coverage.analysis_unnamed_events
            )
            .as_str(),
        );
    }
    if coverage.excluded_selected_events > 0 {
        output.push_str(
            format!(
                "Capture gap: {} selected analysis events lacked an explicit opaque subject ID.\n",
                coverage.excluded_selected_events
            )
            .as_str(),
        );
    }
    let missing_sessions = coverage
        .usable_selected_events
        .saturating_sub(coverage.usable_selected_sessionized_events);
    let missing_traces = coverage
        .usable_selected_events
        .saturating_sub(coverage.usable_selected_trace_linked_events);
    if missing_sessions > 0 || missing_traces > 0 {
        output.push_str(
            format!(
                "Correlation gaps: {missing_sessions} usable selected events lacked a session ID; {missing_traces} lacked a trace ID.\n"
            )
            .as_str(),
        );
    }
    if coverage.excluded_history_selected_events > 0 {
        output.push_str(
            format!(
                "History gap: {} selected history events lacked an explicit opaque subject ID.\n",
                coverage.excluded_history_selected_events
            )
            .as_str(),
        );
    }
    if response.summary.fully_observed_buckets < response.summary.returned_buckets {
        let partial = response
            .buckets
            .iter()
            .find(|bucket| !bucket.fully_observed)
            .map_or_else(
                || String::from("unknown"),
                |bucket| bucket.period.to_string(),
            );
        output.push_str(
            format!(
                "Provisional: {}/{} lifecycle buckets are fully observed; period {partial} is partial.\n",
                response.summary.fully_observed_buckets, response.summary.returned_buckets
            )
            .as_str(),
        );
    }
    output.push_str(
        "Interpretation: new means new only inside bounded observed history; states use one exact event; missing identities are excluded; raw subject IDs are not returned.\n",
    );
}

/// Maps validated stable action codes to local, value-free human guidance.
fn next_step(code: &str) -> &'static str {
    match code {
        "capture_product_activity" => {
            "capture version-1 page views, screen views, or interactions, then retry"
        }
        "choose_captured_lifecycle_event" => {
            "choose one exact captured event from Product Analytics overview"
        }
        "identify_product_users" => {
            "attach one stable opaque context.subject.id to the selected event"
        }
        "investigate_lifecycle_inactivity" => {
            "inspect bounded paths around the selected event before treating inactivity as churn"
        }
        "extend_lifecycle_history" => {
            "increase bounded history periods for stronger new-versus-resurrected separation"
        }
        "complete_lifecycle_observation" => {
            "choose an until value that completes at least one fixed lifecycle period"
        }
        "sessionize_product_activity" => {
            "attach one opaque context.session.id so lifecycle loss can pivot into paths"
        }
        "investigate_lifecycle_loss" => {
            "inspect bounded journeys where the latest complete period lost active subjects"
        }
        "link_product_activity_to_traces" => {
            "attach context.trace.trace_id so lifecycle movement can pivot into traces"
        }
        "compare_lifecycle_contexts" => {
            "compare the same event across bounded releases, environments, or services"
        }
        _ => "retry the bounded analytics lifecycle query",
    }
}

/// Escapes terminal controls and bidirectional-display characters in event names.
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

/// Converts transport and refresh failures into fixed lifecycle-safe recovery.
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
        message: "analytics lifecycle request could not be completed",
        next: "check network connectivity and retry the same analytics lifecycle query",
    }
}

/// Returns one fixed response-contract failure.
const fn invalid_response() -> RuntimeError {
    RuntimeError::AnalyticsLifecycleResponseInvalid
}

/// Converts a failed HTTP status into fixed guidance without reflecting its body.
fn safe_api_error(status: u16, credential: &AuthCredential) -> RuntimeError {
    let (error, code, next) = match status {
        400 | 422 => (
            "analytics lifecycle request rejected",
            "validation_failed",
            "check the exact project, time scope, event selector, interval, and history periods",
        ),
        401 => (
            "authentication required",
            "unauthorized",
            "run logbrew login",
        ),
        403 => (
            "analytics lifecycle request forbidden",
            "forbidden",
            "confirm account access and retry the same analytics lifecycle query",
        ),
        404 => (
            "analytics lifecycle resource not found",
            "not_found",
            "check the project and retry the same analytics lifecycle query",
        ),
        405 => (
            "analytics lifecycle method is not supported",
            "method_not_allowed",
            "use the POST-backed logbrew analytics lifecycle command",
        ),
        429 => (
            "analytics lifecycle request rate limited",
            "rate_limited",
            "retry the same analytics lifecycle query later",
        ),
        500..=599 => (
            "analytics lifecycle service unavailable",
            "service_unavailable",
            "retry the same analytics lifecycle query later",
        ),
        _ => (
            "analytics lifecycle request failed",
            "request_failed",
            "check account access and retry the same analytics lifecycle query",
        ),
    };
    RuntimeError::Api {
        status,
        body: serde_json::json!({"error": error, "code": code, "next": next}).to_string(),
        auth_source: credential.source(),
        auth_label: credential.label(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one stable lifecycle query.
    fn options() -> AnalyticsLifecycleOptions {
        AnalyticsLifecycleOptions {
            project_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".to_owned(),
            since: "24h".to_owned(),
            until: None,
            service_name: None,
            release: None,
            environment: Some("production".to_owned()),
            event_kind: AnalyticsLifecycleEventKind::Interaction,
            event_name: "checkout_completed".to_owned(),
            interval: Some(AnalyticsLifecycleInterval::Hour),
            history_period_count: 2,
        }
    }

    /// Builds one complete response through the strict JSON boundary.
    fn response() -> LifecycleResponse {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "query": {
                "project_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "since": "2026-08-01T00:00:00Z",
                "until": "2026-08-01T03:30:00Z",
                "history_since": "2026-07-31T22:00:00Z",
                "environment": "production",
                "event": {"kind": "interaction", "event_name": "checkout_completed"},
                "interval": "hour",
                "interval_seconds": 3600,
                "history_period_count": 2,
                "expected_buckets": 4
            },
            "purpose": "Classifies bounded identified-user lifecycle state.",
            "summary": {
                "observed_subjects": 10,
                "analysis_active_subjects": 8,
                "history_only_subjects": 2,
                "returned_buckets": 4,
                "fully_observed_buckets": 3,
                "latest_fully_observed_period": 2,
                "buckets_with_resurrection": 4,
                "buckets_with_dormancy": 4
            },
            "coverage": {
                "analysis_classified_events": 100,
                "analysis_named_events": 95,
                "analysis_unnamed_events": 5,
                "analysis_identified_events": 80,
                "selected_events": 30,
                "usable_selected_events": 20,
                "usable_selected_sessionized_events": 18,
                "usable_selected_trace_linked_events": 15,
                "excluded_selected_events": 10,
                "history_selected_events": 10,
                "usable_history_selected_events": 8,
                "excluded_history_selected_events": 2,
                "event_name_rate": 0.95,
                "selected_identity_coverage_rate": 0.666_666_666_666_666_6,
                "selected_sessionization_rate": 0.9,
                "selected_trace_link_rate": 0.75,
                "history_identity_coverage_rate": 0.8,
                "limitations": ["New in observed history is bounded."]
            },
            "buckets": [
                lifecycle_bucket(0, "2026-08-01T00:00:00Z", "2026-08-01T01:00:00Z", true, [6, 3, 2, 1, 2, 4], [0.5, 1.0 / 3.0, 1.0 / 6.0, 0.5], 2),
                lifecycle_bucket(1, "2026-08-01T01:00:00Z", "2026-08-01T02:00:00Z", true, [5, 1, 3, 1, 3, 6], [0.2, 0.6, 0.2, 0.5], -1),
                lifecycle_bucket(2, "2026-08-01T02:00:00Z", "2026-08-01T03:00:00Z", true, [5, 0, 4, 1, 1, 5], [0.0, 0.8, 0.2, 0.2], 0),
                lifecycle_bucket(3, "2026-08-01T03:00:00Z", "2026-08-01T03:30:00Z", false, [4, 0, 3, 1, 2, 5], [0.0, 0.75, 0.25, 0.4], -1)
            ],
            "next_action": {
                "code": "compare_lifecycle_contexts",
                "target": "/api/telemetry/analytics/lifecycle",
                "reason": "Compare exact bounded contexts."
            }
        }))
        .expect("fixture deserializes")
    }

    /// Builds one complete bucket JSON value.
    fn lifecycle_bucket(
        period: u16,
        started_at: &str,
        ended_at: &str,
        fully_observed: bool,
        counts: [u64; 6],
        rates: [f64; 4],
        net_active_change: i64,
    ) -> serde_json::Value {
        let [active, new, returning, resurrected, dormant, previous] = counts;
        let [new_rate, returning_rate, resurrected_rate, dormant_rate] = rates;
        let value = serde_json::json!({
            "period": period,
            "started_at": started_at,
            "ended_at": ended_at,
            "fully_observed": fully_observed,
            "active_subjects": active,
            "new_within_observed_history_subjects": new,
            "returning_subjects": returning,
            "resurrected_subjects": resurrected,
            "dormant_subjects": dormant,
            "previous_active_subjects": previous,
            "new_share_of_active": new_rate,
            "returning_share_of_active": returning_rate,
            "resurrected_share_of_active": resurrected_rate,
            "dormant_share_of_previous_active": dormant_rate,
            "net_active_change": net_active_change
        });
        value
    }

    #[test]
    fn validates_every_lifecycle_contract_layer() {
        let response = response();
        assert!(valid_query(&options(), &response.query), "query");
        assert!(valid_coverage(&response.coverage), "coverage");
        for (index, bucket) in response.buckets.iter().enumerate() {
            assert!(valid_bucket(&response, index, bucket), "bucket {index}");
        }
        assert!(valid_summary_and_buckets(&response), "summary or buckets");
        assert!(valid_next_action(&response), "next action");
    }

    #[test]
    fn rejects_contradictory_derived_lifecycle_values() {
        let mut invalid_coverage = response();
        invalid_coverage.coverage.usable_selected_sessionized_events = 21;
        assert!(
            !valid_coverage(&invalid_coverage.coverage),
            "sessionized events cannot exceed usable events"
        );

        let mut invalid_bucket = response();
        invalid_bucket.buckets[0].net_active_change = 1;
        assert!(
            !valid_summary_and_buckets(&invalid_bucket),
            "bucket net change must match active subject counts"
        );

        let mut invalid_action = response();
        invalid_action.next_action.code = "capture_product_activity".to_owned();
        assert!(
            !valid_next_action(&invalid_action),
            "next action must match the validated lifecycle state"
        );
    }
}
