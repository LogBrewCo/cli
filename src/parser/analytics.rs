//! Closed product-analytics command grammar.

mod compare;
mod funnel;
mod lifecycle;
mod overview;
mod retention;

use crate::ids::is_uuid;
use crate::{
    AnalyticsPathDirection, AnalyticsPathEventKind, AnalyticsPathOptions, CliError, Command,
};

/// Exact recovery text shared by every malformed path invocation.
pub(super) const ANALYTICS_PATHS_NEXT_STEP: &str = "use logbrew analytics paths following|preceding --project <project_id> --since <24h|RFC3339> --anchor-kind <page-view|screen-view|interaction> --anchor-event <name> with optional --until, --service, --release, --environment, --depth 1-8, --path-limit 1-20, --keep-repeated, and --json";

/// Exact recovery text for the product-analytics namespace.
pub(super) const ANALYTICS_NEXT_STEP: &str = "use logbrew analytics overview --help, logbrew analytics compare --help, logbrew analytics paths --help, logbrew analytics funnel --help, logbrew analytics retention --help, or logbrew analytics lifecycle --help";

/// Parses the closed product-analytics namespace.
pub(super) fn parse_analytics(args: &[String]) -> Result<Command, CliError> {
    let normalized = normalize_json(args)?;
    let Some((resource, tail)) = normalized.split_first() else {
        return Err(CliError::MissingArgument {
            argument: "resource",
            next: ANALYTICS_NEXT_STEP,
        });
    };
    match resource.as_str() {
        "compare" | "comparison" | "segments" => compare::parse_compare(tail),
        "funnel" | "funnels" => funnel::parse_funnel(tail),
        "lifecycle" => lifecycle::parse_lifecycle(tail),
        "overview" => overview::parse_overview(tail),
        "paths" => parse_paths(tail),
        "retention" => retention::parse_retention(tail),
        _ => Err(CliError::UnknownResource {
            resource: resource.clone(),
            next: ANALYTICS_NEXT_STEP,
        }),
    }
}

/// Allows the output flag at any command position while keeping it singular.
fn normalize_json(args: &[String]) -> Result<Vec<String>, CliError> {
    let json_count = args
        .iter()
        .filter(|argument| argument.as_str() == "--json")
        .count();
    if json_count > 1 {
        return Err(CliError::DuplicateFlag {
            flag: "--json",
            next: "use --json once",
        });
    }
    let mut normalized = args
        .iter()
        .filter(|argument| argument.as_str() != "--json")
        .cloned()
        .collect::<Vec<_>>();
    if json_count == 1 {
        normalized.push(String::from("--json"));
    }
    Ok(normalized)
}

/// Parses one exact direction and its bounded path flags.
fn parse_paths(args: &[String]) -> Result<Command, CliError> {
    let Some((direction, flags)) = args.split_first() else {
        return Err(CliError::MissingArgument {
            argument: "direction",
            next: ANALYTICS_PATHS_NEXT_STEP,
        });
    };
    let direction = match direction.as_str() {
        "following" | "after" => AnalyticsPathDirection::Following,
        "preceding" | "before" => AnalyticsPathDirection::Preceding,
        value if value.starts_with('-') => {
            return Err(CliError::MissingArgument {
                argument: "direction",
                next: ANALYTICS_PATHS_NEXT_STEP,
            });
        }
        _ => return Err(invalid_argument("invalid direction")),
    };

    let mut parsed = ParsedPathFlags::default();
    let mut seen = Vec::new();
    let mut index = 0;
    while index < flags.len() {
        let raw = &flags[index];
        let (flag, inline) = split_flag(raw);
        match flag {
            "--json" => {
                reject_inline(flag, inline)?;
                mark_seen(&mut seen, "--json")?;
                parsed.json = true;
            }
            "--keep-repeated" => {
                reject_inline(flag, inline)?;
                mark_seen(&mut seen, "--keep-repeated")?;
                parsed.collapse_repeated = false;
            }
            "--project" | "--project-id" => {
                mark_seen(&mut seen, "--project")?;
                parsed.project_id = Some(flag_value(flags, &mut index, "--project", inline)?);
            }
            "--since" => {
                mark_seen(&mut seen, "--since")?;
                parsed.since = Some(flag_value(flags, &mut index, "--since", inline)?);
            }
            "--until" => {
                mark_seen(&mut seen, "--until")?;
                parsed.until = Some(flag_value(flags, &mut index, "--until", inline)?);
            }
            "--service" | "--service-name" => {
                mark_seen(&mut seen, "--service")?;
                parsed.service_name = Some(flag_value(flags, &mut index, "--service", inline)?);
            }
            "--release" => {
                mark_seen(&mut seen, "--release")?;
                parsed.release = Some(flag_value(flags, &mut index, "--release", inline)?);
            }
            "--environment" | "--env" => {
                mark_seen(&mut seen, "--environment")?;
                parsed.environment = Some(flag_value(flags, &mut index, "--environment", inline)?);
            }
            "--anchor-kind" => {
                mark_seen(&mut seen, "--anchor-kind")?;
                parsed.anchor_kind = Some(flag_value(flags, &mut index, "--anchor-kind", inline)?);
            }
            "--anchor-event" => {
                mark_seen(&mut seen, "--anchor-event")?;
                parsed.anchor_event =
                    Some(flag_value(flags, &mut index, "--anchor-event", inline)?);
            }
            "--depth" => {
                mark_seen(&mut seen, "--depth")?;
                parsed.depth = Some(flag_value(flags, &mut index, "--depth", inline)?);
            }
            "--path-limit" => {
                mark_seen(&mut seen, "--path-limit")?;
                parsed.path_limit = Some(flag_value(flags, &mut index, "--path-limit", inline)?);
            }
            value if value.starts_with('-') => {
                return Err(CliError::UnknownFlag {
                    flag: flag.to_owned(),
                    next: ANALYTICS_PATHS_NEXT_STEP,
                });
            }
            _ => {
                return Err(CliError::UnexpectedArgument {
                    argument: raw.clone(),
                    command: "analytics paths",
                    next: ANALYTICS_PATHS_NEXT_STEP,
                });
            }
        }
        index += 1;
    }

    Ok(Command::AnalyticsPaths {
        options: parsed.finish(direction)?,
        json: parsed.json,
    })
}

/// Partially parsed flags before required-value and shape validation.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "fields directly represent the closed analytics path grammar"
)]
struct ParsedPathFlags {
    project_id: Option<String>,
    since: Option<String>,
    until: Option<String>,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    anchor_kind: Option<String>,
    anchor_event: Option<String>,
    depth: Option<String>,
    path_limit: Option<String>,
    collapse_repeated: bool,
    json: bool,
}

impl Default for ParsedPathFlags {
    fn default() -> Self {
        Self {
            project_id: None,
            since: None,
            until: None,
            service_name: None,
            release: None,
            environment: None,
            anchor_kind: None,
            anchor_event: None,
            depth: None,
            path_limit: None,
            collapse_repeated: true,
            json: false,
        }
    }
}

impl ParsedPathFlags {
    /// Requires and normalizes every public request field.
    fn finish(&self, direction: AnalyticsPathDirection) -> Result<AnalyticsPathOptions, CliError> {
        let project_id = required(self.project_id.as_deref(), "project")?.trim();
        if !is_uuid(project_id) {
            return Err(invalid_argument("invalid project id"));
        }
        let since = normalize_text(required(self.since.as_deref(), "since")?, 64)?;
        let until = self
            .until
            .as_deref()
            .map(|value| normalize_text(value, 64))
            .transpose()?;
        let service_name = normalize_optional(self.service_name.as_deref(), 256)?;
        let release = normalize_optional(self.release.as_deref(), 256)?;
        let environment = normalize_optional(self.environment.as_deref(), 256)?;
        let anchor_kind =
            normalize_anchor_kind(required(self.anchor_kind.as_deref(), "anchor-kind")?)?;
        let anchor_event = normalize_anchor_event(
            anchor_kind,
            required(self.anchor_event.as_deref(), "anchor-event")?,
        )?;
        let depth = bounded_u8(self.depth.as_deref(), 4, 1, 8, "invalid depth")?;
        let path_limit = bounded_u8(self.path_limit.as_deref(), 10, 1, 20, "invalid path limit")?;

        Ok(AnalyticsPathOptions {
            project_id: project_id.to_ascii_lowercase(),
            since,
            until,
            service_name,
            release,
            environment,
            direction,
            anchor_kind,
            anchor_event,
            depth,
            collapse_repeated: self.collapse_repeated,
            path_limit,
        })
    }
}

/// Requires one named flag value.
fn required<'a>(value: Option<&'a str>, argument: &'static str) -> Result<&'a str, CliError> {
    value.ok_or(CliError::MissingArgument {
        argument,
        next: ANALYTICS_PATHS_NEXT_STEP,
    })
}

/// Normalizes one classified path kind.
fn normalize_anchor_kind(value: &str) -> Result<AnalyticsPathEventKind, CliError> {
    match value.trim() {
        "page-view" | "page_view" | "page" => Ok(AnalyticsPathEventKind::PageView),
        "screen-view" | "screen_view" | "screen" => Ok(AnalyticsPathEventKind::ScreenView),
        "interaction" => Ok(AnalyticsPathEventKind::Interaction),
        _ => Err(invalid_argument("invalid anchor kind")),
    }
}

/// Applies the server's exact public event-name bounds before any request.
fn normalize_anchor_event(kind: AnalyticsPathEventKind, value: &str) -> Result<String, CliError> {
    let value = normalize_text(value, 256)?;
    if kind == AnalyticsPathEventKind::Interaction
        && (value.len() > 64
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
            }))
    {
        return Err(invalid_argument("invalid interaction anchor event"));
    }
    Ok(value)
}

/// Trims one non-empty, control-free bounded public value.
fn normalize_text(value: &str, limit: usize) -> Result<String, CliError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > limit || value.chars().any(char::is_control) {
        return Err(invalid_argument("invalid analytics path value"));
    }
    Ok(value.to_owned())
}

/// Normalizes one optional bounded context value.
fn normalize_optional(value: Option<&str>, limit: usize) -> Result<Option<String>, CliError> {
    value.map(|value| normalize_text(value, limit)).transpose()
}

/// Parses a bounded integer or supplies its public default.
fn bounded_u8(
    value: Option<&str>,
    default: u8,
    minimum: u8,
    maximum: u8,
    error: &'static str,
) -> Result<u8, CliError> {
    let Some(value) = value else {
        return Ok(default);
    };
    value
        .parse::<u8>()
        .ok()
        .filter(|value| (minimum..=maximum).contains(value))
        .ok_or_else(|| invalid_argument(error))
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
            next: ANALYTICS_PATHS_NEXT_STEP,
        });
    }
    Ok(value.to_owned())
}

/// Rejects values attached to boolean flags.
fn reject_inline(flag: &str, inline: Option<&str>) -> Result<(), CliError> {
    if inline.is_some() {
        Err(CliError::UnknownFlag {
            flag: flag.to_owned(),
            next: ANALYTICS_PATHS_NEXT_STEP,
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
                ANALYTICS_PATHS_NEXT_STEP
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
        command: "analytics paths",
        next: ANALYTICS_PATHS_NEXT_STEP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_aliases_into_one_stable_request() {
        let command = parse_analytics(&args(&[
            "paths",
            "after",
            "--project-id",
            "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA",
            "--since=24h",
            "--anchor-kind",
            "page-view",
            "--anchor-event",
            "/pricing",
            "--env",
            "production",
            "--keep-repeated",
            "--json",
        ]))
        .expect("valid command");

        let Command::AnalyticsPaths { options, json } = command else {
            panic!("wrong command");
        };
        assert_eq!(options.direction, AnalyticsPathDirection::Following);
        assert_eq!(options.anchor_kind, AnalyticsPathEventKind::PageView);
        assert_eq!(options.project_id, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        assert_eq!(options.depth, 4);
        assert_eq!(options.path_limit, 10);
        assert!(!options.collapse_repeated);
        assert!(json);
    }

    #[test]
    fn rejects_missing_unsafe_duplicate_and_out_of_range_values() {
        let base = [
            "paths",
            "following",
            "--project",
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "--since",
            "24h",
            "--anchor-kind",
            "interaction",
            "--anchor-event",
            "checkout_started",
        ];
        assert!(parse_analytics(&args(&base[..8])).is_err());

        let mut unsafe_event = base.to_vec();
        unsafe_event[9] = "checkout started";
        assert!(parse_analytics(&args(&unsafe_event)).is_err());

        let mut duplicate = base.to_vec();
        duplicate.extend(["--project-id", "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"]);
        assert!(matches!(
            parse_analytics(&args(&duplicate)),
            Err(CliError::DuplicateFlag {
                flag: "--project",
                ..
            })
        ));

        let mut too_deep = base.to_vec();
        too_deep.extend(["--depth", "9"]);
        assert!(parse_analytics(&args(&too_deep)).is_err());
    }
}
