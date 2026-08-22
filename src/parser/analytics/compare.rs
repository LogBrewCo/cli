//! Closed product-analytics segment-comparison command grammar.

use std::collections::HashSet;

use crate::ids::is_uuid;
use crate::{
    AnalyticsSegment, AnalyticsSegmentComparisonOptions, AnalyticsSegmentPropertyFilter,
    AnalyticsSegmentUnit, CliError, Command,
};

use super::{Grammar, normalize_property_key};

/// Exact recovery text shared by every malformed comparison invocation.
pub(super) const ANALYTICS_COMPARE_NEXT_STEP: &str = "use logbrew analytics compare --project <project_id> --since <24h|RFC3339> --target-kind <page-view|screen-view|interaction> --target-event <name> --segment <key>=<label> --segment <key>=<label> with two through four ordered segments and optional --segment-service <key>=<value>, --segment-release <key>=<value>, --segment-environment <key>=<value>, --segment-property <segment>:<property-key>=<exact-value> up to four times per segment, --until, --interval auto|1m|5m|15m|1h|6h|1d, --unit session|identified-user, and --json";

/// Canonical parser behavior for segment comparison.
const GRAMMAR: Grammar = Grammar::new("analytics compare", ANALYTICS_COMPARE_NEXT_STEP);

/// Parses one exact target and two through four named context segments.
pub(super) fn parse_compare(args: &[String]) -> Result<Command, CliError> {
    let mut parsed = ParsedCompareFlags::default();
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
            "--unit" | "--analysis-unit" => {
                GRAMMAR.mark_seen(&mut seen, "--unit")?;
                parsed.analysis_unit =
                    Some(GRAMMAR.flag_value(args, &mut index, "--unit", inline)?);
            }
            "--target-kind" | "--event-kind" => {
                GRAMMAR.mark_seen(&mut seen, "--target-kind")?;
                parsed.target_kind =
                    Some(GRAMMAR.flag_value(args, &mut index, "--target-kind", inline)?);
            }
            "--target-event" | "--event" | "--event-name" => {
                GRAMMAR.mark_seen(&mut seen, "--target-event")?;
                parsed.target_event =
                    Some(GRAMMAR.flag_value(args, &mut index, "--target-event", inline)?);
            }
            "--segment" => push_bounded(
                &mut parsed.segments,
                GRAMMAR.flag_value(args, &mut index, "--segment", inline)?,
                "too many analytics segments",
            )?,
            "--segment-service" | "--segment-service-name" => push_bounded(
                &mut parsed.segment_services,
                GRAMMAR.flag_value(args, &mut index, "--segment-service", inline)?,
                "too many segment service filters",
            )?,
            "--segment-release" => push_bounded(
                &mut parsed.segment_releases,
                GRAMMAR.flag_value(args, &mut index, "--segment-release", inline)?,
                "too many segment release filters",
            )?,
            "--segment-environment" | "--segment-env" => push_bounded(
                &mut parsed.segment_environments,
                GRAMMAR.flag_value(args, &mut index, "--segment-environment", inline)?,
                "too many segment environment filters",
            )?,
            "--segment-property" => push_property_assignment(
                &mut parsed.segment_properties,
                GRAMMAR.flag_value(args, &mut index, "--segment-property", inline)?,
            )?,
            value if value.starts_with('-') => {
                return Err(CliError::UnknownFlag {
                    flag: flag.to_owned(),
                    next: ANALYTICS_COMPARE_NEXT_STEP,
                });
            }
            _ => {
                return Err(CliError::UnexpectedArgument {
                    argument: raw.clone(),
                    command: "analytics compare",
                    next: ANALYTICS_COMPARE_NEXT_STEP,
                });
            }
        }
        index += 1;
    }

    Ok(Command::AnalyticsCompare {
        options: parsed.finish()?,
        json: parsed.json,
    })
}

/// Partially parsed flags before required-value and cross-segment validation.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "fields directly represent the closed analytics comparison grammar"
)]
#[derive(Default)]
struct ParsedCompareFlags {
    project_id: Option<String>,
    since: Option<String>,
    until: Option<String>,
    interval: Option<String>,
    analysis_unit: Option<String>,
    target_kind: Option<String>,
    target_event: Option<String>,
    segments: Vec<String>,
    segment_services: Vec<String>,
    segment_releases: Vec<String>,
    segment_environments: Vec<String>,
    segment_properties: Vec<String>,
    json: bool,
}

impl ParsedCompareFlags {
    /// Requires and normalizes every public comparison field.
    fn finish(&self) -> Result<AnalyticsSegmentComparisonOptions, CliError> {
        let project_id = required(self.project_id.as_deref(), "project")?.trim();
        if !is_uuid(project_id) {
            return Err(GRAMMAR.invalid_argument("invalid project id"));
        }
        let since = normalize_text(required(self.since.as_deref(), "since")?, 64)?;
        let until = normalize_optional(self.until.as_deref(), 64)?;
        let interval = normalize_interval(self.interval.as_deref())?;
        let analysis_unit = normalize_unit(self.analysis_unit.as_deref())?;
        let target_kind = GRAMMAR.normalize_event_kind(
            required(self.target_kind.as_deref(), "target-kind")?,
            "invalid analytics comparison target kind",
        )?;
        let target_event = GRAMMAR.normalize_event_name(
            target_kind,
            required(self.target_event.as_deref(), "target-event")?,
            "invalid analytics comparison value",
            "invalid analytics comparison target",
        )?;
        let mut segments = normalize_segments(self.segments.as_slice())?;
        apply_segment_filters(
            segments.as_mut_slice(),
            self.segment_services.as_slice(),
            SegmentFilter::Service,
        )?;
        apply_segment_filters(
            segments.as_mut_slice(),
            self.segment_releases.as_slice(),
            SegmentFilter::Release,
        )?;
        apply_segment_filters(
            segments.as_mut_slice(),
            self.segment_environments.as_slice(),
            SegmentFilter::Environment,
        )?;
        apply_segment_property_filters(
            segments.as_mut_slice(),
            self.segment_properties.as_slice(),
        )?;
        require_unique_filters(segments.as_slice())?;

        Ok(AnalyticsSegmentComparisonOptions {
            project_id: project_id.to_ascii_lowercase(),
            since,
            until,
            interval,
            analysis_unit,
            target_kind,
            target_event,
            segments,
        })
    }
}

/// One exact context dimension assigned to a named segment.
#[derive(Clone, Copy)]
enum SegmentFilter {
    /// Logical service name.
    Service,
    /// Application release.
    Release,
    /// Deployment environment.
    Environment,
}

/// Normalizes the ordered segment declarations and proves unique keys.
fn normalize_segments(values: &[String]) -> Result<Vec<AnalyticsSegment>, CliError> {
    if !(2..=4).contains(&values.len()) {
        return Err(
            GRAMMAR.invalid_argument("analytics comparison requires two through four segments")
        );
    }
    let mut keys = HashSet::with_capacity(values.len());
    values
        .iter()
        .map(|value| {
            let (key, label) = assignment(value.as_str(), 80)?;
            let key = normalize_segment_key(key)?;
            if !keys.insert(key.clone()) {
                return Err(GRAMMAR.invalid_argument("duplicate analytics segment key"));
            }
            Ok(AnalyticsSegment {
                key,
                label: normalize_text(label, 80)?,
                service_name: None,
                release: None,
                environment: None,
                property_filters: Vec::new(),
            })
        })
        .collect()
}

/// Applies repeated exact property predicates after all segment keys are known.
fn apply_segment_property_filters(
    segments: &mut [AnalyticsSegment],
    values: &[String],
) -> Result<(), CliError> {
    for value in values {
        let (selector, value) = assignment(value.as_str(), 256)?;
        let (segment_key, property_key) = selector
            .split_once(':')
            .ok_or_else(|| GRAMMAR.invalid_argument("invalid segment property assignment"))?;
        let segment_key = normalize_segment_key(segment_key)?;
        let property_key = normalize_property_key(property_key)?;
        let Some(segment) = segments
            .iter_mut()
            .find(|segment| segment.key == segment_key)
        else {
            return Err(GRAMMAR.invalid_argument("segment property references an unknown key"));
        };
        if segment.property_filters.len() >= 4 {
            return Err(GRAMMAR.invalid_argument("too many segment property filters"));
        }
        if segment
            .property_filters
            .iter()
            .any(|filter| filter.key == property_key)
        {
            return Err(GRAMMAR.invalid_argument("duplicate segment property key"));
        }
        segment
            .property_filters
            .push(AnalyticsSegmentPropertyFilter {
                key: property_key,
                value: normalize_text(value, 256)?,
            });
    }
    for segment in segments {
        segment
            .property_filters
            .sort_unstable_by(|left, right| left.key.cmp(&right.key));
    }
    Ok(())
}

/// Applies repeated keyed filter assignments after all segment keys are known.
fn apply_segment_filters(
    segments: &mut [AnalyticsSegment],
    values: &[String],
    filter: SegmentFilter,
) -> Result<(), CliError> {
    for value in values {
        let (key, value) = assignment(value.as_str(), 256)?;
        let key = normalize_segment_key(key)?;
        let Some(segment) = segments.iter_mut().find(|segment| segment.key == key) else {
            return Err(GRAMMAR.invalid_argument("segment filter references an unknown key"));
        };
        let slot = match filter {
            SegmentFilter::Service => &mut segment.service_name,
            SegmentFilter::Release => &mut segment.release,
            SegmentFilter::Environment => &mut segment.environment,
        };
        if slot.is_some() {
            return Err(GRAMMAR.invalid_argument("duplicate segment filter assignment"));
        }
        *slot = Some(normalize_text(value, 256)?);
    }
    Ok(())
}

/// Rejects segments with identical exact service, release, and environment filters.
fn require_unique_filters(segments: &[AnalyticsSegment]) -> Result<(), CliError> {
    let mut filters = HashSet::with_capacity(segments.len());
    if segments.iter().all(|segment| {
        filters.insert((
            segment.service_name.as_deref(),
            segment.release.as_deref(),
            segment.environment.as_deref(),
            segment.property_filters.as_slice(),
        ))
    }) {
        Ok(())
    } else {
        Err(GRAMMAR.invalid_argument("duplicate analytics segment filters"))
    }
}

/// Splits one `key=value` assignment while preserving equals signs in the value.
fn assignment(value: &str, limit: usize) -> Result<(&str, &str), CliError> {
    let (key, value) = value
        .split_once('=')
        .ok_or_else(|| GRAMMAR.invalid_argument("invalid keyed segment assignment"))?;
    if key.trim().is_empty()
        || value.trim().is_empty()
        || value.chars().count() > limit
        || value.chars().any(char::is_control)
    {
        return Err(GRAMMAR.invalid_argument("invalid keyed segment assignment"));
    }
    Ok((key, value))
}

/// Applies the server's machine-safe segment-key contract.
fn normalize_segment_key(value: &str) -> Result<String, CliError> {
    let value = value.trim();
    let first_is_alphanumeric = value
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    if value.is_empty()
        || value.len() > 32
        || !first_is_alphanumeric
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
    {
        return Err(GRAMMAR.invalid_argument("invalid analytics segment key"));
    }
    Ok(value.to_owned())
}

/// Normalizes the explicit eligibility and reach boundary.
fn normalize_unit(value: Option<&str>) -> Result<AnalyticsSegmentUnit, CliError> {
    match value.map(str::trim) {
        None | Some("session" | "sessions") => Ok(AnalyticsSegmentUnit::Session),
        Some("identified-user" | "identified_user" | "user" | "users") => {
            Ok(AnalyticsSegmentUnit::IdentifiedUser)
        }
        Some(_) => Err(GRAMMAR.invalid_argument("invalid analytics comparison unit")),
    }
}

/// Normalizes automatic or fixed UTC bucket selection.
fn normalize_interval(value: Option<&str>) -> Result<String, CliError> {
    match value.map(str::trim) {
        None | Some("auto") => Ok("auto".to_owned()),
        Some(value @ ("1m" | "5m" | "15m" | "1h" | "6h" | "1d")) => Ok(value.to_owned()),
        Some(_) => Err(GRAMMAR.invalid_argument("invalid analytics comparison interval")),
    }
}

/// Requires one named comparison flag.
fn required<'a>(value: Option<&'a str>, argument: &'static str) -> Result<&'a str, CliError> {
    GRAMMAR.required(value, argument)
}

/// Trims one non-empty, control-free bounded public value.
fn normalize_text(value: &str, limit: usize) -> Result<String, CliError> {
    GRAMMAR.normalize_text(value, limit, "invalid analytics comparison value")
}

/// Normalizes one optional bounded timestamp value.
fn normalize_optional(value: Option<&str>, limit: usize) -> Result<Option<String>, CliError> {
    GRAMMAR.normalize_optional(value, limit, "invalid analytics comparison value")
}

/// Adds one repeated assignment while preserving the four-segment request bound.
fn push_bounded(
    values: &mut Vec<String>,
    value: String,
    error: &'static str,
) -> Result<(), CliError> {
    if values.len() >= 4 {
        return Err(GRAMMAR.invalid_argument(error));
    }
    values.push(value);
    Ok(())
}

/// Adds one property assignment while preserving the four-by-four request bound.
fn push_property_assignment(values: &mut Vec<String>, value: String) -> Result<(), CliError> {
    if values.len() >= 16 {
        return Err(GRAMMAR.invalid_argument("too many segment property filters"));
    }
    values.push(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds parser-owned arguments without shell behavior.
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_ordered_segments_and_keyed_filters() {
        let command = parse_compare(&args(&[
            "--project-id",
            "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
            "--since=7d",
            "--target-kind",
            "interaction",
            "--target-event",
            "checkout_completed",
            "--segment",
            "old=Old release",
            "--segment",
            "new=New release",
            "--segment-release",
            "old=1.0.0",
            "--segment-release",
            "new=1.1.0",
            "--segment-env",
            "old=production",
            "--segment-env",
            "new=production",
            "--segment-property",
            "old:tag.plan=legacy",
            "--segment-property",
            "new:tag.plan=pro",
            "--segment-property",
            "new:resource.framework.name=React",
            "--unit",
            "identified-user",
            "--json",
        ]))
        .expect("valid command");

        let Command::AnalyticsCompare { options, json } = command else {
            panic!("wrong command");
        };
        assert_eq!(options.project_id, "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
        assert_eq!(options.analysis_unit, AnalyticsSegmentUnit::IdentifiedUser);
        assert_eq!(options.interval, "auto");
        assert_eq!(options.segments[0].key, "old");
        assert_eq!(options.segments[1].release.as_deref(), Some("1.1.0"));
        assert_eq!(options.segments[0].property_filters[0].key, "tag.plan");
        assert_eq!(
            options.segments[1].property_filters[0].key,
            "resource.framework.name"
        );
        assert_eq!(options.segments[1].property_filters[1].value, "pro");
        assert!(json);
    }

    #[test]
    fn rejects_duplicate_keys_filters_assignments_and_unsafe_targets() {
        let base = [
            "--project",
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "--since",
            "7d",
            "--target-kind",
            "interaction",
            "--target-event",
            "checkout_completed",
            "--segment",
            "old=Old",
            "--segment",
            "new=New",
        ];

        let mut duplicate_key = base.to_vec();
        duplicate_key[11] = "old=New";
        assert!(parse_compare(&args(&duplicate_key)).is_err());

        assert!(parse_compare(&args(&base)).is_err());

        let mut duplicate_assignment = base.to_vec();
        duplicate_assignment.extend([
            "--segment-release",
            "old=1.0",
            "--segment-release",
            "old=1.1",
            "--segment-release",
            "new=1.1",
        ]);
        assert!(parse_compare(&args(&duplicate_assignment)).is_err());

        let mut unsafe_target = base.to_vec();
        unsafe_target[7] = "checkout completed";
        assert!(parse_compare(&args(&unsafe_target)).is_err());
    }

    #[test]
    fn rejects_sensitive_duplicate_and_unbounded_property_filters() {
        let base = [
            "--project",
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "--since",
            "7d",
            "--target-kind",
            "interaction",
            "--target-event",
            "checkout_completed",
            "--segment",
            "free=Free",
            "--segment",
            "pro=Pro",
            "--segment-property",
            "free:tag.plan=free",
            "--segment-property",
            "pro:tag.plan=pro",
        ];
        assert!(parse_compare(&args(&base)).is_ok());

        for invalid in [
            "free:tag.user_id=subject-1",
            "free:tag.plan=free",
            "unknown:tag.plan=free",
            "free:resource.unknown.name=value",
        ] {
            let mut values = base.to_vec();
            values.extend(["--segment-property", invalid]);
            assert!(parse_compare(&args(&values)).is_err());
        }

        let mut too_many = base.to_vec();
        too_many.extend([
            "--segment-property",
            "free:tag.region=eu",
            "--segment-property",
            "free:tag.channel=direct",
            "--segment-property",
            "free:tag.cohort=beta",
            "--segment-property",
            "free:tag.locale=en",
        ]);
        assert!(parse_compare(&args(&too_many)).is_err());
    }
}
