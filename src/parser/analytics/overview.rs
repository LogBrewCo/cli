//! Closed product-analytics overview command grammar.

use super::{Grammar, ScopeFlags};
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
        if parsed
            .scope
            .parse(GRAMMAR, &mut seen, args, &mut index, true)?
        {
            index += 1;
            continue;
        }
        let raw = &args[index];
        let (flag, inline) = Grammar::split_flag(raw);
        match flag {
            "--interval" => {
                GRAMMAR.mark_seen(&mut seen, "--interval")?;
                parsed.interval =
                    Some(GRAMMAR.flag_value(args, &mut index, "--interval", inline)?);
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
        json: parsed.scope.json,
    })
}

/// Partially parsed flags before required-value and shape validation.
#[cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "fields directly represent the closed analytics overview grammar"
    )
)]
#[derive(Default)]
struct ParsedOverviewFlags {
    scope: ScopeFlags,
    interval: Option<String>,
    top_limit: Option<String>,
}

impl ParsedOverviewFlags {
    /// Requires and normalizes every public request field.
    fn finish(&self) -> Result<AnalyticsOverviewOptions, CliError> {
        let scope = self
            .scope
            .finish(GRAMMAR, "invalid analytics overview value")?;
        let interval = normalize_interval(self.interval.as_deref())?;
        let top_limit = GRAMMAR.bounded_u8(
            self.top_limit.as_deref().map(str::trim),
            10,
            1..=20,
            "invalid analytics overview top limit",
        )?;

        Ok(AnalyticsOverviewOptions {
            project_id: scope.project_id,
            since: scope.since,
            until: scope.until,
            interval,
            service_name: scope.service_name,
            release: scope.release,
            environment: scope.environment,
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

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::tests::args;

    #[test]
    fn parses_exact_scope_aliases_and_defaults() -> Result<(), Box<dyn std::error::Error>> {
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
        ]))?;

        let Command::AnalyticsOverview { options, json } = command else {
            return Err("wrong command".into());
        };
        assert_eq!(options.project_id, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        assert_eq!(options.interval, "5m");
        assert_eq!(options.service_name.as_deref(), Some("checkout-api"));
        assert_eq!(options.environment.as_deref(), Some("production"));
        assert_eq!(options.top_limit, 12);
        assert!(json);
        Ok(())
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
