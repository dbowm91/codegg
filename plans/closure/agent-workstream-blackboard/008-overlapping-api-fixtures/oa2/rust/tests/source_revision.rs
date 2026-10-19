use summary_producer::summarize;

#[test]
fn source_revision_validation_and_markdown_code_span_are_frozen() {
    let revision = "rev`|<x>";
    let summary = summarize(revision, "").expect("printable source revision");
    assert_eq!(summary.source_revision, revision);
    let serialized = serde_json::to_string(&summary).unwrap();
    assert!(serialized.contains("rev`|<x>"));

    for revision in ["", " leading", "trailing ", "line\nbreak", "bad\u{0000}control"] {
        assert!(summarize(revision, "").is_err(), "revision accepted: {revision:?}");
    }
    assert!(summarize(&"x".repeat(129), "").is_err());
}
