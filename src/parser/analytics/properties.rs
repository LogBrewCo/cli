//! Closed product-analytics property-catalog command grammar.

use super::{Grammar, ScopeFlags};
use crate::{AnalyticsPropertyOptions, CliError, Command};

/// Exact recovery text shared by every malformed property-catalog invocation.
pub(super) const ANALYTICS_PROPERTIES_NEXT_STEP: &str = "use logbrew analytics properties --project <project_id> --since <24h|RFC3339> with optional --until, --service, --release, --environment, --limit 1-50, and --json";

/// Canonical parser behavior for property-catalog reads.
const GRAMMAR: Grammar = Grammar::new("analytics properties", ANALYTICS_PROPERTIES_NEXT_STEP);

/// Parses one bounded privacy-safe analytics property catalog read.
pub(super) fn parse_properties(args: &[String]) -> Result<Command, CliError> {
    let mut parsed = ParsedPropertyFlags::default();
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
            "--limit" => {
                GRAMMAR.mark_seen(&mut seen, "--limit")?;
                parsed.limit = Some(GRAMMAR.flag_value(args, &mut index, "--limit", inline)?);
            }
            value if value.starts_with('-') => {
                return Err(CliError::UnknownFlag {
                    flag: flag.to_owned(),
                    next: ANALYTICS_PROPERTIES_NEXT_STEP,
                });
            }
            _ => {
                return Err(CliError::UnexpectedArgument {
                    argument: raw.clone(),
                    command: "analytics properties",
                    next: ANALYTICS_PROPERTIES_NEXT_STEP,
                });
            }
        }
        index += 1;
    }

    Ok(Command::AnalyticsProperties {
        options: parsed.finish()?,
        json: parsed.scope.json,
    })
}

/// Partially parsed flags before required-value and shape validation.
#[cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "fields directly represent the closed analytics property grammar"
    )
)]
#[derive(Default)]
struct ParsedPropertyFlags {
    scope: ScopeFlags,
    limit: Option<String>,
}

impl ParsedPropertyFlags {
    /// Requires and normalizes every public request field.
    fn finish(&self) -> Result<AnalyticsPropertyOptions, CliError> {
        let scope = self
            .scope
            .finish(GRAMMAR, "invalid analytics property value")?;
        Ok(AnalyticsPropertyOptions {
            project_id: scope.project_id,
            since: scope.since,
            until: scope.until,
            service_name: scope.service_name,
            release: scope.release,
            environment: scope.environment,
            limit: GRAMMAR.bounded_u8(
                self.limit.as_deref().map(str::trim),
                20,
                1..=50,
                "invalid analytics property limit",
            )?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::tests::args;

    #[test]
    fn parses_exact_scope_aliases_and_defaults() -> Result<(), Box<dyn std::error::Error>> {
        let command = parse_properties(&args(&[
            "--project-id",
            "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA",
            "--since=24h",
            "--service-name",
            "checkout-api",
            "--env",
            "production",
            "--limit",
            "40",
            "--json",
        ]))?;

        let Command::AnalyticsProperties { options, json } = command else {
            return Err("wrong command".into());
        };
        assert_eq!(options.project_id, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        assert_eq!(options.service_name.as_deref(), Some("checkout-api"));
        assert_eq!(options.environment.as_deref(), Some("production"));
        assert_eq!(options.limit, 40);
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
                "--limit",
                "51",
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
            assert!(parse_properties(&args(values.as_slice())).is_err());
        }
    }
}
