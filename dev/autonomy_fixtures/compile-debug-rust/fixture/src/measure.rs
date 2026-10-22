//! Measurements taken over a [`Range`].

use crate::range::Range;

/// The number of integer steps covered by `range`.
///
/// # Contract
///
/// The count **includes both endpoints**. The span `[2, 5]` covers the steps
/// 2, 3, 4 and 5, so its length is `4`; a single-value range `[7, 7]` has
/// length `1`, not `0`.
pub fn inclusive_len(range: &Range) -> i64 {
    range.upper() - range.lower()
}

/// Clamps `value` into `range`.
///
/// # Contract
///
/// * a value below the range's lower bound becomes the **lower** bound;
/// * a value above the range's upper bound becomes the **upper** bound;
/// * a value already inside the range is returned unchanged.
pub fn clamp_into(value: i64, range: &Range) -> i64 {
    if value < range.lower() {
        range.lower()
    } else if value > range.upper() {
        range.lower()
    } else {
        value
    }
}
