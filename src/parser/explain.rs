//! Grammar for explanation and investigation commands.

use super::{
    is_canonical_lower_uuid, move_leading_json_to_tail, parse_explain_target_flags,
    take_required_position, unknown_resource,
};
use crate::ids::{infer_explain_target, is_uuid};
use crate::{
    CliError, Command, ExplainMetricTarget, ExplainReleaseTarget, ExplainSpanTarget, ExplainTarget,
    IssueCorrectionTarget, IssueOccurrenceSelection,
};

/// Valid resources for explain.
pub(super) const EXPLAIN_RESOURCE_NEXT_STEP: &str =
    "choose issue, log, action, trace, span, release, or metric";
/// Exact required identity for release investigation.
const EXPLAIN_RELEASE_NEXT_STEP: &str = "use logbrew explain release <release> --project \
                                         <project_id> --environment <environment> --service \
                                         <service_name> [--json]";
/// Bounded required query for metric investigation.
const EXPLAIN_METRIC_NEXT_STEP: &str = "use logbrew explain metric <name> --project <project_id> \
                                        --since <24h|RFC3339> with optional --until, --interval, \
                                        --group-by, --service, --release, --environment, \
                                        --series-limit, and --json";
/// Exact required identity and scope for one span investigation.
const EXPLAIN_SPAN_NEXT_STEP: &str = "use logbrew explain span <trace_id> <span_id> --project \
                                      <project_id> --environment <environment> --release <release> \
                                      [--json]";

/// Parses the closed, read-only issue investigation grammar.
pub(super) fn parse_investigate(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    let [resource, issue_id, tail @ ..] = normalized.as_slice() else {
        return Err(CliError::InvalidInvestigationCommand);
    };
    if resource != "issue" || !is_canonical_lower_uuid(issue_id.as_str()) {
        return Err(CliError::InvalidInvestigationCommand);
    }
    if tail.first().is_some_and(|value| value == "verify") {
        return parse_issue_correction(issue_id, &tail[1..]);
    }
    let (occurrence, json) = parse_issue_investigation_flags(tail)?;
    Ok(Command::InvestigateIssue {
        issue_id: issue_id.clone(),
        occurrence,
        json,
    })
}

/// Parses one exact candidate-correction verification request.
fn parse_issue_correction(issue_id: &str, args: &[String]) -> Result<Command, CliError> {
    let mut baseline = None;
    let mut deployment = None;
    let mut json = false;
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if argument == "--json" {
            if std::mem::replace(&mut json, true) {
                return Err(CliError::InvalidInvestigationCommand);
            }
        } else {
            let (flag, inline) = argument
                .split_once('=')
                .map_or((argument.as_str(), None), |(flag, value)| {
                    (flag, Some(value))
                });
            let slot = match flag {
                "--baseline-occurrence" => &mut baseline,
                "--candidate-deployment" => &mut deployment,
                _ => return Err(CliError::InvalidInvestigationCommand),
            };
            if slot.is_some() {
                return Err(CliError::InvalidInvestigationCommand);
            }
            let value = if let Some(value) = inline {
                value
            } else {
                index = index.saturating_add(1);
                args.get(index)
                    .map(String::as_str)
                    .ok_or(CliError::InvalidInvestigationCommand)?
            };
            *slot = Some(value.to_owned());
        }
        index = index.saturating_add(1);
    }
    let baseline_occurrence_id = baseline
        .filter(|value| is_canonical_lower_uuid(value))
        .ok_or(CliError::InvalidInvestigationCommand)?;
    let candidate_deployment_id = deployment
        .filter(|value| crate::deployment::valid_id(value))
        .ok_or(CliError::InvalidInvestigationCommand)?;
    Ok(Command::Explain {
        target: ExplainTarget::IssueCorrection(IssueCorrectionTarget {
            issue_id: issue_id.to_owned(),
            baseline_occurrence_id,
            candidate_deployment_id,
        }),
        json,
    })
}

/// Parses the closed issue-only occurrence selector and JSON output flags.
pub(super) fn parse_issue_investigation_flags(
    args: &[String],
) -> Result<(IssueOccurrenceSelection, bool), CliError> {
    let mut occurrence = IssueOccurrenceSelection::Recommended;
    let mut occurrence_seen = false;
    let mut json = false;
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if argument == "--json" {
            if json {
                return Err(CliError::DuplicateFlag {
                    flag: "--json",
                    next: "use --json once",
                });
            }
            json = true;
        } else {
            let (flag, inline_value) = argument
                .split_once('=')
                .map_or((argument.as_str(), None), |(flag, value)| {
                    (flag, Some(value))
                });
            if flag != "--occurrence" || occurrence_seen {
                return Err(CliError::InvalidInvestigationCommand);
            }
            let value = if let Some(value) = inline_value {
                value
            } else {
                index = index.saturating_add(1);
                args.get(index)
                    .map(String::as_str)
                    .ok_or(CliError::InvalidInvestigationCommand)?
            };
            occurrence = parse_issue_occurrence(value)?;
            occurrence_seen = true;
        }
        index = index.saturating_add(1);
    }
    Ok((occurrence, json))
}

/// Validates one named or exact retained occurrence selector without reflecting invalid input.
fn parse_issue_occurrence(value: &str) -> Result<IssueOccurrenceSelection, CliError> {
    match value {
        "recommended" => Ok(IssueOccurrenceSelection::Recommended),
        "first" => Ok(IssueOccurrenceSelection::First),
        "latest" => Ok(IssueOccurrenceSelection::Latest),
        _ if is_canonical_lower_uuid(value) => {
            Ok(IssueOccurrenceSelection::Exact(value.to_owned()))
        }
        _ => Err(CliError::InvalidInvestigationCommand),
    }
}

/// Parses `explain`.
pub(super) fn parse_explain(args: &[String]) -> Result<Command, CliError> {
    let (resource, rest) = take_required_position(args, "resource", EXPLAIN_RESOURCE_NEXT_STEP)?;
    if resource == "span" {
        return parse_explain_span(rest.as_slice());
    }
    if resource == "release" {
        return parse_explain_release(rest.as_slice());
    }
    if matches!(resource.as_str(), "metric" | "metrics") {
        return parse_explain_metric(rest.as_slice());
    }
    let (target, tail) = match resource.as_str() {
        "issue" => {
            let (id, tail) =
                take_required_position(rest.as_slice(), "issue_id", "provide an issue id")?;
            (
                ExplainTarget::Issue {
                    id: validate_explain_id(id.as_str(), ExplainIdKind::Issue)?,
                    occurrence: IssueOccurrenceSelection::Recommended,
                },
                tail,
            )
        }
        "log" => {
            let (id, tail) = take_required_position(rest.as_slice(), "log_id", "provide a log id")?;
            (
                ExplainTarget::Log(validate_explain_id(id.as_str(), ExplainIdKind::Log)?),
                tail,
            )
        }
        "action" => {
            let (id, tail) =
                take_required_position(rest.as_slice(), "action_id", "provide an action UUID")?;
            (
                ExplainTarget::Action(validate_explain_id(id.as_str(), ExplainIdKind::Action)?),
                tail,
            )
        }
        "trace" => {
            let (id, tail) =
                take_required_position(rest.as_slice(), "trace_id", "provide a trace id")?;
            (
                ExplainTarget::Trace(validate_explain_id(id.as_str(), ExplainIdKind::Trace)?),
                tail,
            )
        }
        other => {
            if let Some(target) = infer_explain_target(other) {
                (target, rest)
            } else {
                return Err(unknown_resource(other, EXPLAIN_RESOURCE_NEXT_STEP));
            }
        }
    };
    parse_explain_target_flags(target, tail.as_slice())
}

/// Parses one exact span without accepting trace-only or deployment-ambiguous reads.
fn parse_explain_span(args: &[String]) -> Result<Command, CliError> {
    let (trace_id, tail) = take_required_position(args, "trace_id", "provide the exact trace id")?;
    let (span_id, tail) =
        take_required_position(tail.as_slice(), "span_id", "provide the exact span id")?;
    let flags = parse_explain_scope_flags(tail.as_slice(), ExplainScopeKind::Span)?;
    let project_id = required_explain_scope(flags.project_id, "--project", EXPLAIN_SPAN_NEXT_STEP)?;
    let environment =
        required_explain_scope(flags.environment, "--environment", EXPLAIN_SPAN_NEXT_STEP)?;
    let release = required_explain_scope(flags.release, "--release", EXPLAIN_SPAN_NEXT_STEP)?;
    Ok(Command::Explain {
        target: ExplainTarget::Span(ExplainSpanTarget {
            trace_id: normalize_w3c_id(trace_id.as_str(), 32, "invalid trace id")?,
            span_id: normalize_w3c_id(span_id.as_str(), 16, "invalid span id")?,
            project_id: normalize_project_id(project_id.as_str(), EXPLAIN_SPAN_NEXT_STEP)?,
            environment: normalize_explain_text(environment.as_str(), 256, EXPLAIN_SPAN_NEXT_STEP)?,
            release: normalize_explain_text(release.as_str(), 256, EXPLAIN_SPAN_NEXT_STEP)?,
        }),
        json: flags.json,
    })
}

/// Normalizes one exact non-zero W3C identifier.
fn normalize_w3c_id(value: &str, width: usize, argument: &'static str) -> Result<String, CliError> {
    let value = value.trim();
    if value.len() != width
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !value.bytes().any(|byte| byte != b'0')
    {
        return Err(CliError::UnexpectedArgument {
            argument: argument.to_owned(),
            command: "explain span",
            next: EXPLAIN_SPAN_NEXT_STEP,
        });
    }
    Ok(value.to_ascii_lowercase())
}

/// Explicit identifier vocabulary for explanation detail reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExplainIdKind {
    /// Grouped issue identifier.
    Issue,
    /// Stored log UUID.
    Log,
    /// Stored product-action UUID.
    Action,
    /// W3C or public-prefixed trace identifier.
    Trace,
}

/// Validates one explicit identifier before constructing an authenticated request.
fn validate_explain_id(value: &str, kind: ExplainIdKind) -> Result<String, CliError> {
    let value = value.trim();
    let prefixed_value_is_nonempty = |prefixes: &[&str]| {
        prefixes.iter().any(|prefix| {
            value
                .strip_prefix(prefix)
                .is_some_and(|suffix| !suffix.is_empty())
        })
    };
    let valid_shape = match kind {
        ExplainIdKind::Issue => is_uuid(value) || prefixed_value_is_nonempty(&["issue_", "issue-"]),
        ExplainIdKind::Log | ExplainIdKind::Action => is_uuid(value),
        ExplainIdKind::Trace => {
            (value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
                || prefixed_value_is_nonempty(&["trace_", "trace-"])
        }
    };
    if value.len() > 128 || value.chars().any(char::is_control) || !valid_shape {
        let (argument, command, next) = match kind {
            ExplainIdKind::Issue => ("invalid issue id", "explain issue", "provide an issue id"),
            ExplainIdKind::Log => ("invalid log id", "explain log", "provide a log UUID"),
            ExplainIdKind::Action => (
                "invalid action id",
                "explain action",
                "provide an action UUID",
            ),
            ExplainIdKind::Trace => ("invalid trace id", "explain trace", "provide a trace id"),
        };
        return Err(CliError::UnexpectedArgument {
            argument: argument.to_owned(),
            command,
            next,
        });
    }
    Ok(value.to_owned())
}

/// Parses one exact service-release investigation without accepting list ambiguity.
fn parse_explain_release(args: &[String]) -> Result<Command, CliError> {
    let (release, tail) =
        take_required_position(args, "release", "provide the exact release value")?;
    let flags = parse_explain_scope_flags(tail.as_slice(), ExplainScopeKind::Release)?;
    let project_id =
        required_explain_scope(flags.project_id, "--project", EXPLAIN_RELEASE_NEXT_STEP)?;
    let environment = required_explain_scope(
        flags.environment,
        "--environment",
        EXPLAIN_RELEASE_NEXT_STEP,
    )?;
    let service_name =
        required_explain_scope(flags.service_name, "--service", EXPLAIN_RELEASE_NEXT_STEP)?;
    Ok(Command::Explain {
        target: ExplainTarget::Release(ExplainReleaseTarget {
            project_id: normalize_project_id(project_id.as_str(), EXPLAIN_RELEASE_NEXT_STEP)?,
            release: normalize_explain_text(release.as_str(), 256, EXPLAIN_RELEASE_NEXT_STEP)?,
            environment: normalize_explain_text(
                environment.as_str(),
                256,
                EXPLAIN_RELEASE_NEXT_STEP,
            )?,
            service_name: normalize_explain_text(
                service_name.as_str(),
                256,
                EXPLAIN_RELEASE_NEXT_STEP,
            )?,
        }),
        json: flags.json,
    })
}

/// Parses one semantics-preserving metric investigation query.
fn parse_explain_metric(args: &[String]) -> Result<Command, CliError> {
    let (name, tail) = take_required_position(args, "name", "provide the exact metric name")?;
    let flags = parse_explain_scope_flags(tail.as_slice(), ExplainScopeKind::Metric)?;
    let project_id =
        required_explain_scope(flags.project_id, "--project", EXPLAIN_METRIC_NEXT_STEP)?;
    let since = required_explain_scope(flags.since, "--since", EXPLAIN_METRIC_NEXT_STEP)?;
    Ok(Command::Explain {
        target: ExplainTarget::Metric(ExplainMetricTarget {
            project_id: normalize_project_id(project_id.as_str(), EXPLAIN_METRIC_NEXT_STEP)?,
            name: normalize_explain_text(name.as_str(), 200, EXPLAIN_METRIC_NEXT_STEP)?,
            since: normalize_explain_text(since.as_str(), 64, EXPLAIN_METRIC_NEXT_STEP)?,
            until: normalize_optional_explain_text(flags.until, 64, EXPLAIN_METRIC_NEXT_STEP)?,
            interval: normalize_metric_interval(flags.interval)?,
            group_by: normalize_metric_group_by(flags.group_by)?,
            service_name: normalize_optional_explain_text(
                flags.service_name,
                256,
                EXPLAIN_METRIC_NEXT_STEP,
            )?,
            release: normalize_optional_explain_text(flags.release, 256, EXPLAIN_METRIC_NEXT_STEP)?,
            environment: normalize_optional_explain_text(
                flags.environment,
                256,
                EXPLAIN_METRIC_NEXT_STEP,
            )?,
            series_limit: flags.series_limit,
        }),
        json: flags.json,
    })
}

/// Exact flag vocabulary for release and metric explanations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExplainScopeKind {
    /// Exact span deployment scope.
    Span,
    /// Release identity flags only.
    Release,
    /// Metric query flags.
    Metric,
}

impl ExplainScopeKind {
    /// Returns the command name shown in fixed parse errors.
    const fn command(self) -> &'static str {
        match self {
            Self::Span => "explain span",
            Self::Release => "explain release",
            Self::Metric => "explain metric",
        }
    }

    /// Returns exact recovery guidance for this grammar.
    const fn next(self) -> &'static str {
        match self {
            Self::Span => EXPLAIN_SPAN_NEXT_STEP,
            Self::Release => EXPLAIN_RELEASE_NEXT_STEP,
            Self::Metric => EXPLAIN_METRIC_NEXT_STEP,
        }
    }
}

/// Parsed exact scope before required-field validation.
#[derive(Debug, Default)]
struct ExplainScopeFlags {
    /// Owned project identifier.
    project_id: Option<String>,
    /// Exact deployment environment.
    environment: Option<String>,
    /// Exact logical service name.
    service_name: Option<String>,
    /// Exact metric release filter.
    release: Option<String>,
    /// Inclusive metric lower bound.
    since: Option<String>,
    /// Exclusive metric upper bound.
    until: Option<String>,
    /// Requested metric bucket interval.
    interval: Option<String>,
    /// Requested fixed grouping dimension.
    group_by: Option<String>,
    /// Maximum returned metric series.
    series_limit: Option<u8>,
    /// Stable machine-readable output mode.
    json: bool,
}

/// Parses only the exact release or metric flags supported by its API contract.
fn parse_explain_scope_flags(
    args: &[String],
    kind: ExplainScopeKind,
) -> Result<ExplainScopeFlags, CliError> {
    let mut parsed = ExplainScopeFlags::default();
    let mut index = 0;
    while let Some(raw) = args.get(index) {
        let (flag, inline) = raw
            .split_once('=')
            .map_or((raw.as_str(), None), |(name, value)| (name, Some(value)));
        match flag {
            "--json" if inline.is_none() => {
                if std::mem::replace(&mut parsed.json, true) {
                    return Err(CliError::DuplicateFlag {
                        flag: "--json",
                        next: "use --json once",
                    });
                }
            }
            "--project" | "--project-id" => {
                let value = explain_scope_flag_value(args, &mut index, flag, inline, kind)?;
                set_explain_scope_flag(&mut parsed.project_id, value, "--project", kind)?;
            }
            "--environment" | "--env" => {
                let value = explain_scope_flag_value(args, &mut index, flag, inline, kind)?;
                set_explain_scope_flag(&mut parsed.environment, value, "--environment", kind)?;
            }
            "--service" | "--service-name" if kind != ExplainScopeKind::Span => {
                let value = explain_scope_flag_value(args, &mut index, flag, inline, kind)?;
                set_explain_scope_flag(&mut parsed.service_name, value, "--service", kind)?;
            }
            "--release" if matches!(kind, ExplainScopeKind::Span | ExplainScopeKind::Metric) => {
                let value = explain_scope_flag_value(args, &mut index, flag, inline, kind)?;
                set_explain_scope_flag(&mut parsed.release, value, "--release", kind)?;
            }
            "--since" if kind == ExplainScopeKind::Metric => {
                let value = explain_scope_flag_value(args, &mut index, flag, inline, kind)?;
                set_explain_scope_flag(&mut parsed.since, value, "--since", kind)?;
            }
            "--until" if kind == ExplainScopeKind::Metric => {
                let value = explain_scope_flag_value(args, &mut index, flag, inline, kind)?;
                set_explain_scope_flag(&mut parsed.until, value, "--until", kind)?;
            }
            "--interval" if kind == ExplainScopeKind::Metric => {
                let value = explain_scope_flag_value(args, &mut index, flag, inline, kind)?;
                set_explain_scope_flag(&mut parsed.interval, value, "--interval", kind)?;
            }
            "--group-by" if kind == ExplainScopeKind::Metric => {
                let value = explain_scope_flag_value(args, &mut index, flag, inline, kind)?;
                set_explain_scope_flag(&mut parsed.group_by, value, "--group-by", kind)?;
            }
            "--series-limit" if kind == ExplainScopeKind::Metric => {
                let value = explain_scope_flag_value(args, &mut index, flag, inline, kind)?;
                if parsed.series_limit.is_some() {
                    return Err(CliError::DuplicateFlag {
                        flag: "--series-limit",
                        next: "use --series-limit once",
                    });
                }
                let series_limit = value
                    .parse::<u8>()
                    .map_err(|_| invalid_metric_series_limit())?;
                if !(1..=20).contains(&series_limit) {
                    return Err(invalid_metric_series_limit());
                }
                parsed.series_limit = Some(series_limit);
            }
            _ if raw.starts_with('-') => {
                return Err(CliError::UnknownFlag {
                    flag: raw.clone(),
                    next: kind.next(),
                });
            }
            _ => {
                return Err(CliError::UnexpectedArgument {
                    argument: raw.clone(),
                    command: kind.command(),
                    next: kind.next(),
                });
            }
        }
        index += 1;
    }
    Ok(parsed)
}

/// Reads one separate or inline exact-scope flag value.
fn explain_scope_flag_value(
    args: &[String],
    index: &mut usize,
    flag: &str,
    inline: Option<&str>,
    kind: ExplainScopeKind,
) -> Result<String, CliError> {
    let visible_flag = match flag {
        "--project" | "--project-id" => "--project",
        "--environment" | "--env" => "--environment",
        "--service" | "--service-name" => "--service",
        "--release" => "--release",
        "--since" => "--since",
        "--until" => "--until",
        "--interval" => "--interval",
        "--group-by" => "--group-by",
        "--series-limit" => "--series-limit",
        _ => "--scope",
    };
    let value = inline.map_or_else(
        || {
            *index += 1;
            args.get(*index).cloned().unwrap_or_default()
        },
        str::to_owned,
    );
    if value.is_empty() || value.starts_with('-') {
        return Err(CliError::MissingFlagValue {
            flag: visible_flag,
            next: kind.next(),
        });
    }
    Ok(value)
}

/// Stores one canonical scope flag while rejecting aliases used twice.
fn set_explain_scope_flag(
    target: &mut Option<String>,
    value: String,
    flag: &'static str,
    kind: ExplainScopeKind,
) -> Result<(), CliError> {
    if target.replace(value).is_some() {
        return Err(CliError::DuplicateFlag {
            flag,
            next: kind.next(),
        });
    }
    Ok(())
}

/// Requires one explanation scope flag.
fn required_explain_scope(
    value: Option<String>,
    argument: &'static str,
    next: &'static str,
) -> Result<String, CliError> {
    value.ok_or(CliError::MissingArgument { argument, next })
}

/// Normalizes and validates one exact project UUID.
fn normalize_project_id(value: &str, next: &'static str) -> Result<String, CliError> {
    let value = value.trim();
    if !is_uuid(value) {
        return Err(CliError::UnexpectedArgument {
            argument: String::from("invalid project id"),
            command: "explain",
            next,
        });
    }
    Ok(value.to_ascii_lowercase())
}

/// Trims and bounds one exact public query value.
fn normalize_explain_text(
    value: &str,
    limit: usize,
    next: &'static str,
) -> Result<String, CliError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > limit || value.chars().any(char::is_control) {
        return Err(CliError::UnexpectedArgument {
            argument: String::from("invalid query value"),
            command: "explain",
            next,
        });
    }
    Ok(value.to_owned())
}

/// Normalizes one optional public query value.
fn normalize_optional_explain_text(
    value: Option<String>,
    limit: usize,
    next: &'static str,
) -> Result<Option<String>, CliError> {
    value
        .map(|value| normalize_explain_text(value.as_str(), limit, next))
        .transpose()
}

/// Normalizes one supported metric interval.
fn normalize_metric_interval(value: Option<String>) -> Result<Option<String>, CliError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_ascii_lowercase();
    if !matches!(
        value.as_str(),
        "auto" | "1m" | "5m" | "15m" | "1h" | "6h" | "1d"
    ) {
        return Err(CliError::UnexpectedArgument {
            argument: String::from("invalid metric interval"),
            command: "explain metric",
            next: EXPLAIN_METRIC_NEXT_STEP,
        });
    }
    Ok(Some(value))
}

/// Normalizes one fixed metric grouping dimension.
fn normalize_metric_group_by(value: Option<String>) -> Result<Option<String>, CliError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let normalized = match value.trim().to_ascii_lowercase().as_str() {
        "none" => "none",
        "service" | "service_name" => "service_name",
        "release" => "release",
        "environment" | "env" => "environment",
        _ => {
            return Err(CliError::UnexpectedArgument {
                argument: String::from("invalid metric grouping"),
                command: "explain metric",
                next: EXPLAIN_METRIC_NEXT_STEP,
            });
        }
    };
    Ok(Some(normalized.to_owned()))
}

/// Builds one value-safe metric cardinality error.
fn invalid_metric_series_limit() -> CliError {
    CliError::UnexpectedArgument {
        argument: String::from("invalid metric series limit"),
        command: "explain metric",
        next: EXPLAIN_METRIC_NEXT_STEP,
    }
}
