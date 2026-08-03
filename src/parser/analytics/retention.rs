//! Closed product-analytics retention command grammar.

use crate::ids::is_uuid;
use crate::{
    AnalyticsRetentionCohortMode, AnalyticsRetentionEventKind, AnalyticsRetentionInterval,
    AnalyticsRetentionMode, AnalyticsRetentionOptions, CliError, Command,
};

/// Exact recovery text shared by every malformed retention invocation.
pub(super) const ANALYTICS_RETENTION_NEXT_STEP: &str = "use logbrew analytics retention --project <project_id> --since <24h|RFC3339> --start-kind <page-view|screen-view|interaction> --start-event <name> --return-kind <page-view|screen-view|interaction> --return-event <name> with optional --until, --service, --release, --environment, --interval hour|day|week|thirty-day, --interval-count 1-31, --mode return-on|return-on-or-after, --cohort-mode first-in-range, and --json";

/// Parses the exact retention selectors and bounded query controls.
pub(super) fn parse_retention(args: &[String]) -> Result<Command, CliError> {
    let mut parsed = ParsedRetentionFlags::default();
    let mut seen = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let raw = &args[index];
        let (flag, inline) = split_flag(raw);
        match flag {
            "--json" => {
                reject_inline(flag, inline)?;
                mark_seen(&mut seen, "--json")?;
                parsed.json = true;
            }
            "--project" | "--project-id" => {
                mark_seen(&mut seen, "--project")?;
                parsed.project_id = Some(flag_value(args, &mut index, "--project", inline)?);
            }
            "--since" => {
                mark_seen(&mut seen, "--since")?;
                parsed.since = Some(flag_value(args, &mut index, "--since", inline)?);
            }
            "--until" => {
                mark_seen(&mut seen, "--until")?;
                parsed.until = Some(flag_value(args, &mut index, "--until", inline)?);
            }
            "--service" | "--service-name" => {
                mark_seen(&mut seen, "--service")?;
                parsed.service_name = Some(flag_value(args, &mut index, "--service", inline)?);
            }
            "--release" => {
                mark_seen(&mut seen, "--release")?;
                parsed.release = Some(flag_value(args, &mut index, "--release", inline)?);
            }
            "--environment" | "--env" => {
                mark_seen(&mut seen, "--environment")?;
                parsed.environment = Some(flag_value(args, &mut index, "--environment", inline)?);
            }
            "--start-kind" => {
                mark_seen(&mut seen, "--start-kind")?;
                parsed.start_kind = Some(flag_value(args, &mut index, "--start-kind", inline)?);
            }
            "--start-event" => {
                mark_seen(&mut seen, "--start-event")?;
                parsed.start_event = Some(flag_value(args, &mut index, "--start-event", inline)?);
            }
            "--return-kind" => {
                mark_seen(&mut seen, "--return-kind")?;
                parsed.return_kind = Some(flag_value(args, &mut index, "--return-kind", inline)?);
            }
            "--return-event" => {
                mark_seen(&mut seen, "--return-event")?;
                parsed.return_event = Some(flag_value(args, &mut index, "--return-event", inline)?);
            }
            "--interval" => {
                mark_seen(&mut seen, "--interval")?;
                parsed.interval = Some(flag_value(args, &mut index, "--interval", inline)?);
            }
            "--interval-count" => {
                mark_seen(&mut seen, "--interval-count")?;
                parsed.interval_count =
                    Some(flag_value(args, &mut index, "--interval-count", inline)?);
            }
            "--mode" => {
                mark_seen(&mut seen, "--mode")?;
                parsed.mode = Some(flag_value(args, &mut index, "--mode", inline)?);
            }
            "--cohort-mode" => {
                mark_seen(&mut seen, "--cohort-mode")?;
                parsed.cohort_mode = Some(flag_value(args, &mut index, "--cohort-mode", inline)?);
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
            return Err(invalid_argument("invalid project id"));
        }
        let since = normalize_text(required(self.since.as_deref(), "since")?, 64)?;
        let until = normalize_optional(self.until.as_deref(), 64)?;
        let service_name = normalize_optional(self.service_name.as_deref(), 256)?;
        let release = normalize_optional(self.release.as_deref(), 256)?;
        let environment = normalize_optional(self.environment.as_deref(), 256)?;
        let start_kind = normalize_event_kind(required(self.start_kind.as_deref(), "start-kind")?)?;
        let start_event = normalize_event_name(
            start_kind,
            required(self.start_event.as_deref(), "start-event")?,
            "invalid start event",
        )?;
        let return_kind =
            normalize_event_kind(required(self.return_kind.as_deref(), "return-kind")?)?;
        let return_event = normalize_event_name(
            return_kind,
            required(self.return_event.as_deref(), "return-event")?,
            "invalid return event",
        )?;
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
    value.ok_or(CliError::MissingArgument {
        argument,
        next: ANALYTICS_RETENTION_NEXT_STEP,
    })
}

/// Normalizes one supported classified event kind.
fn normalize_event_kind(value: &str) -> Result<AnalyticsRetentionEventKind, CliError> {
    match value.trim() {
        "page-view" | "page_view" | "page" => Ok(AnalyticsRetentionEventKind::PageView),
        "screen-view" | "screen_view" | "screen" => Ok(AnalyticsRetentionEventKind::ScreenView),
        "interaction" => Ok(AnalyticsRetentionEventKind::Interaction),
        _ => Err(invalid_argument("invalid retention event kind")),
    }
}

/// Applies the server's exact public event-name bounds before any request.
fn normalize_event_name(
    kind: AnalyticsRetentionEventKind,
    value: &str,
    error: &'static str,
) -> Result<String, CliError> {
    let value = normalize_text(value, 256)?;
    if kind == AnalyticsRetentionEventKind::Interaction
        && (value.len() > 64
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
            }))
    {
        return Err(invalid_argument(error));
    }
    Ok(value)
}

/// Normalizes the selected fixed interval or applies the safe CLI default.
fn normalize_interval(value: Option<&str>) -> Result<AnalyticsRetentionInterval, CliError> {
    match value.map(str::trim) {
        None | Some("day" | "1d") => Ok(AnalyticsRetentionInterval::Day),
        Some("hour" | "1h") => Ok(AnalyticsRetentionInterval::Hour),
        Some("week" | "1w") => Ok(AnalyticsRetentionInterval::Week),
        Some("thirty-day" | "thirty_day" | "30d") => Ok(AnalyticsRetentionInterval::ThirtyDay),
        Some(_) => Err(invalid_argument("invalid retention interval")),
    }
}

/// Normalizes exact or rolling return semantics.
fn normalize_mode(value: Option<&str>) -> Result<AnalyticsRetentionMode, CliError> {
    match value.map(str::trim) {
        None | Some("return-on" | "return_on" | "exact") => Ok(AnalyticsRetentionMode::ReturnOn),
        Some("return-on-or-after" | "return_on_or_after" | "rolling") => {
            Ok(AnalyticsRetentionMode::ReturnOnOrAfter)
        }
        Some(_) => Err(invalid_argument("invalid retention mode")),
    }
}

/// Normalizes the only version-1 cohort-anchor mode.
fn normalize_cohort_mode(value: Option<&str>) -> Result<AnalyticsRetentionCohortMode, CliError> {
    match value.map(str::trim) {
        None | Some("first-in-range" | "first_in_range") => {
            Ok(AnalyticsRetentionCohortMode::FirstInRange)
        }
        Some(_) => Err(invalid_argument("invalid retention cohort mode")),
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
        .ok_or_else(|| invalid_argument("invalid retention interval count"))
}

/// Trims one non-empty, control-free bounded public value.
fn normalize_text(value: &str, limit: usize) -> Result<String, CliError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > limit || value.chars().any(char::is_control) {
        return Err(invalid_argument("invalid analytics retention value"));
    }
    Ok(value.to_owned())
}

/// Normalizes one optional bounded context value.
fn normalize_optional(value: Option<&str>, limit: usize) -> Result<Option<String>, CliError> {
    value.map(|value| normalize_text(value, limit)).transpose()
}

/// Reads a separate or inline flag value without swallowing another flag.
fn flag_value(
    args: &[String],
    index: &mut usize,
    flag: &'static str,
    inline: Option<&str>,
) -> Result<String, CliError> {
    let value = inline.unwrap_or_else(|| {
        *index += 1;
        args.get(*index).map(String::as_str).unwrap_or_default()
    });
    if value.is_empty() || value.starts_with('-') {
        return Err(CliError::MissingFlagValue {
            flag,
            next: ANALYTICS_RETENTION_NEXT_STEP,
        });
    }
    Ok(value.to_owned())
}

/// Rejects values attached to boolean flags.
fn reject_inline(flag: &str, inline: Option<&str>) -> Result<(), CliError> {
    if inline.is_some() {
        Err(CliError::UnknownFlag {
            flag: flag.to_owned(),
            next: ANALYTICS_RETENTION_NEXT_STEP,
        })
    } else {
        Ok(())
    }
}

/// Marks a canonical flag and rejects aliases used together.
fn mark_seen(seen: &mut Vec<&'static str>, flag: &'static str) -> Result<(), CliError> {
    if seen.contains(&flag) {
        return Err(CliError::DuplicateFlag {
            flag,
            next: if flag == "--json" {
                "use --json once"
            } else {
                ANALYTICS_RETENTION_NEXT_STEP
            },
        });
    }
    seen.push(flag);
    Ok(())
}

/// Splits one inline `--flag=value` token.
fn split_flag(value: &str) -> (&str, Option<&str>) {
    value
        .split_once('=')
        .map_or((value, None), |(flag, value)| (flag, Some(value)))
}

/// Returns a value-free deterministic grammar error.
fn invalid_argument(argument: &'static str) -> CliError {
    CliError::UnexpectedArgument {
        argument: argument.to_owned(),
        command: "analytics retention",
        next: ANALYTICS_RETENTION_NEXT_STEP,
    }
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
        assert_eq!(options.start_kind, AnalyticsRetentionEventKind::PageView);
        assert_eq!(
            options.return_kind,
            AnalyticsRetentionEventKind::Interaction
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
