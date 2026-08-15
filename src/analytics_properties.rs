//! Versioned, bounded product-analytics property catalog reporting.

use serde::Deserialize;

use crate::auth::{AuthCredential, send_authenticated_with_refresh};
use crate::{AnalyticsPropertyOptions, CliEnvironment, RuntimeError};

/// Public response version implemented by this CLI.
const SCHEMA_VERSION: u8 = 1;
/// Maximum accepted response body.
const RESPONSE_LIMIT: usize = 1024 * 1024;
/// Server-side classified-event scan cap.
const COUNT_LIMIT: u64 = 10_000_000;
/// Conservative bound for approximate cross-event cardinalities.
const CARDINALITY_LIMIT: u64 = 500_000_000;
/// Conservative bound for one approximate per-key value cardinality.
const DISTINCT_VALUE_LIMIT: u64 = 20_000_000;
/// Hard maximum returned property descriptors.
const PROPERTY_LIMIT: usize = 50;
/// Maximum material limitations accepted from the bounded API.
const LIMITATION_LIMIT: usize = 8;

/// Builds the exact public GET path with explicit CLI defaults.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command model consumes this private-module helper"
)]
pub(super) fn request_path(options: &AnalyticsPropertyOptions) -> String {
    let limit = options.limit.to_string();
    crate::path_with_query(
        "/api/telemetry/analytics/properties",
        &[
            ("project_id", Some(options.project_id.as_str())),
            ("since", Some(options.since.as_str())),
            ("until", options.until.as_deref()),
            ("service_name", options.service_name.as_deref()),
            ("release", options.release.as_deref()),
            ("environment", options.environment.as_deref()),
            ("limit", Some(limit.as_str())),
        ],
    )
}

/// Executes one aggregate, value-free analytics property catalog request.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    options: &AnalyticsPropertyOptions,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let origin =
        crate::http::normalized_origin(env.base_url.as_str()).ok_or_else(transport_error)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_error| transport_error())?;
    let url = format!("{origin}{}", request_path(options));
    let response = send_authenticated_with_refresh(&client, env, |client, credential| {
        client.get(url.as_str()).bearer_auth(credential.token())
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
struct PropertyCatalogResponse {
    schema_version: u8,
    query: PropertyQuery,
    purpose: String,
    summary: PropertySummary,
    coverage: PropertyCoverage,
    estimation: PropertyEstimation,
    properties: Vec<PropertyDescriptor>,
    next_action: NextAction,
}

/// Normalized effective scope echoed by the backend.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyQuery {
    project_id: String,
    since: String,
    until: String,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    limit: u8,
}

/// Aggregate property availability in the selected window.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertySummary {
    classified_events: u64,
    available_property_keys: u64,
    returned_properties: u8,
}

/// Capture, migration, privacy, and result-bound receipts.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyCoverage {
    indexed_events: u64,
    unindexed_events: u64,
    complete_index_events: u64,
    incomplete_index_events: u64,
    privacy_filtered_events: u64,
    events_with_properties: u64,
    index_coverage_rate: Option<f64>,
    property_capture_rate: Option<f64>,
    properties_truncated: bool,
    values_returned: bool,
    limitations: Vec<String>,
}

/// Stable source class for one returned property.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PropertySource {
    /// Typed runtime, framework, OS, device, or application context.
    StandardContext,
    /// Non-sensitive custom tag.
    CustomTag,
}

impl PropertySource {
    /// Returns concise human wording.
    const fn human_label(self) -> &'static str {
        match self {
            Self::StandardContext => "standard context",
            Self::CustomTag => "custom tag",
        }
    }
}

/// Supported version-1 property scalar type.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PropertyValueType {
    /// Exact bounded UTF-8 string.
    String,
}

/// One key-only descriptor and aggregate availability receipt.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyDescriptor {
    key: String,
    source: PropertySource,
    value_type: PropertyValueType,
    events: u64,
    coverage_rate: Option<f64>,
    distinct_values: u64,
}

/// Accuracy class for distinct key and value cardinality.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CountAccuracy {
    /// Bounded approximate cardinality estimation.
    Approximate,
}

/// Cardinality-estimation contract.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyEstimation {
    count_accuracy: CountAccuracy,
    method: String,
    description: String,
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
    options: &AnalyticsPropertyOptions,
    body: &str,
) -> Result<PropertyCatalogResponse, RuntimeError> {
    let response = serde_json::from_str::<PropertyCatalogResponse>(body)
        .map_err(|_error| invalid_response())?;
    if response.schema_version != SCHEMA_VERSION
        || !valid_query(options, &response.query)
        || !bounded_contract_text(response.purpose.as_str(), 4096)
        || !valid_summary_and_coverage(&response)
        || !valid_properties(&response)
        || !valid_estimation(&response.estimation)
        || !valid_next_action(&response)
    {
        return Err(invalid_response());
    }
    Ok(response)
}

/// Requires the backend echo to match every exact client-selected scope field.
fn valid_query(options: &AnalyticsPropertyOptions, query: &PropertyQuery) -> bool {
    query.project_id == options.project_id
        && bounded_timestamp(query.since.as_str())
        && bounded_timestamp(query.until.as_str())
        && query.since < query.until
        && query.service_name == options.service_name
        && query.release == options.release
        && query.environment == options.environment
        && query.limit == options.limit
        && [
            query.service_name.as_deref(),
            query.release.as_deref(),
            query.environment.as_deref(),
        ]
        .into_iter()
        .flatten()
        .all(|value| bounded_contract_text(value, 256))
}

/// Proves every aggregate counter, equation, rate, and privacy invariant.
fn valid_summary_and_coverage(response: &PropertyCatalogResponse) -> bool {
    let summary = &response.summary;
    let coverage = &response.coverage;
    bounded_counts(&[
        summary.classified_events,
        coverage.indexed_events,
        coverage.unindexed_events,
        coverage.complete_index_events,
        coverage.incomplete_index_events,
        coverage.privacy_filtered_events,
        coverage.events_with_properties,
    ]) && summary.available_property_keys <= CARDINALITY_LIMIT
        && usize::from(summary.returned_properties) == response.properties.len()
        && response.properties.len() <= usize::from(response.query.limit)
        && response.properties.len() <= PROPERTY_LIMIT
        && coverage.indexed_events <= summary.classified_events
        && coverage.unindexed_events == summary.classified_events - coverage.indexed_events
        && coverage.complete_index_events <= coverage.indexed_events
        && coverage.incomplete_index_events
            == coverage.indexed_events - coverage.complete_index_events
        && coverage.privacy_filtered_events <= coverage.indexed_events
        && coverage.events_with_properties <= coverage.indexed_events
        && ratio_matches(
            coverage.index_coverage_rate,
            coverage.indexed_events,
            summary.classified_events,
        )
        && ratio_matches(
            coverage.property_capture_rate,
            coverage.events_with_properties,
            summary.classified_events,
        )
        && !coverage.values_returned
        && (!coverage.properties_truncated
            || response.properties.len() == usize::from(response.query.limit))
        && (2..=LIMITATION_LIMIT).contains(&coverage.limitations.len())
        && coverage
            .limitations
            .iter()
            .all(|value| bounded_contract_text(value, 1024))
        && (coverage.events_with_properties == 0) == response.properties.is_empty()
        && (response.properties.is_empty() || summary.available_property_keys > 0)
}

/// Proves key safety, source type, order, uniqueness, and availability rates.
fn valid_properties(response: &PropertyCatalogResponse) -> bool {
    let mut keys = std::collections::HashSet::with_capacity(response.properties.len());
    let mut previous: Option<&PropertyDescriptor> = None;
    response.properties.iter().all(|property| {
        let expected_source = if property.key.starts_with("tag.") {
            PropertySource::CustomTag
        } else {
            PropertySource::StandardContext
        };
        let ordered = previous.is_none_or(|previous| {
            property.events < previous.events
                || property.events == previous.events && property.key > previous.key
        });
        let valid = crate::analytics_property_contract::is_safe_key(property.key.as_str())
            && property.source == expected_source
            && property.value_type == PropertyValueType::String
            && property.events > 0
            && property.events <= response.summary.classified_events
            && property.distinct_values > 0
            && property.distinct_values <= DISTINCT_VALUE_LIMIT
            && ratio_matches(
                property.coverage_rate,
                property.events,
                response.summary.classified_events,
            )
            && ordered
            && keys.insert(property.key.as_str());
        previous = Some(property);
        valid
    })
}

/// Proves the fixed approximate-cardinality disclosure.
fn valid_estimation(estimation: &PropertyEstimation) -> bool {
    estimation.count_accuracy == CountAccuracy::Approximate
        && estimation.method == "clickhouse_uniq_combined64"
        && bounded_contract_text(estimation.description.as_str(), 2048)
}

/// Requires the stable action code and target implied by aggregate state.
fn valid_next_action(response: &PropertyCatalogResponse) -> bool {
    if !bounded_contract_text(response.next_action.reason.as_str(), 768) {
        return false;
    }
    let expected = if response.summary.classified_events == 0 {
        ("capture_product_activity", "analyticsSchemaVersion=1")
    } else if response.coverage.indexed_events == 0 {
        ("capture_current_property_index", "context.schemaVersion=1")
    } else if response.coverage.events_with_properties == 0 {
        ("add_product_properties", "context.resource or context.tags")
    } else if response.coverage.properties_truncated {
        (
            "narrow_property_scope",
            "/api/telemetry/analytics/properties",
        )
    } else {
        (
            "compare_property_segments",
            "/api/telemetry/analytics/segments/compare",
        )
    };
    response.next_action.code == expected.0 && response.next_action.target == expected.1
}

/// Returns whether every exact event counter stays inside the public scan bound.
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
        value.is_finite()
            && (0.0..=1.0).contains(&value)
            && (value - expected).abs() <= 1.0e-12 * expected.abs().max(1.0)
    })
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
    matches!(bytes.get(suffix..), Some([b'Z']))
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

/// Validates one backend-authored, non-telemetry contract string.
fn bounded_contract_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= limit
        && !value.chars().any(char::is_control)
}

/// Renders a progressive key-only catalog without reflecting backend prose.
fn render_response(response: &PropertyCatalogResponse) -> String {
    let mut output = String::new();
    output.push_str("Product analytics properties\n");
    output.push_str(
        format!(
            "Window: {} to {}; service={}; release={}; environment={}\n",
            response.query.since,
            response.query.until,
            filter_label(response.query.service_name.as_deref()),
            filter_label(response.query.release.as_deref()),
            filter_label(response.query.environment.as_deref()),
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Index coverage: {}/{} classified events ({}) | complete {}/{} | properties on {} events ({})\n",
            response.coverage.indexed_events,
            response.summary.classified_events,
            percentage(response.coverage.index_coverage_rate),
            response.coverage.complete_index_events,
            response.coverage.indexed_events,
            response.coverage.events_with_properties,
            percentage(response.coverage.property_capture_rate),
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Privacy and migration: {} unindexed; {} incomplete; {} privacy-filtered. Values and identities are not returned.\n",
            response.coverage.unindexed_events,
            response.coverage.incomplete_index_events,
            response.coverage.privacy_filtered_events,
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Properties: {} returned; approximately {} available{}\n",
            response.summary.returned_properties,
            response.summary.available_property_keys,
            if response.coverage.properties_truncated {
                " (truncated)"
            } else {
                ""
            },
        )
        .as_str(),
    );
    for property in &response.properties {
        output.push_str(
            format!(
                "  {} [{}]: {} events ({}) | approximately {} distinct values\n",
                display_text(property.key.as_str()),
                property.source.human_label(),
                property.events,
                percentage(property.coverage_rate),
                property.distinct_values,
            )
            .as_str(),
        );
    }
    output.push_str("Next: ");
    output.push_str(next_step(response.next_action.code.as_str()));
    output.push('\n');
    output
}

/// Returns a terminal-safe exact filter label or wildcard marker.
fn filter_label(value: Option<&str>) -> String {
    value.map_or_else(|| "<any>".to_owned(), display_text)
}

/// Formats one optional fraction as a percentage.
fn percentage(value: Option<f64>) -> String {
    value.map_or_else(
        || "n/a".to_owned(),
        |value| format!("{:.1}%", value * 100.0),
    )
}

/// Maps validated stable action codes to local, value-free human guidance.
fn next_step(code: &str) -> &'static str {
    match code {
        "capture_product_activity" => {
            "capture version-1 page views, screen views, or interactions before discovering properties"
        }
        "capture_current_property_index" => {
            "capture new classified events with context.schemaVersion=1; historical events cannot be backfilled from this catalog"
        }
        "add_product_properties" => {
            "attach bounded typed resource context or non-sensitive context.tags to classified events"
        }
        "narrow_property_scope" => {
            "narrow service, release, environment, or time scope to inspect additional keys"
        }
        "compare_property_segments" => {
            "use an exact returned key and an application-known value with analytics compare --segment-property"
        }
        _ => "retry the bounded analytics property query",
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

/// Converts transport and refresh failures into fixed property-safe recovery.
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
        message: "analytics property request could not be completed",
        next: "check network connectivity and retry the same analytics property query",
    }
}

/// Returns one fixed response-contract failure.
const fn invalid_response() -> RuntimeError {
    RuntimeError::AnalyticsPropertiesResponseInvalid
}

/// Converts a failed HTTP status into fixed guidance without reflecting its body.
fn safe_api_error(status: u16, credential: &AuthCredential) -> RuntimeError {
    let (error, code, next) = match status {
        400 | 422 => (
            "analytics property request rejected",
            "validation_failed",
            "check the exact project, time scope, deployment filters, and limit",
        ),
        401 => (
            "authentication required",
            "unauthorized",
            "run logbrew login",
        ),
        403 => (
            "analytics property request forbidden",
            "forbidden",
            "confirm account access and retry the same analytics property query",
        ),
        404 => (
            "analytics property resource not found",
            "not_found",
            "check the project and retry the same analytics property query",
        ),
        405 => (
            "analytics property method is not supported",
            "method_not_allowed",
            "use the GET-backed logbrew analytics properties command",
        ),
        429 => (
            "analytics property request rate limited",
            "rate_limited",
            "retry the same analytics property query later",
        ),
        500..=599 => (
            "analytics property service unavailable",
            "service_unavailable",
            "retry the same analytics property query later",
        ),
        _ => (
            "analytics property request failed",
            "request_failed",
            "check account access and retry the same analytics property query",
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

    /// Fixed request used by response-contract tests.
    fn options() -> AnalyticsPropertyOptions {
        AnalyticsPropertyOptions {
            project_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned(),
            since: "24h".to_owned(),
            until: None,
            service_name: None,
            release: None,
            environment: None,
            limit: 20,
        }
    }

    /// One internally consistent key-only property response.
    fn body() -> String {
        serde_json::json!({
            "schema_version": 1,
            "query": {
                "project_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "since": "2026-08-03T00:00:00Z",
                "until": "2026-08-04T00:00:00Z",
                "limit": 20
            },
            "purpose": "Safe aggregate property catalog.",
            "summary": {
                "classified_events": 100,
                "available_property_keys": 2,
                "returned_properties": 2
            },
            "coverage": {
                "indexed_events": 80,
                "unindexed_events": 20,
                "complete_index_events": 75,
                "incomplete_index_events": 5,
                "privacy_filtered_events": 2,
                "events_with_properties": 60,
                "index_coverage_rate": 0.8,
                "property_capture_rate": 0.6,
                "properties_truncated": false,
                "values_returned": false,
                "limitations": ["Keys and counts only.", "Exact values are application-known."]
            },
            "estimation": {
                "count_accuracy": "approximate",
                "method": "clickhouse_uniq_combined64",
                "description": "Cardinalities are approximate."
            },
            "properties": [
                {
                    "key": "tag.plan",
                    "source": "custom_tag",
                    "value_type": "string",
                    "events": 60,
                    "coverage_rate": 0.6,
                    "distinct_values": 3
                },
                {
                    "key": "resource.framework.name",
                    "source": "standard_context",
                    "value_type": "string",
                    "events": 40,
                    "coverage_rate": 0.4,
                    "distinct_values": 2
                }
            ],
            "next_action": {
                "code": "compare_property_segments",
                "target": "/api/telemetry/analytics/segments/compare",
                "reason": "Compare exact known values."
            }
        })
        .to_string()
    }

    #[test]
    fn validates_key_only_catalog_and_renders_capture_receipts() {
        let response = validated_response(&options(), body().as_str()).expect("valid response");
        let rendered = render_response(&response);

        assert!(rendered.contains("Privacy and migration: 20 unindexed; 5 incomplete"));
        assert!(rendered.contains("tag.plan [custom tag]: 60 events"));
        assert!(rendered.contains("Values and identities are not returned"));
    }

    #[test]
    fn rejects_values_sensitive_keys_and_contradictory_coverage() {
        for (pointer, value) in [
            ("/coverage/values_returned", serde_json::json!(true)),
            ("/properties/0/key", serde_json::json!("tag.user_id")),
            ("/coverage/unindexed_events", serde_json::json!(19)),
        ] {
            let mut response: serde_json::Value =
                serde_json::from_str(body().as_str()).expect("fixture parses");
            *response.pointer_mut(pointer).expect("pointer exists") = value;
            assert!(validated_response(&options(), response.to_string().as_str()).is_err());
        }
    }
}
