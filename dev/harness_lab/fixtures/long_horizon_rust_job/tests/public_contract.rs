use harness_rust_repair::parse_version;

#[test]
fn accepts_optional_v_prefix_and_surrounding_whitespace() {
    assert_eq!(parse_version("  v2.7.11  "), Some((2, 7, 11)));
}

#[test]
fn rejects_empty_or_extra_components() {
    assert_eq!(parse_version("1..3"), None);
    assert_eq!(parse_version("1.2.3.4"), None);
}

#[test]
fn rejects_numeric_overflow_and_suffixes() {
    assert_eq!(parse_version("4294967296.0.0"), None);
    assert_eq!(parse_version("1.2.3-beta"), None);
}
