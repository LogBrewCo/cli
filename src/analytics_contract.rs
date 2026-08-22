//! Shared fail-closed validation for bounded analytics responses.

#![expect(
    clippy::redundant_pub_crate,
    reason = "sibling analytics modules share this private contract"
)]

/// Server-side scan cap that bounds every returned exact count.
pub(super) const COUNT_LIMIT: u64 = 10_000_000;

/// Returns whether every exact count stays inside the public scan bound.
pub(super) fn bounded_counts(values: &[u64]) -> bool {
    values.iter().all(|value| *value <= COUNT_LIMIT)
}

/// Verifies one optional exact proportion bounded between zero and one.
pub(super) fn ratio_matches(value: Option<f64>, numerator: u64, denominator: u64) -> bool {
    if denominator == 0 {
        return value.is_none();
    }
    if numerator > denominator {
        return false;
    }
    let (Ok(numerator), Ok(denominator)) = (u32::try_from(numerator), u32::try_from(denominator))
    else {
        return false;
    };
    let expected = f64::from(numerator) / f64::from(denominator);
    value.is_some_and(|value| {
        value.is_finite() && (0.0..=1.0).contains(&value) && (value - expected).abs() <= 1.0e-12
    })
}
