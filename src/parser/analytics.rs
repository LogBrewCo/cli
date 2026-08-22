//! Closed product-analytics command grammar.

use std::collections::HashSet;

mod compare;
mod funnel;
mod lifecycle;
mod overview;
mod properties;
mod retention;

use crate::ids::is_uuid;
use crate::{
    AnalyticsPathDirection, AnalyticsPathEventKind, AnalyticsPathOptions,
    AnalyticsPathPropertyFilter, AnalyticsRetentionEventKind, CliError, Command,
};

/// Exact recovery text shared by every malformed path invocation.
pub(super) const ANALYTICS_PATHS_NEXT_STEP: &str = "use logbrew analytics paths following|preceding --project <project_id> --since <24h|RFC3339> --anchor-kind <page-view|screen-view|interaction> --anchor-event <name> with optional repeated --property <key=value>, --until, --service, --release, --environment, --depth 1-8, --path-limit 1-20, --keep-repeated, and --json";

/// Exact recovery text for the product-analytics namespace.
pub(super) const ANALYTICS_NEXT_STEP: &str = "use logbrew analytics overview --help, logbrew analytics properties --help, logbrew analytics compare --help, logbrew analytics paths --help, logbrew analytics funnel --help, logbrew analytics retention --help, or logbrew analytics lifecycle --help";

/// Canonical parser behavior for path analysis.
const PATH_GRAMMAR: Grammar = Grammar::new("analytics paths", ANALYTICS_PATHS_NEXT_STEP);

/// Shared closed-grammar behavior for one analytics command.
#[derive(Clone, Copy)]
pub(super) struct Grammar {
    /// Stable command name used by deterministic parse errors.
    command: &'static str,
    /// Stable recovery text for every malformed invocation.
    next: &'static str,
}

impl Grammar {
    /// Creates one command-specific grammar helper.
    pub(super) const fn new(command: &'static str, next: &'static str) -> Self {
        Self { command, next }
    }

    /// Reads a separate or inline flag value without swallowing another flag.
    pub(super) fn flag_value(
        self,
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
                next: self.next,
            });
        }
        Ok(value.to_owned())
    }

    /// Rejects values attached to boolean flags.
    pub(super) fn reject_inline(self, flag: &str, inline: Option<&str>) -> Result<(), CliError> {
        if inline.is_some() {
            Err(CliError::UnknownFlag {
                flag: flag.to_owned(),
                next: self.next,
            })
        } else {
            Ok(())
        }
    }

    /// Marks a canonical singular flag and rejects aliases used together.
    pub(super) fn mark_seen(
        self,
        seen: &mut Vec<&'static str>,
        flag: &'static str,
    ) -> Result<(), CliError> {
        if seen.contains(&flag) {
            return Err(CliError::DuplicateFlag {
                flag,
                next: if flag == "--json" {
                    "use --json once"
                } else {
                    self.next
                },
            });
        }
        seen.push(flag);
        Ok(())
    }

    /// Requires one named flag.
    pub(super) fn required<'a>(
        self,
        value: Option<&'a str>,
        argument: &'static str,
    ) -> Result<&'a str, CliError> {
        value.ok_or(CliError::MissingArgument {
            argument,
            next: self.next,
        })
    }

    /// Trims one non-empty, control-free bounded public value.
    pub(super) fn normalize_text(
        self,
        value: &str,
        limit: usize,
        error: &'static str,
    ) -> Result<String, CliError> {
        let value = value.trim();
        if value.is_empty() || value.chars().count() > limit || value.chars().any(char::is_control)
        {
            return Err(self.invalid_argument(error));
        }
        Ok(value.to_owned())
    }

    /// Normalizes one optional bounded context value.
    pub(super) fn normalize_optional(
        self,
        value: Option<&str>,
        limit: usize,
        error: &'static str,
    ) -> Result<Option<String>, CliError> {
        value
            .map(|value| self.normalize_text(value, limit, error))
            .transpose()
    }

    /// Normalizes one classified product-event kind.
    pub(super) fn normalize_event_kind(
        self,
        value: &str,
        error: &'static str,
    ) -> Result<AnalyticsPathEventKind, CliError> {
        match value.trim() {
            "page-view" | "page_view" | "page" => Ok(AnalyticsPathEventKind::PageView),
            "screen-view" | "screen_view" | "screen" => Ok(AnalyticsPathEventKind::ScreenView),
            "interaction" => Ok(AnalyticsPathEventKind::Interaction),
            _ => Err(self.invalid_argument(error)),
        }
    }

    /// Applies the exact public event-name bounds before any request.
    pub(super) fn normalize_event_name(
        self,
        kind: AnalyticsPathEventKind,
        value: &str,
        value_error: &'static str,
        interaction_error: &'static str,
    ) -> Result<String, CliError> {
        let value = self.normalize_text(value, 256, value_error)?;
        if kind == AnalyticsPathEventKind::Interaction
            && (value.len() > 64
                || !value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
                }))
        {
            return Err(self.invalid_argument(interaction_error));
        }
        Ok(value)
    }

    /// Splits one inline `--flag=value` token.
    pub(super) fn split_flag(value: &str) -> (&str, Option<&str>) {
        value
            .split_once('=')
            .map_or((value, None), |(flag, value)| (flag, Some(value)))
    }

    /// Returns one value-free deterministic grammar error.
    pub(super) fn invalid_argument(self, argument: &'static str) -> CliError {
        CliError::UnexpectedArgument {
            argument: argument.to_owned(),
            command: self.command,
            next: self.next,
        }
    }
}

/// Shared project, time, deployment, and output flags for analytics commands.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "fields directly represent the shared analytics grammar"
)]
#[derive(Default)]
struct ScopeFlags {
    project_id: Option<String>,
    since: Option<String>,
    until: Option<String>,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    json: bool,
}

impl ScopeFlags {
    /// Parses a shared flag and reports whether it consumed the token.
    fn parse(
        &mut self,
        grammar: Grammar,
        seen: &mut Vec<&'static str>,
        args: &[String],
        index: &mut usize,
        deployment: bool,
    ) -> Result<bool, CliError> {
        let (flag, inline) = Grammar::split_flag(&args[*index]);
        let (canonical, destination) = match flag {
            "--project" | "--project-id" => ("--project", &mut self.project_id),
            "--since" => ("--since", &mut self.since),
            "--until" => ("--until", &mut self.until),
            "--service" | "--service-name" if deployment => ("--service", &mut self.service_name),
            "--release" if deployment => ("--release", &mut self.release),
            "--environment" | "--env" if deployment => ("--environment", &mut self.environment),
            "--json" => {
                grammar.reject_inline(flag, inline)?;
                grammar.mark_seen(seen, "--json")?;
                self.json = true;
                return Ok(true);
            }
            _ => return Ok(false),
        };
        grammar.mark_seen(seen, canonical)?;
        *destination = Some(grammar.flag_value(args, index, canonical, inline)?);
        Ok(true)
    }

    /// Requires and normalizes the shared analytics request scope.
    fn finish(
        &self,
        grammar: Grammar,
        value_error: &'static str,
    ) -> Result<AnalyticsScope, CliError> {
        let project_id = grammar
            .required(self.project_id.as_deref(), "project")?
            .trim();
        if !is_uuid(project_id) {
            return Err(grammar.invalid_argument("invalid project id"));
        }
        Ok(AnalyticsScope {
            project_id: project_id.to_ascii_lowercase(),
            since: grammar.normalize_text(
                grammar.required(self.since.as_deref(), "since")?,
                64,
                value_error,
            )?,
            until: grammar.normalize_optional(self.until.as_deref(), 64, value_error)?,
            service_name: grammar.normalize_optional(
                self.service_name.as_deref(),
                256,
                value_error,
            )?,
            release: grammar.normalize_optional(self.release.as_deref(), 256, value_error)?,
            environment: grammar.normalize_optional(
                self.environment.as_deref(),
                256,
                value_error,
            )?,
        })
    }
}

/// Normalized shared analytics request scope.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "fields mirror normalized analytics request scope"
)]
struct AnalyticsScope {
    project_id: String,
    since: String,
    until: Option<String>,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
}

/// Maps the shared grammar kind to the stable retention-family public type.
const fn retention_event_kind(kind: AnalyticsPathEventKind) -> AnalyticsRetentionEventKind {
    match kind {
        AnalyticsPathEventKind::PageView => AnalyticsRetentionEventKind::PageView,
        AnalyticsPathEventKind::ScreenView => AnalyticsRetentionEventKind::ScreenView,
        AnalyticsPathEventKind::Interaction => AnalyticsRetentionEventKind::Interaction,
    }
}

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
        "properties" | "property" | "dimensions" => properties::parse_properties(tail),
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
    let grammar = PATH_GRAMMAR;
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
        _ => return Err(grammar.invalid_argument("invalid direction")),
    };

    let parsed = parse_path_flags(flags)?;
    Ok(Command::AnalyticsPaths {
        options: parsed.finish(direction)?,
        json: parsed.scope.json,
    })
}

/// Collects path flags before their values are normalized together.
fn parse_path_flags(flags: &[String]) -> Result<ParsedPathFlags, CliError> {
    let grammar = PATH_GRAMMAR;
    let mut parsed = ParsedPathFlags::default();
    let mut seen = Vec::new();
    let mut index = 0;
    while index < flags.len() {
        if parsed
            .scope
            .parse(grammar, &mut seen, flags, &mut index, true)?
        {
            index += 1;
            continue;
        }
        let raw = &flags[index];
        let (flag, inline) = Grammar::split_flag(raw);
        match flag {
            "--keep-repeated" => {
                grammar.reject_inline(flag, inline)?;
                grammar.mark_seen(&mut seen, "--keep-repeated")?;
                parsed.collapse_repeated = false;
            }
            "--anchor-kind" => {
                grammar.mark_seen(&mut seen, "--anchor-kind")?;
                parsed.anchor_kind =
                    Some(grammar.flag_value(flags, &mut index, "--anchor-kind", inline)?);
            }
            "--anchor-event" => {
                grammar.mark_seen(&mut seen, "--anchor-event")?;
                parsed.anchor_event =
                    Some(grammar.flag_value(flags, &mut index, "--anchor-event", inline)?);
            }
            "--property" | "--property-filter" => {
                parsed.properties.push(grammar.flag_value(
                    flags,
                    &mut index,
                    "--property",
                    inline,
                )?);
            }
            "--depth" => {
                grammar.mark_seen(&mut seen, "--depth")?;
                parsed.depth = Some(grammar.flag_value(flags, &mut index, "--depth", inline)?);
            }
            "--path-limit" => {
                grammar.mark_seen(&mut seen, "--path-limit")?;
                parsed.path_limit =
                    Some(grammar.flag_value(flags, &mut index, "--path-limit", inline)?);
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
    Ok(parsed)
}

/// Partially parsed flags before required-value and shape validation.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "fields directly represent the closed analytics path grammar"
)]
struct ParsedPathFlags {
    scope: ScopeFlags,
    anchor_kind: Option<String>,
    anchor_event: Option<String>,
    properties: Vec<String>,
    depth: Option<String>,
    path_limit: Option<String>,
    collapse_repeated: bool,
}

impl Default for ParsedPathFlags {
    fn default() -> Self {
        Self {
            scope: ScopeFlags::default(),
            anchor_kind: None,
            anchor_event: None,
            properties: Vec::new(),
            depth: None,
            path_limit: None,
            collapse_repeated: true,
        }
    }
}

impl ParsedPathFlags {
    /// Requires and normalizes every public request field.
    fn finish(&self, direction: AnalyticsPathDirection) -> Result<AnalyticsPathOptions, CliError> {
        let scope = self
            .scope
            .finish(PATH_GRAMMAR, "invalid analytics path value")?;
        let anchor_kind = PATH_GRAMMAR.normalize_event_kind(
            required(self.anchor_kind.as_deref(), "anchor-kind")?,
            "invalid anchor kind",
        )?;
        let anchor_event = PATH_GRAMMAR.normalize_event_name(
            anchor_kind,
            required(self.anchor_event.as_deref(), "anchor-event")?,
            "invalid analytics path value",
            "invalid interaction anchor event",
        )?;
        let property_filters = normalize_path_property_filters(self.properties.as_slice())?;
        let depth = bounded_u8(self.depth.as_deref(), 4, 1, 8, "invalid depth")?;
        let path_limit = bounded_u8(self.path_limit.as_deref(), 10, 1, 20, "invalid path limit")?;

        Ok(AnalyticsPathOptions {
            project_id: scope.project_id,
            since: scope.since,
            until: scope.until,
            service_name: scope.service_name,
            release: scope.release,
            environment: scope.environment,
            direction,
            anchor_kind,
            anchor_event,
            property_filters,
            depth,
            collapse_repeated: self.collapse_repeated,
            path_limit,
        })
    }
}

/// Normalizes, deduplicates, and canonically orders exact anchor predicates.
fn normalize_path_property_filters(
    values: &[String],
) -> Result<Vec<AnalyticsPathPropertyFilter>, CliError> {
    if values.len() > 4 {
        return Err(PATH_GRAMMAR.invalid_argument("too many analytics path property filters"));
    }
    let mut keys = HashSet::with_capacity(values.len());
    let mut filters = values
        .iter()
        .map(|raw| {
            let (key, value) = raw.split_once('=').ok_or_else(|| {
                PATH_GRAMMAR.invalid_argument("invalid analytics path property assignment")
            })?;
            let key = normalize_property_key(key)?;
            if !keys.insert(key.clone()) {
                return Err(PATH_GRAMMAR.invalid_argument("duplicate analytics path property key"));
            }
            Ok(AnalyticsPathPropertyFilter {
                key,
                value: normalize_text(value, 256)?,
            })
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    filters.sort_unstable_by(|left, right| left.key.cmp(&right.key));
    Ok(filters)
}

/// Applies the backend's exact safe analytics-property key contract.
pub(super) fn normalize_property_key(value: &str) -> Result<String, CliError> {
    let value = value.trim();
    if crate::analytics_property_contract::is_safe_key(value) {
        Ok(value.to_owned())
    } else {
        Err(PATH_GRAMMAR.invalid_argument("unsupported analytics property key"))
    }
}

/// Requires one named flag value.
fn required<'a>(value: Option<&'a str>, argument: &'static str) -> Result<&'a str, CliError> {
    PATH_GRAMMAR.required(value, argument)
}

/// Trims one non-empty, control-free bounded public value.
fn normalize_text(value: &str, limit: usize) -> Result<String, CliError> {
    PATH_GRAMMAR.normalize_text(value, limit, "invalid analytics path value")
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
        .ok_or_else(|| PATH_GRAMMAR.invalid_argument(error))
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
            "--property",
            "tag.plan=pro",
            "--property=resource.framework.name=React",
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
        assert_eq!(options.property_filters.len(), 2);
        assert_eq!(options.property_filters[0].key, "resource.framework.name");
        assert_eq!(options.property_filters[0].value, "React");
        assert_eq!(options.property_filters[1].key, "tag.plan");
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

        let mut unsafe_property = base.to_vec();
        unsafe_property.extend(["--property", "tag.user_id=123"]);
        assert!(parse_analytics(&args(&unsafe_property)).is_err());

        let mut duplicate_property = base.to_vec();
        duplicate_property.extend([
            "--property",
            "tag.plan=free",
            "--property-filter",
            "tag.plan=pro",
        ]);
        assert!(parse_analytics(&args(&duplicate_property)).is_err());

        let mut too_many_properties = base.to_vec();
        too_many_properties.extend([
            "--property",
            "tag.a=1",
            "--property",
            "tag.b=2",
            "--property",
            "tag.c=3",
            "--property",
            "tag.d=4",
            "--property",
            "tag.e=5",
        ]);
        assert!(parse_analytics(&args(&too_many_properties)).is_err());
    }
}
