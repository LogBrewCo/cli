//! Closed product-analytics overview command grammar.

use crate::ids::is_uuid;
use crate::{AnalyticsOverviewOptions, CliError, Command};

/// Exact recovery text shared by every malformed overview invocation.
pub(super) const ANALYTICS_OVERVIEW_NEXT_STEP: &str = "use logbrew analytics overview --project <project_id> --since <24h|RFC3339> with optional --until, --interval auto|1m|5m|15m|1h|6h|1d, --service, --release, --environment, --top-limit 1-20, and --json";

/// Parses one bounded project activity and capture-quality overview.
pub(super) fn parse_overview(args: &[String]) -> Result<Command, CliError> {
    let mut parsed = ParsedOverviewFlags::default();
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
            "--interval" => {
                mark_seen(&mut seen, "--interval")?;
                parsed.interval = Some(flag_value(args, &mut index, "--interval", inline)?);
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
            "--top-limit" | "--limit" => {
                mark_seen(&mut seen, "--top-limit")?;
                parsed.top_limit = Some(flag_value(args, &mut index, "--top-limit", inline)?);
            }
            value if value.starts_with('-') => {
                return Err(CliError::UnknownFlag {
                    flag: flag.to_owned(),
                    next: ANALYTICS_OVERVIEW_NEXT_STEP,
                });
            }
            _ => {
                return Err(CliError::UnexpectedArgument {
                    argument: raw.clone(),
                    command: "analytics overview",
                    next: ANALYTICS_OVERVIEW_NEXT_STEP,
                });
            }
        }
        index += 1;
    }

    Ok(Command::AnalyticsOverview {
        options: parsed.finish()?,
        json: parsed.json,
    })
}

/// Partially parsed flags before required-value and shape validation.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "fields directly represent the closed analytics overview grammar"
)]
#[derive(Default)]
struct ParsedOverviewFlags {
    project_id: Option<String>,
    since: Option<String>,
    until: Option<String>,
    interval: Option<String>,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    top_limit: Option<String>,
    json: bool,
}

impl ParsedOverviewFlags {
    /// Requires and normalizes every public request field.
    fn finish(&self) -> Result<AnalyticsOverviewOptions, CliError> {
        let project_id = required(self.project_id.as_deref(), "project")?.trim();
        if !is_uuid(project_id) {
            return Err(invalid_argument("invalid project id"));
        }
        let since = normalize_text(required(self.since.as_deref(), "since")?, 64)?;
        let until = normalize_optional(self.until.as_deref(), 64)?;
        let interval = normalize_interval(self.interval.as_deref())?;
        let service_name = normalize_optional(self.service_name.as_deref(), 256)?;
        let release = normalize_optional(self.release.as_deref(), 256)?;
        let environment = normalize_optional(self.environment.as_deref(), 256)?;
        let top_limit = bounded_top_limit(self.top_limit.as_deref())?;

        Ok(AnalyticsOverviewOptions {
            project_id: project_id.to_ascii_lowercase(),
            since,
            until,
            interval,
            service_name,
            release,
            environment,
            top_limit,
        })
    }
}

/// Requires one named overview flag.
fn required<'a>(value: Option<&'a str>, argument: &'static str) -> Result<&'a str, CliError> {
    value.ok_or(CliError::MissingArgument {
        argument,
        next: ANALYTICS_OVERVIEW_NEXT_STEP,
    })
}

/// Normalizes the automatic or fixed series interval.
fn normalize_interval(value: Option<&str>) -> Result<String, CliError> {
    match value.map(str::trim).map(str::to_ascii_lowercase) {
        None => Ok("auto".to_owned()),
        Some(value)
            if matches!(
                value.as_str(),
                "auto" | "1m" | "5m" | "15m" | "1h" | "6h" | "1d"
            ) =>
        {
            Ok(value)
        }
        Some(_) => Err(invalid_argument("invalid analytics overview interval")),
    }
}

/// Parses the bounded top-ranking limit with the public default.
fn bounded_top_limit(value: Option<&str>) -> Result<u8, CliError> {
    value.map_or(Ok(10), |value| {
        value
            .trim()
            .parse::<u8>()
            .ok()
            .filter(|limit| (1..=20).contains(limit))
            .ok_or_else(|| invalid_argument("invalid analytics overview top limit"))
    })
}

/// Trims one non-empty, control-free bounded public value.
fn normalize_text(value: &str, limit: usize) -> Result<String, CliError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > limit || value.chars().any(char::is_control) {
        return Err(invalid_argument("invalid analytics overview value"));
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
            next: ANALYTICS_OVERVIEW_NEXT_STEP,
        });
    }
    Ok(value.to_owned())
}

/// Rejects values attached to boolean flags.
fn reject_inline(flag: &str, inline: Option<&str>) -> Result<(), CliError> {
    if inline.is_some() {
        Err(CliError::UnknownFlag {
            flag: flag.to_owned(),
            next: ANALYTICS_OVERVIEW_NEXT_STEP,
        })
    } else {
        Ok(())
    }
}

/// Marks a canonical singular flag and rejects aliases used together.
fn mark_seen(seen: &mut Vec<&'static str>, flag: &'static str) -> Result<(), CliError> {
    if seen.contains(&flag) {
        return Err(CliError::DuplicateFlag {
            flag,
            next: if flag == "--json" {
                "use --json once"
            } else {
                ANALYTICS_OVERVIEW_NEXT_STEP
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
        command: "analytics overview",
        next: ANALYTICS_OVERVIEW_NEXT_STEP,
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
    fn parses_exact_scope_aliases_and_defaults() {
        let command = parse_overview(&args(&[
            "--project-id",
            "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA",
            "--since=24h",
            "--interval",
            "5M",
            "--service-name",
            "checkout-api",
            "--env",
            "production",
            "--limit",
            "12",
            "--json",
        ]))
        .expect("valid command");

        let Command::AnalyticsOverview { options, json } = command else {
            panic!("wrong command");
        };
        assert_eq!(options.project_id, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        assert_eq!(options.interval, "5m");
        assert_eq!(options.service_name.as_deref(), Some("checkout-api"));
        assert_eq!(options.environment.as_deref(), Some("production"));
        assert_eq!(options.top_limit, 12);
        assert!(json);
    }

    #[test]
    fn rejects_missing_duplicate_unknown_and_unbounded_values() {
        for values in [
            vec!["--since", "24h"],
            vec!["--project", "not-a-uuid", "--since", "24h"],
            vec![
                "--project",
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "--since",
                "24h",
                "--interval",
                "2m",
            ],
            vec![
                "--project",
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "--since",
                "24h",
                "--top-limit",
                "21",
            ],
            vec![
                "--project",
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "--project-id",
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "--since",
                "24h",
            ],
            vec![
                "--project",
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "--since",
                "24h",
                "--actor-id",
                "secret",
            ],
        ] {
            assert!(parse_overview(&args(values.as_slice())).is_err());
        }
    }
}
