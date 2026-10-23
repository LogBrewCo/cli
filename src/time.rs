//! Exact RFC 3339 parsing and validation shared by every response contract.

#![expect(
    clippy::redundant_pub_crate,
    reason = "shared across private sibling modules"
)]

/// Parsed RFC 3339 instant at nanosecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ParsedTimestamp {
    /// Whole UTC seconds since the Unix epoch.
    epoch_seconds: i64,
    /// Positive sub-second nanoseconds.
    nanoseconds: u32,
}

impl ParsedTimestamp {
    /// Adds whole seconds without changing fractional precision.
    pub(crate) fn checked_add_seconds<T: TryInto<i64>>(self, seconds: T) -> Option<Self> {
        Some(Self {
            epoch_seconds: self.epoch_seconds.checked_add(seconds.try_into().ok()?)?,
            nanoseconds: self.nanoseconds,
        })
    }

    /// Returns the instant at the API's truncating millisecond precision.
    pub(super) fn epoch_millis(self) -> i128 {
        i128::from(self.epoch_seconds) * 1_000 + i128::from(self.nanoseconds / 1_000_000)
    }

    /// Returns whether no precision below milliseconds is present.
    pub(super) const fn is_millisecond_normalized(self) -> bool {
        self.nanoseconds.is_multiple_of(1_000_000)
    }

    /// Returns the exact nanosecond offset used by analytics contracts.
    pub(crate) fn epoch_nanos(self) -> i128 {
        i128::from(self.epoch_seconds) * 1_000_000_000 + i128::from(self.nanoseconds)
    }
}

/// Parses one RFC 3339 timestamp with a required UTC designator or numeric offset.
pub(crate) fn parse_rfc3339(value: &str) -> Option<ParsedTimestamp> {
    if value.starts_with("0000") || value.get(17..19) == Some("60") {
        return None;
    }
    let timestamp = parse_timestamp(value)?;
    Some(ParsedTimestamp {
        epoch_seconds: timestamp.timestamp(),
        nanoseconds: timestamp.timestamp_subsec_nanos(),
    })
}

/// Parses UTC evidence without collapsing a represented leap second into the next minute.
pub(crate) fn parse_utc_evidence(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    (value.ends_with('Z') || value.ends_with("+00:00"))
        .then_some(value)
        .and_then(parse_timestamp)
}

/// Returns signed elapsed milliseconds, counting only leap seconds represented by an endpoint.
pub(crate) fn evidence_offset_millis(
    occurred: chrono::DateTime<chrono::Utc>,
    started: chrono::DateTime<chrono::Utc>,
) -> Option<i64> {
    let seconds = i128::from(occurred.timestamp()) - i128::from(started.timestamp());
    let start_fraction = started.timestamp_subsec_nanos();
    let end_fraction = occurred.timestamp_subsec_nanos();
    let represented_leaps = i128::from(seconds > 0 && start_fraction >= 1_000_000_000)
        - i128::from(seconds < 0 && end_fraction >= 1_000_000_000);
    let nanos = (seconds + represented_leaps) * 1_000_000_000 + i128::from(end_fraction)
        - i128::from(start_fraction);
    i64::try_from(nanos / 1_000_000).ok()
}

/// Applies the wire format and precision limits before Gregorian calendar validation.
fn parse_timestamp(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let bytes = value.as_bytes();
    if !(20..=35).contains(&bytes.len()) || !bytes.is_ascii() || !has_calendar_shape(bytes) {
        return None;
    }
    let zone = bytes.len() - if value.ends_with('Z') { 1 } else { 6 };
    if zone != 19
        && (!(21..=29).contains(&zone)
            || bytes[19] != b'.'
            || !bytes[20..zone].iter().all(u8::is_ascii_digit))
        || !value.ends_with('Z')
            && (!matches!(bytes[zone], b'+' | b'-')
                || bytes[zone + 3] != b':'
                || !bytes[zone + 1..zone + 3].iter().all(u8::is_ascii_digit)
                || !bytes[zone + 4..].iter().all(u8::is_ascii_digit))
    {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.to_utc())
}

/// Checks the fixed-width date and whole-second prefix without accepting relaxed syntax.
pub(crate) fn has_calendar_shape(bytes: &[u8]) -> bool {
    bytes.get(..19).is_some_and(|seconds| {
        seconds.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            _ => byte.is_ascii_digit(),
        })
    })
}

/// Checks the UTC RFC3339 shape returned by cursor endpoints.
pub(crate) fn is_rfc3339_utc(value: &str) -> bool {
    let Some(without_zone) = value
        .strip_suffix('Z')
        .or_else(|| value.strip_suffix("+00:00"))
    else {
        return false;
    };
    let (seconds, fraction) = match without_zone.split_once('.') {
        Some((seconds, fraction)) => (seconds, Some(fraction)),
        None => (without_zone, None),
    };
    if fraction.is_some_and(|digits| {
        digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        return false;
    }

    if seconds.len() != 19 || !has_calendar_shape(seconds.as_bytes()) {
        return false;
    }

    chrono::NaiveDateTime::parse_from_str(seconds, "%Y-%m-%dT%H:%M:%S").is_ok()
}

/// Parses the canonical UTC `Z` timestamps emitted by analytics contracts.
pub(crate) fn parse_utc_timestamp(value: &str) -> Option<ParsedTimestamp> {
    ((20..=30).contains(&value.len()) && value.ends_with('Z'))
        .then_some(value)
        .and_then(parse_rfc3339)
}

/// Subtracts whole positive seconds without changing fractional precision.
pub(crate) fn subtract_seconds(
    timestamp: ParsedTimestamp,
    seconds: u64,
) -> Option<ParsedTimestamp> {
    timestamp.checked_add_seconds(i64::try_from(seconds).ok()?.checked_neg()?)
}

/// Parses one already validated UTC timestamp into exact whole milliseconds.
pub(crate) fn parse_utc_millis(value: &str) -> Option<i128> {
    (value.ends_with('Z') || value.ends_with("+00:00"))
        .then_some(value)
        .and_then(parse_rfc3339)
        .filter(|value| value.is_millisecond_normalized())
        .map(ParsedTimestamp::epoch_millis)
}

#[cfg(test)]
mod tests {
    use super::{
        ParsedTimestamp, evidence_offset_millis, is_rfc3339_utc, parse_rfc3339, parse_utc_evidence,
        parse_utc_millis,
    };

    #[test]
    fn utc_evidence_preserves_calendar_order_and_signed_offsets() -> Result<(), &'static str> {
        for (before, after, millis) in [
            ("0000-01-01T00:00:00Z", "0000-01-01T00:00:00.001Z", 1),
            ("2000-02-29T23:59:59Z", "2000-03-01T00:00:00Z", 1_000),
            (
                "2016-12-31T23:59:59.500Z",
                "2016-12-31T23:59:60.500Z",
                1_000,
            ),
            ("2016-12-31T23:59:60.500Z", "2017-01-01T00:00:00Z", 500),
            ("2026-05-31T23:59:59.999999999Z", "2026-06-01T00:00:00Z", 0),
            (
                "9999-12-31T23:59:59Z",
                "9999-12-31T23:59:59.999999999+00:00",
                999,
            ),
        ] {
            let before = parse_utc_evidence(before).ok_or("before timestamp")?;
            let after = parse_utc_evidence(after).ok_or("after timestamp")?;
            assert!(before < after);
            assert_eq!(evidence_offset_millis(after, before), Some(millis));
            assert_eq!(evidence_offset_millis(before, after), Some(-millis));
        }
        Ok(())
    }

    #[test]
    fn timestamp_policies_keep_exact_format_precision_and_arithmetic() -> Result<(), &'static str> {
        for value in [
            "",
            "2026-02-29T00:00:00Z",
            "1900-02-29T00:00:00Z",
            "2026-01-01t00:00:00Z",
            "2026-01-01T00:00:00z",
            "2026-01-01 00:00:00Z",
            "2026-01-01T00:00:00.Z",
            "2026-01-01T00:00:00.0000000001Z",
            "2026-01-01T00:00:00+24:00",
            "2026-01-01T00:00:00+00:60",
            "2026-01-01T24:00:00Z",
            "2026-01-01T00:60:00Z",
            "2026-01-01T00:00:61Z",
            "+026-01-01T00:00:00Z",
            "2026-+1-01T00:00:00Z",
            "2026-01-01T00:00:00Z ",
            "2026-01-01T00:00:00é",
        ] {
            assert!(parse_rfc3339(value).is_none(), "accepted {value}");
            assert!(
                parse_utc_evidence(value).is_none(),
                "accepted evidence {value}"
            );
            assert_eq!(
                is_rfc3339_utc(value),
                value == "2026-01-01T00:00:00.0000000001Z"
            );
        }
        for value in ["0000-01-01T00:00:00Z", "2016-12-31T23:59:60Z"] {
            assert!(parse_rfc3339(value).is_none());
            assert!(parse_utc_evidence(value).is_some());
            assert!(is_rfc3339_utc(value));
        }
        for (value, nanos) in [
            ("1970-01-01T00:00:00.000000001Z", 1),
            ("1969-12-31T23:59:59.999999999Z", -1),
            ("1970-01-01T23:59:00+23:59", 0),
            ("1969-12-31T00:01:00-23:59", 0),
            ("0001-01-01T00:00:00+00:01", -62_135_596_860_000_000_000),
        ] {
            assert_eq!(
                parse_rfc3339(value).map(ParsedTimestamp::epoch_nanos),
                Some(nanos)
            );
        }
        let epoch = parse_rfc3339("1970-01-01T00:00:00.123Z").ok_or("epoch")?;
        let limit = epoch.checked_add_seconds(i64::MAX).ok_or("integer range")?;
        assert_eq!(limit.nanoseconds, 123_000_000);
        assert!(limit.checked_add_seconds(1).is_none());
        Ok(())
    }

    #[test]
    fn utc_parser_preserves_leap_days_and_milliseconds() {
        let before = parse_utc_millis("2024-02-29T23:59:59Z");
        let after = parse_utc_millis("2024-02-29T23:59:59.500Z");

        assert_eq!(
            after.zip(before).map(|(after, before)| after - before),
            Some(500)
        );
        assert!(parse_utc_millis("2026-02-29T00:00:00Z").is_none());
        assert!(parse_utc_millis("2026-01-01T00:00:00.0001Z").is_none());
    }
}
