//! The hidden contract test for the record parser.
//!
//! This file lives beside the verifier, never inside the fixture, so the
//! candidate can neither read it nor edit it. It re-derives the field
//! contract independently of the provided test and uses different inputs, so
//! an implementation that special-cases the provided test's strings does not
//! pass.

extern crate candidate;

use candidate::parse_record;

#[test]
fn the_empty_record_is_one_empty_field() {
    assert_eq!(parse_record("").fields, vec![""]);
}

#[test]
fn a_record_of_only_separators_keeps_every_field() {
    assert_eq!(parse_record(",,,").fields, vec!["", "", "", ""]);
}

#[test]
fn an_interior_empty_field_is_kept() {
    assert_eq!(parse_record("x,,y").fields, vec!["x", "", "y"]);
}

#[test]
fn surrounding_whitespace_is_normalized() {
    assert_eq!(parse_record("  gamma  ").fields, vec!["gamma"]);
}

#[test]
fn a_trailing_empty_field_is_significant() {
    assert_eq!(parse_record("p,q,").fields, vec!["p", "q", ""]);
}

#[test]
fn a_single_field_record_is_one_field() {
    assert_eq!(parse_record("z").fields, vec!["z"]);
}
