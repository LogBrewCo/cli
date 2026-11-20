//! Exact RFC 3339 parsing and validation shared by every response contract.

/// Parsed RFC 3339 instant at nanosecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ParsedTimestamp {
    /// Whole UTC seconds since the Unix epoch.
    epoch_seconds: i64,
    /// Sub-second nanoseconds; 1 second or more only inside a represented leap second.
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
    parse_timestamp(value)
}

/// Parses UTC evidence without collapsing a represented leap second into the next minute.
pub(crate) fn parse_utc_evidence(value: &str) -> Option<ParsedTimestamp> {
    (value.ends_with('Z') || value.ends_with("+00:00"))
        .then_some(value)
        .and_then(parse_timestamp)
}

/// Returns signed elapsed milliseconds, counting only leap seconds represented by an endpoint.
pub(crate) fn evidence_offset_millis(
    occurred: ParsedTimestamp,
    started: ParsedTimestamp,
) -> Option<i64> {
    let seconds = i128::from(occurred.epoch_seconds) - i128::from(started.epoch_seconds);
    let start_fraction = started.nanoseconds;
    let end_fraction = occurred.nanoseconds;
    let represented_leaps = i128::from(seconds > 0 && start_fraction >= 1_000_000_000)
        - i128::from(seconds < 0 && end_fraction >= 1_000_000_000);
    let nanos = (seconds + represented_leaps) * 1_000_000_000 + i128::from(end_fraction)
        - i128::from(start_fraction);
    i64::try_from(nanos / 1_000_000).ok()
}

/// Applies the wire format and precision limits before Gregorian calendar validation.
fn parse_timestamp(value: &str) -> Option<ParsedTimestamp> {
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
    let (local_seconds, leap) = civil_seconds(value)?;
    let nanoseconds = match value.get(20..zone) {
        Some(digits) if zone > 19 => {
            let scale =
                10_u32.checked_pow(9_u32.checked_sub(u32::try_from(digits.len()).ok()?)?)?;
            digits.parse::<u32>().ok()?.checked_mul(scale)?
        }
        _ => 0,
    };
    let offset_seconds = if value.ends_with('Z') {
        0
    } else {
        let hours = digits_at(value, zone + 1..zone + 3)?;
        let minutes = digits_at(value, zone + 4..zone + 6)?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        let offset = i64::from(hours * 3_600 + minutes * 60);
        if bytes[zone] == b'-' { -offset } else { offset }
    };
    Some(ParsedTimestamp {
        epoch_seconds: local_seconds.checked_sub(offset_seconds)?,
        nanoseconds: nanoseconds + if leap { 1_000_000_000 } else { 0 },
    })
}

/// Reads the unsigned decimal digits at `range`, which the shape checks already proved ASCII digits.
fn digits_at(value: &str, range: std::ops::Range<usize>) -> Option<u32> {
    value.get(range)?.parse().ok()
}

/// Returns the Unix seconds of the date and whole-second prefix, and whether it names a leap second.
///
/// A leap second (`:60`) counts as second 59, as RFC 3339 section 5.6 allows it at any minute.
fn civil_seconds(value: &str) -> Option<(i64, bool)> {
    let year = digits_at(value, 0..4)?;
    let month = digits_at(value, 5..7)?;
    let day = digits_at(value, 8..10)?;
    let hour = digits_at(value, 11..13)?;
    let minute = digits_at(value, 14..16)?;
    let second = digits_at(value, 17..19)?;
    let leap_year =
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let month_days = match month {
        2 if leap_year => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1..=12 => 31,
        _ => return None,
    };
    if day == 0 || day > month_days || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    // Days from 1970-01-01 in the proleptic Gregorian calendar (Hinnant's days_from_civil).
    let shifted_year = i64::from(year) - i64::from(month <= 2);
    let era = shifted_year.div_euclid(400);
    let year_of_era = shifted_year.rem_euclid(400);
    let day_of_year = (153 * ((i64::from(month) + 9) % 12) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let seconds = i64::from(hour * 3_600 + minute * 60 + second.min(59));
    Some((days * 86_400 + seconds, second == 60))
}

/// Formats Unix milliseconds from 1970 through 9999 as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub(crate) fn format_utc_millis(millis: i64) -> Option<String> {
    if !(0..253_402_300_800_000).contains(&millis) {
        return None;
    }
    let (days, day_millis) = (millis / 86_400_000, millis % 86_400_000);
    // Hinnant's civil_from_days, the inverse of the days_from_civil step in `civil_seconds`.
    let shifted = days + 719_468;
    let (era, day_of_era) = (shifted / 146_097, shifted % 146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = era * 400 + year_of_era + i64::from(month <= 2);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        day_millis / 3_600_000,
        day_millis / 60_000 % 60,
        day_millis / 1_000 % 60,
        day_millis % 1_000
    ))
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

    civil_seconds(seconds).is_some()
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
        ParsedTimestamp, evidence_offset_millis, format_utc_millis, is_rfc3339_utc, parse_rfc3339,
        parse_timestamp, parse_utc_evidence, parse_utc_millis,
    };

    #[test]
    fn utc_millis_format_as_canonical_rfc_3339() {
        for (millis, text) in [
            (0, "1970-01-01T00:00:00.000Z"),
            (951_782_400_000, "2000-02-29T00:00:00.000Z"),
            (1_794_929_400_005, "2026-11-17T15:30:00.005Z"),
            (253_402_300_799_999, "9999-12-31T23:59:59.999Z"),
        ] {
            assert_eq!(format_utc_millis(millis).as_deref(), Some(text), "{millis}");
            assert_eq!(parse_utc_millis(text), Some(i128::from(millis)), "{text}");
            assert!(is_rfc3339_utc(text), "{text}");
        }
        for millis in [-1, 253_402_300_800_000, i64::MAX] {
            assert_eq!(format_utc_millis(millis), None, "{millis}");
        }
    }

    #[test]
    fn calendar_and_offsets_follow_rfc_3339() {
        for value in [
            "2026-04-31T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-00-10T00:00:00Z",
            "2026-01-00T00:00:00Z",
            "2100-02-29T00:00:00Z",
        ] {
            assert!(parse_rfc3339(value).is_none(), "accepted {value}");
            assert!(!is_rfc3339_utc(value), "accepted {value}");
        }
        for (value, seconds) in [
            ("2000-02-29T00:00:00Z", 951_782_400),
            ("1600-03-01T00:00:00Z", -11_670_912_000),
            ("2026-11-17T15:30:00-00:00", 1_794_929_400),
            ("2026-11-17T16:30:00.5+01:00", 1_794_929_400),
            ("2026-11-17T14:00:00-01:30", 1_794_929_400),
            ("9999-12-31T23:59:59Z", 253_402_300_799),
        ] {
            assert_eq!(
                parse_rfc3339(value).map(|parsed| parsed.epoch_seconds),
                Some(seconds),
                "{value}"
            );
        }
        assert_eq!(
            parse_timestamp("2017-01-01T00:59:60.5+01:00"),
            parse_utc_evidence("2016-12-31T23:59:60.500Z")
        );
    }

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
