//! Versioned, bounded product-analytics retention reporting.

#![expect(
    clippy::missing_docs_in_private_items,
    reason = "response fields mirror the exact public analytics contract"
)]

use serde::Deserialize;

use crate::analytics_contract::{NextAction, bounded_counts, ratio_matches};
use crate::analytics_request::{self, Kind, scoped_body};
use crate::http::{nonempty_control_safe as bounded_contract_text, terminal_safe as display_text};
use crate::time::{
    ParsedTimestamp as UtcTimestamp, add_seconds, parse_utc_timestamp, timestamp_nanos,
};
use crate::{
    AnalyticsRetentionCohortMode, AnalyticsRetentionEventKind, AnalyticsRetentionInterval,
    AnalyticsRetentionMode, AnalyticsRetentionOptions, CliEnvironment, RuntimeError,
};

/// Public response version implemented by this CLI.
const SCHEMA_VERSION: u8 = 1;
/// Maximum accepted response body.
const RESPONSE_LIMIT: usize = 1024 * 1024;
/// Maximum returned cohort-period matrix cells.
const MATRIX_CELL_LIMIT: usize = 500;
/// Maximum material limitations accepted from the bounded API.
const LIMITATION_LIMIT: usize = 12;
/// Nanoseconds in one second.
const NANOS_PER_SECOND: i128 = 1_000_000_000;

/// Builds the exact public POST body with explicit CLI defaults.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command model consumes this private-module helper"
)]
pub(super) fn request_body(options: &AnalyticsRetentionOptions) -> serde_json::Value {
    let mut body = scoped_body(
        &options.project_id,
        &options.since,
        options.until.as_deref(),
        options.service_name.as_deref(),
        options.release.as_deref(),
        options.environment.as_deref(),
    );
    drop(body.insert(
        "start_event".to_owned(),
        serde_json::json!({
            "kind": options.start_kind.as_str(),
            "event_name": options.start_event,
        }),
    ));
    drop(body.insert(
        "return_event".to_owned(),
        serde_json::json!({
            "kind": options.return_kind.as_str(),
            "event_name": options.return_event,
        }),
    ));
    drop(body.insert(
        "interval".to_owned(),
        serde_json::Value::String(options.interval.as_str().to_owned()),
    ));
    drop(body.insert("interval_count".to_owned(), options.interval_count.into()));
    drop(body.insert(
        "mode".to_owned(),
        serde_json::Value::String(options.mode.as_str().to_owned()),
    ));
    drop(body.insert(
        "cohort_mode".to_owned(),
        serde_json::Value::String(options.cohort_mode.as_str().to_owned()),
    ));
    serde_json::Value::Object(body)
}

/// Executes one aggregate, identity-safe retention request.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    options: &AnalyticsRetentionOptions,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let request = request_body(options);
    let body = analytics_request::send(
        env,
        "/api/telemetry/analytics/retention",
        Kind::Retention,
        Some(&request),
        RESPONSE_LIMIT,
    )
    .await?;
    let response = validated_response(options, body.as_str())?;
    if json {
        writeln!(output, "{body}")?;
    } else {
        write!(output, "{}", render_response(&response))?;
    }
    Ok(())
}

/// Complete response with unknown fields rejected at every level.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionResponse {
    schema_version: u8,
    query: RetentionQuery,
    purpose: String,
    summary: RetentionSummary,
    coverage: RetentionCoverage,
    curve: Vec<RetentionPeriod>,
    cohorts: Vec<RetentionCohort>,
    next_action: NextAction,
}

/// Normalized effective query echoed by the backend.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionQuery {
    project_id: String,
    since: String,
    until: String,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    start_event: RetentionEvent,
    return_event: RetentionEvent,
    interval: AnalyticsRetentionInterval,
    interval_seconds: u64,
    interval_count: u8,
    mode: AnalyticsRetentionMode,
    cohort_mode: AnalyticsRetentionCohortMode,
}

/// One exact classified event selector.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionEvent {
    kind: AnalyticsRetentionEventKind,
    event_name: String,
}

/// Aggregate identified-subject outcome.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionSummary {
    cohort_subjects: u64,
    subjects_returned_after_start: u64,
    return_rate_after_start: Option<f64>,
    non_empty_cohorts: u64,
    periods_with_eligible_subjects: u8,
    fully_observed_periods: u8,
}

/// Capture coverage qualifying the retention result.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionCoverage {
    classified_events: u64,
    named_events: u64,
    unnamed_events: u64,
    identified_events: u64,
    start_events: u64,
    usable_start_events: u64,
    excluded_start_events: u64,
    return_events: u64,
    usable_return_events: u64,
    excluded_return_events: u64,
    event_name_rate: Option<f64>,
    start_identity_coverage_rate: Option<f64>,
    return_identity_coverage_rate: Option<f64>,
    limitations: Vec<String>,
}

/// One maturity-aware aggregate retention period.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionPeriod {
    period: u8,
    threshold_seconds: u64,
    window_end_seconds: Option<u64>,
    eligible_subjects: u64,
    retained_subjects: u64,
    weighted_retention_rate: Option<f64>,
    fully_observed_cohorts: u64,
    fully_observed_cohort_average_rate: Option<f64>,
    all_subjects_eligible: bool,
}

/// One period cell in a query-relative cohort row.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionCell {
    period: u8,
    eligible_subjects: u64,
    retained_subjects: u64,
    retention_rate: Option<f64>,
    all_subjects_eligible: bool,
}

/// One non-empty query-relative cohort.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionCohort {
    cohort: u64,
    started_at: String,
    ended_at: String,
    subjects: u64,
    subjects_returned_after_start: u64,
    periods: Vec<RetentionCell>,
}

/// Parses and proves the complete schema-version-1 response.
fn validated_response(
    options: &AnalyticsRetentionOptions,
    body: &str,
) -> Result<RetentionResponse, RuntimeError> {
    let response = serde_json::from_str::<RetentionResponse>(body)
        .map_err(|_error| Kind::Retention.invalid())?;
    if response.schema_version != SCHEMA_VERSION
        || !bounded_contract_text(response.purpose.as_str(), 4096)
        || !valid_query(options, &response.query)
        || !valid_coverage(&response.coverage)
        || !valid_cohorts(&response)
        || !valid_summary(&response)
        || !valid_curve(&response)
        || !valid_next_action(&response)
    {
        return Err(Kind::Retention.invalid());
    }
    Ok(response)
}

/// Requires the backend echo to match every exact client-selected scope field.
fn valid_query(options: &AnalyticsRetentionOptions, query: &RetentionQuery) -> bool {
    let (Some(since), Some(until)) = (
        parse_utc_timestamp(query.since.as_str()),
        parse_utc_timestamp(query.until.as_str()),
    ) else {
        return false;
    };
    query.project_id == options.project_id
        && since < until
        && query.service_name == options.service_name
        && query.release == options.release
        && query.environment == options.environment
        && query.start_event.kind == options.start_kind
        && query.start_event.event_name == options.start_event
        && query.return_event.kind == options.return_kind
        && query.return_event.event_name == options.return_event
        && query.interval == options.interval
        && query.interval_seconds == options.interval.seconds()
        && query.interval_count == options.interval_count
        && query.mode == options.mode
        && query.cohort_mode == options.cohort_mode
}

/// Proves every derived coverage count, ratio, and limitation bound.
fn valid_coverage(coverage: &RetentionCoverage) -> bool {
    if !bounded_counts(&[
        coverage.classified_events,
        coverage.named_events,
        coverage.unnamed_events,
        coverage.identified_events,
        coverage.start_events,
        coverage.usable_start_events,
        coverage.excluded_start_events,
        coverage.return_events,
        coverage.usable_return_events,
        coverage.excluded_return_events,
    ]) || coverage.named_events > coverage.classified_events
        || coverage.identified_events > coverage.classified_events
        || coverage.start_events > coverage.named_events
        || coverage.return_events > coverage.named_events
        || coverage.usable_start_events > coverage.start_events
        || coverage.usable_start_events > coverage.identified_events
        || coverage.usable_return_events > coverage.return_events
        || coverage.usable_return_events > coverage.identified_events
        || coverage.unnamed_events != coverage.classified_events - coverage.named_events
        || coverage.excluded_start_events != coverage.start_events - coverage.usable_start_events
        || coverage.excluded_return_events != coverage.return_events - coverage.usable_return_events
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
        coverage.named_events,
        coverage.classified_events,
    ) && ratio_matches(
        coverage.start_identity_coverage_rate,
        coverage.usable_start_events,
        coverage.start_events,
    ) && ratio_matches(
        coverage.return_identity_coverage_rate,
        coverage.usable_return_events,
        coverage.return_events,
    )
}

/// Proves cohort ordering, timestamps, maturity, counts, and matrix bounds.
fn valid_cohorts(response: &RetentionResponse) -> bool {
    let Some((since, until, bucket_count)) = query_time_bounds(&response.query) else {
        return false;
    };
    let interval_count = usize::from(response.query.interval_count);
    if response
        .cohorts
        .len()
        .checked_mul(interval_count)
        .is_none_or(|cells| cells > MATRIX_CELL_LIMIT)
    {
        return false;
    }
    let mut previous_index = None;
    for cohort in &response.cohorts {
        if previous_index.is_some_and(|previous| cohort.cohort <= previous)
            || cohort.cohort >= bucket_count
            || !valid_cohort(response, cohort, since, until)
        {
            return false;
        }
        previous_index = Some(cohort.cohort);
    }
    true
}

/// Returns query timestamps and the exact maximum bucket count.
fn query_time_bounds(query: &RetentionQuery) -> Option<(UtcTimestamp, UtcTimestamp, u64)> {
    let since = parse_utc_timestamp(query.since.as_str())?;
    let until = parse_utc_timestamp(query.until.as_str())?;
    let duration = timestamp_nanos(until).checked_sub(timestamp_nanos(since))?;
    if duration <= 0 {
        return None;
    }
    let interval = i128::from(query.interval_seconds).checked_mul(NANOS_PER_SECOND)?;
    let buckets = duration.checked_add(interval.checked_sub(1)?)? / interval;
    Some((since, until, u64::try_from(buckets).ok()?))
}

/// Proves one cohort's bucket bounds and every requested period cell.
fn valid_cohort(
    response: &RetentionResponse,
    cohort: &RetentionCohort,
    since: UtcTimestamp,
    until: UtcTimestamp,
) -> bool {
    if cohort.subjects == 0
        || !bounded_counts(&[cohort.subjects, cohort.subjects_returned_after_start])
        || cohort.subjects_returned_after_start > cohort.subjects
        || cohort.periods.len() != usize::from(response.query.interval_count)
    {
        return false;
    }
    let (Some(started), Some(ended)) = (
        parse_utc_timestamp(cohort.started_at.as_str()),
        parse_utc_timestamp(cohort.ended_at.as_str()),
    ) else {
        return false;
    };
    let Some(offset) = cohort
        .cohort
        .checked_mul(response.query.interval_seconds)
        .and_then(|value| i64::try_from(value).ok())
    else {
        return false;
    };
    let Some(expected_start) = add_seconds(since, offset) else {
        return false;
    };
    let Some(uncapped_end) = i64::try_from(response.query.interval_seconds)
        .ok()
        .and_then(|seconds| add_seconds(expected_start, seconds))
    else {
        return false;
    };
    if started != expected_start || ended != uncapped_end.min(until) || started >= ended {
        return false;
    }
    cohort.periods.iter().enumerate().all(|(index, cell)| {
        cell.period == u8::try_from(index).unwrap_or(u8::MAX) && valid_cell(cohort.subjects, cell)
    })
}

/// Proves one cohort cell's bounded counts, exact rate, and maturity flag.
fn valid_cell(cohort_subjects: u64, cell: &RetentionCell) -> bool {
    bounded_counts(&[cell.eligible_subjects, cell.retained_subjects])
        && cell.eligible_subjects <= cohort_subjects
        && cell.retained_subjects <= cell.eligible_subjects
        && ratio_matches(
            cell.retention_rate,
            cell.retained_subjects,
            cell.eligible_subjects,
        )
        && cell.all_subjects_eligible == (cell.eligible_subjects == cohort_subjects)
}

/// Proves headline counts against the complete cohort matrix.
fn valid_summary(response: &RetentionResponse) -> bool {
    let summary = &response.summary;
    if !bounded_counts(&[
        summary.cohort_subjects,
        summary.subjects_returned_after_start,
        summary.non_empty_cohorts,
    ]) || summary.subjects_returned_after_start > summary.cohort_subjects
        || summary.non_empty_cohorts != u64::try_from(response.cohorts.len()).unwrap_or(u64::MAX)
        || summary.periods_with_eligible_subjects > response.query.interval_count
        || summary.fully_observed_periods > response.query.interval_count
        || !ratio_matches(
            summary.return_rate_after_start,
            summary.subjects_returned_after_start,
            summary.cohort_subjects,
        )
    {
        return false;
    }
    let subjects = checked_sum(response.cohorts.iter().map(|cohort| cohort.subjects));
    let returned = checked_sum(
        response
            .cohorts
            .iter()
            .map(|cohort| cohort.subjects_returned_after_start),
    );
    subjects == Some(summary.cohort_subjects)
        && returned == Some(summary.subjects_returned_after_start)
        && summary.cohort_subjects <= response.coverage.usable_start_events
        && summary.subjects_returned_after_start <= response.coverage.usable_return_events
}

/// Proves every weighted curve point against its cohort cells.
fn valid_curve(response: &RetentionResponse) -> bool {
    if response.curve.len() != usize::from(response.query.interval_count) {
        return false;
    }
    let mut periods_with_eligible = 0_u8;
    let mut fully_observed_periods = 0_u8;
    for (index, period) in response.curve.iter().enumerate() {
        if !valid_curve_period(response, period, index) {
            return false;
        }
        if period.eligible_subjects > 0 {
            periods_with_eligible = periods_with_eligible.saturating_add(1);
        }
        if period.all_subjects_eligible {
            fully_observed_periods = fully_observed_periods.saturating_add(1);
        }
    }
    periods_with_eligible == response.summary.periods_with_eligible_subjects
        && fully_observed_periods == response.summary.fully_observed_periods
}

/// Proves one curve point's timing, totals, weighted rate, and complete-cohort mean.
fn valid_curve_period(
    response: &RetentionResponse,
    period: &RetentionPeriod,
    index: usize,
) -> bool {
    let expected_period = u8::try_from(index).unwrap_or(u8::MAX);
    let Some(threshold) = u64::from(expected_period).checked_mul(response.query.interval_seconds)
    else {
        return false;
    };
    let expected_end = match response.query.mode {
        AnalyticsRetentionMode::ReturnOn => u64::from(expected_period.saturating_add(1))
            .checked_mul(response.query.interval_seconds),
        AnalyticsRetentionMode::ReturnOnOrAfter => None,
    };
    let Some(aggregate) = aggregate_period(response, index) else {
        return false;
    };
    period.period == expected_period
        && period.threshold_seconds == threshold
        && period.window_end_seconds == expected_end
        && bounded_counts(&[
            period.eligible_subjects,
            period.retained_subjects,
            period.fully_observed_cohorts,
        ])
        && period.eligible_subjects == aggregate.eligible
        && period.retained_subjects == aggregate.retained
        && period.fully_observed_cohorts == aggregate.complete_cohorts
        && ratio_matches(
            period.weighted_retention_rate,
            period.retained_subjects,
            period.eligible_subjects,
        )
        && optional_float_matches(
            period.fully_observed_cohort_average_rate,
            aggregate.complete_average,
        )
        && period.all_subjects_eligible
            == (response.summary.cohort_subjects > 0
                && period.eligible_subjects == response.summary.cohort_subjects)
}

/// Derived aggregate for one curve period.
#[derive(Debug, Clone, Copy)]
struct PeriodAggregate {
    /// Eligible identified subjects.
    eligible: u64,
    /// Retained eligible subjects.
    retained: u64,
    /// Cohorts whose entire subject set is eligible.
    complete_cohorts: u64,
    /// Equal-weight mean across complete cohort rates.
    complete_average: Option<f64>,
}

/// Recomputes one curve period from the complete cohort matrix.
fn aggregate_period(response: &RetentionResponse, index: usize) -> Option<PeriodAggregate> {
    let mut eligible = 0_u64;
    let mut retained = 0_u64;
    let mut complete_cohorts = 0_u64;
    let mut complete_rate_sum = 0.0_f64;
    for cohort in &response.cohorts {
        let cell = cohort.periods.get(index)?;
        eligible = eligible.checked_add(cell.eligible_subjects)?;
        retained = retained.checked_add(cell.retained_subjects)?;
        if cell.all_subjects_eligible {
            complete_cohorts = complete_cohorts.checked_add(1)?;
            complete_rate_sum += cell.retention_rate?;
        }
    }
    let complete_average = if complete_cohorts == 0 {
        None
    } else {
        let denominator = u32::try_from(complete_cohorts).ok()?;
        Some(complete_rate_sum / f64::from(denominator))
    };
    Some(PeriodAggregate {
        eligible,
        retained,
        complete_cohorts,
        complete_average,
    })
}

/// Requires the stable action code and target implied by the response state.
fn valid_next_action(response: &RetentionResponse) -> bool {
    let expected = expected_next_action(response);
    response.next_action.matches(expected.0, expected.1, 768)
}

/// Derives the server's stable next action from validated result state.
const fn expected_next_action(response: &RetentionResponse) -> (&'static str, &'static str) {
    if response.coverage.classified_events == 0 {
        ("capture_product_activity", "analyticsSchemaVersion=1")
    } else if response.coverage.start_events == 0 {
        (
            "choose_captured_retention_start",
            "/api/telemetry/analytics/overview",
        )
    } else if response.coverage.usable_start_events == 0 {
        ("identify_product_users", "context.subject.kind=user")
    } else if response.coverage.return_events == 0 {
        (
            "capture_or_choose_retention_return",
            "/api/telemetry/analytics/overview",
        )
    } else if response.coverage.usable_return_events == 0 {
        ("identify_returning_users", "context.subject.kind=user")
    } else if response.summary.cohort_subjects == 0 {
        (
            "verify_retention_scope",
            "/api/telemetry/analytics/retention",
        )
    } else if response.summary.subjects_returned_after_start == 0 {
        ("investigate_missing_returns", "/api/telemetry/traces")
    } else if response.summary.fully_observed_periods < response.query.interval_count {
        (
            "extend_retention_observation_window",
            "/api/telemetry/analytics/retention",
        )
    } else {
        (
            "compare_retention_contexts",
            "/api/telemetry/analytics/retention",
        )
    }
}

/// Sums bounded counts without silent overflow.
fn checked_sum(mut values: impl Iterator<Item = u64>) -> Option<u64> {
    values.try_fold(0_u64, u64::checked_add)
}

/// Verifies two optional finite rates with the public precision tolerance.
fn optional_float_matches(actual: Option<f64>, expected: Option<f64>) -> bool {
    match (actual, expected) {
        (None, None) => true,
        (Some(actual), Some(expected)) => {
            actual.is_finite()
                && expected.is_finite()
                && (0.0..=1.0).contains(&actual)
                && (actual - expected).abs() <= 1.0e-12
        }
        (None, Some(_)) | (Some(_), None) => false,
    }
}

/// Renders a useful human interpretation without reflecting backend prose.
fn render_response(response: &RetentionResponse) -> String {
    let mut output = String::new();
    output.push_str("Product retention ");
    output.push_str(response.query.start_event.kind.as_str());
    output.push(' ');
    output.push_str(display_text(response.query.start_event.event_name.as_str()).as_str());
    output.push_str(" -> ");
    output.push_str(response.query.return_event.kind.as_str());
    output.push(' ');
    output.push_str(display_text(response.query.return_event.event_name.as_str()).as_str());
    output.push('\n');
    output.push_str(
        format!(
            "Window: {} to {}; interval: {} ({}s); mode: {}; cohort: {}\n",
            response.query.since,
            response.query.until,
            response.query.interval.as_str(),
            response.query.interval_seconds,
            response.query.mode.as_str(),
            response.query.cohort_mode.as_str(),
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Cohort subjects: {}; returned after start: {} ({})\n",
            response.summary.cohort_subjects,
            response.summary.subjects_returned_after_start,
            display_rate(response.summary.return_rate_after_start),
        )
        .as_str(),
    );
    render_curve(response, &mut output);
    render_cohorts(response, &mut output);
    render_coverage(response, &mut output);
    output.push_str("Next: ");
    output.push_str(next_step(response.next_action.code.as_str()));
    output.push('\n');
    output
}

/// Adds every maturity-aware curve point to human output.
fn render_curve(response: &RetentionResponse, output: &mut String) {
    output.push_str("Retention curve:\n");
    for period in &response.curve {
        let window = period_window(response.query.mode, period);
        let maturity = if period.all_subjects_eligible {
            "complete"
        } else {
            "partial"
        };
        output.push_str(
            format!(
                "  P{} {}: {}/{} retained ({}); complete cohorts: {} (mean {}); maturity: {}\n",
                period.period,
                window,
                period.retained_subjects,
                period.eligible_subjects,
                display_rate(period.weighted_retention_rate),
                period.fully_observed_cohorts,
                display_rate(period.fully_observed_cohort_average_rate),
                maturity,
            )
            .as_str(),
        );
    }
}

/// Describes the exact or rolling period window without ambiguity.
fn period_window(mode: AnalyticsRetentionMode, period: &RetentionPeriod) -> String {
    match mode {
        AnalyticsRetentionMode::ReturnOn => format!(
            "[{}s, {}s)",
            period.threshold_seconds,
            period
                .window_end_seconds
                .unwrap_or(period.threshold_seconds)
        ),
        AnalyticsRetentionMode::ReturnOnOrAfter => {
            format!("[>={}s, until)", period.threshold_seconds)
        }
    }
}

/// Adds the complete bounded cohort matrix to human output.
fn render_cohorts(response: &RetentionResponse, output: &mut String) {
    if response.cohorts.is_empty() {
        output.push_str("Cohorts: no identified subject formed a cohort.\n");
        return;
    }
    output.push_str("Cohorts:\n");
    for cohort in &response.cohorts {
        output.push_str(
            format!(
                "  C{} {} to {}: {} subjects; {} returned after start\n",
                cohort.cohort,
                cohort.started_at,
                cohort.ended_at,
                cohort.subjects,
                cohort.subjects_returned_after_start,
            )
            .as_str(),
        );
        output.push_str("    ");
        output.push_str(
            cohort
                .periods
                .iter()
                .map(|cell| {
                    let maturity = if cell.all_subjects_eligible {
                        "complete"
                    } else {
                        "partial"
                    };
                    format!(
                        "P{} {}/{} {} {}",
                        cell.period,
                        cell.retained_subjects,
                        cell.eligible_subjects,
                        display_rate(cell.retention_rate),
                        maturity,
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
                .as_str(),
        );
        output.push('\n');
    }
}

/// Adds capture coverage, identity gaps, and maturity limits to human output.
fn render_coverage(response: &RetentionResponse, output: &mut String) {
    let coverage = &response.coverage;
    output.push_str(
        format!(
            "Coverage: named {}/{}; identified {}/{}; usable starts {}/{}; usable returns {}/{}\n",
            coverage.named_events,
            coverage.classified_events,
            coverage.identified_events,
            coverage.classified_events,
            coverage.usable_start_events,
            coverage.start_events,
            coverage.usable_return_events,
            coverage.return_events,
        )
        .as_str(),
    );
    if coverage.unnamed_events > 0 {
        output.push_str(
            format!(
                "Capture gap: {} classified events lacked an exact derived event name.\n",
                coverage.unnamed_events
            )
            .as_str(),
        );
    }
    let unidentified = coverage
        .classified_events
        .saturating_sub(coverage.identified_events);
    if unidentified > 0 {
        output.push_str(
            format!(
                "Capture gap: {unidentified} classified events lacked an explicit opaque subject ID.\n"
            )
            .as_str(),
        );
    }
    if coverage.excluded_start_events > 0 || coverage.excluded_return_events > 0 {
        output.push_str(
            format!(
                "Identity exclusions: {} start events; {} return events.\n",
                coverage.excluded_start_events, coverage.excluded_return_events
            )
            .as_str(),
        );
    }
    if response.summary.fully_observed_periods < response.query.interval_count {
        output.push_str(
            format!(
                "Observation limit: {}/{} periods are fully observed for every cohort subject.\n",
                response.summary.fully_observed_periods, response.query.interval_count
            )
            .as_str(),
        );
    }
    output.push_str(
        "Interpretation: first_in_range is not lifetime-first; anonymous, legacy-untyped, \
         historical-unindexed, and missing subjects are excluded; raw subject IDs are not \
         returned.\n",
    );
}

/// Formats one optional ratio for concise human output.
fn display_rate(value: Option<f64>) -> String {
    value.map_or_else(
        || "not observable".to_owned(),
        |value| format!("{:.1}%", value * 100.0),
    )
}

/// Maps validated stable action codes to local, value-free human guidance.
fn next_step(code: &str) -> &'static str {
    match code {
        "capture_product_activity" => {
            "capture version-1 page views, screen views, or interactions, then retry"
        }
        "choose_captured_retention_start" => {
            "choose an exact captured start event from Product Analytics overview"
        }
        "identify_product_users" => {
            "attach one stable opaque context.subject.id and set context.subject.kind=user on \
             start events"
        }
        "capture_or_choose_retention_return" => {
            "capture the intended return or choose an exact captured return event"
        }
        "identify_returning_users" => {
            "attach the same stable opaque context.subject.id and set context.subject.kind=user \
             on return events"
        }
        "verify_retention_scope" => {
            "verify the exact start selector and project, time, and context scope"
        }
        "investigate_missing_returns" => {
            "inspect correlated traces around the intended return journey before assuming churn"
        }
        "extend_retention_observation_window" => {
            "move since earlier, until later, or request fewer periods for a complete comparison"
        }
        "compare_retention_contexts" => {
            "compare the same start and return events across releases, environments, or services"
        }
        _ => "retry the bounded analytics retention query",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one stable retention query.
    fn options() -> AnalyticsRetentionOptions {
        AnalyticsRetentionOptions {
            project_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned(),
            since: "30d".to_owned(),
            until: None,
            service_name: None,
            release: None,
            environment: Some("production".to_owned()),
            start_kind: AnalyticsRetentionEventKind::PageView,
            start_event: "/signup".to_owned(),
            return_kind: AnalyticsRetentionEventKind::Interaction,
            return_event: "dashboard_opened".to_owned(),
            interval: AnalyticsRetentionInterval::Day,
            interval_count: 2,
            mode: AnalyticsRetentionMode::ReturnOn,
            cohort_mode: AnalyticsRetentionCohortMode::FirstInRange,
        }
    }

    /// Builds a complete internally consistent response fixture.
    fn response() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "query": {
                "project_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "since": "2026-08-01T00:00:00Z",
                "until": "2026-08-04T00:00:00Z",
                "environment": "production",
                "start_event": {"kind": "page_view", "event_name": "/signup"},
                "return_event": {"kind": "interaction", "event_name": "dashboard_opened"},
                "interval": "day",
                "interval_seconds": 86400,
                "interval_count": 2,
                "mode": "return_on",
                "cohort_mode": "first_in_range"
            },
            "purpose": "Measures maturity-aware identified-user retention.",
            "summary": {
                "cohort_subjects": 10,
                "subjects_returned_after_start": 6,
                "return_rate_after_start": 0.6,
                "non_empty_cohorts": 2,
                "periods_with_eligible_subjects": 2,
                "fully_observed_periods": 1
            },
            "coverage": {
                "classified_events": 100,
                "named_events": 90,
                "unnamed_events": 10,
                "identified_events": 80,
                "start_events": 20,
                "usable_start_events": 16,
                "excluded_start_events": 4,
                "return_events": 18,
                "usable_return_events": 14,
                "excluded_return_events": 4,
                "event_name_rate": 0.9,
                "start_identity_coverage_rate": 0.8,
                "return_identity_coverage_rate": 0.777_777_777_777_777_8,
                "limitations": ["Only explicit opaque subject IDs qualify."]
            },
            "curve": [
                {
                    "period": 0,
                    "threshold_seconds": 0,
                    "window_end_seconds": 86400,
                    "eligible_subjects": 10,
                    "retained_subjects": 5,
                    "weighted_retention_rate": 0.5,
                    "fully_observed_cohorts": 2,
                    "fully_observed_cohort_average_rate": 0.5,
                    "all_subjects_eligible": true
                },
                {
                    "period": 1,
                    "threshold_seconds": 86400,
                    "window_end_seconds": 172_800,
                    "eligible_subjects": 6,
                    "retained_subjects": 2,
                    "weighted_retention_rate": 0.333_333_333_333_333_3,
                    "fully_observed_cohorts": 1,
                    "fully_observed_cohort_average_rate": 0.333_333_333_333_333_3,
                    "all_subjects_eligible": false
                }
            ],
            "cohorts": [
                {
                    "cohort": 0,
                    "started_at": "2026-08-01T00:00:00Z",
                    "ended_at": "2026-08-02T00:00:00Z",
                    "subjects": 6,
                    "subjects_returned_after_start": 4,
                    "periods": [
                        {"period": 0, "eligible_subjects": 6, "retained_subjects": 3, "retention_rate": 0.5, "all_subjects_eligible": true},
                        {"period": 1, "eligible_subjects": 6, "retained_subjects": 2, "retention_rate": 0.333_333_333_333_333_3, "all_subjects_eligible": true}
                    ]
                },
                {
                    "cohort": 1,
                    "started_at": "2026-08-02T00:00:00Z",
                    "ended_at": "2026-08-03T00:00:00Z",
                    "subjects": 4,
                    "subjects_returned_after_start": 2,
                    "periods": [
                        {"period": 0, "eligible_subjects": 4, "retained_subjects": 2, "retention_rate": 0.5, "all_subjects_eligible": true},
                        {"period": 1, "eligible_subjects": 0, "retained_subjects": 0, "all_subjects_eligible": false}
                    ]
                }
            ],
            "next_action": {
                "code": "extend_retention_observation_window",
                "target": "/api/telemetry/analytics/retention",
                "reason": "Extend the window."
            }
        })
    }

    #[test]
    fn validates_and_renders_complete_retention_evidence() {
        let body = response().to_string();
        let response = validated_response(&options(), body.as_str()).expect("valid response");
        let rendered = render_response(&response);
        assert!(rendered.contains("Cohort subjects: 10; returned after start: 6 (60.0%)"));
        assert!(rendered.contains("P1 [86400s, 172800s): 2/6 retained (33.3%)"));
        assert!(rendered.contains("Observation limit: 1/2 periods"));
        assert!(!rendered.contains(response.next_action.reason.as_str()));
    }

    #[test]
    fn rejects_unknown_identity_fields_and_inconsistent_derived_values() {
        let mut unknown = response();
        unknown["cohorts"][0]["distinct_id"] = "must-not-escape".into();
        assert!(validated_response(&options(), unknown.to_string().as_str()).is_err());

        let mut wrong_curve = response();
        wrong_curve["curve"][1]["retained_subjects"] = 3.into();
        assert!(validated_response(&options(), wrong_curve.to_string().as_str()).is_err());

        let mut wrong_maturity = response();
        wrong_maturity["cohorts"][1]["periods"][1]["all_subjects_eligible"] = true.into();
        assert!(validated_response(&options(), wrong_maturity.to_string().as_str()).is_err());
    }

    #[test]
    fn parses_canonical_utc_boundaries_and_rejects_invalid_dates() {
        let leap = parse_utc_timestamp("2024-02-29T23:59:59.123456789Z");
        let next = parse_utc_timestamp("2024-03-01T00:00:00Z");
        assert!(leap.is_some_and(|leap| next.is_some_and(|next| leap < next)));
        assert!(parse_utc_timestamp("2026-02-29T00:00:00Z").is_none());
        assert!(parse_utc_timestamp("2026-08-01T00:00:00+03:00").is_none());
    }

    #[test]
    fn accepts_typed_user_capture_targets_for_retention_gaps() {
        let mut missing_start = response();
        missing_start["coverage"]["usable_start_events"] = 0.into();
        missing_start["next_action"]["code"] = "identify_product_users".into();
        missing_start["next_action"]["target"] = "context.subject.kind=user".into();
        let missing_start: RetentionResponse =
            serde_json::from_value(missing_start).expect("start fixture deserializes");
        assert!(valid_next_action(&missing_start));

        let mut missing_return = response();
        missing_return["coverage"]["usable_return_events"] = 0.into();
        missing_return["next_action"]["code"] = "identify_returning_users".into();
        missing_return["next_action"]["target"] = "context.subject.kind=user".into();
        let missing_return: RetentionResponse =
            serde_json::from_value(missing_return).expect("return fixture deserializes");
        assert!(valid_next_action(&missing_return));
    }
}
