//! Percent-encoded REST endpoint paths for read, explain, and set commands.
#![expect(
    clippy::redundant_pub_crate,
    reason = "the parent command modules consume these private-module helpers"
)]

use crate::{
    ExplainMetricTarget, ExplainTarget, IssueOccurrenceSelection, ReadOptions, ReadTarget,
    SetTarget,
};

/// Read endpoint filter values.
struct ReadPathFilters<'a> {
    /// Optional action name filter.
    name: Option<&'a str>,
    /// Optional service name filter.
    service: Option<&'a str>,
    /// Optional lower time bound.
    since: Option<&'a str>,
    /// Optional exclusive upper time bound.
    until: Option<&'a str>,
    /// Optional user or actor filter.
    user: Option<&'a str>,
    /// Optional trace ID filter.
    trace: Option<&'a str>,
    /// Optional log severity filter.
    level: Option<&'a str>,
    /// Optional log message substring search.
    search: Option<&'a str>,
    /// Optional project filter.
    project: Option<&'a str>,
    /// Optional release filter.
    release: Option<&'a str>,
    /// Optional environment filter.
    environment: Option<&'a str>,
    /// Optional issue status filter.
    status: Option<&'a str>,
    /// Optional issue priority filter.
    priority: Option<&'a str>,
    /// Optional issue assignee filter.
    assignee: Option<&'a str>,
    /// Optional row limit.
    limit: Option<&'a str>,
    /// Optional minimum end-to-end trace duration in milliseconds.
    min_duration_ms: Option<&'a str>,
    /// Optional pagination mode.
    pagination: Option<&'a str>,
    /// Optional continuation timestamp.
    cursor_time: Option<&'a str>,
    /// Optional continuation identifier.
    cursor_id: Option<&'a str>,
}

/// Builds a read endpoint path from the parsed read options.
pub(crate) fn read_path(target: &ReadTarget, options: &ReadOptions) -> String {
    filtered_read_path(
        target,
        &ReadPathFilters {
            name: options.name.as_deref(),
            service: options.service.as_deref(),
            since: options.since.as_deref(),
            until: options.until.as_deref(),
            user: options.user.as_deref(),
            trace: options.trace.as_deref(),
            level: options.level.as_deref(),
            search: options.search.as_deref(),
            project: options.project.as_deref(),
            release: options.release.as_deref(),
            environment: options.environment.as_deref(),
            status: options.status.as_deref(),
            priority: options.priority.as_deref(),
            assignee: options.assignee.as_deref(),
            limit: options.limit.as_deref(),
            min_duration_ms: options.min_duration_ms.as_deref(),
            pagination: options.pagination.as_deref(),
            cursor_time: options.cursor_time.as_deref(),
            cursor_id: options.cursor_id.as_deref(),
        },
    )
}

/// Builds a read endpoint path from borrowed filter values.
fn filtered_read_path(target: &ReadTarget, filters: &ReadPathFilters<'_>) -> String {
    match target {
        ReadTarget::Logs => path_with_query(
            "/api/logs",
            &[
                ("service_name", filters.service),
                ("severity", filters.level),
                ("search", filters.search),
                ("since", filters.since),
                ("until", filters.until),
                ("trace_id", filters.trace),
                ("project_id", filters.project),
                ("release", filters.release),
                ("environment", filters.environment),
                ("pagination", filters.pagination),
                ("cursor_time", filters.cursor_time),
                ("cursor_id", filters.cursor_id),
                ("limit", filters.limit),
            ],
        ),
        ReadTarget::Issues => issue_list_path(filters),
        ReadTarget::Actions => path_with_query(
            "/api/telemetry/actions",
            &[
                ("service_name", filters.service),
                ("name", filters.name),
                ("since", filters.since),
                ("until", filters.until),
                ("distinct_id", filters.user),
                ("project_id", filters.project),
                ("release", filters.release),
                ("environment", filters.environment),
                ("pagination", filters.pagination),
                ("cursor_time", filters.cursor_time),
                ("cursor_id", filters.cursor_id),
                ("limit", filters.limit),
            ],
        ),
        ReadTarget::Releases => path_with_query(
            "/api/telemetry/releases",
            &[
                ("service_name", filters.service),
                ("since", filters.since),
                ("until", filters.until),
                ("project_id", filters.project),
                ("release", filters.release),
                ("environment", filters.environment),
                ("limit", filters.limit),
            ],
        ),
        ReadTarget::Metrics => metric_list_path(filters),
        ReadTarget::Traces => trace_list_path(filters),
        ReadTarget::Trace(id) => path_with_query(
            &format!("/api/telemetry/traces/{}", encode_component(id)),
            &[
                ("project_id", filters.project),
                ("release", filters.release),
                ("environment", filters.environment),
            ],
        ),
        ReadTarget::Issue(id) => format!(
            "/api/telemetry/issues/{}?response_version=4",
            encode_component(id)
        ),
        ReadTarget::IssueAssignees(project) => {
            format!("/api/telemetry/issues/assignees?project_id={project}&response_version=2")
        }
        ReadTarget::IssueOwners(id) => {
            format!(
                "/api/telemetry/issues/{}/owners?response_version=2",
                encode_component(id)
            )
        }
        ReadTarget::IssueActivity(id) => path_with_query(
            &format!("/api/telemetry/issues/{}/activity", encode_component(id)),
            &[
                ("limit", filters.limit),
                ("cursor_time", filters.cursor_time),
                ("cursor_id", filters.cursor_id),
                ("response_version", Some("2")),
            ],
        ),
        ReadTarget::IssueOwnership(project) => {
            format!("/api/telemetry/issues/ownership?project_id={project}")
        }
        ReadTarget::Notifications => path_with_query(
            "/api/telemetry/issues/notifications",
            &[
                ("limit", filters.limit),
                ("cursor_time", filters.cursor_time),
                ("cursor_id", filters.cursor_id),
            ],
        ),
    }
}

/// Builds the grouped issue list path in response version 4.
fn issue_list_path(filters: &ReadPathFilters<'_>) -> String {
    path_with_query(
        "/api/telemetry/issues",
        &[
            ("service_name", filters.service),
            ("since", filters.since),
            ("until", filters.until),
            ("status", filters.status),
            ("priority", filters.priority),
            ("assignee", filters.assignee),
            ("project_id", filters.project),
            ("release", filters.release),
            ("environment", filters.environment),
            ("pagination", filters.pagination),
            ("cursor_time", filters.cursor_time),
            ("cursor_id", filters.cursor_id),
            ("limit", filters.limit),
            ("response_version", Some("4")),
        ],
    )
}

/// Builds the metric sample list path.
fn metric_list_path(filters: &ReadPathFilters<'_>) -> String {
    path_with_query(
        "/api/telemetry/metrics",
        &[
            ("name", filters.name),
            ("service_name", filters.service),
            ("since", filters.since),
            ("until", filters.until),
            ("project_id", filters.project),
            ("release", filters.release),
            ("environment", filters.environment),
            ("pagination", filters.pagination),
            ("cursor_time", filters.cursor_time),
            ("cursor_id", filters.cursor_id),
            ("limit", filters.limit),
        ],
    )
}

/// Builds the recent trace list path with its trace-specific cursor key.
fn trace_list_path(filters: &ReadPathFilters<'_>) -> String {
    path_with_query(
        "/api/telemetry/traces",
        &[
            ("project_id", filters.project),
            ("service_name", filters.service),
            ("release", filters.release),
            ("environment", filters.environment),
            ("status", filters.status),
            ("since", filters.since),
            ("until", filters.until),
            ("min_duration_ms", filters.min_duration_ms),
            ("pagination", filters.pagination),
            ("cursor_time", filters.cursor_time),
            ("cursor_trace_id", filters.cursor_id),
            ("limit", filters.limit),
        ],
    )
}

/// Builds an explain endpoint path.
pub(crate) fn explain_path(target: &ExplainTarget) -> String {
    match target {
        ExplainTarget::Issue { id, occurrence } => issue_explain_path(id, occurrence),
        ExplainTarget::IssueCorrection(correction) => path_with_query(
            &format!(
                "/api/telemetry/issues/{}/correction-verification",
                encode_component(correction.issue_id.as_str())
            ),
            &[
                (
                    "baseline_occurrence_id",
                    Some(correction.baseline_occurrence_id.as_str()),
                ),
                (
                    "candidate_deployment_id",
                    Some(correction.candidate_deployment_id.as_str()),
                ),
            ],
        ),
        ExplainTarget::Log(id) => {
            format!("/api/logs/{}/investigation", encode_component(id))
        }
        ExplainTarget::Action(id) => {
            format!(
                "/api/telemetry/actions/{}/investigation",
                encode_component(id)
            )
        }
        ExplainTarget::Trace(id) => {
            format!(
                "/api/telemetry/traces/{}/investigation?response_version=3",
                encode_component(id)
            )
        }
        ExplainTarget::Span(span) => path_with_query(
            &format!(
                "/api/telemetry/traces/{}/spans/{}/investigation",
                encode_component(span.trace_id.as_str()),
                encode_component(span.span_id.as_str())
            ),
            &[
                ("project_id", Some(span.project_id.as_str())),
                ("environment", Some(span.environment.as_str())),
                ("release", Some(span.release.as_str())),
            ],
        ),
        ExplainTarget::Release(release) => path_with_query(
            "/api/telemetry/releases/investigation",
            &[
                ("project_id", Some(release.project_id.as_str())),
                ("release", Some(release.release.as_str())),
                ("environment", Some(release.environment.as_str())),
                ("service_name", Some(release.service_name.as_str())),
                ("response_version", Some("5")),
            ],
        ),
        ExplainTarget::Metric(metric) => metric_explain_path(metric),
    }
}

/// Builds one explicit version-14 issue investigation path.
fn issue_explain_path(id: &str, occurrence: &IssueOccurrenceSelection) -> String {
    let base = format!(
        "/api/telemetry/issues/{}/investigation",
        encode_component(id)
    );
    let (name, value) = match occurrence {
        IssueOccurrenceSelection::Recommended => ("selection", "recommended"),
        IssueOccurrenceSelection::First => ("selection", "first"),
        IssueOccurrenceSelection::Latest => ("selection", "latest"),
        IssueOccurrenceSelection::Exact(id) => ("occurrence_id", id.as_str()),
    };
    path_with_query(
        base.as_str(),
        &[("response_version", Some("14")), (name, Some(value))],
    )
}

/// Builds one bounded metric-investigation endpoint path.
fn metric_explain_path(metric: &ExplainMetricTarget) -> String {
    let series_limit = metric.series_limit.map(|value| value.to_string());
    path_with_query(
        "/api/telemetry/metrics/investigation",
        &[
            ("project_id", Some(metric.project_id.as_str())),
            ("name", Some(metric.name.as_str())),
            ("since", Some(metric.since.as_str())),
            ("until", metric.until.as_deref()),
            ("interval", metric.interval.as_deref()),
            ("group_by", metric.group_by.as_deref()),
            ("service_name", metric.service_name.as_deref()),
            ("release", metric.release.as_deref()),
            ("environment", metric.environment.as_deref()),
            ("series_limit", series_limit.as_deref()),
            ("response_version", Some("2")),
        ],
    )
}

/// Builds a mutation endpoint path.
pub(crate) fn set_path(target: &SetTarget) -> String {
    match target {
        // Version 3 reports `archived`, priority, and assignee.
        SetTarget::IssueStatus { id, .. }
        | SetTarget::IssuePriority { id, .. }
        | SetTarget::IssueAssignee { id, .. } => {
            format!(
                "/api/telemetry/issues/{}?response_version=4",
                encode_component(id)
            )
        }
        // Version 5 reports when an archived issue reopens.
        SetTarget::IssueArchive { id, .. } => format!(
            "/api/telemetry/issues/{}?response_version=5",
            encode_component(id)
        ),
        SetTarget::IssueBulk(_) => String::from("/api/telemetry/issues/bulk"),
        SetTarget::IssueOwnership { .. } => String::from("/api/telemetry/issues/ownership"),
        SetTarget::NotificationsRead => String::from("/api/telemetry/issues/notifications/read"),
        SetTarget::IssueRule { id, .. } => {
            format!("/api/telemetry/issues/{}/triage-rule", encode_component(id))
        }
        SetTarget::LogRule { id, .. } => format!("/api/logs/{}/triage-rule", encode_component(id)),
    }
}

/// Builds a path with query parameters.
pub(crate) fn path_with_query(path: &str, params: &[(&str, Option<&str>)]) -> String {
    let query = params
        .iter()
        .filter_map(|(name, value)| value.map(|v| format!("{name}={}", encode_component(v))))
        .collect::<Vec<_>>();

    if query.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{}", query.join("&"))
    }
}

/// Percent-encodes a path or query component without adding a dependency.
pub(crate) fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

/// Converts a nibble to an uppercase hexadecimal digit.
fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'A' + (nibble - 10)),
        _ => '?',
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_component, path_with_query};

    #[test]
    fn components_keep_unreserved_bytes_and_encode_the_rest() {
        for (value, expected) in [
            ("", ""),
            ("AZaz09-_.~", "AZaz09-_.~"),
            ("a b+c&d=e/f?g#h%", "a%20b%2Bc%26d%3De%2Ff%3Fg%23h%25"),
            ("ü\n", "%C3%BC%0A"),
        ] {
            assert_eq!(encode_component(value), expected, "{value:?}");
        }
    }

    #[test]
    fn queries_skip_absent_values_and_keep_parameter_order() {
        assert_eq!(
            path_with_query("/api/logs", &[("since", None)]),
            "/api/logs"
        );
        assert_eq!(
            path_with_query(
                "/api/logs",
                &[
                    ("level", Some("error")),
                    ("service", None),
                    ("search", Some("a&b"))
                ]
            ),
            "/api/logs?level=error&search=a%26b"
        );
        assert_eq!(
            path_with_query("/api/logs", &[("q", Some(""))]),
            "/api/logs?q="
        );
    }
}
