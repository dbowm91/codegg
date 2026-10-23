//! The provided test for the record parser.
//!
//! This file is part of the frozen fixture. It is the case's stated failure
//! condition, so it must not be edited: the independent verifier in
//! `../verifier` checks its SHA-256 digest and refuses a weakened copy.

extern crate candidate;

use candidate::parse_record;

#[test]
fn a_plain_record_keeps_every_field() {
    assert_eq!(parse_record("a,b,c").fields, vec!["a", "b", "c"]);
}

#[test]
fn a_trailing_empty_field_is_significant() {
    assert_eq!(parse_record("a,b,").fields, vec!["a", "b", ""]);
}

#[test]
fn an_interior_empty_field_is_kept() {
    assert_eq!(parse_record("a,,b").fields, vec!["a", "", "b"]);
}

#[test]
fn surrounding_whitespace_is_normalized() {
    assert_eq!(parse_record(" alpha , beta ").fields, vec!["alpha", "beta"]);
}
