//! Operations performance view: latency, errors, and target status per operation.
//!
//! `logbrew operations list` ranks every operation: target breaches first, then stale data,
//! too few samples, measurement gaps, operations that did not run, and the rest.
//! `logbrew operations show <operation_id>` adds the operation's boundaries, every observation
//! below 100 samples, and one distribution per span name. The p99 is reported only from 100
//! samples; below that the view gives the count, the maximum, and every observation.
#![expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes these private-module helpers"
)]

use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::{CliEnvironment, Command, RuntimeError};

/// Maximum accepted report body.
const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
/// Maximum accepted text field length.
const TEXT_LIMIT: usize = 1024;
/// Samples the p99 needs.
const P99_MIN_SAMPLES: u64 = 100;
/// Operations shown per section in the human list.
const LIST_LIMIT: usize = 25;

/// Exact report shape.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    /// Always 1.
    response_version: u8,
    /// Inventory content revision.
    inventory_revision: String,
    /// Window start.
    since: String,
    /// Window end.
    until: String,
    /// Report time.
    generated_at: String,
    /// Environment filter.
    environment: Option<String>,
    /// Release filter.
    release: Option<String>,
    /// Coverage counts.
    coverage: Coverage,
    /// Ranked operations.
    operations: Vec<Operation>,
    /// Busiest uninventoried span groups.
    unmapped: Vec<Unmapped>,
    /// Uninventoried span groups in the window.
    unmapped_total: u64,
}

/// Coverage counts.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Coverage {
    /// Reported operations.
    operations: u64,
    /// Fully instrumented operations.
    instrumented: u64,
    /// Partly instrumented operations.
    partial: u64,
    /// Operations with no instrumentation.
    missing: u64,
    /// Operations with observations.
    observed: u64,
    /// Target breaches.
    breaches: u64,
    /// Targets with too few samples.
    unsupported: u64,
    /// Operations on stale data.
    stale: u64,
}

/// One operation.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    /// Stable identifier.
    id: String,
    /// What it does.
    #[serde(rename = "operation")]
    title: String,
    /// Owner.
    component: String,
    /// Timing start.
    start: String,
    /// Timing end.
    end: String,
    /// Which runs count.
    workload: String,
    /// Success criteria.
    success: String,
    /// Instrumentation status.
    instrumentation: Instrumentation,
    /// How to obtain the missing measurement.
    gap: Option<String>,
    /// Observation source.
    source: Source,
    /// Judgement.
    status: Status,
    /// Value unit.
    unit: String,
    /// Target.
    target: Option<Target>,
    /// Observations.
    samples: u64,
    /// Median.
    p50: Option<i64>,
    /// 95th percentile.
    p95: Option<i64>,
    /// 99th percentile, from 100 samples.
    p99: Option<i64>,
    /// Whether the p99 is supported.
    p99_supported: bool,
    /// Largest observation.
    max: Option<i64>,
    /// Failed runs.
    errors: Option<u64>,
    /// Timed-out runs.
    timeouts: Option<u64>,
    /// Measured window start.
    measured_since: Option<String>,
    /// Measured window end.
    measured_until: Option<String>,
    /// Latest observation.
    latest_at: Option<String>,
    /// Whether the data is stale.
    stale: bool,
    /// Every observation below 100 samples.
    observations: Vec<i64>,
    /// Judged statistic over the target.
    target_ratio: Option<f64>,
    /// Per span name.
    breakdown: Vec<Breakdown>,
}

/// Instrumentation status.
#[derive(serde::Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Instrumentation {
    /// Every run is observed.
    Instrumented,
    /// Some runs or stages are observed.
    Partial,
    /// Nothing is observed.
    Missing,
}

/// Observation source.
#[derive(serde::Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Source {
    /// Spans in the window.
    Spans,
    /// The last gate measurement.
    Baseline,
    /// None.
    None,
}

/// Judgement, in rank order.
#[derive(serde::Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Status {
    /// Worse than the target.
    Breach,
    /// Meets the target on stale data.
    Stale,
    /// Too few samples for the target.
    Unsupported,
    /// Not instrumented.
    Gap,
    /// No runs in the window.
    Unmeasured,
    /// Observed without a target.
    Measured,
    /// Meets the target.
    Met,
}

/// Target.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    /// Target identifier.
    id: String,
    /// Judged statistic.
    statistic: Statistic,
    /// Target value in the operation's unit.
    value: i64,
    /// Samples the statistic needs.
    min_samples: u64,
}

/// Judged statistic.
#[derive(serde::Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Statistic {
    /// 99th percentile.
    P99,
    /// Maximum.
    Max,
}

/// One span name's distribution.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Breakdown {
    /// Service.
    service: String,
    /// Span operation.
    operation: String,
    /// Span name.
    name: String,
    /// Observations.
    samples: u64,
    /// Median.
    p50: i64,
    /// 95th percentile.
    p95: i64,
    /// 99th percentile, from 100 samples.
    p99: Option<i64>,
    /// Largest observation.
    max: i64,
    /// Failed runs.
    errors: u64,
    /// Timed-out runs.
    timeouts: u64,
    /// Latest observation.
    latest_at: Option<String>,
}

/// One uninventoried span group.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Unmapped {
    /// Service.
    service: String,
    /// Span operation.
    operation: String,
    /// Span name.
    name: String,
    /// Spans in the window.
    samples: u64,
}

/// Reads the operations report and prints it.
pub(crate) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    command: &Command,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let Command::Operations { options, json } = command else {
        return Err(transport());
    };
    let json = *json;
    let origin = crate::http::normalized_origin(env.base_url.as_str()).ok_or_else(transport)?;
    let client = crate::http::api_client().map_err(|_| transport())?;
    let mut url = reqwest::Url::parse(format!("{origin}/api/operations").as_str())
        .map_err(|_| transport())?;
    {
        let mut query = url.query_pairs_mut();
        let _query = query.append_pair("response_version", "1");
        for (name, value) in [
            ("operation", options.operation.as_deref()),
            ("since", options.since.as_deref()),
            ("until", options.until.as_deref()),
            ("environment", options.environment.as_deref()),
            ("release", options.release.as_deref()),
        ] {
            if let Some(value) = value {
                let _query = query.append_pair(name, value);
            }
        }
    }
    let (response, credential) =
        send_account_authenticated_with_refresh(&client, env, |client, credential| {
            client.get(url.as_str()).bearer_auth(credential.token())
        })
        .await
        .map_err(|error| error.auth_or(transport()))?;
    let status = response.status().as_u16();
    let body = crate::http::bounded_body(response, RESPONSE_LIMIT)
        .await
        .map_err(|error| match error {
            crate::http::BodyError::Invalid => invalid(),
            crate::http::BodyError::Transport => transport(),
        })?;
    if status != 200 {
        return Err(api_error(status, body.as_str(), &credential));
    }
    let report = serde_json::from_str::<Report>(body.as_str()).map_err(|_| invalid())?;
    if !valid(&report, options.operation.as_deref()) {
        return Err(invalid());
    }
    if json {
        let value =
            serde_json::from_str::<serde_json::Value>(body.as_str()).map_err(|_| invalid())?;
        writeln!(output, "{value}")?;
    } else if options.operation.is_some() {
        write_show(&report, output)?;
    } else {
        write_list(&report, output)?;
    }
    Ok(())
}

/// Whether text is nonempty, bounded, and safe to print.
fn text(value: &str) -> bool {
    crate::http::nonempty_display_safe(value, TEXT_LIMIT)
}

/// Whether an optional timestamp is valid UTC evidence.
fn time(value: Option<&str>) -> bool {
    value.is_none_or(|value| crate::time::parse_utc_evidence(value).is_some())
}

/// Checks the whole report before anything is printed.
fn valid(report: &Report, requested: Option<&str>) -> bool {
    let coverage = &report.coverage;
    let count = |status: Status| {
        report
            .operations
            .iter()
            .filter(|operation| operation.status == status)
            .count() as u64
    };
    report.response_version == 1
        && report.inventory_revision.len() == 16
        && report
            .inventory_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        && [&report.since, &report.until, &report.generated_at]
            .iter()
            .all(|value| time(Some(value.as_str())))
        && [&report.environment, &report.release]
            .iter()
            .all(|value| value.as_deref().is_none_or(text))
        && !report.operations.is_empty()
        && coverage.operations == report.operations.len() as u64
        && coverage.instrumented + coverage.partial + coverage.missing == coverage.operations
        && coverage.breaches == count(Status::Breach)
        && coverage.unsupported == count(Status::Unsupported)
        && coverage.observed <= coverage.operations
        && coverage.stale <= coverage.operations
        && report
            .operations
            .windows(2)
            .all(|pair| pair[0].status <= pair[1].status)
        && report.operations.iter().all(valid_operation)
        && requested.is_none_or(|id| report.operations.len() == 1 && report.operations[0].id == id)
        && report.unmapped.len() as u64 <= report.unmapped_total
        && report.unmapped.iter().all(|span| {
            [&span.service, &span.operation, &span.name]
                .iter()
                .all(|value| text(value))
        })
}

/// Checks one operation's fields and the p99 rule.
fn valid_operation(operation: &Operation) -> bool {
    let ordered = [operation.p50, operation.p95, operation.p99, operation.max]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    crate::ids::is_operation_id(operation.id.as_str())
        && [
            &operation.title,
            &operation.component,
            &operation.start,
            &operation.end,
            &operation.workload,
            &operation.success,
            &operation.unit,
        ]
        .iter()
        .all(|value| text(value))
        && operation.gap.as_deref().is_none_or(text)
        && (operation.instrumentation == Instrumentation::Instrumented) == operation.gap.is_none()
        && operation.target.as_ref().is_none_or(|target| {
            crate::ids::is_operation_id(target.id.as_str()) && target.value >= 0
        })
        && operation.p99_supported == operation.p99.is_some()
        && (operation.p99.is_none() || operation.samples >= P99_MIN_SAMPLES)
        && ordered.windows(2).all(|pair| pair[0] <= pair[1])
        && operation
            .errors
            .is_none_or(|errors| errors <= operation.samples)
        && operation
            .timeouts
            .is_none_or(|timeouts| timeouts <= operation.samples)
        && (operation.observations.is_empty()
            || (operation.samples < P99_MIN_SAMPLES
                && operation.observations.len() as u64 == operation.samples
                && operation
                    .observations
                    .windows(2)
                    .all(|pair| pair[0] <= pair[1])))
        && operation.target_ratio.is_none_or(f64::is_finite)
        && (operation.source != Source::None || operation.samples == 0)
        && time(operation.measured_since.as_deref())
        && time(operation.measured_until.as_deref())
        && time(operation.latest_at.as_deref())
        && operation.breakdown.iter().all(|span| {
            [&span.service, &span.operation, &span.name]
                .iter()
                .all(|value| text(value))
                && span.p50 <= span.p95
                && span.p95 <= span.max
                && span
                    .p99
                    .is_none_or(|p99| span.samples >= P99_MIN_SAMPLES && p99 <= span.max)
                && span.errors <= span.samples
                && span.timeouts <= span.samples
                && time(span.latest_at.as_deref())
        })
}

/// Formats a value in its unit: milliseconds as ms, s, or h; other units as counts.
fn value(value: Option<i64>, unit: &str) -> String {
    let Some(value) = value else {
        return String::from("-");
    };
    if unit != "ms" {
        return format!("{value} {unit}");
    }
    let tenths = |divisor: i64| {
        let scaled = value.saturating_mul(10) / divisor;
        format!("{}.{}", scaled / 10, (scaled % 10).abs())
    };
    match value.abs() {
        0..1_000 => format!("{value} ms"),
        1_000..3_600_000 => format!("{} s", tenths(1_000)),
        _ => format!("{} h", tenths(3_600_000)),
    }
}

/// Stable status word.
const fn status_word(status: Status) -> &'static str {
    match status {
        Status::Breach => "breach",
        Status::Stale => "stale",
        Status::Unsupported => "unsupported",
        Status::Gap => "gap",
        Status::Unmeasured => "unmeasured",
        Status::Measured => "measured",
        Status::Met => "met",
    }
}

/// The target as `p99 <= 86 ms`.
fn target_text(operation: &Operation) -> String {
    operation.target.as_ref().map_or_else(
        || String::from("-"),
        |target| {
            let statistic = match target.statistic {
                Statistic::P99 => "p99",
                Statistic::Max => "max",
            };
            format!(
                "{statistic} <= {} ({})",
                value(Some(target.value), operation.unit.as_str()),
                target.id
            )
        },
    )
}

/// One ranked row: status, id, samples, p99 or max, target.
fn row(operation: &Operation) -> String {
    let judged = operation.p99.map_or_else(
        || format!("max {}", value(operation.max, operation.unit.as_str())),
        |p99| format!("p99 {}", value(Some(p99), operation.unit.as_str())),
    );
    format!(
        "  {:<11} {:<26} n={:<8} {:<16} target {}",
        status_word(operation.status),
        operation.id,
        operation.samples,
        judged,
        target_text(operation)
    )
}

/// Prints the ranked list.
fn write_list<W: std::io::Write>(report: &Report, output: &mut W) -> std::io::Result<()> {
    let coverage = &report.coverage;
    writeln!(
        output,
        "Operations from {} to {} (inventory {})",
        report.since, report.until, report.inventory_revision
    )?;
    writeln!(
        output,
        "Coverage: {} operations, {} instrumented, {} partial, {} missing; {} observed in the window",
        coverage.operations,
        coverage.instrumented,
        coverage.partial,
        coverage.missing,
        coverage.observed
    )?;
    writeln!(
        output,
        "Targets: {} breached, {} with too few samples, {} on stale data",
        coverage.breaches, coverage.unsupported, coverage.stale
    )?;
    let ranked = report
        .operations
        .iter()
        .filter(|operation| operation.status != Status::Gap)
        .collect::<Vec<_>>();
    writeln!(output, "Ranked:")?;
    for operation in ranked.iter().take(LIST_LIMIT) {
        writeln!(output, "{}", row(operation))?;
    }
    if ranked.len() > LIST_LIMIT {
        writeln!(
            output,
            "  ... {} more; use --json for all",
            ranked.len() - LIST_LIMIT
        )?;
    }
    let gaps = report
        .operations
        .iter()
        .filter(|operation| operation.instrumentation != Instrumentation::Instrumented)
        .collect::<Vec<_>>();
    if !gaps.is_empty() {
        writeln!(output, "Gaps ({}):", gaps.len())?;
        for operation in gaps {
            writeln!(
                output,
                "  {} [{}]: {}",
                operation.id,
                match operation.instrumentation {
                    Instrumentation::Partial => "partial",
                    Instrumentation::Missing | Instrumentation::Instrumented => "missing",
                },
                operation.gap.as_deref().unwrap_or("-")
            )?;
        }
    }
    if report.unmapped_total > 0 {
        writeln!(
            output,
            "Not in the inventory: {} span groups",
            report.unmapped_total
        )?;
        for span in report.unmapped.iter().take(10) {
            writeln!(
                output,
                "  {} {} {} (n={})",
                span.service, span.operation, span.name, span.samples
            )?;
        }
    }
    writeln!(output, "Next: run logbrew operations show <operation_id>")
}

/// Prints one operation in full.
fn write_show<W: std::io::Write>(report: &Report, output: &mut W) -> std::io::Result<()> {
    let Some(operation) = report.operations.first() else {
        return Ok(());
    };
    let unit = operation.unit.as_str();
    writeln!(
        output,
        "{} ({})",
        operation.id,
        status_word(operation.status)
    )?;
    writeln!(output, "{}", operation.title)?;
    writeln!(output, "Owner: {}", operation.component)?;
    writeln!(output, "Start: {}", operation.start)?;
    writeln!(output, "End: {}", operation.end)?;
    writeln!(output, "Workload: {}", operation.workload)?;
    writeln!(output, "Success: {}", operation.success)?;
    let source = match operation.source {
        Source::Spans => "spans in the window",
        Source::Baseline => "the last gate measurement",
        Source::None => "none",
    };
    writeln!(
        output,
        "Instrumentation: {} (source: {source})",
        match operation.instrumentation {
            Instrumentation::Instrumented => "instrumented",
            Instrumentation::Partial => "partial",
            Instrumentation::Missing => "missing",
        }
    )?;
    if let Some(gap) = &operation.gap {
        writeln!(output, "Gap: {gap}")?;
    }
    if let (Some(since), Some(until)) = (&operation.measured_since, &operation.measured_until) {
        writeln!(output, "Measured: {since} to {until}")?;
    }
    let p99 = if operation.p99_supported {
        value(operation.p99, unit)
    } else {
        String::from("unsupported (fewer than 100 samples)")
    };
    writeln!(
        output,
        "Samples: {}  p50 {}  p95 {}  p99 {}  max {}",
        operation.samples,
        value(operation.p50, unit),
        value(operation.p95, unit),
        p99,
        value(operation.max, unit)
    )?;
    let count =
        |value: Option<u64>| value.map_or_else(|| String::from("-"), |value| value.to_string());
    writeln!(
        output,
        "Errors: {}  Timeouts: {}",
        count(operation.errors),
        count(operation.timeouts)
    )?;
    let ratio = operation
        .target_ratio
        .map(|ratio| format!(" ({ratio:.2}x)"))
        .unwrap_or_default();
    writeln!(output, "Target: {}{ratio}", target_text(operation))?;
    if let Some(target) = &operation.target
        && operation.samples < target.min_samples
    {
        writeln!(
            output,
            "The target needs {} samples; it cannot pass on {}.",
            target.min_samples, operation.samples
        )?;
    }
    if let Some(latest) = &operation.latest_at {
        let stale = if operation.stale { " (stale)" } else { "" };
        writeln!(output, "Latest: {latest}{stale}")?;
    }
    if !operation.observations.is_empty() {
        let values = operation
            .observations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(output, "Observations ({unit}): {values}")?;
    }
    write_breakdown(operation, output)
}

/// Prints one operation's distribution per span name.
fn write_breakdown<W: std::io::Write>(
    operation: &Operation,
    output: &mut W,
) -> std::io::Result<()> {
    let unit = operation.unit.as_str();
    if !operation.breakdown.is_empty() {
        writeln!(output, "By span:")?;
        for span in &operation.breakdown {
            writeln!(
                output,
                "  {} {} {}: n={} p50 {} p99 {} max {} errors {} timeouts {}",
                span.service,
                span.operation,
                span.name,
                span.samples,
                value(Some(span.p50), unit),
                span.p99
                    .map_or_else(|| String::from("unsupported"), |p99| value(Some(p99), unit)),
                value(Some(span.max), unit),
                span.errors,
                span.timeouts
            )?;
        }
    }
    Ok(())
}

/// Validates the error envelope, then replaces all server text with fixed guidance.
fn api_error(status: u16, body: &str, credential: &AuthCredential) -> RuntimeError {
    let Some(shape) = crate::http::ErrorShape::parse(body, status, 512, |text, limit| {
        text.len() <= limit && crate::http::nonempty_display_safe(text, usize::MAX)
    }) else {
        return invalid();
    };
    let action = shape.action();
    let missing = status == 404 && action == ("not_found", "check_resource", "resource");
    let refused = status == 422 && action.0 == "validation_failed";
    if !missing && !refused && !shape.matches_read_status(status) {
        return invalid();
    }
    let unknown_operation = missing && shape_error(body) == Some("operation not found");
    let body = shape.rate_limit_body("operations").unwrap_or_else(|| {
        let (code, next) = match status {
            401 => ("unauthorized", "run logbrew login"),
            404 if unknown_operation => (
                "operation_not_found",
                "run logbrew operations list for the operation ids",
            ),
            404 => (
                "operations_unavailable",
                "sign in with an account that can read LogBrew's operations",
            ),
            422 => (
                "invalid_operations_query",
                "use --since 24h or 7d (at most 31 days) or RFC 3339 times, and nonempty filters",
            ),
            405 => ("method_not_allowed", "update logbrew and retry"),
            _ => ("server_error", "retry the same command later"),
        };
        serde_json::json!({"error": "operations request failed", "code": code, "next": next})
            .to_string()
    });
    RuntimeError::Api {
        status,
        body,
        auth_source: credential.source(),
        auth_label: credential.label(),
    }
}

/// The validated envelope's `error` text.
fn shape_error(body: &str) -> Option<&str> {
    #[derive(serde::Deserialize)]
    /// The one field read here; the envelope was validated whole.
    struct Envelope<'a> {
        /// Error text.
        #[serde(borrow)]
        error: &'a str,
    }
    serde_json::from_str::<Envelope<'_>>(body)
        .ok()
        .map(|envelope| envelope.error)
}

/// Fixed recovery for a malformed response.
const fn invalid() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "operations response was invalid",
        next: "retry the same command; if it repeats, update logbrew",
    }
}

/// Fixed recovery for a failed request.
const fn transport() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "operations request could not be completed",
        next: "check network connectivity and retry the same command",
    }
}

#[cfg(test)]
mod tests {
    use super::value;

    #[test]
    fn values_read_in_their_unit() {
        assert_eq!(value(None, "ms"), "-");
        assert_eq!(value(Some(86), "ms"), "86 ms");
        assert_eq!(value(Some(1_234), "ms"), "1.2 s");
        assert_eq!(value(Some(87_424_000), "ms"), "24.2 h");
        assert_eq!(value(Some(9), "reads"), "9 reads");
    }
}
