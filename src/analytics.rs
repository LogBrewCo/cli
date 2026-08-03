//! Versioned, bounded product-analytics path exploration.

use serde::Deserialize;

use crate::auth::{AuthCredential, send_authenticated_with_refresh};
use crate::{
    AnalyticsPathDirection, AnalyticsPathEventKind, AnalyticsPathOptions, CliEnvironment,
    RuntimeError,
};

/// Public version implemented by this CLI.
const SCHEMA_VERSION: u8 = 1;
/// Maximum accepted response body.
const RESPONSE_LIMIT: usize = 1024 * 1024;
/// Storage contract's direction-nearest event cap.
const ORDERED_EVENT_CAP: u16 = 1024;
/// Server-side scan cap also bounds every returned count.
const COUNT_LIMIT: u64 = 10_000_000;
/// Maximum material limitations accepted from the bounded API.
const LIMITATION_LIMIT: usize = 8;

/// Builds the exact public POST body with explicit CLI defaults.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command model consumes this private-module helper"
)]
pub(super) fn request_body(options: &AnalyticsPathOptions) -> serde_json::Value {
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
        "direction".to_owned(),
        serde_json::Value::String(options.direction.as_str().to_owned()),
    ));
    drop(body.insert(
        "anchor".to_owned(),
        serde_json::json!({
            "kind": options.anchor_kind.as_str(),
            "event_name": options.anchor_event,
        }),
    ));
    drop(body.insert("depth".to_owned(), options.depth.into()));
    drop(body.insert(
        "collapse_repeated".to_owned(),
        options.collapse_repeated.into(),
    ));
    drop(body.insert("path_limit".to_owned(), options.path_limit.into()));
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

/// Executes one aggregate, identity-safe product-path request.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    options: &AnalyticsPathOptions,
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
    let url = format!("{origin}/api/telemetry/analytics/paths");
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
struct PathsResponse {
    schema_version: u8,
    query: PathQuery,
    purpose: String,
    summary: PathSummary,
    coverage: PathCoverage,
    paths: Vec<AggregatePath>,
    next_action: NextAction,
}

/// Normalized effective query echoed by the backend.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathQuery {
    project_id: String,
    since: String,
    until: String,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    direction: AnalyticsPathDirection,
    anchor: PathAnchor,
    depth: u8,
    collapse_repeated: bool,
    path_limit: u8,
}

/// Exact classified event anchoring each returned sequence.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathAnchor {
    kind: AnalyticsPathEventKind,
    event_name: String,
}

/// Headline aggregate coverage.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathSummary {
    anchored_sessions: u64,
    represented_sessions: u64,
    unrepresented_sessions: u64,
    returned_paths: u8,
    paths_truncated: bool,
}

/// Capture and query coverage qualifying the result.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathCoverage {
    classified_events: u64,
    named_events: u64,
    unnamed_events: u64,
    sessionized_events: u64,
    unsessionized_events: u64,
    anchor_events: u64,
    usable_anchor_events: u64,
    excluded_anchor_events: u64,
    event_name_rate: Option<f64>,
    sessionization_rate: Option<f64>,
    anchor_session_coverage_rate: Option<f64>,
    ordered_event_cap_per_session: u16,
    limitations: Vec<String>,
}

/// One highest-volume exact aggregate sequence.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AggregatePath {
    rank: u8,
    sessions: u64,
    share_of_anchored_sessions: f64,
    nodes: Vec<PathNode>,
}

/// One named event positioned relative to the anchor.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathNode {
    relative_position: i8,
    kind: AnalyticsPathEventKind,
    event_name: String,
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

/// Parses and proves the complete schema-version-1 response.
fn validated_response(
    options: &AnalyticsPathOptions,
    body: &str,
) -> Result<PathsResponse, RuntimeError> {
    let response =
        serde_json::from_str::<PathsResponse>(body).map_err(|_error| invalid_response())?;
    if !valid_query(options, &response.query)
        || response.schema_version != SCHEMA_VERSION
        || !bounded_contract_text(response.purpose.as_str(), 2048)
        || !valid_summary(&response)
        || !valid_coverage(&response.coverage)
        || response.summary.anchored_sessions > response.coverage.usable_anchor_events
        || !valid_paths(options, &response)
        || !valid_next_action(&response)
    {
        return Err(invalid_response());
    }
    Ok(response)
}

/// Requires the backend echo to match every exact client-selected scope field.
fn valid_query(options: &AnalyticsPathOptions, query: &PathQuery) -> bool {
    query.project_id == options.project_id
        && bounded_timestamp(query.since.as_str())
        && bounded_timestamp(query.until.as_str())
        && query.service_name == options.service_name
        && query.release == options.release
        && query.environment == options.environment
        && query.direction == options.direction
        && query.anchor.kind == options.anchor_kind
        && query.anchor.event_name == options.anchor_event
        && query.depth == options.depth
        && query.collapse_repeated == options.collapse_repeated
        && query.path_limit == options.path_limit
}

/// Validates the UTC RFC 3339 shape emitted by the versioned API.
fn bounded_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if !(20..=35).contains(&bytes.len())
        || !bytes.is_ascii()
        || !ascii_digits(bytes, 0, 4)
        || bytes.get(4) != Some(&b'-')
        || !ascii_digits(bytes, 5, 7)
        || bytes.get(7) != Some(&b'-')
        || !ascii_digits(bytes, 8, 10)
        || bytes.get(10) != Some(&b'T')
        || !ascii_digits(bytes, 11, 13)
        || bytes.get(13) != Some(&b':')
        || !ascii_digits(bytes, 14, 16)
        || bytes.get(16) != Some(&b':')
        || !ascii_digits(bytes, 17, 19)
        || !bounded_pair(bytes, 5, 1, 12)
        || !bounded_pair(bytes, 8, 1, 31)
        || !bounded_pair(bytes, 11, 0, 23)
        || !bounded_pair(bytes, 14, 0, 59)
        || !bounded_pair(bytes, 17, 0, 59)
    {
        return false;
    }
    let mut suffix = 19;
    if bytes.get(suffix) == Some(&b'.') {
        suffix += 1;
        let fraction_start = suffix;
        while bytes.get(suffix).is_some_and(u8::is_ascii_digit) {
            suffix += 1;
        }
        if suffix == fraction_start {
            return false;
        }
    }
    match bytes.get(suffix..) {
        Some([b'Z']) => true,
        Some(
            [
                sign @ (b'+' | b'-'),
                hour_a,
                hour_b,
                b':',
                minute_a,
                minute_b,
            ],
        ) => {
            let offset = [*hour_a, *hour_b, *minute_a, *minute_b];
            matches!(sign, b'+' | b'-')
                && offset.iter().all(u8::is_ascii_digit)
                && (*hour_a - b'0') * 10 + (*hour_b - b'0') <= 23
                && (*minute_a - b'0') * 10 + (*minute_b - b'0') <= 59
        }
        _ => false,
    }
}

/// Returns whether one half-open byte range contains only ASCII digits.
fn ascii_digits(bytes: &[u8], start: usize, end: usize) -> bool {
    bytes
        .get(start..end)
        .is_some_and(|digits| digits.iter().all(u8::is_ascii_digit))
}

/// Parses one two-digit timestamp field and applies inclusive bounds.
fn bounded_pair(bytes: &[u8], start: usize, minimum: u8, maximum: u8) -> bool {
    let Some([left, right]) = bytes.get(start..start.saturating_add(2)) else {
        return false;
    };
    let value = (*left - b'0') * 10 + (*right - b'0');
    (minimum..=maximum).contains(&value)
}

/// Proves headline counts against the returned path aggregates.
fn valid_summary(response: &PathsResponse) -> bool {
    let summary = &response.summary;
    if !bounded_counts(&[
        summary.anchored_sessions,
        summary.represented_sessions,
        summary.unrepresented_sessions,
    ]) || usize::from(summary.returned_paths) != response.paths.len()
        || summary.returned_paths > response.query.path_limit
    {
        return false;
    }
    summary
        .represented_sessions
        .checked_add(summary.unrepresented_sessions)
        .is_some_and(|total| total == summary.anchored_sessions)
        && summary.paths_truncated == (summary.unrepresented_sessions > 0)
}

/// Proves every derived coverage count and ratio.
fn valid_coverage(coverage: &PathCoverage) -> bool {
    if !bounded_counts(&[
        coverage.classified_events,
        coverage.named_events,
        coverage.unnamed_events,
        coverage.sessionized_events,
        coverage.unsessionized_events,
        coverage.anchor_events,
        coverage.usable_anchor_events,
        coverage.excluded_anchor_events,
    ]) || coverage.named_events > coverage.classified_events
        || coverage.sessionized_events > coverage.classified_events
        || coverage.anchor_events > coverage.named_events
        || coverage.usable_anchor_events > coverage.anchor_events
        || coverage.usable_anchor_events > coverage.sessionized_events
        || coverage.unnamed_events != coverage.classified_events - coverage.named_events
        || coverage.unsessionized_events != coverage.classified_events - coverage.sessionized_events
        || coverage.excluded_anchor_events != coverage.anchor_events - coverage.usable_anchor_events
        || coverage.ordered_event_cap_per_session != ORDERED_EVENT_CAP
        || coverage.limitations.is_empty()
        || coverage.limitations.len() > LIMITATION_LIMIT
        || !coverage
            .limitations
            .iter()
            .all(|value| bounded_contract_text(value, 512))
    {
        return false;
    }
    ratio_matches(
        coverage.event_name_rate,
        coverage.named_events,
        coverage.classified_events,
    ) && ratio_matches(
        coverage.sessionization_rate,
        coverage.sessionized_events,
        coverage.classified_events,
    ) && ratio_matches(
        coverage.anchor_session_coverage_rate,
        coverage.usable_anchor_events,
        coverage.anchor_events,
    )
}

/// Proves ordering, anchors, shares, ranking, and aggregate totals.
fn valid_paths(options: &AnalyticsPathOptions, response: &PathsResponse) -> bool {
    let mut represented = 0_u64;
    let mut previous_sessions = None;
    let mut signatures = std::collections::HashSet::new();
    for (index, path) in response.paths.iter().enumerate() {
        if path.rank != u8::try_from(index.saturating_add(1)).unwrap_or(u8::MAX)
            || path.sessions == 0
            || path.sessions > COUNT_LIMIT
            || path.nodes.is_empty()
            || path.nodes.len() > usize::from(options.depth.saturating_add(1))
            || !ratio_matches(
                Some(path.share_of_anchored_sessions),
                path.sessions,
                response.summary.anchored_sessions,
            )
            || previous_sessions.is_some_and(|previous| path.sessions > previous)
            || !valid_nodes(options, path.nodes.as_slice())
        {
            return false;
        }
        previous_sessions = Some(path.sessions);
        let signature = path
            .nodes
            .iter()
            .map(|node| format!("{}\u{1f}{}", node.kind.as_str(), node.event_name))
            .collect::<Vec<_>>()
            .join("\u{1e}");
        if !signatures.insert(signature) {
            return false;
        }
        let Some(total) = represented.checked_add(path.sessions) else {
            return false;
        };
        represented = total;
    }
    represented == response.summary.represented_sessions
}

/// Proves one chronological path's relative positions and exact anchor.
fn valid_nodes(options: &AnalyticsPathOptions, nodes: &[PathNode]) -> bool {
    let last = i8::try_from(nodes.len().saturating_sub(1)).unwrap_or(i8::MAX);
    for (index, node) in nodes.iter().enumerate() {
        let index = i8::try_from(index).unwrap_or(i8::MAX);
        let expected = match options.direction {
            AnalyticsPathDirection::Following => index,
            AnalyticsPathDirection::Preceding => index.saturating_sub(last),
        };
        if node.relative_position != expected
            || !valid_event_name(node.kind, node.event_name.as_str())
        {
            return false;
        }
    }
    if options.collapse_repeated
        && nodes
            .windows(2)
            .any(|pair| pair[0].kind == pair[1].kind && pair[0].event_name == pair[1].event_name)
    {
        return false;
    }
    let anchor = match options.direction {
        AnalyticsPathDirection::Following => nodes.first(),
        AnalyticsPathDirection::Preceding => nodes.last(),
    };
    anchor.is_some_and(|anchor| {
        anchor.relative_position == 0
            && anchor.kind == options.anchor_kind
            && anchor.event_name == options.anchor_event
    })
}

/// Applies the same version-1 public name contract to every returned node.
fn valid_event_name(kind: AnalyticsPathEventKind, value: &str) -> bool {
    if value.is_empty() || value.chars().count() > 256 || value.chars().any(char::is_control) {
        return false;
    }
    kind != AnalyticsPathEventKind::Interaction
        || value.len() <= 64
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
            })
}

/// Requires the stable action code and target implied by the response state.
fn valid_next_action(response: &PathsResponse) -> bool {
    if !bounded_contract_text(response.next_action.reason.as_str(), 512) {
        return false;
    }
    let expected = if response.coverage.classified_events == 0 {
        ("capture_product_activity", "analyticsSchemaVersion=1")
    } else if response.coverage.anchor_events == 0 {
        (
            "choose_captured_path_anchor",
            "/api/telemetry/analytics/overview",
        )
    } else if response.coverage.usable_anchor_events == 0 {
        ("sessionize_product_activity", "context.session.id")
    } else if response.summary.returned_paths == 0 {
        (
            "narrow_or_move_path_anchor",
            "/api/telemetry/analytics/paths",
        )
    } else if response.summary.paths_truncated {
        (
            "measure_top_path_as_funnel",
            "/api/telemetry/analytics/funnel",
        )
    } else {
        ("compare_path_contexts", "/api/telemetry/analytics/paths")
    };
    response.next_action.code == expected.0 && response.next_action.target == expected.1
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

/// Renders the useful human interpretation without reflecting backend prose.
fn render_response(response: &PathsResponse) -> String {
    let mut output = String::new();
    output.push_str("Product paths ");
    output.push_str(response.query.direction.as_str());
    output.push_str(" from ");
    output.push_str(response.query.anchor.kind.as_str());
    output.push(' ');
    output.push_str(display_text(response.query.anchor.event_name.as_str()).as_str());
    output.push('\n');
    output.push_str(
        format!(
            "Window: {} to {}\n",
            response.query.since, response.query.until
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Anchored sessions: {}; represented: {}; unrepresented: {}\n",
            response.summary.anchored_sessions,
            response.summary.represented_sessions,
            response.summary.unrepresented_sessions,
        )
        .as_str(),
    );

    if response.paths.is_empty() {
        output.push_str("No usable aggregate path was returned.\n");
    } else {
        for path in &response.paths {
            let nodes = path
                .nodes
                .iter()
                .map(|node| {
                    let position = if node.relative_position > 0 {
                        format!("+{}", node.relative_position)
                    } else {
                        node.relative_position.to_string()
                    };
                    format!(
                        "[{position}] {} {}",
                        node.kind.as_str(),
                        display_text(node.event_name.as_str())
                    )
                })
                .collect::<Vec<_>>()
                .join(" -> ");
            output.push_str(
                format!(
                    "{}. {} sessions ({:.1}%): {nodes}\n",
                    path.rank,
                    path.sessions,
                    path.share_of_anchored_sessions * 100.0,
                )
                .as_str(),
            );
        }
    }

    output.push_str(
        format!(
            "Coverage: named {}/{}; sessionized {}/{}; usable anchors {}/{}\n",
            response.coverage.named_events,
            response.coverage.classified_events,
            response.coverage.sessionized_events,
            response.coverage.classified_events,
            response.coverage.usable_anchor_events,
            response.coverage.anchor_events,
        )
        .as_str(),
    );
    if response.coverage.unsessionized_events > 0 {
        output.push_str(
            format!(
                "Capture gap: {} classified events lacked an explicit session ID.\n",
                response.coverage.unsessionized_events
            )
            .as_str(),
        );
    }
    if response.summary.paths_truncated {
        output
            .push_str("Limit: lower-volume or per-session-capped journeys are not represented.\n");
    }
    output.push_str("Next: ");
    output.push_str(next_step(response.next_action.code.as_str()));
    output.push('\n');
    output
}

/// Maps validated stable action codes to local, value-free human guidance.
fn next_step(code: &str) -> &'static str {
    match code {
        "capture_product_activity" => {
            "capture version-1 page views, screen views, or interactions, then retry"
        }
        "choose_captured_path_anchor" => {
            "choose an exact event shown in Product Analytics overview, then retry"
        }
        "sessionize_product_activity" => "attach context.session.id to product events, then retry",
        "narrow_or_move_path_anchor" => {
            "narrow the time/context scope or move the anchor closer to retained activity"
        }
        "measure_top_path_as_funnel" => {
            "measure the most material returned sequence as an exact funnel"
        }
        "compare_path_contexts" => "repeat this anchor across releases, environments, or services",
        _ => "retry the bounded analytics path query",
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

/// Converts transport and refresh failures into fixed path-free recovery.
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
        | RuntimeError::AnalyticsResponseInvalid
        | RuntimeError::AnalyticsRetentionResponseInvalid
        | RuntimeError::NativeDebugArtifactInvalid
        | RuntimeError::NativeDebugResponseInvalid
        | RuntimeError::NativeDebugVerificationFailed => transport_error(),
    }
}

/// Returns one fixed path-free transport failure.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "analytics path request could not be completed",
        next: "check network connectivity and retry the same analytics path query",
    }
}

/// Returns one fixed response-contract failure.
const fn invalid_response() -> RuntimeError {
    RuntimeError::AnalyticsResponseInvalid
}

/// Converts a failed HTTP status into fixed guidance without reflecting its body.
fn safe_api_error(status: u16, credential: &AuthCredential) -> RuntimeError {
    let (error, code, next) = match status {
        400 | 422 => (
            "analytics path request rejected",
            "validation_failed",
            "check the exact project, time scope, direction, anchor, depth, and path limit",
        ),
        401 => (
            "authentication required",
            "unauthorized",
            "run logbrew login",
        ),
        403 => (
            "analytics path request forbidden",
            "forbidden",
            "confirm account access and retry the same analytics path query",
        ),
        404 => (
            "analytics path resource not found",
            "not_found",
            "check the project and retry the same analytics path query",
        ),
        405 => (
            "analytics path method is not supported",
            "method_not_allowed",
            "use the POST-backed logbrew analytics paths command",
        ),
        429 => (
            "analytics path request rate limited",
            "rate_limited",
            "retry the same analytics path query later",
        ),
        500..=599 => (
            "analytics path service unavailable",
            "service_unavailable",
            "retry the same analytics path query later",
        ),
        _ => (
            "analytics path request failed",
            "request_failed",
            "check account access and retry the same analytics path query",
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

    fn options(direction: AnalyticsPathDirection) -> AnalyticsPathOptions {
        AnalyticsPathOptions {
            project_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned(),
            since: "24h".to_owned(),
            until: None,
            service_name: None,
            release: None,
            environment: Some("production".to_owned()),
            direction,
            anchor_kind: AnalyticsPathEventKind::PageView,
            anchor_event: "/pricing".to_owned(),
            depth: 4,
            collapse_repeated: true,
            path_limit: 10,
        }
    }

    fn response(direction: &str, nodes: serde_json::Value) -> String {
        serde_json::json!({
            "schema_version": 1,
            "query": {
                "project_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "since": "2026-08-02T00:00:00Z",
                "until": "2026-08-03T00:00:00Z",
                "service_name": null,
                "release": null,
                "environment": "production",
                "direction": direction,
                "anchor": {"kind": "page_view", "event_name": "/pricing"},
                "depth": 4,
                "collapse_repeated": true,
                "path_limit": 10
            },
            "purpose": "Aggregate paths around one exact event.",
            "summary": {
                "anchored_sessions": 20,
                "represented_sessions": 12,
                "unrepresented_sessions": 8,
                "returned_paths": 1,
                "paths_truncated": true
            },
            "coverage": {
                "classified_events": 100,
                "named_events": 90,
                "unnamed_events": 10,
                "sessionized_events": 80,
                "unsessionized_events": 20,
                "anchor_events": 30,
                "usable_anchor_events": 24,
                "excluded_anchor_events": 6,
                "event_name_rate": 0.9,
                "sessionization_rate": 0.8,
                "anchor_session_coverage_rate": 0.8,
                "ordered_event_cap_per_session": 1024,
                "limitations": ["Only classified events are included."]
            },
            "paths": [{
                "rank": 1,
                "sessions": 12,
                "share_of_anchored_sessions": 0.6,
                "nodes": nodes
            }],
            "next_action": {
                "code": "measure_top_path_as_funnel",
                "target": "/api/telemetry/analytics/funnel",
                "reason": "Measure the top path as a funnel."
            }
        })
        .to_string()
    }

    #[test]
    fn validates_and_renders_following_paths() {
        let body = response(
            "following",
            serde_json::json!([
                {"relative_position": 0, "kind": "page_view", "event_name": "/pricing"},
                {"relative_position": 1, "kind": "interaction", "event_name": "signup_started"}
            ]),
        );
        let response = validated_response(&options(AnalyticsPathDirection::Following), &body)
            .expect("valid response");
        let rendered = render_response(&response);
        assert!(rendered.contains("[0] page_view /pricing -> [+1] interaction signup_started"));
        assert!(rendered.contains("Capture gap: 20"));
        assert!(!rendered.contains(response.next_action.reason.as_str()));
    }

    #[test]
    fn rejects_wrong_positions_counts_actions_and_unknown_fields() {
        let options = options(AnalyticsPathDirection::Following);
        let valid_nodes = serde_json::json!([
            {"relative_position": 0, "kind": "page_view", "event_name": "/pricing"}
        ]);
        let wrong_position = response(
            "following",
            serde_json::json!([
                {"relative_position": 1, "kind": "page_view", "event_name": "/pricing"}
            ]),
        );
        assert!(validated_response(&options, &wrong_position).is_err());

        let mut wrong_count: serde_json::Value =
            serde_json::from_str(&response("following", valid_nodes.clone())).expect("json");
        wrong_count["summary"]["represented_sessions"] = 11.into();
        assert!(validated_response(&options, &wrong_count.to_string()).is_err());

        let mut wrong_action: serde_json::Value =
            serde_json::from_str(&response("following", valid_nodes.clone())).expect("json");
        wrong_action["next_action"]["target"] = "/unexpected".into();
        assert!(validated_response(&options, &wrong_action.to_string()).is_err());

        let mut unknown: serde_json::Value =
            serde_json::from_str(&response("following", valid_nodes)).expect("json");
        unknown["query"]["session_id"] = "secret".into();
        assert!(validated_response(&options, &unknown.to_string()).is_err());
    }

    #[test]
    fn request_body_omits_absent_context_and_sends_explicit_defaults() {
        let body = request_body(&options(AnalyticsPathDirection::Preceding));
        assert_eq!(body["direction"], "preceding");
        assert_eq!(body["depth"], 4);
        assert_eq!(body["collapse_repeated"], true);
        assert_eq!(body["path_limit"], 10);
        assert!(body.get("service_name").is_none());
        assert!(body.get("until").is_none());
    }
}
