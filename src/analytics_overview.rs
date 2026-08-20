//! Versioned, bounded product-analytics overview reporting.

#![expect(
    clippy::missing_docs_in_private_items,
    reason = "response fields mirror the exact public analytics contract"
)]

use serde::Deserialize;

use crate::analytics_request::{self, Kind};
use crate::http::{nonempty_control_safe as bounded_contract_text, terminal_safe as display_text};
use crate::{AnalyticsOverviewOptions, CliEnvironment, RuntimeError};

/// Public response version implemented by this CLI.
const SCHEMA_VERSION: u8 = 2;
/// Subject-kind index required by response schema version 2.
const SUBJECT_INDEX_VERSION: u8 = 1;
/// Maximum accepted response body.
const RESPONSE_LIMIT: usize = 1024 * 1024;
/// Server-side scan cap also bounds every returned count.
const COUNT_LIMIT: u64 = 10_000_000;
/// Hard maximum non-empty time buckets.
const BUCKET_LIMIT: u64 = 500;
/// Hard maximum rows in each ranking.
const TOP_LIMIT: usize = 20;
/// Maximum material limitations accepted from either bounded API section.
const LIMITATION_LIMIT: usize = 12;

/// Builds the exact public GET path with explicit CLI defaults.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command model consumes this private-module helper"
)]
pub(super) fn request_path(options: &AnalyticsOverviewOptions) -> String {
    let top_limit = options.top_limit.to_string();
    crate::path_with_query(
        "/api/telemetry/analytics/overview",
        &[
            ("project_id", Some(options.project_id.as_str())),
            ("since", Some(options.since.as_str())),
            ("until", options.until.as_deref()),
            ("interval", Some(options.interval.as_str())),
            ("service_name", options.service_name.as_deref()),
            ("release", options.release.as_deref()),
            ("environment", options.environment.as_deref()),
            ("top_limit", Some(top_limit.as_str())),
            ("response_version", Some("2")),
        ],
    )
}

/// Executes one aggregate, identity-safe product-analytics overview request.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    options: &AnalyticsOverviewOptions,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let path = request_path(options);
    let body =
        analytics_request::send(env, path.as_str(), Kind::Overview, None, RESPONSE_LIMIT).await?;
    let response = validated_response(options, body.as_str())?;
    if json {
        writeln!(output, "{body}")?;
    } else {
        write!(output, "{}", render_response(&response))?;
    }
    Ok(())
}

/// Complete response with unknown fields rejected at every level.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OverviewResponse {
    schema_version: u8,
    query: OverviewQuery,
    purpose: String,
    analysis_level: AnalysisLevel,
    summary: ActionSummary,
    coverage: ActionCoverage,
    estimation: Estimation,
    series: Vec<ActionPoint>,
    top_actions: Vec<TopAction>,
    classified_activity: ClassifiedActivity,
    next_action: NextAction,
}

/// Normalized effective query echoed by the backend.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OverviewQuery {
    project_id: String,
    since: String,
    until: String,
    interval: String,
    interval_seconds: u64,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    top_limit: u8,
}

/// Strongest analysis supported by explicit identity and session coverage.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AnalysisLevel {
    /// No matching product activity.
    NoData,
    /// Event volume without explicit identified-user coverage.
    EventOnly,
    /// Identified-user coverage without explicit session coverage.
    UserLevel,
    /// Identified-user and session coverage.
    UserAndSession,
}

impl AnalysisLevel {
    /// Returns concise human wording.
    #[must_use]
    const fn human_label(self) -> &'static str {
        match self {
            Self::NoData => "no matching activity",
            Self::EventOnly => "event volume only",
            Self::UserLevel => "identified-user analysis",
            Self::UserAndSession => "identified-user and session analysis",
        }
    }
}

/// Aggregate action activity in the selected window.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionSummary {
    actions: u64,
    active_identified_users: u64,
    active_anonymous_subjects: u64,
    sessions: u64,
    distinct_action_names: u64,
    actions_per_identified_user: Option<f64>,
    actions_per_session: Option<f64>,
}

/// Capture and result coverage qualifying the action summary.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionCoverage {
    identified_actions: u64,
    anonymous_actions: u64,
    subject_coverage: SubjectCoverage,
    sessionized_actions: u64,
    traced_actions: u64,
    identification_rate: Option<f64>,
    sessionization_rate: Option<f64>,
    trace_link_rate: Option<f64>,
    expected_buckets: u64,
    returned_points: u64,
    returned_top_actions: u64,
    top_actions_truncated: bool,
    first_seen_at: Option<String>,
    last_seen_at: Option<String>,
    limitations: Vec<String>,
}

/// Exact exhaustive subject-kind receipt for one retained event population.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubjectCoverage {
    index_version: u8,
    identified_user_events: u64,
    anonymous_subject_events: u64,
    legacy_unknown_kind_events: u64,
    missing_subject_events: u64,
    historical_unindexed_events: u64,
}

/// Accuracy contract for approximate unique counts.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Estimation {
    unique_counts_are_approximate: bool,
    method: String,
    description: String,
}

/// One non-empty action bucket.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionPoint {
    bucket_start: String,
    bucket_end: String,
    actions: u64,
    active_identified_users: u64,
    sessions: u64,
    identified_actions: u64,
    sessionized_actions: u64,
    traced_actions: u64,
}

/// One highest-volume action name.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TopAction {
    name: String,
    actions: u64,
    active_identified_users: u64,
    sessions: u64,
    share_of_actions: f64,
}

/// Explicitly classified page, screen, and interaction activity.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifiedActivity {
    summary: ClassifiedSummary,
    coverage: ClassifiedCoverage,
    series: Vec<ClassifiedPoint>,
    top_surfaces: Vec<TopSurface>,
    top_events: Vec<TopEvent>,
}

/// Aggregate counts for the supported versioned analytics vocabulary.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifiedSummary {
    events: u64,
    page_views: u64,
    screen_views: u64,
    interactions: u64,
    distinct_surfaces: u64,
    distinct_event_names: u64,
    active_identified_users: u64,
    active_anonymous_subjects: u64,
    sessions: u64,
}

/// Capture and result coverage qualifying classified activity.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifiedCoverage {
    classified_actions: u64,
    unclassified_actions: u64,
    action_classification_rate: Option<f64>,
    surfaced_events: u64,
    named_events: u64,
    identified_events: u64,
    subject_coverage: SubjectCoverage,
    sessionized_events: u64,
    traced_events: u64,
    surface_rate: Option<f64>,
    event_name_rate: Option<f64>,
    identification_rate: Option<f64>,
    sessionization_rate: Option<f64>,
    trace_link_rate: Option<f64>,
    expected_buckets: u64,
    returned_points: u64,
    returned_top_surfaces: u64,
    returned_top_events: u64,
    top_surfaces_truncated: bool,
    top_events_truncated: bool,
    first_seen_at: Option<String>,
    last_seen_at: Option<String>,
    limitations: Vec<String>,
}

/// One non-empty classified-activity bucket.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifiedPoint {
    bucket_start: String,
    bucket_end: String,
    events: u64,
    page_views: u64,
    screen_views: u64,
    interactions: u64,
}

/// Supported version-1 classified event kind.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
enum ClassifiedKind {
    /// Browser route view.
    PageView,
    /// Application screen view.
    ScreenView,
    /// Explicit product interaction.
    Interaction,
}

impl ClassifiedKind {
    /// Returns the stable public token.
    #[must_use]
    const fn as_str(self) -> &'static str {
        match self {
            Self::PageView => "page_view",
            Self::ScreenView => "screen_view",
            Self::Interaction => "interaction",
        }
    }
}

/// One highest-volume classified surface.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TopSurface {
    kind: ClassifiedKind,
    surface: String,
    events: u64,
    active_identified_users: u64,
    sessions: u64,
    share_of_classified_events: f64,
}

/// One highest-volume exact classified event key.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TopEvent {
    kind: ClassifiedKind,
    event_name: String,
    events: u64,
    active_identified_users: u64,
    sessions: u64,
    share_of_classified_events: f64,
}

/// Stable recommended follow-up.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NextAction {
    code: String,
    target: String,
    reason: String,
}

/// Parses and proves the complete schema-version-2 response.
fn validated_response(
    options: &AnalyticsOverviewOptions,
    body: &str,
) -> Result<OverviewResponse, RuntimeError> {
    let response = serde_json::from_str::<OverviewResponse>(body)
        .map_err(|_error| Kind::Overview.invalid())?;
    if response.schema_version != SCHEMA_VERSION
        || !valid_query(options, &response.query)
        || !bounded_contract_text(response.purpose.as_str(), 2048)
        || !valid_action_activity(&response)
        || !valid_estimation(&response.estimation)
        || !valid_classified_activity(&response)
        || !valid_analysis_level(&response)
        || !valid_next_action(&response)
    {
        return Err(Kind::Overview.invalid());
    }
    Ok(response)
}

/// Requires the backend echo to match every exact client-selected scope field.
fn valid_query(options: &AnalyticsOverviewOptions, query: &OverviewQuery) -> bool {
    let interval_matches = interval_seconds(query.interval.as_str())
        .is_some_and(|seconds| seconds == query.interval_seconds)
        && (options.interval == "auto" || options.interval == query.interval);
    query.project_id == options.project_id
        && bounded_timestamp(query.since.as_str())
        && bounded_timestamp(query.until.as_str())
        && query.since != query.until
        && interval_matches
        && query.service_name == options.service_name
        && query.release == options.release
        && query.environment == options.environment
        && query.top_limit == options.top_limit
}

/// Maps one supported fixed interval token to seconds.
fn interval_seconds(interval: &str) -> Option<u64> {
    match interval {
        "1m" => Some(60),
        "5m" => Some(5 * 60),
        "15m" => Some(15 * 60),
        "1h" => Some(60 * 60),
        "6h" => Some(6 * 60 * 60),
        "1d" => Some(24 * 60 * 60),
        _ => None,
    }
}

/// Validates the UTC RFC 3339 shape emitted by the versioned API.
fn bounded_timestamp(value: &str) -> bool {
    crate::time::parse_utc_timestamp(value).is_some()
}

/// Proves action aggregates, coverage, buckets, and rankings together.
fn valid_action_activity(response: &OverviewResponse) -> bool {
    let summary = &response.summary;
    let coverage = &response.coverage;
    if !bounded_counts(&[
        summary.actions,
        summary.active_identified_users,
        summary.active_anonymous_subjects,
        summary.sessions,
        summary.distinct_action_names,
        coverage.identified_actions,
        coverage.anonymous_actions,
        coverage.sessionized_actions,
        coverage.traced_actions,
    ]) || !valid_subject_coverage(&coverage.subject_coverage, summary.actions)
        || coverage.identified_actions != coverage.subject_coverage.identified_user_events
        || summary.active_identified_users > coverage.subject_coverage.identified_user_events
        || summary.active_anonymous_subjects > coverage.subject_coverage.anonymous_subject_events
        || summary.sessions > coverage.sessionized_actions
        || summary.distinct_action_names > summary.actions
        || coverage.identified_actions > summary.actions
        || coverage.sessionized_actions > summary.actions
        || coverage.traced_actions > summary.actions
        || coverage.anonymous_actions != summary.actions - coverage.identified_actions
        || !ratio_matches(
            coverage.identification_rate,
            coverage.identified_actions,
            summary.actions,
        )
        || !ratio_matches(
            coverage.sessionization_rate,
            coverage.sessionized_actions,
            summary.actions,
        )
        || !ratio_matches(
            coverage.trace_link_rate,
            coverage.traced_actions,
            summary.actions,
        )
        || !quotient_matches(
            summary.actions_per_identified_user,
            coverage.identified_actions,
            summary.active_identified_users,
        )
        || !quotient_matches(
            summary.actions_per_session,
            coverage.sessionized_actions,
            summary.sessions,
        )
        || !valid_result_bounds(
            coverage.expected_buckets,
            coverage.returned_points,
            response.series.len(),
            coverage.returned_top_actions,
            response.top_actions.len(),
            response.query.top_limit,
            coverage.top_actions_truncated,
        )
        || !valid_presence(
            summary.actions,
            coverage.first_seen_at.as_deref(),
            coverage.last_seen_at.as_deref(),
        )
        || coverage.limitations.len() > LIMITATION_LIMIT
        || !coverage
            .limitations
            .iter()
            .all(|value| bounded_contract_text(value, 512))
    {
        return false;
    }
    valid_action_series(response) && valid_top_actions(response)
}

/// Proves the current index and exhaustive, overflow-safe subject-state partition.
fn valid_subject_coverage(coverage: &SubjectCoverage, total_events: u64) -> bool {
    if coverage.index_version != SUBJECT_INDEX_VERSION
        || !bounded_counts(&[
            coverage.identified_user_events,
            coverage.anonymous_subject_events,
            coverage.legacy_unknown_kind_events,
            coverage.missing_subject_events,
            coverage.historical_unindexed_events,
        ])
    {
        return false;
    }
    coverage
        .identified_user_events
        .checked_add(coverage.anonymous_subject_events)
        .and_then(|value| value.checked_add(coverage.legacy_unknown_kind_events))
        .and_then(|value| value.checked_add(coverage.missing_subject_events))
        .and_then(|value| value.checked_add(coverage.historical_unindexed_events))
        == Some(total_events)
}

/// Proves common bucket and top-ranking response bounds.
fn valid_result_bounds(
    expected_buckets: u64,
    returned_points: u64,
    point_count: usize,
    returned_top: u64,
    top_count: usize,
    requested_top: u8,
    top_truncated: bool,
) -> bool {
    expected_buckets > 0
        && expected_buckets <= BUCKET_LIMIT
        && returned_points <= expected_buckets
        && usize::try_from(returned_points).ok() == Some(point_count)
        && usize::try_from(returned_top).ok() == Some(top_count)
        && top_count <= usize::from(requested_top)
        && top_count <= TOP_LIMIT
        && (!top_truncated || top_count == usize::from(requested_top))
}

/// Requires first and last timestamps exactly when matching events exist.
fn valid_presence(total: u64, first: Option<&str>, last: Option<&str>) -> bool {
    match (total, first, last) {
        (0, None, None) => true,
        (1.., Some(first), Some(last)) => bounded_timestamp(first) && bounded_timestamp(last),
        _ => false,
    }
}

/// Proves non-empty ordered action buckets and their exact totals.
fn valid_action_series(response: &OverviewResponse) -> bool {
    if (response.summary.actions == 0) != response.series.is_empty() {
        return false;
    }
    let mut actions = 0_u64;
    let mut identified = 0_u64;
    let mut sessionized = 0_u64;
    let mut traced = 0_u64;
    let mut previous = None;
    for point in &response.series {
        if point.actions == 0
            || !bounded_counts(&[
                point.actions,
                point.active_identified_users,
                point.sessions,
                point.identified_actions,
                point.sessionized_actions,
                point.traced_actions,
            ])
            || point.identified_actions > point.actions
            || point.sessionized_actions > point.actions
            || point.traced_actions > point.actions
            || point.active_identified_users > point.identified_actions
            || point.sessions > point.sessionized_actions
            || !bounded_timestamp(point.bucket_start.as_str())
            || !bounded_timestamp(point.bucket_end.as_str())
            || point.bucket_start >= point.bucket_end
            || previous.is_some_and(|value: &str| value >= point.bucket_start.as_str())
        {
            return false;
        }
        previous = Some(point.bucket_start.as_str());
        let Some(next_actions) = actions.checked_add(point.actions) else {
            return false;
        };
        let Some(next_identified) = identified.checked_add(point.identified_actions) else {
            return false;
        };
        let Some(next_sessionized) = sessionized.checked_add(point.sessionized_actions) else {
            return false;
        };
        let Some(next_traced) = traced.checked_add(point.traced_actions) else {
            return false;
        };
        actions = next_actions;
        identified = next_identified;
        sessionized = next_sessionized;
        traced = next_traced;
    }
    actions == response.summary.actions
        && identified == response.coverage.identified_actions
        && sessionized == response.coverage.sessionized_actions
        && traced == response.coverage.traced_actions
}

/// Proves top-action names, order, uniqueness, shares, and bounded totals.
fn valid_top_actions(response: &OverviewResponse) -> bool {
    let mut previous = None;
    let mut returned_total = 0_u64;
    let mut names = std::collections::HashSet::new();
    for action in &response.top_actions {
        if !valid_name(action.name.as_str(), 256)
            || action.actions == 0
            || !bounded_counts(&[
                action.actions,
                action.active_identified_users,
                action.sessions,
            ])
            || action.actions > response.summary.actions
            || action.active_identified_users > action.actions
            || action.sessions > action.actions
            || previous.is_some_and(|count| action.actions > count)
            || !ratio_matches(
                Some(action.share_of_actions),
                action.actions,
                response.summary.actions,
            )
            || !names.insert(action.name.as_str())
        {
            return false;
        }
        previous = Some(action.actions);
        let Some(next) = returned_total.checked_add(action.actions) else {
            return false;
        };
        returned_total = next;
    }
    returned_total <= response.summary.actions
        && (response.summary.distinct_action_names == 0 || !response.top_actions.is_empty())
}

/// Proves the fixed approximate-cardinality disclosure.
fn valid_estimation(estimation: &Estimation) -> bool {
    estimation.unique_counts_are_approximate
        && estimation.method == "clickhouse_uniq_combined64"
        && bounded_contract_text(estimation.description.as_str(), 1024)
}

/// Proves classified aggregates, capture coverage, buckets, and rankings together.
fn valid_classified_activity(response: &OverviewResponse) -> bool {
    let activity = &response.classified_activity;
    let summary = &activity.summary;
    let coverage = &activity.coverage;
    let Some(kind_total) = summary
        .page_views
        .checked_add(summary.screen_views)
        .and_then(|value| value.checked_add(summary.interactions))
    else {
        return false;
    };
    let classified_actions = summary
        .screen_views
        .saturating_add(summary.interactions)
        .min(response.summary.actions);
    if !bounded_counts(&[
        summary.events,
        summary.page_views,
        summary.screen_views,
        summary.interactions,
        summary.distinct_surfaces,
        summary.distinct_event_names,
        summary.active_identified_users,
        summary.active_anonymous_subjects,
        summary.sessions,
        coverage.classified_actions,
        coverage.unclassified_actions,
        coverage.surfaced_events,
        coverage.named_events,
        coverage.identified_events,
        coverage.sessionized_events,
        coverage.traced_events,
    ]) || !valid_subject_coverage(&coverage.subject_coverage, summary.events)
        || coverage.identified_events != coverage.subject_coverage.identified_user_events
        || summary.active_identified_users > coverage.subject_coverage.identified_user_events
        || summary.active_anonymous_subjects > coverage.subject_coverage.anonymous_subject_events
        || summary.sessions > coverage.sessionized_events
        || summary.distinct_surfaces > summary.events
        || summary.distinct_event_names > summary.events
        || kind_total != summary.events
        || coverage.classified_actions != classified_actions
        || coverage.unclassified_actions != response.summary.actions - classified_actions
        || coverage.surfaced_events > summary.events
        || coverage.named_events > summary.events
        || coverage.identified_events > summary.events
        || coverage.sessionized_events > summary.events
        || coverage.traced_events > summary.events
        || !ratio_matches(
            coverage.action_classification_rate,
            classified_actions,
            response.summary.actions,
        )
        || !ratio_matches(
            coverage.surface_rate,
            coverage.surfaced_events,
            summary.events,
        )
        || !ratio_matches(
            coverage.event_name_rate,
            coverage.named_events,
            summary.events,
        )
        || !ratio_matches(
            coverage.identification_rate,
            coverage.identified_events,
            summary.events,
        )
        || !ratio_matches(
            coverage.sessionization_rate,
            coverage.sessionized_events,
            summary.events,
        )
        || !ratio_matches(
            coverage.trace_link_rate,
            coverage.traced_events,
            summary.events,
        )
        || coverage.expected_buckets != response.coverage.expected_buckets
        || !valid_classified_result_bounds(response)
        || !valid_presence(
            summary.events,
            coverage.first_seen_at.as_deref(),
            coverage.last_seen_at.as_deref(),
        )
        || coverage.limitations.is_empty()
        || coverage.limitations.len() > LIMITATION_LIMIT
        || !coverage
            .limitations
            .iter()
            .all(|value| bounded_contract_text(value, 512))
    {
        return false;
    }
    valid_classified_series(response) && valid_top_surfaces(response) && valid_top_events(response)
}

/// Proves classified point and ranking lengths against their coverage receipts.
fn valid_classified_result_bounds(response: &OverviewResponse) -> bool {
    let activity = &response.classified_activity;
    let coverage = &activity.coverage;
    valid_result_bounds(
        coverage.expected_buckets,
        coverage.returned_points,
        activity.series.len(),
        coverage.returned_top_surfaces,
        activity.top_surfaces.len(),
        response.query.top_limit,
        coverage.top_surfaces_truncated,
    ) && usize::try_from(coverage.returned_top_events).ok() == Some(activity.top_events.len())
        && activity.top_events.len() <= usize::from(response.query.top_limit)
        && activity.top_events.len() <= TOP_LIMIT
        && (!coverage.top_events_truncated
            || activity.top_events.len() == usize::from(response.query.top_limit))
}

/// Proves non-empty ordered classified buckets and exact kind totals.
fn valid_classified_series(response: &OverviewResponse) -> bool {
    let activity = &response.classified_activity;
    if (activity.summary.events == 0) != activity.series.is_empty() {
        return false;
    }
    let mut events = 0_u64;
    let mut page_views = 0_u64;
    let mut screen_views = 0_u64;
    let mut interactions = 0_u64;
    let mut previous = None;
    for point in &activity.series {
        let Some(point_total) = point
            .page_views
            .checked_add(point.screen_views)
            .and_then(|value| value.checked_add(point.interactions))
        else {
            return false;
        };
        if point.events == 0
            || point_total != point.events
            || !bounded_counts(&[
                point.events,
                point.page_views,
                point.screen_views,
                point.interactions,
            ])
            || !bounded_timestamp(point.bucket_start.as_str())
            || !bounded_timestamp(point.bucket_end.as_str())
            || point.bucket_start >= point.bucket_end
            || previous.is_some_and(|value: &str| value >= point.bucket_start.as_str())
        {
            return false;
        }
        previous = Some(point.bucket_start.as_str());
        let Some(next_events) = events.checked_add(point.events) else {
            return false;
        };
        let Some(next_page_views) = page_views.checked_add(point.page_views) else {
            return false;
        };
        let Some(next_screen_views) = screen_views.checked_add(point.screen_views) else {
            return false;
        };
        let Some(next_interactions) = interactions.checked_add(point.interactions) else {
            return false;
        };
        events = next_events;
        page_views = next_page_views;
        screen_views = next_screen_views;
        interactions = next_interactions;
    }
    events == activity.summary.events
        && page_views == activity.summary.page_views
        && screen_views == activity.summary.screen_views
        && interactions == activity.summary.interactions
}

/// Proves top surfaces, order, uniqueness, shares, and bounded totals.
fn valid_top_surfaces(response: &OverviewResponse) -> bool {
    let activity = &response.classified_activity;
    let mut previous = None;
    let mut total = 0_u64;
    let mut keys = std::collections::HashSet::new();
    for surface in &activity.top_surfaces {
        if !valid_name(surface.surface.as_str(), 256)
            || surface.events == 0
            || !bounded_counts(&[
                surface.events,
                surface.active_identified_users,
                surface.sessions,
            ])
            || surface.events > activity.summary.events
            || surface.active_identified_users > surface.events
            || surface.sessions > surface.events
            || previous.is_some_and(|count| surface.events > count)
            || !ratio_matches(
                Some(surface.share_of_classified_events),
                surface.events,
                activity.summary.events,
            )
            || !keys.insert((surface.kind, surface.surface.as_str()))
        {
            return false;
        }
        previous = Some(surface.events);
        let Some(next) = total.checked_add(surface.events) else {
            return false;
        };
        total = next;
    }
    total <= activity.summary.events
        && (activity.summary.distinct_surfaces == 0 || !activity.top_surfaces.is_empty())
}

/// Proves exact classified event keys, order, uniqueness, shares, and totals.
fn valid_top_events(response: &OverviewResponse) -> bool {
    let activity = &response.classified_activity;
    let mut previous = None;
    let mut total = 0_u64;
    let mut keys = std::collections::HashSet::new();
    for event in &activity.top_events {
        if !valid_event_name(event.kind, event.event_name.as_str())
            || event.events == 0
            || !bounded_counts(&[event.events, event.active_identified_users, event.sessions])
            || event.events > activity.summary.events
            || event.active_identified_users > event.events
            || event.sessions > event.events
            || previous.is_some_and(|count| event.events > count)
            || !ratio_matches(
                Some(event.share_of_classified_events),
                event.events,
                activity.summary.events,
            )
            || !keys.insert((event.kind, event.event_name.as_str()))
        {
            return false;
        }
        previous = Some(event.events);
        let Some(next) = total.checked_add(event.events) else {
            return false;
        };
        total = next;
    }
    total <= activity.summary.events
        && (activity.summary.distinct_event_names == 0 || !activity.top_events.is_empty())
}

/// Applies the version-1 event-name contract to one ranked classified key.
fn valid_event_name(kind: ClassifiedKind, value: &str) -> bool {
    valid_name(value, 256)
        && (kind != ClassifiedKind::Interaction
            || value.len() <= 64
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
                }))
}

/// Validates one bounded telemetry name without terminal control characters.
fn valid_name(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= limit
        && !value.chars().any(char::is_control)
}

/// Proves the declared analysis level from explicit aggregate coverage.
fn valid_analysis_level(response: &OverviewResponse) -> bool {
    let classified = &response.classified_activity.summary;
    let expected = if response.summary.actions == 0 && classified.events == 0 {
        AnalysisLevel::NoData
    } else if response.summary.active_identified_users == 0
        && classified.active_identified_users == 0
    {
        AnalysisLevel::EventOnly
    } else if response.summary.sessions == 0 && classified.sessions == 0 {
        AnalysisLevel::UserLevel
    } else {
        AnalysisLevel::UserAndSession
    };
    response.analysis_level == expected
}

/// Requires the stable action code and target implied by aggregate state.
fn valid_next_action(response: &OverviewResponse) -> bool {
    if !bounded_contract_text(response.next_action.reason.as_str(), 512) {
        return false;
    }
    let classified = &response.classified_activity.summary;
    let expected = if response.summary.actions == 0 && classified.events == 0 {
        ("capture_product_activity", "/api/telemetry/ingest")
    } else if classified.events == 0 {
        ("classify_product_activity", "analyticsSchemaVersion=1")
    } else if response.summary.active_identified_users == 0
        && classified.active_identified_users == 0
    {
        ("identify_product_users", "context.subject.kind=user")
    } else if response.summary.sessions == 0 && classified.sessions == 0 {
        ("sessionize_product_activity", "context.session.id")
    } else if classified.distinct_event_names >= 2 {
        ("build_product_funnel", "/api/telemetry/analytics/funnel")
    } else {
        ("capture_funnel_steps", "classified_activity.top_events")
    };
    response.next_action.code == expected.0 && response.next_action.target == expected.1
}

/// Returns whether every count stays inside the server's public scan bound.
fn bounded_counts(values: &[u64]) -> bool {
    values.iter().all(|value| *value <= COUNT_LIMIT)
}

/// Verifies one optional exact proportion.
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

/// Verifies one optional exact per-unit quotient, which can be above one.
fn quotient_matches(value: Option<f64>, numerator: u64, denominator: u64) -> bool {
    if denominator == 0 {
        return value.is_none();
    }
    let (Ok(numerator), Ok(denominator)) = (u32::try_from(numerator), u32::try_from(denominator))
    else {
        return false;
    };
    value.is_some_and(|value| {
        value.is_finite()
            && value >= 0.0
            && (value - f64::from(numerator) / f64::from(denominator)).abs() <= 1.0e-12
    })
}

/// Renders the useful human interpretation without reflecting backend prose.
fn render_response(response: &OverviewResponse) -> String {
    let classified = &response.classified_activity;
    let mut output = String::from("Product analytics overview\n");
    output.push_str(
        format!(
            "Window: {} to {}; interval {}\n",
            response.query.since, response.query.until, response.query.interval
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Analysis readiness: {}\n",
            response.analysis_level.human_label()
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Actions: {}; active identified users: {}; active anonymous subjects: {}; sessions: \
             {}; distinct names: {}\n",
            response.summary.actions,
            response.summary.active_identified_users,
            response.summary.active_anonymous_subjects,
            response.summary.sessions,
            response.summary.distinct_action_names,
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Classified events: {} (page views {}, screen views {}, interactions {}); surfaces: {}; exact events: {}\n",
            classified.summary.events,
            classified.summary.page_views,
            classified.summary.screen_views,
            classified.summary.interactions,
            classified.summary.distinct_surfaces,
            classified.summary.distinct_event_names,
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Classified subjects: active identified users {}; active anonymous subjects {}; \
             sessions {}\n",
            classified.summary.active_identified_users,
            classified.summary.active_anonymous_subjects,
            classified.summary.sessions,
        )
        .as_str(),
    );
    render_capture_receipts(response, &mut output);
    render_top_actions(response, &mut output);
    render_top_surfaces(response, &mut output);
    render_top_events(response, &mut output);
    render_capture_gaps(response, &mut output);
    output.push_str(
        "Accuracy: unique user, anonymous-subject, session, action-name, surface, and event-name \
         counts are approximate; event and coverage totals are exact.\n",
    );
    output.push_str("Next: ");
    output.push_str(next_step(response.next_action.code.as_str()));
    output.push('\n');
    output
}

/// Adds exact capture, subject, and bounded-series receipts.
fn render_capture_receipts(response: &OverviewResponse, output: &mut String) {
    let classified = &response.classified_activity;
    output.push_str(
        format!(
            "Action capture: typed-user eligible {}/{}; sessionized {}/{}; trace-linked {}/{}\n",
            response.coverage.identified_actions,
            response.summary.actions,
            response.coverage.sessionized_actions,
            response.summary.actions,
            response.coverage.traced_actions,
            response.summary.actions,
        )
        .as_str(),
    );
    render_subject_coverage(
        "Action subject coverage",
        &response.coverage.subject_coverage,
        output,
    );
    output.push_str(
        format!(
            "Classified capture: surfaced {}/{}; named {}/{}; typed-user eligible {}/{}; sessionized {}/{}; trace-linked {}/{}\n",
            classified.coverage.surfaced_events,
            classified.summary.events,
            classified.coverage.named_events,
            classified.summary.events,
            classified.coverage.identified_events,
            classified.summary.events,
            classified.coverage.sessionized_events,
            classified.summary.events,
            classified.coverage.traced_events,
            classified.summary.events,
        )
        .as_str(),
    );
    render_subject_coverage(
        "Classified subject coverage",
        &classified.coverage.subject_coverage,
        output,
    );
    output.push_str(
        format!(
            "Series coverage: action buckets {}/{}; classified buckets {}/{}\n",
            response.coverage.returned_points,
            response.coverage.expected_buckets,
            classified.coverage.returned_points,
            classified.coverage.expected_buckets,
        )
        .as_str(),
    );
}

/// Adds one exact subject-state receipt without returning opaque identifiers.
fn render_subject_coverage(label: &str, coverage: &SubjectCoverage, output: &mut String) {
    output.push_str(
        format!(
            "{label} (index v{}): user {}; anonymous {}; legacy kind unknown {}; missing {}; \
             historical unindexed {}\n",
            coverage.index_version,
            coverage.identified_user_events,
            coverage.anonymous_subject_events,
            coverage.legacy_unknown_kind_events,
            coverage.missing_subject_events,
            coverage.historical_unindexed_events,
        )
        .as_str(),
    );
}

/// Adds bounded top action names to human output.
fn render_top_actions(response: &OverviewResponse, output: &mut String) {
    output.push_str("Top actions:\n");
    if response.top_actions.is_empty() {
        output.push_str("  none\n");
        return;
    }
    for action in &response.top_actions {
        output.push_str(
            format!(
                "  {} — {} actions ({:.1}%); users {}; sessions {}\n",
                display_text(action.name.as_str()),
                action.actions,
                action.share_of_actions * 100.0,
                action.active_identified_users,
                action.sessions,
            )
            .as_str(),
        );
    }
}

/// Adds bounded classified surfaces to human output.
fn render_top_surfaces(response: &OverviewResponse, output: &mut String) {
    output.push_str("Top surfaces:\n");
    if response.classified_activity.top_surfaces.is_empty() {
        output.push_str("  none\n");
        return;
    }
    for surface in &response.classified_activity.top_surfaces {
        output.push_str(
            format!(
                "  {} {} — {} events ({:.1}%); users {}; sessions {}\n",
                surface.kind.as_str(),
                display_text(surface.surface.as_str()),
                surface.events,
                surface.share_of_classified_events * 100.0,
                surface.active_identified_users,
                surface.sessions,
            )
            .as_str(),
        );
    }
}

/// Adds exact classified event names that feed deeper analysis commands.
fn render_top_events(response: &OverviewResponse, output: &mut String) {
    output.push_str("Exact events for paths, funnels, retention, and lifecycle:\n");
    if response.classified_activity.top_events.is_empty() {
        output.push_str("  none\n");
        return;
    }
    for event in &response.classified_activity.top_events {
        output.push_str(
            format!(
                "  {} {} — {} events ({:.1}%); users {}; sessions {}\n",
                event.kind.as_str(),
                display_text(event.event_name.as_str()),
                event.events,
                event.share_of_classified_events * 100.0,
                event.active_identified_users,
                event.sessions,
            )
            .as_str(),
        );
    }
}

/// Discloses material capture and truncation gaps from validated counts only.
fn render_capture_gaps(response: &OverviewResponse, output: &mut String) {
    let classified = &response.classified_activity;
    render_subject_gaps("actions", &response.coverage.subject_coverage, output);
    render_subject_gaps(
        "classified events",
        &classified.coverage.subject_coverage,
        output,
    );
    if response.coverage.sessionized_actions < response.summary.actions {
        output.push_str(
            format!(
                "Capture gap: {} actions lacked an explicit session ID.\n",
                response.summary.actions - response.coverage.sessionized_actions
            )
            .as_str(),
        );
    }
    if response.coverage.traced_actions < response.summary.actions {
        output.push_str(
            format!(
                "Correlation gap: {} actions could not link to a trace.\n",
                response.summary.actions - response.coverage.traced_actions
            )
            .as_str(),
        );
    }
    if classified.coverage.unclassified_actions > 0 {
        output.push_str(
            format!(
                "Classification gap: {} actions were absent from version-1 screen-view or interaction breakdowns.\n",
                classified.coverage.unclassified_actions
            )
            .as_str(),
        );
    }
    if classified.coverage.named_events < classified.summary.events {
        output.push_str(
            format!(
                "Naming gap: {} classified events could not appear in exact-event rankings.\n",
                classified.summary.events - classified.coverage.named_events
            )
            .as_str(),
        );
    }
    if classified.coverage.traced_events < classified.summary.events {
        output.push_str(
            format!(
                "Correlation gap: {} classified events could not link to a trace.\n",
                classified.summary.events - classified.coverage.traced_events
            )
            .as_str(),
        );
    }
    if response.coverage.top_actions_truncated
        || classified.coverage.top_surfaces_truncated
        || classified.coverage.top_events_truncated
    {
        output.push_str("Limit: at least one lower-volume ranking was omitted by --top-limit.\n");
    }
}

/// Discloses legacy, missing, and pre-index subject populations without identities.
fn render_subject_gaps(label: &str, coverage: &SubjectCoverage, output: &mut String) {
    if coverage.legacy_unknown_kind_events > 0 {
        output.push_str(
            format!(
                "Identity gap: {} {label} had an opaque ID without a typed subject kind.\n",
                coverage.legacy_unknown_kind_events
            )
            .as_str(),
        );
    }
    if coverage.missing_subject_events > 0 {
        output.push_str(
            format!(
                "Identity gap: {} {label} lacked usable subject context.\n",
                coverage.missing_subject_events
            )
            .as_str(),
        );
    }
    if coverage.historical_unindexed_events > 0 {
        output.push_str(
            format!(
                "History gap: {} {label} predate subject-kind indexing.\n",
                coverage.historical_unindexed_events
            )
            .as_str(),
        );
    }
}

/// Maps validated stable action codes to local, value-free human guidance.
fn next_step(code: &str) -> &'static str {
    match code {
        "capture_product_activity" => {
            "capture bounded page views, screen views, or interactions, then retry"
        }
        "classify_product_activity" => {
            "use version-1 page-view, screen-view, or product-action SDK helpers, then retry"
        }
        "identify_product_users" => {
            "attach a stable opaque context.subject.id and set context.subject.kind=user, then \
             retry"
        }
        "sessionize_product_activity" => {
            "attach an opaque context.session.id to product events, then retry"
        }
        "build_product_funnel" => {
            "choose two exact events above and measure their ordered conversion with analytics funnel"
        }
        "capture_funnel_steps" => {
            "capture at least two stable exact product events before building a funnel"
        }
        _ => "retry the bounded analytics overview query",
    }
}
