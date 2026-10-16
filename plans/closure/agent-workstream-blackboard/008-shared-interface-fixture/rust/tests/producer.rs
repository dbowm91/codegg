use summary_producer::summarize;

#[test]
fn frozen_fixture_normalizes_deduplicates_sorts_and_aggregates() {
    let jsonl = include_str!("../../fixtures/events.ndjson");
    let summary = summarize("abc123", jsonl).expect("valid fixture");
    let actual = serde_json::to_value(summary).expect("serialize summary");
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/expected-summary.json"))
            .expect("frozen JSON fixture");
    assert_eq!(actual, expected);
}

#[test]
fn empty_input_is_a_valid_zero_summary() {
    let summary = summarize("revision-1", "").expect("empty input is valid");
    assert_eq!(summary.events, Vec::<serde_json::Value>::new());
    assert_eq!(summary.totals["events"], 0);
    assert_eq!(summary.totals["files_changed"], 0);
    assert_eq!(summary.by_kind["test_passed"], 0);
}

#[test]
fn conflicting_duplicate_ids_are_rejected() {
    let jsonl = concat!(
        "{\"id\":\"same\",\"occurred_at\":\"2026-10-16T09:00:00Z\",\"kind\":\"test_passed\"}\n",
        "{\"id\":\"same\",\"occurred_at\":\"2026-10-16T09:00:00Z\",\"kind\":\"test_failed\"}\n",
    );
    let error = summarize("abc123", jsonl).expect_err("conflicting id must fail");
    assert!(error.to_string().to_lowercase().contains("duplicate"));
}

#[test]
fn unsafe_paths_are_rejected() {
    let unsafe_path = r#"{"id":"x","occurred_at":"2026-10-16T09:00:00Z","kind":"file_changed","path":"../secret","insertions":1,"deletions":0}"#;
    let error = summarize("abc123", unsafe_path).expect_err("unsafe path must fail");
    assert!(error.to_string().to_lowercase().contains("path"));
}

#[test]
fn line_count_overflow_is_rejected() {
    let overflow = concat!(
        "{\"id\":\"a\",\"occurred_at\":\"2026-10-16T09:00:00Z\",\"kind\":\"file_changed\",\"path\":\"a.rs\",\"insertions\":18446744073709551615,\"deletions\":0}\n",
        "{\"id\":\"b\",\"occurred_at\":\"2026-10-16T09:01:00Z\",\"kind\":\"file_changed\",\"path\":\"b.rs\",\"insertions\":1,\"deletions\":0}\n",
    );
    let error = summarize("abc123", overflow).expect_err("overflow must fail");
    assert!(error.to_string().to_lowercase().contains("overflow"));
}

#[test]
fn malformed_second_line_reports_its_line_number() {
    let jsonl = concat!(
        "{\"id\":\"ok\",\"occurred_at\":\"2026-10-16T09:00:00Z\",\"kind\":\"test_passed\"}\n",
        "{not-json}\n",
    );
    let error = summarize("abc123", jsonl).expect_err("malformed JSON must fail");
    assert!(error.to_string().contains("line 2"));
}

#[test]
fn invalid_source_revision_and_kind_specific_fields_are_rejected() {
    let valid = r#"{"id":"x","occurred_at":"2026-10-16T09:00:00Z","kind":"test_passed"}"#;
    let error = summarize("", valid).expect_err("empty revision must fail");
    assert!(error.to_string().to_lowercase().contains("revision"));

    let file_without_counts = r#"{"id":"x","occurred_at":"2026-10-16T09:00:00Z","kind":"file_changed","path":"src/lib.rs"}"#;
    assert!(summarize("abc123", file_without_counts).is_err());

    let test_with_path = r#"{"id":"x","occurred_at":"2026-10-16T09:00:00Z","kind":"test_passed","path":"src/lib.rs"}"#;
    assert!(summarize("abc123", test_with_path).is_err());

    let invalid_time = r#"{"id":"x","occurred_at":"yesterday","kind":"test_passed"}"#;
    assert!(summarize("abc123", invalid_time).is_err());

    let unknown_field = r#"{"id":"x","occurred_at":"2026-10-16T09:00:00Z","kind":"test_passed","extra":1}"#;
    assert!(summarize("abc123", unknown_field).is_err());
}
