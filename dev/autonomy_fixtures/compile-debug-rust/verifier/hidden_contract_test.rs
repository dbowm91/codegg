// Hidden contract test.
//
// This file lives beside the verifier, never inside `fixture/`, so the agent
// cannot edit or weaken it. It re-derives the same three contracts the
// provided test file checks, but with different inputs, and it uses only the
// public API that the frozen provided test already pins down.

extern crate candidate;

use candidate::measure::{clamp_into, inclusive_len};
use candidate::range::Range;
use candidate::spec::parse_spec;

#[test]
fn inclusive_len_counts_both_endpoints_for_other_spans() {
    // A single-value range still covers one step.
    assert_eq!(inclusive_len(&Range::new(7, 7)), 1);
    // A span that straddles zero.
    assert_eq!(inclusive_len(&Range::new(-3, 2)), 6);
    // A span entirely above zero.
    assert_eq!(inclusive_len(&Range::new(10, 13)), 4);
}

#[test]
fn clamp_into_picks_the_matching_bound_for_out_of_range_values() {
    let range = Range::new(5, 9);
    // Above the range: the upper bound, not the lower one.
    assert_eq!(clamp_into(50, &range), 9);
    // Below the range: the lower bound.
    assert_eq!(clamp_into(-50, &range), 5);
    // Inside the range: unchanged.
    assert_eq!(clamp_into(7, &range), 7);
}

#[test]
fn parse_spec_rejects_signed_and_non_numeric_bounds() {
    // A leading sign is not part of a non-negative decimal field.
    assert!(parse_spec("-1..4").is_err());
    assert!(parse_spec("3..-9").is_err());
    // A non-digit in either field is malformed.
    assert!(parse_spec("a..4").is_err());
    assert!(parse_spec("3..b").is_err());
    // Whitespace around a field is malformed.
    assert!(parse_spec(" 3..4").is_err());
    // A well-formed specification is still accepted.
    assert!(parse_spec("3..4").is_ok());
}
