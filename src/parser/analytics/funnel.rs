//! Closed product-analytics funnel command grammar.

use super::{Grammar, ScopeFlags};
use crate::{AnalyticsFunnelOptions, AnalyticsFunnelStep, AnalyticsFunnelUnit, CliError, Command};

/// Exact recovery text shared by every malformed funnel invocation.
pub(super) const ANALYTICS_FUNNEL_NEXT_STEP: &str = "use logbrew analytics funnel --project <project_id> --since <24h|RFC3339> --step <page-view|screen-view|interaction> <name> --step <kind> <name> with two through eight ordered --step values and optional --until, --service, --release, --environment, --unit session|identified-user, --conversion-window <seconds|1h|1d>, and --json";

/// Canonical parser behavior for funnels.
const GRAMMAR: Grammar = Grammar::new("analytics funnel", ANALYTICS_FUNNEL_NEXT_STEP);

/// Parses two through eight exact ordered event selectors and bounded funnel controls.
pub(super) fn parse_funnel(args: &[String]) -> Result<Command, CliError> {
    let mut parsed = ParsedFunnelFlags::default();
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
            "--unit" | "--analysis-unit" => {
                GRAMMAR.mark_seen(&mut seen, "--unit")?;
                parsed.analysis_unit =
                    Some(GRAMMAR.flag_value(args, &mut index, "--unit", inline)?);
            }
            "--conversion-window" | "--conversion-window-seconds" => {
                GRAMMAR.mark_seen(&mut seen, "--conversion-window")?;
                parsed.conversion_window =
                    Some(GRAMMAR.flag_value(args, &mut index, "--conversion-window", inline)?);
            }
            "--step" => {
                if parsed.steps.len() >= 8 {
                    return Err(GRAMMAR.invalid_argument("too many funnel steps"));
                }
                let kind = GRAMMAR.flag_value(args, &mut index, "--step", inline)?;
                let event_name = following_step_value(args, &mut index)?;
                parsed.steps.push((kind, event_name));
            }
            value if value.starts_with('-') => {
                return Err(CliError::UnknownFlag {
                    flag: flag.to_owned(),
                    next: ANALYTICS_FUNNEL_NEXT_STEP,
                });
            }
            _ => {
                return Err(CliError::UnexpectedArgument {
                    argument: raw.clone(),
                    command: "analytics funnel",
                    next: ANALYTICS_FUNNEL_NEXT_STEP,
                });
            }
        }
        index += 1;
    }

    Ok(Command::AnalyticsFunnel {
        options: parsed.finish()?,
        json: parsed.scope.json,
    })
}

/// Partially parsed flags before required-value and shape validation.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "fields directly represent the closed analytics funnel grammar"
)]
#[derive(Default)]
struct ParsedFunnelFlags {
    scope: ScopeFlags,
    analysis_unit: Option<String>,
    conversion_window: Option<String>,
    steps: Vec<(String, String)>,
}

impl ParsedFunnelFlags {
    /// Requires and normalizes every public request field.
    fn finish(&self) -> Result<AnalyticsFunnelOptions, CliError> {
        let scope = self
            .scope
            .finish(GRAMMAR, "invalid analytics funnel value")?;
        let analysis_unit = normalize_unit(self.analysis_unit.as_deref())?;
        let conversion_window_seconds = self
            .conversion_window
            .as_deref()
            .map(parse_duration_seconds)
            .transpose()?;
        if !(2..=8).contains(&self.steps.len()) {
            return Err(GRAMMAR.invalid_argument("funnel requires two through eight steps"));
        }
        let steps = self
            .steps
            .iter()
            .map(|(kind, event_name)| {
                let kind =
                    GRAMMAR.normalize_event_kind(kind.as_str(), "invalid funnel step kind")?;
                Ok(AnalyticsFunnelStep {
                    kind,
                    event_name: GRAMMAR.normalize_event_name(
                        kind,
                        event_name.as_str(),
                        "invalid analytics funnel value",
                        "invalid funnel interaction name",
                    )?,
                })
            })
            .collect::<Result<Vec<_>, CliError>>()?;

        Ok(AnalyticsFunnelOptions {
            project_id: scope.project_id,
            since: scope.since,
            until: scope.until,
            service_name: scope.service_name,
            release: scope.release,
            environment: scope.environment,
            analysis_unit,
            conversion_window_seconds,
            steps,
        })
    }
}

/// Normalizes the explicit funnel counting boundary.
fn normalize_unit(value: Option<&str>) -> Result<AnalyticsFunnelUnit, CliError> {
    match value.map(str::trim) {
        None | Some("session" | "sessions") => Ok(AnalyticsFunnelUnit::Session),
        Some("identified-user" | "identified_user" | "user" | "users") => {
            Ok(AnalyticsFunnelUnit::IdentifiedUser)
        }
        Some(_) => Err(GRAMMAR.invalid_argument("invalid funnel analysis unit")),
    }
}

/// Parses a positive second, minute, hour, or day duration under the API range cap.
fn parse_duration_seconds(value: &str) -> Result<u32, CliError> {
    let value = value.trim();
    let (digits, multiplier) = match value.as_bytes().last().copied() {
        Some(b's') => (&value[..value.len().saturating_sub(1)], 1_u32),
        Some(b'm') => (&value[..value.len().saturating_sub(1)], 60_u32),
        Some(b'h') => (&value[..value.len().saturating_sub(1)], 3_600_u32),
        Some(b'd') => (&value[..value.len().saturating_sub(1)], 86_400_u32),
        _ => (value, 1_u32),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(GRAMMAR.invalid_argument("invalid funnel conversion window"));
    }
    digits
        .parse::<u32>()
        .ok()
        .and_then(|count| count.checked_mul(multiplier))
        .filter(|seconds| (1..=31 * 24 * 60 * 60).contains(seconds))
        .ok_or_else(|| GRAMMAR.invalid_argument("invalid funnel conversion window"))
}

/// Reads the event-name half of one two-token `--step` value.
fn following_step_value(args: &[String], index: &mut usize) -> Result<String, CliError> {
    *index += 1;
    let value = args.get(*index).map(String::as_str).unwrap_or_default();
    if value.is_empty() || value.starts_with('-') {
        return Err(CliError::MissingFlagValue {
            flag: "--step",
            next: ANALYTICS_FUNNEL_NEXT_STEP,
        });
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds parser-owned arguments without shell behavior.
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_repeated_steps_aliases_and_human_duration() {
        let command = parse_funnel(&args(&[
            "--project-id",
            "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA",
            "--since=24h",
            "--step=page",
            "/pricing",
            "--step",
            "interaction",
            "signup_completed",
            "--analysis-unit",
            "identified_user",
            "--conversion-window",
            "2h",
            "--env",
            "production",
            "--json",
        ]))
        .expect("valid command");

        let Command::AnalyticsFunnel { options, json } = command else {
            panic!("wrong command");
        };
        assert_eq!(options.project_id, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        assert_eq!(options.analysis_unit, AnalyticsFunnelUnit::IdentifiedUser);
        assert_eq!(options.conversion_window_seconds, Some(7_200));
        assert_eq!(options.steps.len(), 2);
        assert_eq!(
            options.steps[0].kind,
            crate::AnalyticsPathEventKind::PageView
        );
        assert!(json);
    }

    #[test]
    fn rejects_missing_unsafe_duplicate_and_unbounded_values() {
        let base = [
            "--project",
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "--since",
            "24h",
            "--step",
            "page-view",
            "/pricing",
            "--step",
            "interaction",
            "signup_completed",
        ];
        assert!(parse_funnel(&args(&base[..7])).is_err());

        let mut unsafe_event = base.to_vec();
        unsafe_event[9] = "signup completed";
        assert!(parse_funnel(&args(&unsafe_event)).is_err());

        let mut duplicate = base.to_vec();
        duplicate.extend(["--project-id", "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"]);
        assert!(matches!(
            parse_funnel(&args(&duplicate)),
            Err(CliError::DuplicateFlag {
                flag: "--project",
                ..
            })
        ));

        let mut too_long = base.to_vec();
        too_long.extend(["--conversion-window", "32d"]);
        assert!(parse_funnel(&args(&too_long)).is_err());
    }
}
