//! Exact UTC millisecond parsing shared by strict investigation contracts.

/// Milliseconds in one second.
const MILLIS_PER_SECOND: i128 = 1_000;

/// Parses one already validated UTC RFC 3339 timestamp into exact milliseconds.
pub(super) fn parse_utc_millis(value: &str) -> Option<i128> {
    if !crate::render::is_rfc3339_utc(value) {
        return None;
    }
    let value = value
        .strip_suffix('Z')
        .or_else(|| value.strip_suffix("+00:00"))?;
    let (date, time) = value.split_once('T')?;
    let mut date = date.split('-');
    let year = parse_i64(date.next()?)?;
    let month = parse_u32(date.next()?)?;
    let day = parse_u32(date.next()?)?;
    if date.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }
    let mut time = time.split(':');
    let hour = parse_u32(time.next()?)?;
    let minute = parse_u32(time.next()?)?;
    let second_and_fraction = time.next()?;
    if time.next().is_some() || hour > 23 || minute > 59 {
        return None;
    }
    let (second, fraction) = second_and_fraction
        .split_once('.')
        .map_or((second_and_fraction, ""), |parts| parts);
    let second = parse_u32(second)?;
    if second > 59 || day == 0 || day > days_in_month(year, month)? {
        return None;
    }
    let millis = fraction_millis(fraction)?;
    let days = days_from_civil(year, month, day)?;
    days.checked_mul(86_400_000)?
        .checked_add(i128::from(hour).checked_mul(3_600_000)?)?
        .checked_add(i128::from(minute).checked_mul(60_000)?)?
        .checked_add(i128::from(second).checked_mul(MILLIS_PER_SECOND)?)?
        .checked_add(i128::from(millis))
}

/// Parses an ASCII unsigned integer without accepting signs or whitespace.
fn parse_u32(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// Parses an ASCII nonnegative year without accepting signs or whitespace.
fn parse_i64(value: &str) -> Option<i64> {
    if value.len() != 4 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// Converts an optional fractional second into exact milliseconds.
fn fraction_millis(value: &str) -> Option<u32> {
    if value.len() > 9 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut digits = value.bytes();
    let mut millis = 0_u32;
    for place in [100_u32, 10, 1] {
        millis = millis.checked_add(
            digits
                .next()
                .map_or(0, |digit| u32::from(digit - b'0'))
                .checked_mul(place)?,
        )?;
    }
    if digits.any(|digit| digit != b'0') {
        None
    } else {
        Some(millis)
    }
}

/// Returns the valid day count for one Gregorian month.
const fn days_in_month(year: i64, month: u32) -> Option<u32> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if year.rem_euclid(4) == 0
            && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0) =>
        {
            Some(29)
        }
        2 => Some(28),
        _ => None,
    }
}

/// Converts a Gregorian date to days relative to the Unix epoch.
fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i128> {
    let adjusted_year = year.checked_sub(i64::from(month <= 2))?;
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year.checked_sub(era.checked_mul(400)?)?;
    let shifted_month = i64::from(month).checked_add(if month > 2 { -3 } else { 9 })?;
    let day_of_year = 153_i64
        .checked_mul(shifted_month)?
        .checked_add(2)?
        .checked_div(5)?
        .checked_add(i64::from(day).checked_sub(1)?)?;
    let day_of_era = year_of_era
        .checked_mul(365)?
        .checked_add(year_of_era.checked_div(4)?)?
        .checked_sub(year_of_era.checked_div(100)?)?
        .checked_add(day_of_year)?;
    i128::from(era)
        .checked_mul(146_097)?
        .checked_add(i128::from(day_of_era))?
        .checked_sub(719_468)
}

#[cfg(test)]
mod tests {
    use super::parse_utc_millis;

    #[test]
    fn utc_parser_preserves_leap_days_and_milliseconds() {
        let before = parse_utc_millis("2024-02-29T23:59:59.500Z");
        let after = parse_utc_millis("2024-03-01T00:00:00Z");

        assert_eq!(
            after.zip(before).map(|(after, before)| after - before),
            Some(500)
        );
        assert!(parse_utc_millis("2026-02-29T00:00:00Z").is_none());
        assert!(parse_utc_millis("2026-01-01T00:00:00.0001Z").is_none());
    }
}
