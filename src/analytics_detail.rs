//! Previous-period change, sample size, and example units shared by analytics responses.

#![expect(
    clippy::redundant_pub_crate,
    reason = "sibling analytics modules share this private contract"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde::Deserialize;

use crate::analytics_contract::bounded_counts;
use crate::http::{nonempty_control_safe, terminal_safe};
use crate::render::percentage;
use crate::time::parse_utc_timestamp;

/// Most groups one response may carry.
const GROUP_LIMIT: usize = 2048;
/// Most example units listed for one group.
const EXAMPLE_LIMIT: u8 = 5;
/// Most action and trace IDs listed for one example unit.
const LINK_LIMIT: usize = 3;
/// Longest accepted key, label, value, or unit ID.
const TEXT_LIMIT: usize = 1024;
/// Most distinct example units listed in human output.
const RENDERED_UNIT_LIMIT: usize = 10;

/// Unit that every group counts and every example names.
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DetailUnit {
    /// Explicit application sessions.
    Session,
    /// Opaque user-subject IDs.
    IdentifiedUser,
}

impl DetailUnit {
    /// Plural noun for human output.
    const fn plural(self) -> &'static str {
        match self {
            Self::Session => "sessions",
            Self::IdentifiedUser => "users",
        }
    }
}

/// Previous-period comparison, sample size, and supporting units for one result.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Detail {
    /// Unit behind every number.
    pub(crate) unit: DetailUnit,
    /// Inclusive start of the previous period.
    previous_since: String,
    /// Exclusive end of the previous period, equal to the selected start.
    previous_until: String,
    /// Units behind the headline result.
    sample_size: u64,
    /// The same population in the previous period.
    previous_sample_size: u64,
    /// Most examples listed for one group.
    example_limit: u8,
    /// One entry per number in the result.
    pub(crate) groups: Vec<DetailGroup>,
    /// How to read the evidence behind an example.
    evidence_hint: String,
}

/// One number with its previous value and supporting units.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DetailGroup {
    /// Stable machine key.
    pub(crate) key: String,
    /// Short label.
    label: String,
    /// Property value of a breakdown group.
    #[serde(default)]
    breakdown: Option<BreakdownValue>,
    /// Units now.
    units: u64,
    /// Units in the previous period.
    previous_units: u64,
    /// `units - previous_units`.
    change: i64,
    /// Signed relative change.
    #[serde(default)]
    change_rate: Option<f64>,
    /// Rate denominator now.
    #[serde(default)]
    base_units: Option<u64>,
    /// `units / base_units`.
    #[serde(default)]
    rate: Option<f64>,
    /// The same rate before.
    #[serde(default)]
    previous_rate: Option<f64>,
    /// Matching events.
    #[serde(default)]
    events: Option<u64>,
    /// Units behind the number.
    examples: Vec<ExampleUnit>,
}

/// Exact property value of one breakdown group.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BreakdownValue {
    /// Property key.
    property_key: String,
    /// Exact value; absent when the unit never carried the key.
    #[serde(default)]
    value: Option<String>,
}

/// One unit behind a number with links to its evidence.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExampleUnit {
    /// Captured session or user-subject ID.
    unit_id: String,
    /// Product action IDs of this unit.
    action_ids: Vec<String>,
    /// Trace IDs of this unit, where its page views live.
    trace_ids: Vec<String>,
    /// First event of this unit in scope.
    #[serde(default)]
    first_seen_at: Option<String>,
    /// Last event of this unit in scope.
    #[serde(default)]
    last_seen_at: Option<String>,
}

/// Proves the detail block against the selected unit and window start.
pub(crate) fn valid(detail: &Detail, unit: DetailUnit, since: &str) -> bool {
    let (Some(previous_since), Some(previous_until), Some(since)) = (
        parse_utc_timestamp(detail.previous_since.as_str()),
        parse_utc_timestamp(detail.previous_until.as_str()),
        parse_utc_timestamp(since),
    ) else {
        return false;
    };
    let mut keys = BTreeSet::new();
    detail.unit == unit
        && previous_until.epoch_nanos() == since.epoch_nanos()
        && previous_since.epoch_nanos() < previous_until.epoch_nanos()
        && bounded_counts(&[detail.sample_size, detail.previous_sample_size])
        && detail.example_limit == EXAMPLE_LIMIT
        && detail.groups.len() <= GROUP_LIMIT
        && nonempty_control_safe(detail.evidence_hint.as_str(), 2048)
        && detail
            .groups
            .iter()
            .all(|group| keys.insert(group.key.as_str()) && valid_group(group))
}

/// Proves one group's arithmetic, rates, and bounded examples.
fn valid_group(group: &DetailGroup) -> bool {
    let change = i128::from(group.units) - i128::from(group.previous_units);
    let rate_ok = group.base_units.map_or_else(
        || group.rate.is_none() && group.previous_rate.is_none(),
        |_| rate_value(group.rate) && rate_value(group.previous_rate),
    );
    nonempty_control_safe(group.key.as_str(), TEXT_LIMIT)
        && nonempty_control_safe(group.label.as_str(), TEXT_LIMIT)
        && bounded_counts(&[group.units, group.previous_units])
        && i128::from(group.change) == change
        && group.change_rate.is_none_or(f64::is_finite)
        && (group.previous_units == 0) == group.change_rate.is_none()
        && rate_ok
        && group.breakdown.as_ref().is_none_or(|breakdown| {
            nonempty_control_safe(breakdown.property_key.as_str(), TEXT_LIMIT)
                && breakdown
                    .value
                    .as_deref()
                    .is_none_or(|value| value.chars().count() <= TEXT_LIMIT)
        })
        && group.examples.len() <= usize::from(EXAMPLE_LIMIT)
        && group.examples.iter().all(valid_example)
}

/// Accepts an absent rate or a finite non-negative one; approximate unique counts can exceed 1.
fn rate_value(value: Option<f64>) -> bool {
    value.is_none_or(|value| value.is_finite() && value >= 0.0)
}

/// Proves one example's identifiers and time bounds.
fn valid_example(example: &ExampleUnit) -> bool {
    nonempty_control_safe(example.unit_id.as_str(), TEXT_LIMIT)
        && example.action_ids.len() <= LINK_LIMIT
        && example.action_ids.iter().all(|id| crate::ids::is_uuid(id))
        && example.trace_ids.len() <= LINK_LIMIT
        && example
            .trace_ids
            .iter()
            .all(|id| nonempty_control_safe(id, 128))
        && [&example.first_seen_at, &example.last_seen_at]
            .iter()
            .all(|value| {
                value
                    .as_deref()
                    .is_none_or(|value| parse_utc_timestamp(value).is_some())
            })
}

/// Renders the previous-period comparison, every group, and bounded example evidence.
pub(crate) fn render(detail: &Detail, output: &mut String) {
    let unit = detail.unit.plural();
    let _ = writeln!(
        output,
        "Compared with {} to {}: {} {unit} now, {} before",
        terminal_safe(detail.previous_since.as_str()),
        terminal_safe(detail.previous_until.as_str()),
        detail.sample_size,
        detail.previous_sample_size,
    );
    for group in &detail.groups {
        let _ = write!(
            output,
            "  {}: {} {unit} (before {}, change {:+}",
            terminal_safe(group.label.as_str()),
            group.units,
            group.previous_units,
            group.change,
        );
        if let Some(change_rate) = group.change_rate {
            let _ = write!(output, ", {:+.1}%", change_rate * 100.0);
        }
        output.push(')');
        if let Some(base) = group.base_units {
            let _ = write!(
                output,
                "; {} of {base} (before {})",
                percentage(group.rate),
                percentage(group.previous_rate),
            );
        }
        if let Some(events) = group.events {
            let _ = write!(output, "; {events} events");
        }
        output.push('\n');
        if !group.examples.is_empty() {
            let ids = group
                .examples
                .iter()
                .map(|example| terminal_safe(example.unit_id.as_str()))
                .collect::<Vec<_>>();
            let _ = writeln!(output, "    Examples: {}", ids.join(", "));
        }
    }
    render_evidence(detail, output);
}

/// Lists each distinct example unit once with its action and trace IDs.
fn render_evidence(detail: &Detail, output: &mut String) {
    let mut seen = BTreeSet::new();
    let examples = detail
        .groups
        .iter()
        .flat_map(|group| group.examples.iter())
        .filter(|example| seen.insert(example.unit_id.as_str()))
        .take(RENDERED_UNIT_LIMIT)
        .collect::<Vec<_>>();
    if examples.is_empty() {
        return;
    }
    output.push_str("Evidence:\n");
    for example in examples {
        let _ = write!(output, "  {}", terminal_safe(example.unit_id.as_str()));
        if !example.action_ids.is_empty() {
            let _ = write!(output, " actions {}", example.action_ids.join(", "));
        }
        if !example.trace_ids.is_empty() {
            let traces = example
                .trace_ids
                .iter()
                .map(|id| terminal_safe(id))
                .collect::<Vec<_>>();
            let _ = write!(output, " traces {}", traces.join(", "));
        }
        output.push('\n');
    }
    let flag = match detail.unit {
        DetailUnit::Session => "--session",
        DetailUnit::IdentifiedUser => "--user",
    };
    let _ = writeln!(
        output,
        "Read an example with logbrew read actions {flag} <id>; page views are trace spans, so \
         read them with logbrew read trace <trace_id>."
    );
}

/// One consistent detail block for module fixtures whose window starts at `since`.
#[cfg(test)]
pub(crate) fn test_detail(unit: &str, previous_since: &str, since: &str) -> serde_json::Value {
    serde_json::json!({
        "unit": unit,
        "previous_since": previous_since,
        "previous_until": since,
        "sample_size": 4,
        "previous_sample_size": 2,
        "example_limit": 5,
        "groups": [{
            "key": "example",
            "label": "Example group",
            "units": 3,
            "previous_units": 1,
            "change": 2,
            "change_rate": 2.0,
            "examples": [{"unit_id": "example-unit", "action_ids": [], "trace_ids": []}]
        }],
        "evidence_hint": "Page views are trace spans."
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail(json: serde_json::Value) -> Result<Detail, serde_json::Error> {
        serde_json::from_value(json)
    }

    fn sample() -> serde_json::Value {
        serde_json::json!({
            "unit": "session",
            "previous_since": "2026-06-01T10:00:00Z",
            "previous_until": "2026-06-01T11:00:00Z",
            "sample_size": 12,
            "previous_sample_size": 10,
            "example_limit": 5,
            "groups": [{
                "key": "step:1",
                "label": "page_view /pricing",
                "breakdown": {"property_key": "tag.utm_source", "value": "newsletter"},
                "units": 6,
                "previous_units": 8,
                "change": -2,
                "change_rate": -0.25,
                "base_units": 12,
                "rate": 0.5,
                "previous_rate": 0.8,
                "examples": [{
                    "unit_id": "s-1",
                    "action_ids": ["11111111-1111-4111-8111-111111111111"],
                    "trace_ids": ["abc"],
                    "first_seen_at": "2026-06-01T11:05:00Z",
                    "last_seen_at": "2026-06-01T11:06:00Z"
                }]
            }],
            "evidence_hint": "Read actions by session."
        })
    }

    #[test]
    fn valid_detail_renders_change_rates_and_evidence() -> Result<(), serde_json::Error> {
        let value = detail(sample())?;

        assert!(valid(&value, DetailUnit::Session, "2026-06-01T11:00:00Z"));
        let mut output = String::new();
        render(&value, &mut output);
        assert!(output.contains("12 sessions now, 10 before"));
        assert!(output.contains("page_view /pricing: 6 sessions (before 8, change -2, -25.0%)"));
        assert!(output.contains("50.0% of 12 (before 80.0%)"));
        assert!(output.contains("Examples: s-1"));
        assert!(output.contains("s-1 actions 11111111-1111-4111-8111-111111111111 traces abc"));
        assert!(output.contains("logbrew read actions --session <id>"));
        Ok(())
    }

    #[test]
    fn detail_rejects_wrong_arithmetic_unit_window_and_bounds()
    -> Result<(), Box<dyn std::error::Error>> {
        let since = "2026-06-01T11:00:00Z";
        assert!(!valid(
            &detail(sample())?,
            DetailUnit::IdentifiedUser,
            since
        ));
        assert!(!valid(
            &detail(sample())?,
            DetailUnit::Session,
            "2026-06-01T12:00:00Z"
        ));
        for (pointer, value) in [
            ("/groups/0/change", serde_json::json!(2)),
            ("/groups/0/change_rate", serde_json::Value::Null),
            ("/groups/0/rate", serde_json::json!(-0.5)),
            (
                "/groups/0/examples/0/action_ids/0",
                serde_json::json!("not-a-uuid"),
            ),
            ("/example_limit", serde_json::json!(6)),
        ] {
            let mut json = sample();
            *json.pointer_mut(pointer).ok_or("pointer exists")? = value;
            assert!(
                !valid(&detail(json)?, DetailUnit::Session, since),
                "{pointer}"
            );
        }
        let mut json = sample();
        json["unexpected"] = serde_json::json!(1);
        assert!(detail(json).is_err());
        Ok(())
    }
}
