extern crate candidate;

use candidate::measure::{clamp_into, inclusive_len};
use candidate::range::Range;
use candidate::spec::parse_spec;

#[test]
fn inclusive_len_counts_both_endpoints() {
    assert_eq!(inclusive_len(&Range::new(2, 5)), 4);
}

#[test]
fn clamp_into_returns_the_upper_bound_for_a_value_above_the_range() {
    assert_eq!(clamp_into(100, &Range::new(0, 10)), 10);
}

#[test]
fn parse_spec_rejects_a_negative_lower_bound() {
    assert!(parse_spec("-5..10").is_err());
}
