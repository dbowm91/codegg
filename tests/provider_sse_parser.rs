use codegg::provider::sse_parser::{parse_anthropic_buffer, parse_openai_buffer};
use codegg::provider::ChatEvent;

#[test]
fn test_parse_openai_text_delta() {
    let mut buffer = r#"data: {"choices":[{"delta":{"content":"Hello"}}]}"#.to_string();
    buffer.push('\n');
    let result = parse_openai_buffer(&mut buffer);
    assert!(result.is_some());
    let event = result.unwrap();
    assert!(event.is_ok());
    if let Ok(ChatEvent::TextDelta(text)) = event {
        assert_eq!(text.as_ref(), "Hello");
    } else {
        panic!("Expected TextDelta");
    }
}

#[test]
fn test_parse_openai_tool_calls_single() {
    let mut buffer = r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call_1","function":{"name":"bash","arguments":"{}"}}]}}]}"#.to_string();
    buffer.push('\n');
    let result = parse_openai_buffer(&mut buffer);
    assert!(result.is_some());
    let event = result.unwrap();
    assert!(event.is_ok());
    if let Ok(ChatEvent::ToolCall(tc)) = event {
        assert_eq!(tc.name.as_ref(), "bash");
        assert_eq!(tc.id.as_ref(), "call_1");
    } else {
        panic!("Expected ToolCall");
    }
}

#[test]
fn test_parse_openai_tool_calls_multiple() {
    let mut buffer = r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call_1","function":{"name":"bash","arguments":"{}"}},{"id":"call_2","function":{"name":"read","arguments":"{}"}}]}}]}"#.to_string();
    buffer.push('\n');
    let result = parse_openai_buffer(&mut buffer);
    assert!(result.is_some());
    let event = result.unwrap();
    assert!(event.is_ok());
    if let Ok(ChatEvent::ToolCall(tc)) = event {
        assert_eq!(tc.name.as_ref(), "bash");
        assert_eq!(tc.id.as_ref(), "call_1");
    } else {
        panic!("Expected first ToolCall");
    }

    let result2 = parse_openai_buffer(&mut buffer);
    assert!(result2.is_some());
    let event2 = result2.unwrap();
    assert!(event2.is_ok());
    if let Ok(ChatEvent::ToolCall(tc)) = event2 {
        assert_eq!(tc.name.as_ref(), "read");
        assert_eq!(tc.id.as_ref(), "call_2");
    }
}

#[test]
fn test_parse_anthropic_text_delta() {
    let mut buffer = r#"event: content_block_start
data: {"index":0,"content_block":{"type":"text","text":"Hello"}}

event: content_block_delta
data: {"index":0,"delta":{"type":"text_delta","text":" World"}}

event: message_delta
data: {"usage":{"input_tokens":10,"output_tokens":5}}

event: content_block_stop
data: {}

event: message_stop
data: {}"#
        .to_string();
    let result = parse_anthropic_buffer(&mut buffer);
    assert!(result.is_some());
    let event = result.unwrap();
    assert!(event.is_ok());
    if let Ok(ChatEvent::TextDelta(text)) = event {
        assert_eq!(text.as_ref(), "Hello");
    } else {
        panic!("Expected TextDelta");
    }
}

#[test]
fn test_parse_anthropic_thinking_delta() {
    let mut buffer = r#"event: content_block_start
data: {"index":0,"content_block":{"type":"thinking","thinking_id":"thought_1"}}

event: content_block_delta
data: {"index":0,"delta":{"type":"thinking_delta","thinking":"Let me think about this"}}

event: message_delta
data: {"usage":{"input_tokens":10,"output_tokens":5}}

event: content_block_stop
data: {}

event: message_stop
data: {}"#
        .to_string();
    let result = parse_anthropic_buffer(&mut buffer);
    assert!(result.is_some());
    let event = result.unwrap();
    assert!(event.is_ok());
    if let Ok(ChatEvent::ReasoningDelta(reasoning)) = event {
        assert_eq!(reasoning.as_ref(), "Let me think about this");
    } else {
        panic!("Expected ReasoningDelta");
    }
}

#[test]
fn test_parse_openai_finish() {
    let mut buffer = r#"data: {"choices":[{"finish_reason":"stop","delta":{}}],"#.to_string();
    buffer.push_str(r#""usage":{"prompt_tokens":10,"completion_tokens":5}}"#);
    buffer.push('\n');
    let result = parse_openai_buffer(&mut buffer);
    assert!(result.is_some());
    let event = result.unwrap();
    assert!(event.is_ok());
    if let Ok(ChatEvent::Finish { stop_reason, .. }) = event {
        assert_eq!(stop_reason.as_ref(), "stop");
    } else {
        panic!("Expected Finish");
    }
}

#[test]
fn test_parse_openai_error() {
    let mut buffer =
        r#"data: {"error":{"message":"Rate limit exceeded","type":"rate_limit_error"}}"#
            .to_string();
    buffer.push('\n');
    let result = parse_openai_buffer(&mut buffer);
    assert!(result.is_some());
    let event = result.unwrap();
    assert!(event.is_err());
}

#[test]
fn test_parse_empty_buffer() {
    let mut buffer = String::new();
    let result = parse_openai_buffer(&mut buffer);
    assert!(result.is_none());
}

#[test]
fn test_parse_malformed_json() {
    let mut buffer = r#"data: not valid json"#.to_string();
    let result = parse_openai_buffer(&mut buffer);
    assert!(result.is_none());
}

#[test]
fn test_anthropic_tool_use_streaming() {
    let mut buffer = r#"event: content_block_start
data: {"index":0,"content_block":{"type":"tool_use","id":"call_1","name":"bash"}}

event: content_block_delta
data: {"index":0,"delta":{"type":"input_json_delta","partial_json":"{\"command"}}

event: content_block_delta
data: {"index":0,"delta":{"type":"input_json_delta","partial_json":"\": \"ls\"}"}}

event: content_block_stop
data: {}

event: message_delta
data: {"usage":{"input_tokens":10,"output_tokens":5}}

event: message_stop
data: {}"#
        .to_string();
    let result = parse_anthropic_buffer(&mut buffer);
    assert!(result.is_some());
    let event = result.unwrap();
    assert!(event.is_ok());
    if let Ok(ChatEvent::ToolCall(tc)) = event {
        assert_eq!(tc.name.as_ref(), "bash");
        assert_eq!(tc.id.as_ref(), "call_1");
        if let Some(cmd) = tc.arguments.get("command") {
            assert_eq!(cmd, "ls");
        }
    } else {
        panic!("Expected ToolCall");
    }
}
