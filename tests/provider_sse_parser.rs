use codegg::provider::sse_parser::{parse_anthropic_buffer, parse_anthropic_buffer_with_state};
use codegg::provider::ChatEvent;

#[test]
fn parses_anthropic_text_delta() {
    let mut buffer = "event: content_block_delta\ndata: {\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n".to_string();
    assert!(
        matches!(parse_anthropic_buffer(&mut buffer), Some(Ok(ChatEvent::TextDelta(text))) if text.as_ref() == "hello")
    );
}

#[test]
fn parses_anthropic_reasoning_delta() {
    let mut buffer = "event: content_block_delta\ndata: {\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"private\"}}\n\n".to_string();
    assert!(
        matches!(parse_anthropic_buffer(&mut buffer), Some(Ok(ChatEvent::ReasoningDelta(text))) if text.as_ref() == "private")
    );
}

#[test]
fn assembles_anthropic_tool_arguments_at_block_stop() {
    let mut current_tool = None;
    let mut args = String::new();
    let mut buffer = "event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"c1\",\"name\":\"read\"}}\n\nevent: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"a\\\"}\"}}\n\nevent: content_block_stop\ndata: {}\n\n".to_string();
    assert!(parse_anthropic_buffer_with_state(&mut buffer, &mut current_tool, &mut args).is_none());
    assert!(parse_anthropic_buffer_with_state(&mut buffer, &mut current_tool, &mut args).is_none());
    assert!(
        matches!(parse_anthropic_buffer_with_state(&mut buffer, &mut current_tool, &mut args), Some(Ok(ChatEvent::ToolCall(call))) if call.name.as_ref() == "read" && call.arguments["path"] == "a")
    );
}
