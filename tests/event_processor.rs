#[cfg(test)]
mod tests {
    use codegg::agent::processor::EventProcessor;
    use codegg::provider::{ChatEvent, ContentPart, Message, TokenUsage, ToolCall};

    fn text_delta(text: &str) -> ChatEvent {
        ChatEvent::TextDelta(text.to_string().into())
    }

    fn reasoning_delta(text: &str) -> ChatEvent {
        ChatEvent::ReasoningDelta(text.to_string().into())
    }

    fn tool_call(id: &str, name: &str, args: serde_json::Value) -> ChatEvent {
        ChatEvent::ToolCall(ToolCall {
            id: id.to_string().into(),
            name: name.to_string().into(),
            arguments: args,
        })
    }

    fn tool_result(id: &str, content: &str) -> ChatEvent {
        ChatEvent::ToolResult {
            tool_call_id: id.to_string().into(),
            content: content.to_string().into(),
        }
    }

    fn finish(stop_reason: &str, input_tokens: usize, output_tokens: usize) -> ChatEvent {
        ChatEvent::Finish {
            stop_reason: stop_reason.to_string().into(),
            usage: TokenUsage {
                input_tokens,
                output_tokens,
                total_tokens: input_tokens + output_tokens,
                reasoning_tokens: 0,
                cached_tokens: None,
            },
        }
    }

    #[test]
    fn test_event_processor_text_accumulation() {
        let mut processor = EventProcessor::new();

        processor.process(text_delta("Hello"));
        processor.process(text_delta(", "));
        processor.process(text_delta("world!"));

        assert_eq!(processor.text(), "Hello, world!");
        assert!(!processor.is_complete());
    }

    #[test]
    fn test_event_processor_reasoning_accumulation() {
        let mut processor = EventProcessor::new();

        processor.process(reasoning_delta("thinking..."));
        processor.process(reasoning_delta(" more thoughts"));

        assert_eq!(processor.reasoning(), "thinking... more thoughts");
    }

    #[test]
    fn test_event_processor_tool_calls() {
        let mut processor = EventProcessor::new();

        processor.process(tool_call(
            "tc1",
            "read",
            serde_json::json!({"path": "/test"}),
        ));

        assert!(processor.has_tool_calls());
        assert_eq!(processor.tool_calls().len(), 1);
        assert_eq!(processor.tool_calls()[0].name.as_ref(), "read");
    }

    #[test]
    fn test_event_processor_multiple_tool_calls() {
        let mut processor = EventProcessor::new();

        processor.process(tool_call("tc1", "read", serde_json::json!({"path": "/a"})));
        processor.process(tool_call("tc2", "write", serde_json::json!({"path": "/b"})));
        processor.process(tool_call(
            "tc3",
            "bash",
            serde_json::json!({"command": "ls"}),
        ));

        assert_eq!(processor.tool_calls().len(), 3);
        assert_eq!(processor.tool_calls()[0].id.as_ref(), "tc1");
        assert_eq!(processor.tool_calls()[1].id.as_ref(), "tc2");
        assert_eq!(processor.tool_calls()[2].id.as_ref(), "tc3");
    }

    #[test]
    fn test_event_processor_tool_results() {
        let mut processor = EventProcessor::new();

        processor.process(tool_result("tc1", "file contents here"));

        assert_eq!(processor.tool_results().len(), 1);
        assert_eq!(processor.tool_results()[0].0.as_str(), "tc1");
        assert_eq!(processor.tool_results()[0].1.as_str(), "file contents here");
    }

    #[test]
    fn test_event_processor_finish() {
        let mut processor = EventProcessor::new();

        processor.process(text_delta("final response"));
        processor.process(finish("stop", 100, 50));

        assert!(processor.is_complete());
        assert_eq!(processor.stop_reason(), Some("stop"));
        assert_eq!(processor.input_tokens(), 100);
        assert_eq!(processor.output_tokens(), 50);
    }

    #[test]
    fn test_event_processor_reset() {
        let mut processor = EventProcessor::new();

        processor.process(text_delta("hello"));
        processor.process(reasoning_delta("thinking"));
        processor.process(tool_call("tc1", "read", serde_json::json!({})));
        processor.process(tool_result("tc1", "result"));
        processor.process(finish("stop", 10, 20));

        processor.reset();

        assert!(processor.text().is_empty());
        assert!(processor.reasoning().is_empty());
        assert!(processor.tool_calls().is_empty());
        assert!(processor.tool_results().is_empty());
        assert!(processor.stop_reason().is_none());
        assert!(!processor.is_complete());
    }

    #[test]
    fn test_event_processor_to_assistant_message() {
        let mut processor = EventProcessor::new();

        processor.process(text_delta("Hello"));

        let msg = processor.to_assistant_message();
        assert!(msg.is_some());

        let msg = msg.unwrap();
        match msg {
            Message::Assistant { content, .. } => {
                assert_eq!(content.len(), 1);
                match &content[0] {
                    ContentPart::Text { text } => assert_eq!(text.as_ref(), "Hello"),
                    _ => panic!("expected Text variant"),
                }
            }
            _ => panic!("expected Assistant message"),
        }
    }

    #[test]
    fn test_event_processor_to_assistant_message_empty() {
        let processor = EventProcessor::new();

        let msg = processor.to_assistant_message();
        assert!(msg.is_none());
    }

    #[test]
    fn test_event_processor_to_assistant_message_with_tool_calls() {
        let mut processor = EventProcessor::new();

        processor.process(text_delta("I'll read that file."));
        processor.process(tool_call(
            "tc1",
            "read",
            serde_json::json!({"path": "/test"}),
        ));

        let msg = processor.to_assistant_message();
        assert!(msg.is_some());

        let msg = msg.unwrap();
        match msg {
            Message::Assistant {
                content,
                tool_calls,
            } => {
                assert_eq!(content.len(), 1);
                match &content[0] {
                    ContentPart::Text { text } => assert_eq!(text.as_ref(), "I'll read that file."),
                    _ => panic!("expected Text variant"),
                }
                assert!(!tool_calls.is_empty());
                assert_eq!(tool_calls[0].name.as_ref(), "read");
            }
            _ => panic!("expected Assistant message"),
        }
    }

    #[test]
    fn test_event_processor_to_tool_messages() {
        let mut processor = EventProcessor::new();

        processor.process(tool_result("tc1", "file content"));
        processor.process(tool_result("tc2", "success"));

        let msgs = processor.to_tool_messages();

        assert_eq!(msgs.len(), 2);
        match &msgs[0] {
            Message::Tool {
                tool_call_id,
                content,
            } => {
                assert_eq!(tool_call_id.as_ref(), "tc1");
                assert_eq!(content.as_ref(), "file content");
            }
            _ => panic!("expected Tool message"),
        }
    }

    #[test]
    fn test_event_processor_token_tracking() {
        let mut processor = EventProcessor::new();

        assert_eq!(processor.input_tokens(), 0);
        assert_eq!(processor.output_tokens(), 0);

        processor.process(finish("stop", 1500, 750));

        assert_eq!(processor.input_tokens(), 1500);
        assert_eq!(processor.output_tokens(), 750);
    }

    #[test]
    fn test_event_processor_complete_flag() {
        let mut processor = EventProcessor::new();

        assert!(!processor.is_complete());

        processor.process(finish("stop", 10, 20));

        assert!(processor.is_complete());
    }

    #[test]
    fn test_event_processor_mixed_events() {
        let mut processor = EventProcessor::new();

        processor.process(reasoning_delta("analyzing..."));
        processor.process(text_delta("I'll help you with "));
        processor.process(text_delta("that."));
        processor.process(tool_call(
            "tc1",
            "read",
            serde_json::json!({"path": "/file"}),
        ));
        processor.process(tool_result("tc1", "content here"));
        processor.process(text_delta(" Here is what I found."));
        processor.process(finish("stop", 100, 50));

        assert_eq!(processor.reasoning(), "analyzing...");
        assert_eq!(
            processor.text(),
            "I'll help you with that. Here is what I found."
        );
        assert_eq!(processor.tool_calls().len(), 1);
        assert_eq!(processor.tool_results().len(), 1);
        assert!(processor.is_complete());
    }
}
