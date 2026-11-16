//! Work package A: the fixture document and its test-local DTO.
//!
//! The format is a contract, not an accident of the first scenario, so its
//! decode rules are tested before any scenario uses them: every supported
//! event variant lowers to the production event type, and an unknown variant
//! or an unknown format version fails to load and names what was unknown.

use codegg::provider::ChatEvent;

use super::common::session_replay::ReplayFixture;

/// Build a minimal but complete fixture document around one response list.
fn document(format: u32, id: &str, events: serde_json::Value) -> String {
    serde_json::json!({
        "format": format,
        "id": id,
        "session_id": "inline",
        "user_input": "inline",
        "profile": "plain",
        "responses": [events],
        "expect": {
            "assistant_text": "",
            "terminal": "completed",
            "request_count": 1
        }
    })
    .to_string()
}

#[test]
fn fixture_round_trip_covers_every_supported_variant() {
    let fixture = ReplayFixture::named("all_variants").expect("the decode fixture loads");
    let lowered = fixture.lowered_responses();

    assert_eq!(lowered.len(), 1, "one scripted turn");
    let turn = &lowered[0];
    let kinds: Vec<&str> = turn
        .iter()
        .map(|event| match event {
            ChatEvent::TextDelta(_) => "text_delta",
            ChatEvent::ReasoningDelta(_) => "reasoning_delta",
            ChatEvent::ToolCall(_) => "tool_call",
            ChatEvent::ToolResult { .. } => "tool_result",
            ChatEvent::Finish { .. } => "finish",
            ChatEvent::Error(_) => "error",
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            "text_delta",
            "reasoning_delta",
            "tool_call",
            "tool_result",
            "text_delta",
            "finish"
        ],
        "every supported variant lowers in document order"
    );
    assert!(
        matches!(&turn[2], ChatEvent::ToolCall(call)
            if call.name.as_str() == "read"
                && call.arguments == serde_json::json!({"path": "inline.txt"})),
        "tool call lowers with its id, name and arguments intact"
    );
    assert!(
        matches!(&turn[3], ChatEvent::ToolResult { tool_call_id, content }
            if tool_call_id.as_str() == "call_declared" && content.as_str() == "declared output"),
        "tool result lowers with its call id and content intact"
    );
}

#[test]
fn unknown_event_variant_fails_to_load_and_names_it() {
    let text = document(
        1,
        "unknown_variant",
        serde_json::json!([{"type": "audio_delta", "text": "unrecorded"}]),
    );
    let error = ReplayFixture::parse(&text, "inline", "unknown_variant")
        .expect_err("an unknown event variant must not load");
    assert!(
        error.to_string().contains("audio_delta"),
        "the load error must name the unknown variant: {error}"
    );
}

#[test]
fn unknown_format_version_fails_to_load_and_names_it() {
    let text = document(
        99,
        "future_format",
        serde_json::json!([{"type": "finish", "stop_reason": "stop"}]),
    );
    let error = ReplayFixture::parse(&text, "inline", "future_format")
        .expect_err("an unsupported format version must not load");
    let message = error.to_string();
    assert!(
        message.contains("99") && message.contains("format"),
        "the load error must name the version it cannot read: {message}"
    );
}

#[test]
fn fixture_identity_must_match_its_file_stem() {
    let text = document(
        1,
        "declared_id",
        serde_json::json!([{"type": "finish", "stop_reason": "stop"}]),
    );
    let error = ReplayFixture::parse(&text, "inline", "file_stem")
        .expect_err("a fixture whose id disagrees with its stem must not load");
    assert!(
        error.to_string().contains("declared_id") && error.to_string().contains("file_stem"),
        "the load error must name both identities: {error}"
    );
}
