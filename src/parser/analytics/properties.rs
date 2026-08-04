//! Closed product-analytics property-catalog command grammar.

use crate::ids::is_uuid;
use crate::{AnalyticsPropertyOptions, CliError, Command};

/// Exact recovery text shared by every malformed property-catalog invocation.
pub(super) const ANALYTICS_PROPERTIES_NEXT_STEP: &str = "use logbrew analytics properties --project <project_id> --since <24h|RFC3339> with optional --until, --service, --release, --environment, --limit 1-50, and --json";

/// Parses one bounded privacy-safe analytics property catalog read.
pub(super) fn parse_properties(args: &[String]) -> Result<Command, CliError> {
    let mut parsed = ParsedPropertyFlags::default();
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
            "--limit" => {
                mark_seen(&mut seen, "--limit")?;
                parsed.limit = Some(flag_value(args, &mut index, "--limit", inline)?);
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
        json: parsed.json,
    })
}

/// Partially parsed flags before required-value and shape validation.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "fields directly represent the closed analytics property grammar"
)]
#[derive(Default)]
struct ParsedPropertyFlags {
    project_id: Option<String>,
    since: Option<String>,
    until: Option<String>,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    limit: Option<String>,
    json: bool,
}

impl ParsedPropertyFlags {
    /// Requires and normalizes every public request field.
    fn finish(&self) -> Result<AnalyticsPropertyOptions, CliError> {
        let project_id = required(self.project_id.as_deref(), "project")?.trim();
        if !is_uuid(project_id) {
            return Err(invalid_argument("invalid project id"));
        }
        Ok(AnalyticsPropertyOptions {
            project_id: project_id.to_ascii_lowercase(),
            since: normalize_text(required(self.since.as_deref(), "since")?, 64)?,
            until: normalize_optional(self.until.as_deref(), 64)?,
            service_name: normalize_optional(self.service_name.as_deref(), 256)?,
            release: normalize_optional(self.release.as_deref(), 256)?,
            environment: normalize_optional(self.environment.as_deref(), 256)?,
            limit: bounded_limit(self.limit.as_deref())?,
        })
    }
}

/// Requires one named property-catalog flag.
fn required<'a>(value: Option<&'a str>, argument: &'static str) -> Result<&'a str, CliError> {
    value.ok_or(CliError::MissingArgument {
        argument,
        next: ANALYTICS_PROPERTIES_NEXT_STEP,
    })
}

/// Parses the bounded property-key limit with the public default.
fn bounded_limit(value: Option<&str>) -> Result<u8, CliError> {
    value.map_or(Ok(20), |value| {
        value
            .trim()
            .parse::<u8>()
            .ok()
            .filter(|limit| (1..=50).contains(limit))
            .ok_or_else(|| invalid_argument("invalid analytics property limit"))
    })
}

/// Trims one non-empty, control-free bounded public value.
fn normalize_text(value: &str, limit: usize) -> Result<String, CliError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > limit || value.chars().any(char::is_control) {
        return Err(invalid_argument("invalid analytics property value"));
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
            next: ANALYTICS_PROPERTIES_NEXT_STEP,
        });
    }
    Ok(value.to_owned())
}

/// Rejects values attached to boolean flags.
fn reject_inline(flag: &str, inline: Option<&str>) -> Result<(), CliError> {
    if inline.is_some() {
        Err(CliError::UnknownFlag {
            flag: flag.to_owned(),
            next: ANALYTICS_PROPERTIES_NEXT_STEP,
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
                ANALYTICS_PROPERTIES_NEXT_STEP
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
        command: "analytics properties",
        next: ANALYTICS_PROPERTIES_NEXT_STEP,
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
        ]))
        .expect("valid command");

        let Command::AnalyticsProperties { options, json } = command else {
            panic!("wrong command");
        };
        assert_eq!(options.project_id, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        assert_eq!(options.service_name.as_deref(), Some("checkout-api"));
        assert_eq!(options.environment.as_deref(), Some("production"));
        assert_eq!(options.limit, 40);
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
