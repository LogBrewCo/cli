//! Closed product-analytics retention command grammar.

use super::{Grammar, retention_event_kind};
use crate::ids::is_uuid;
use crate::{
    AnalyticsRetentionCohortMode, AnalyticsRetentionInterval, AnalyticsRetentionMode,
    AnalyticsRetentionOptions, CliError, Command,
};

/// Exact recovery text shared by every malformed retention invocation.
pub(super) const ANALYTICS_RETENTION_NEXT_STEP: &str = "use logbrew analytics retention --project <project_id> --since <24h|RFC3339> --start-kind <page-view|screen-view|interaction> --start-event <name> --return-kind <page-view|screen-view|interaction> --return-event <name> with optional --until, --service, --release, --environment, --interval hour|day|week|thirty-day, --interval-count 1-31, --mode return-on|return-on-or-after, --cohort-mode first-in-range, and --json";

/// Canonical parser behavior for retention reads.
const GRAMMAR: Grammar = Grammar::new("analytics retention", ANALYTICS_RETENTION_NEXT_STEP);

/// Parses the exact retention selectors and bounded query controls.
pub(super) fn parse_retention(args: &[String]) -> Result<Command, CliError> {
    let mut parsed = ParsedRetentionFlags::default();
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
            "--start-kind" => {
                GRAMMAR.mark_seen(&mut seen, "--start-kind")?;
                parsed.start_kind =
                    Some(GRAMMAR.flag_value(args, &mut index, "--start-kind", inline)?);
            }
            "--start-event" => {
                GRAMMAR.mark_seen(&mut seen, "--start-event")?;
                parsed.start_event =
                    Some(GRAMMAR.flag_value(args, &mut index, "--start-event", inline)?);
            }
            "--return-kind" => {
                GRAMMAR.mark_seen(&mut seen, "--return-kind")?;
                parsed.return_kind =
                    Some(GRAMMAR.flag_value(args, &mut index, "--return-kind", inline)?);
            }
            "--return-event" => {
                GRAMMAR.mark_seen(&mut seen, "--return-event")?;
                parsed.return_event =
                    Some(GRAMMAR.flag_value(args, &mut index, "--return-event", inline)?);
            }
            "--interval" => {
                GRAMMAR.mark_seen(&mut seen, "--interval")?;
                parsed.interval =
                    Some(GRAMMAR.flag_value(args, &mut index, "--interval", inline)?);
            }
            "--interval-count" => {
                GRAMMAR.mark_seen(&mut seen, "--interval-count")?;
                parsed.interval_count =
                    Some(GRAMMAR.flag_value(args, &mut index, "--interval-count", inline)?);
            }
            "--mode" => {
                GRAMMAR.mark_seen(&mut seen, "--mode")?;
                parsed.mode = Some(GRAMMAR.flag_value(args, &mut index, "--mode", inline)?);
            }
            "--cohort-mode" => {
                GRAMMAR.mark_seen(&mut seen, "--cohort-mode")?;
                parsed.cohort_mode =
                    Some(GRAMMAR.flag_value(args, &mut index, "--cohort-mode", inline)?);
            }
            value if value.starts_with('-') => {
                return Err(CliError::UnknownFlag {
                    flag: flag.to_owned(),
                    next: ANALYTICS_RETENTION_NEXT_STEP,
                });
            }
            _ => {
                return Err(CliError::UnexpectedArgument {
                    argument: raw.clone(),
                    command: "analytics retention",
                    next: ANALYTICS_RETENTION_NEXT_STEP,
                });
            }
        }
        index += 1;
    }

    Ok(Command::AnalyticsRetention {
        options: parsed.finish()?,
        json: parsed.json,
    })
}

/// Partially parsed flags before required-value and shape validation.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "fields directly represent the closed analytics retention grammar"
)]
#[derive(Default)]
struct ParsedRetentionFlags {
    project_id: Option<String>,
    since: Option<String>,
    until: Option<String>,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    start_kind: Option<String>,
    start_event: Option<String>,
    return_kind: Option<String>,
    return_event: Option<String>,
    interval: Option<String>,
    interval_count: Option<String>,
    mode: Option<String>,
    cohort_mode: Option<String>,
    json: bool,
}

impl ParsedRetentionFlags {
    /// Requires and normalizes every public request field.
    fn finish(&self) -> Result<AnalyticsRetentionOptions, CliError> {
        let project_id = required(self.project_id.as_deref(), "project")?.trim();
        if !is_uuid(project_id) {
            return Err(GRAMMAR.invalid_argument("invalid project id"));
        }
        let since = normalize_text(required(self.since.as_deref(), "since")?, 64)?;
        let until = normalize_optional(self.until.as_deref(), 64)?;
        let service_name = normalize_optional(self.service_name.as_deref(), 256)?;
        let release = normalize_optional(self.release.as_deref(), 256)?;
        let environment = normalize_optional(self.environment.as_deref(), 256)?;
        let parsed_start_kind = GRAMMAR.normalize_event_kind(
            required(self.start_kind.as_deref(), "start-kind")?,
            "invalid retention event kind",
        )?;
        let start_event = GRAMMAR.normalize_event_name(
            parsed_start_kind,
            required(self.start_event.as_deref(), "start-event")?,
            "invalid analytics retention value",
            "invalid start event",
        )?;
        let parsed_return_kind = GRAMMAR.normalize_event_kind(
            required(self.return_kind.as_deref(), "return-kind")?,
            "invalid retention event kind",
        )?;
        let return_event = GRAMMAR.normalize_event_name(
            parsed_return_kind,
            required(self.return_event.as_deref(), "return-event")?,
            "invalid analytics retention value",
            "invalid return event",
        )?;
        let start_kind = retention_event_kind(parsed_start_kind);
        let return_kind = retention_event_kind(parsed_return_kind);
        let interval = normalize_interval(self.interval.as_deref())?;
        let interval_count = bounded_interval_count(self.interval_count.as_deref())?;
        let mode = normalize_mode(self.mode.as_deref())?;
        let cohort_mode = normalize_cohort_mode(self.cohort_mode.as_deref())?;

        Ok(AnalyticsRetentionOptions {
            project_id: project_id.to_ascii_lowercase(),
            since,
            until,
            service_name,
            release,
            environment,
            start_kind,
            start_event,
            return_kind,
            return_event,
            interval,
            interval_count,
            mode,
            cohort_mode,
        })
    }
}

/// Requires one named retention flag.
fn required<'a>(value: Option<&'a str>, argument: &'static str) -> Result<&'a str, CliError> {
    GRAMMAR.required(value, argument)
}

/// Normalizes the selected fixed interval or applies the safe CLI default.
fn normalize_interval(value: Option<&str>) -> Result<AnalyticsRetentionInterval, CliError> {
    match value.map(str::trim) {
        None | Some("day" | "1d") => Ok(AnalyticsRetentionInterval::Day),
        Some("hour" | "1h") => Ok(AnalyticsRetentionInterval::Hour),
        Some("week" | "1w") => Ok(AnalyticsRetentionInterval::Week),
        Some("thirty-day" | "thirty_day" | "30d") => Ok(AnalyticsRetentionInterval::ThirtyDay),
        Some(_) => Err(GRAMMAR.invalid_argument("invalid retention interval")),
    }
}

/// Normalizes exact or rolling return semantics.
fn normalize_mode(value: Option<&str>) -> Result<AnalyticsRetentionMode, CliError> {
    match value.map(str::trim) {
        None | Some("return-on" | "return_on" | "exact") => Ok(AnalyticsRetentionMode::ReturnOn),
        Some("return-on-or-after" | "return_on_or_after" | "rolling") => {
            Ok(AnalyticsRetentionMode::ReturnOnOrAfter)
        }
        Some(_) => Err(GRAMMAR.invalid_argument("invalid retention mode")),
    }
}

/// Normalizes the only version-1 cohort-anchor mode.
fn normalize_cohort_mode(value: Option<&str>) -> Result<AnalyticsRetentionCohortMode, CliError> {
    match value.map(str::trim) {
        None | Some("first-in-range" | "first_in_range") => {
            Ok(AnalyticsRetentionCohortMode::FirstInRange)
        }
        Some(_) => Err(GRAMMAR.invalid_argument("invalid retention cohort mode")),
    }
}

/// Parses the bounded period count or supplies its public default.
fn bounded_interval_count(value: Option<&str>) -> Result<u8, CliError> {
    let Some(value) = value else {
        return Ok(10);
    };
    value
        .parse::<u8>()
        .ok()
        .filter(|count| (1..=31).contains(count))
        .ok_or_else(|| GRAMMAR.invalid_argument("invalid retention interval count"))
}

/// Trims one non-empty, control-free bounded public value.
fn normalize_text(value: &str, limit: usize) -> Result<String, CliError> {
    GRAMMAR.normalize_text(value, limit, "invalid analytics retention value")
}

/// Normalizes one optional bounded context value.
fn normalize_optional(value: Option<&str>, limit: usize) -> Result<Option<String>, CliError> {
    GRAMMAR.normalize_optional(value, limit, "invalid analytics retention value")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds parser-owned arguments without shell behavior.
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_aliases_and_explicit_defaults_into_one_stable_request() {
        let command = parse_retention(&args(&[
            "--project-id",
            "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA",
            "--since=30d",
            "--start-kind",
            "page",
            "--start-event",
            "/signup",
            "--return-kind",
            "interaction",
            "--return-event",
            "dashboard_opened",
            "--env",
            "production",
            "--json",
        ]))
        .expect("valid command");

        let Command::AnalyticsRetention { options, json } = command else {
            panic!("wrong command");
        };
        assert_eq!(options.project_id, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        assert_eq!(
            options.start_kind,
            crate::AnalyticsRetentionEventKind::PageView
        );
        assert_eq!(
            options.return_kind,
            crate::AnalyticsRetentionEventKind::Interaction
        );
        assert_eq!(options.interval, AnalyticsRetentionInterval::Day);
        assert_eq!(options.interval_count, 10);
        assert_eq!(options.mode, AnalyticsRetentionMode::ReturnOn);
        assert_eq!(
            options.cohort_mode,
            AnalyticsRetentionCohortMode::FirstInRange
        );
        assert!(json);
    }

    #[test]
    fn rejects_missing_unsafe_duplicate_and_out_of_range_values() {
        let base = [
            "--project",
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "--since",
            "30d",
            "--start-kind",
            "interaction",
            "--start-event",
            "signup_started",
            "--return-kind",
            "interaction",
            "--return-event",
            "dashboard_opened",
        ];
        assert!(parse_retention(&args(&base[..10])).is_err());

        let mut unsafe_event = base.to_vec();
        unsafe_event[11] = "dashboard opened";
        assert!(parse_retention(&args(&unsafe_event)).is_err());

        let mut duplicate = base.to_vec();
        duplicate.extend(["--project-id", "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"]);
        assert!(matches!(
            parse_retention(&args(&duplicate)),
            Err(CliError::DuplicateFlag {
                flag: "--project",
                ..
            })
        ));

        let mut too_many = base.to_vec();
        too_many.extend(["--interval-count", "32"]);
        assert!(parse_retention(&args(&too_many)).is_err());
    }
}
