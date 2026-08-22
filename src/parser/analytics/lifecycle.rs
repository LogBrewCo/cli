//! Closed product-analytics lifecycle command grammar.

use super::{Grammar, retention_event_kind};
use crate::ids::is_uuid;
use crate::{AnalyticsLifecycleInterval, AnalyticsLifecycleOptions, CliError, Command};

/// Exact recovery text shared by every malformed lifecycle invocation.
pub(super) const ANALYTICS_LIFECYCLE_NEXT_STEP: &str = "use logbrew analytics lifecycle --project <project_id> --since <24h|RFC3339> --event-kind <page-view|screen-view|interaction> --event <name> with optional --until, --service, --release, --environment, --interval hour|day|week|thirty-day, --history-periods 2-31, and --json";

/// Canonical parser behavior for lifecycle reads.
const GRAMMAR: Grammar = Grammar::new("analytics lifecycle", ANALYTICS_LIFECYCLE_NEXT_STEP);

/// Parses one exact event selector and bounded lifecycle controls.
pub(super) fn parse_lifecycle(args: &[String]) -> Result<Command, CliError> {
    let mut parsed = ParsedLifecycleFlags::default();
    let mut seen = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let raw = &args[index];
        let (flag, inline) = Grammar::split_flag(raw);
        match flag {
            "--json" => {
                GRAMMAR.reject_inline(flag, inline)?;
                GRAMMAR.mark_seen(&mut seen, "--json")?;
                parsed.json = true;
            }
            "--project" | "--project-id" => {
                GRAMMAR.mark_seen(&mut seen, "--project")?;
                parsed.project_id =
                    Some(GRAMMAR.flag_value(args, &mut index, "--project", inline)?);
            }
            "--since" => {
                GRAMMAR.mark_seen(&mut seen, "--since")?;
                parsed.since = Some(GRAMMAR.flag_value(args, &mut index, "--since", inline)?);
            }
            "--until" => {
                GRAMMAR.mark_seen(&mut seen, "--until")?;
                parsed.until = Some(GRAMMAR.flag_value(args, &mut index, "--until", inline)?);
            }
            "--service" | "--service-name" => {
                GRAMMAR.mark_seen(&mut seen, "--service")?;
                parsed.service_name =
                    Some(GRAMMAR.flag_value(args, &mut index, "--service", inline)?);
            }
            "--release" => {
                GRAMMAR.mark_seen(&mut seen, "--release")?;
                parsed.release = Some(GRAMMAR.flag_value(args, &mut index, "--release", inline)?);
            }
            "--environment" | "--env" => {
                GRAMMAR.mark_seen(&mut seen, "--environment")?;
                parsed.environment =
                    Some(GRAMMAR.flag_value(args, &mut index, "--environment", inline)?);
            }
            "--event-kind" => {
                GRAMMAR.mark_seen(&mut seen, "--event-kind")?;
                parsed.event_kind =
                    Some(GRAMMAR.flag_value(args, &mut index, "--event-kind", inline)?);
            }
            "--event" | "--event-name" => {
                GRAMMAR.mark_seen(&mut seen, "--event")?;
                parsed.event_name = Some(GRAMMAR.flag_value(args, &mut index, "--event", inline)?);
            }
            "--interval" => {
                GRAMMAR.mark_seen(&mut seen, "--interval")?;
                parsed.interval =
                    Some(GRAMMAR.flag_value(args, &mut index, "--interval", inline)?);
            }
            "--history-periods" | "--history-period-count" => {
                GRAMMAR.mark_seen(&mut seen, "--history-periods")?;
                parsed.history_period_count =
                    Some(GRAMMAR.flag_value(args, &mut index, "--history-periods", inline)?);
            }
            value if value.starts_with('-') => {
                return Err(CliError::UnknownFlag {
                    flag: flag.to_owned(),
                    next: ANALYTICS_LIFECYCLE_NEXT_STEP,
                });
            }
            _ => {
                return Err(CliError::UnexpectedArgument {
                    argument: raw.clone(),
                    command: "analytics lifecycle",
                    next: ANALYTICS_LIFECYCLE_NEXT_STEP,
                });
            }
        }
        index += 1;
    }

    Ok(Command::AnalyticsLifecycle {
        options: parsed.finish()?,
        json: parsed.json,
    })
}

/// Partially parsed flags before required-value and shape validation.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "fields directly represent the closed analytics lifecycle grammar"
)]
#[derive(Default)]
struct ParsedLifecycleFlags {
    project_id: Option<String>,
    since: Option<String>,
    until: Option<String>,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    event_kind: Option<String>,
    event_name: Option<String>,
    interval: Option<String>,
    history_period_count: Option<String>,
    json: bool,
}

impl ParsedLifecycleFlags {
    /// Requires and normalizes every public request field.
    fn finish(&self) -> Result<AnalyticsLifecycleOptions, CliError> {
        let project_id = required(self.project_id.as_deref(), "project")?.trim();
        if !is_uuid(project_id) {
            return Err(GRAMMAR.invalid_argument("invalid project id"));
        }
        let since = normalize_text(required(self.since.as_deref(), "since")?, 64)?;
        let until = normalize_optional(self.until.as_deref(), 64)?;
        let service_name = normalize_optional(self.service_name.as_deref(), 256)?;
        let release = normalize_optional(self.release.as_deref(), 256)?;
        let environment = normalize_optional(self.environment.as_deref(), 256)?;
        let parsed_kind = GRAMMAR.normalize_event_kind(
            required(self.event_kind.as_deref(), "event-kind")?,
            "invalid lifecycle event kind",
        )?;
        let event_name = GRAMMAR.normalize_event_name(
            parsed_kind,
            required(self.event_name.as_deref(), "event")?,
            "invalid analytics lifecycle value",
            "invalid lifecycle event",
        )?;
        let event_kind = retention_event_kind(parsed_kind);
        let interval = normalize_interval(self.interval.as_deref())?;
        let history_period_count = bounded_history_periods(self.history_period_count.as_deref())?;
        validate_history_span(interval, history_period_count)?;

        Ok(AnalyticsLifecycleOptions {
            project_id: project_id.to_ascii_lowercase(),
            since,
            until,
            service_name,
            release,
            environment,
            event_kind,
            event_name,
            interval,
            history_period_count,
        })
    }
}

/// Requires one named lifecycle flag.
fn required<'a>(value: Option<&'a str>, argument: &'static str) -> Result<&'a str, CliError> {
    GRAMMAR.required(value, argument)
}

/// Normalizes an optional fixed lifecycle interval.
fn normalize_interval(value: Option<&str>) -> Result<Option<AnalyticsLifecycleInterval>, CliError> {
    match value.map(str::trim) {
        None => Ok(None),
        Some("hour" | "1h") => Ok(Some(AnalyticsLifecycleInterval::Hour)),
        Some("day" | "1d") => Ok(Some(AnalyticsLifecycleInterval::Day)),
        Some("week" | "1w") => Ok(Some(AnalyticsLifecycleInterval::Week)),
        Some("thirty-day" | "thirty_day" | "30d") => {
            Ok(Some(AnalyticsLifecycleInterval::ThirtyDay))
        }
        Some(_) => Err(GRAMMAR.invalid_argument("invalid lifecycle interval")),
    }
}

/// Parses the bounded complete-history count or supplies the server default.
fn bounded_history_periods(value: Option<&str>) -> Result<u8, CliError> {
    let Some(value) = value else {
        return Ok(2);
    };
    value
        .parse::<u8>()
        .ok()
        .filter(|count| (2..=31).contains(count))
        .ok_or_else(|| GRAMMAR.invalid_argument("invalid lifecycle history period count"))
}

/// Rejects interval/count combinations exceeding the server's 62-day history cap.
fn validate_history_span(
    interval: Option<AnalyticsLifecycleInterval>,
    history_period_count: u8,
) -> Result<(), CliError> {
    if interval.is_some_and(|interval| {
        interval
            .seconds()
            .saturating_mul(u64::from(history_period_count))
            > 62 * 24 * 60 * 60
    }) {
        return Err(GRAMMAR.invalid_argument("lifecycle history exceeds 62 days"));
    }
    Ok(())
}

/// Trims one non-empty, control-free bounded public value.
fn normalize_text(value: &str, limit: usize) -> Result<String, CliError> {
    GRAMMAR.normalize_text(value, limit, "invalid analytics lifecycle value")
}

/// Normalizes one optional bounded context value.
fn normalize_optional(value: Option<&str>, limit: usize) -> Result<Option<String>, CliError> {
    GRAMMAR.normalize_optional(value, limit, "invalid analytics lifecycle value")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds parser-owned arguments without shell behavior.
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_aliases_and_server_defaults_into_one_stable_request() {
        let command = parse_lifecycle(&args(&[
            "--project-id",
            "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA",
            "--since=24h",
            "--event-kind",
            "page",
            "--event-name",
            "/pricing",
            "--env",
            "production",
            "--json",
        ]))
        .expect("valid command");

        let Command::AnalyticsLifecycle { options, json } = command else {
            panic!("wrong command");
        };
        assert_eq!(options.project_id, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        assert_eq!(
            options.event_kind,
            crate::AnalyticsRetentionEventKind::PageView
        );
        assert_eq!(options.interval, None);
        assert_eq!(options.history_period_count, 2);
        assert!(json);
    }

    #[test]
    fn rejects_missing_unsafe_duplicate_and_unbounded_values() {
        let base = [
            "--project",
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "--since",
            "24h",
            "--event-kind",
            "interaction",
            "--event",
            "checkout_completed",
        ];
        assert!(parse_lifecycle(&args(&base[..6])).is_err());

        let mut unsafe_event = base.to_vec();
        unsafe_event[7] = "checkout completed";
        assert!(parse_lifecycle(&args(&unsafe_event)).is_err());

        let mut duplicate = base.to_vec();
        duplicate.extend(["--project-id", "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"]);
        assert!(matches!(
            parse_lifecycle(&args(&duplicate)),
            Err(CliError::DuplicateFlag {
                flag: "--project",
                ..
            })
        ));

        let mut too_much_history = base.to_vec();
        too_much_history.extend(["--interval", "week", "--history-periods", "9"]);
        assert!(parse_lifecycle(&args(&too_much_history)).is_err());
    }
}
