//! Exact RFC 3339 parsing shared by deployment and investigation contracts.

#![expect(
    clippy::redundant_pub_crate,
    reason = "shared across private sibling modules"
)]

/// Parsed RFC 3339 instant at nanosecond precision.
#[derive(Clone, Copy)]
pub(crate) struct ParsedTimestamp {
    /// Whole UTC seconds since the Unix epoch.
    epoch_seconds: i64,
    /// Positive sub-second nanoseconds.
    nanoseconds: u32,
}

impl ParsedTimestamp {
    /// Returns the instant at the API's truncating millisecond precision.
    pub(super) fn epoch_millis(self) -> i128 {
        i128::from(self.epoch_seconds) * 1_000 + i128::from(self.nanoseconds / 1_000_000)
    }

    /// Returns whether no precision below milliseconds is present.
    pub(super) const fn is_millisecond_normalized(self) -> bool {
        self.nanoseconds.is_multiple_of(1_000_000)
    }
}

/// Parses one RFC 3339 timestamp with a required UTC designator or numeric offset.
pub(crate) fn parse_rfc3339(value: &str) -> Option<ParsedTimestamp> {
    let bytes = value.as_bytes();
    if !(20..=35).contains(&bytes.len())
        || !bytes.is_ascii()
        || [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')]
            .into_iter()
            .any(|(index, expected)| bytes.get(index) != Some(&expected))
    {
        return None;
    }
    let [year, month, day, hour, minute, second] =
        [0..4, 5..7, 8..10, 11..13, 14..16, 17..19].map(|range| decimal(bytes.get(range)?));
    let [year, month, day, hour, minute, second] = [year?, month?, day?, hour?, minute?, second?];
    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let mut index = 19;
    let nanoseconds = if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        parse_fraction(bytes.get(start..index)?)?
    } else {
        0
    };
    let offset_seconds = match bytes.get(index) {
        Some(b'Z') if index + 1 == bytes.len() => 0_i64,
        Some(sign @ (b'+' | b'-')) if index + 6 == bytes.len() => {
            if bytes.get(index + 3) != Some(&b':') {
                return None;
            }
            let hour = decimal(bytes.get(index + 1..index + 3)?)?;
            let minute = decimal(bytes.get(index + 4..index + 6)?)?;
            if hour > 23 || minute > 59 {
                return None;
            }
            let magnitude = i64::from(hour) * 3_600 + i64::from(minute) * 60;
            if *sign == b'+' { magnitude } else { -magnitude }
        }
        Some(_) | None => return None,
    };
    let seconds = days_from_civil(i64::from(year), i64::from(month), i64::from(day))
        .checked_mul(86_400)?
        .checked_add(i64::from(hour).checked_mul(3_600)?)?
        .checked_add(i64::from(minute).checked_mul(60)?)?
        .checked_add(i64::from(second))?
        .checked_sub(offset_seconds)?;
    Some(ParsedTimestamp {
        epoch_seconds: seconds,
        nanoseconds,
    })
}

/// Parses one already validated UTC timestamp into exact whole milliseconds.
pub(crate) fn parse_utc_millis(value: &str) -> Option<i128> {
    (value.ends_with('Z') || value.ends_with("+00:00"))
        .then(|| parse_rfc3339(value))
        .flatten()
        .filter(|value| value.is_millisecond_normalized())
        .map(ParsedTimestamp::epoch_millis)
}

/// Parses one fixed-width ASCII decimal field.
fn decimal(bytes: &[u8]) -> Option<u32> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// Expands one one-to-nine-digit fractional second to nanoseconds.
fn parse_fraction(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || bytes.len() > 9 || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    decimal(bytes)?.checked_mul(10_u32.checked_pow(u32::try_from(9 - bytes.len()).ok()?)?)
}

/// Returns the valid day count for one Gregorian month.
const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

/// Converts one civil date to days relative to the Unix epoch.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = (if year >= 0 { year } else { year - 399 }) / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    era * 146_097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year - 719_468
}

#[cfg(test)]
mod tests {
    use super::parse_utc_millis;

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
