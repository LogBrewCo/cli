//! Closed product-analytics overview command grammar.

use super::Grammar;
use crate::ids::is_uuid;
use crate::{AnalyticsOverviewOptions, CliError, Command};

/// Exact recovery text shared by every malformed overview invocation.
pub(super) const ANALYTICS_OVERVIEW_NEXT_STEP: &str = "use logbrew analytics overview --project <project_id> --since <24h|RFC3339> with optional --until, --interval auto|1m|5m|15m|1h|6h|1d, --service, --release, --environment, --top-limit 1-20, and --json";

/// Canonical parser behavior for the overview command.
const GRAMMAR: Grammar = Grammar::new("analytics overview", ANALYTICS_OVERVIEW_NEXT_STEP);

/// Parses one bounded project activity and capture-quality overview.
pub(super) fn parse_overview(args: &[String]) -> Result<Command, CliError> {
    let mut parsed = ParsedOverviewFlags::default();
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
            "--interval" => {
                GRAMMAR.mark_seen(&mut seen, "--interval")?;
                parsed.interval =
                    Some(GRAMMAR.flag_value(args, &mut index, "--interval", inline)?);
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
            "--top-limit" | "--limit" => {
                GRAMMAR.mark_seen(&mut seen, "--top-limit")?;
                parsed.top_limit =
                    Some(GRAMMAR.flag_value(args, &mut index, "--top-limit", inline)?);
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
        let project_id = GRAMMAR
            .required(self.project_id.as_deref(), "project")?
            .trim();
        if !is_uuid(project_id) {
            return Err(GRAMMAR.invalid_argument("invalid project id"));
        }
        let since = GRAMMAR.normalize_text(
            GRAMMAR.required(self.since.as_deref(), "since")?,
            64,
            "invalid analytics overview value",
        )?;
        let until = GRAMMAR.normalize_optional(
            self.until.as_deref(),
            64,
            "invalid analytics overview value",
        )?;
        let interval = normalize_interval(self.interval.as_deref())?;
        let service_name = GRAMMAR.normalize_optional(
            self.service_name.as_deref(),
            256,
            "invalid analytics overview value",
        )?;
        let release = GRAMMAR.normalize_optional(
            self.release.as_deref(),
            256,
            "invalid analytics overview value",
        )?;
        let environment = GRAMMAR.normalize_optional(
            self.environment.as_deref(),
            256,
            "invalid analytics overview value",
        )?;
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
        Some(_) => Err(GRAMMAR.invalid_argument("invalid analytics overview interval")),
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
            .ok_or_else(|| GRAMMAR.invalid_argument("invalid analytics overview top limit"))
    })
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
