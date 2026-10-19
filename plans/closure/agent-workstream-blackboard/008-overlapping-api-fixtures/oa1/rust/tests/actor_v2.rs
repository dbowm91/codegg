use summary_producer::summarize;

#[test]
fn actor_emits_normalized_v2_and_duplicates_compare_after_normalization() {
    let input = concat!(
        "{\"id\":\"a\",\"occurred_at\":\"2026-10-19T10:00:00Z\",\"kind\":\"file_changed\",\"path\":\"src/lib.rs\",\"insertions\":2,\"deletions\":1,\"actor\":\"  Alice_1  \"}\n",
        "{\"id\":\"a\",\"occurred_at\":\"2026-10-19T10:00:00Z\",\"kind\":\"file_changed\",\"path\":\"src/lib.rs\",\"insertions\":2,\"deletions\":1,\"actor\":\"Alice_1\"}\n",
    );
    let summary = summarize("abc123", input).expect("valid actor events");
    assert_eq!(summary.schema, "codegg.project-summary.v2");
    assert_eq!(summary.events.len(), 1);
    assert_eq!(summary.events[0]["actor"], "Alice_1");
}

#[test]
fn actor_absence_preserves_v1_and_invalid_actor_values_fail() {
    let v1 = summarize("abc123", "").expect("empty v1 input");
    assert_eq!(v1.schema, "codegg.project-summary.v1");
    let invalid = ["", "  ", "has space", "bad|actor", "é"];
    for actor in invalid {
        let input = format!(
            "{{\"id\":\"a\",\"occurred_at\":\"2026-10-19T10:00:00Z\",\"kind\":\"test_passed\",\"actor\":{}}}",
            serde_json::to_string(actor).unwrap()
        );
        assert!(summarize("abc123", &input).is_err(), "actor accepted: {actor:?}");
    }
}
