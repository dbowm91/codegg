use isolated_build_fixture::clamp_count;

#[test]
fn negative_counts_clamp_to_zero() {
    assert_eq!(clamp_count(-4), 0);
}

#[test]
fn nonnegative_counts_are_preserved() {
    assert_eq!(clamp_count(8), 8);
}
