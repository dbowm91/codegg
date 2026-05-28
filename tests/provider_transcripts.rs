#[cfg(test)]
mod tests {
    use codegg::provider::{ChatRequest, ContentPart, Message, ToolCall, ToolDefinition};
    use std::sync::Arc;

    fn make_chat_request(messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            messages,
            model: "gpt-4o".to_string(),
            tools: Some(vec![ToolDefinition {
                name: "echo_args".to_string(),
                description: "Echoes the input".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "value": { "type": "string" }
                    },
                    "required": ["value"]
                }),
            }]),
            system: Some("You are a helpful assistant.".to_string()),
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
        }
    }

    fn text_content(text: &str) -> ContentPart {
        ContentPart::Text {
            text: Arc::new(text.to_string()),
        }
    }

    fn tc(id: &str, name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: Arc::new(id.to_string()),
            name: Arc::new(name.to_string()),
            arguments: args,
        }
    }

    fn tool_msg(id: &str, content: &str) -> Message {
        Message::Tool {
            tool_call_id: Arc::new(id.to_string()),
            content: Arc::new(content.to_string()),
        }
    }

    #[test]
    fn test_openai_serialize_user_message() {
        use codegg::provider::openai::OpenAiConfig;
        use codegg::provider::openai::OpenAiProvider;

        let config = OpenAiConfig::default();
        let provider = OpenAiProvider::new(config);

        let request = make_chat_request(vec![Message::User {
            content: vec![text_content("Hello")],
        }]);

        let body = provider.build_body(&request);
        let messages = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].get("role").unwrap().as_str().unwrap(), "user");
        assert_eq!(
            messages[0].get("content").unwrap().as_str().unwrap(),
            "Hello"
        );
    }

    #[test]
    fn test_openai_serialize_assistant_with_tool_calls() {
        use codegg::provider::openai::OpenAiConfig;
        use codegg::provider::openai::OpenAiProvider;

        let config = OpenAiConfig::default();
        let provider = OpenAiProvider::new(config);

        let messages = vec![
            Message::User {
                content: vec![text_content("Use echo_args")],
            },
            Message::Assistant {
                content: vec![text_content("I'll use echo_args")],
                tool_calls: vec![tc(
                    "call_1",
                    "echo_args",
                    serde_json::json!({"value": "hello"}),
                )],
            },
            tool_msg("call_1", r#"{"value":"hello"}"#),
        ];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 3);

        let assistant_msg = &msgs[1];
        assert_eq!(
            assistant_msg.get("role").unwrap().as_str().unwrap(),
            "assistant"
        );
        assert_eq!(
            assistant_msg.get("content").unwrap().as_str().unwrap(),
            "I'll use echo_args"
        );

        let tool_calls = assistant_msg.get("tool_calls").unwrap().as_array().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].get("id").unwrap().as_str().unwrap(), "call_1");
        assert_eq!(
            tool_calls[0].get("type").unwrap().as_str().unwrap(),
            "function"
        );
        let func = tool_calls[0].get("function").unwrap();
        assert_eq!(func.get("name").unwrap().as_str().unwrap(), "echo_args");
        let args: serde_json::Value = serde_json::from_str(
            func.get("arguments").unwrap().as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(
            args.get("value").unwrap().as_str().unwrap(),
            "hello"
        );

        let tool_msg_result = &msgs[2];
        assert_eq!(
            tool_msg_result.get("role").unwrap().as_str().unwrap(),
            "tool"
        );
        assert_eq!(
            tool_msg_result
                .get("tool_call_id")
                .unwrap()
                .as_str()
                .unwrap(),
            "call_1"
        );
    }

    #[test]
    fn test_openai_serialize_text_plus_tool_call_same_turn() {
        use codegg::provider::openai::OpenAiConfig;
        use codegg::provider::openai::OpenAiProvider;

        let config = OpenAiConfig::default();
        let provider = OpenAiProvider::new(config);

        let messages = vec![
            Message::User {
                content: vec![text_content("Use echo_args")],
            },
            Message::Assistant {
                content: vec![
                    text_content("I'll use echo_args with "),
                    text_content("this value"),
                ],
                tool_calls: vec![tc(
                    "call_1",
                    "echo_args",
                    serde_json::json!({"value": "hello"}),
                )],
            },
        ];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 2);

        let assistant_msg = &msgs[1];
        assert_eq!(
            assistant_msg.get("role").unwrap().as_str().unwrap(),
            "assistant"
        );
        assert_eq!(
            assistant_msg.get("content").unwrap().as_str().unwrap(),
            "I'll use echo_args with this value"
        );

        let tool_calls = assistant_msg.get("tool_calls").unwrap().as_array().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].get("id").unwrap().as_str().unwrap(), "call_1");
    }

    #[test]
    fn test_anthropic_serialize_assistant_tool_use() {
        use codegg::provider::anthropic::AnthropicProvider;

        let provider = AnthropicProvider::new("test-key".to_string());

        let messages = vec![
            Message::User {
                content: vec![text_content("Use echo_args")],
            },
            Message::Assistant {
                content: vec![text_content("I'll use echo_args")],
                tool_calls: vec![tc(
                    "call_1",
                    "echo_args",
                    serde_json::json!({"value": "hello"}),
                )],
            },
        ];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 2);

        let assistant_msg = &msgs[1];
        assert_eq!(
            assistant_msg.get("role").unwrap().as_str().unwrap(),
            "assistant"
        );

        let content = assistant_msg.get("content").unwrap().as_array().unwrap();
        assert!(content.len() >= 2);

        let text_part = &content[0];
        assert_eq!(text_part.get("type").unwrap().as_str().unwrap(), "text");
        assert_eq!(
            text_part.get("text").unwrap().as_str().unwrap(),
            "I'll use echo_args"
        );

        let tool_use_part = &content[1];
        assert_eq!(
            tool_use_part.get("type").unwrap().as_str().unwrap(),
            "tool_use"
        );
        assert_eq!(tool_use_part.get("id").unwrap().as_str().unwrap(), "call_1");
        assert_eq!(
            tool_use_part.get("name").unwrap().as_str().unwrap(),
            "echo_args"
        );
        assert_eq!(
            tool_use_part
                .get("input")
                .unwrap()
                .get("value")
                .unwrap()
                .as_str()
                .unwrap(),
            "hello"
        );
    }

    #[test]
    fn test_anthropic_serialize_tool_result() {
        use codegg::provider::anthropic::AnthropicProvider;

        let provider = AnthropicProvider::new("test-key".to_string());

        let messages = vec![
            Message::User {
                content: vec![text_content("Use echo_args")],
            },
            Message::Assistant {
                content: vec![text_content("I'll use echo_args")],
                tool_calls: vec![tc(
                    "call_1",
                    "echo_args",
                    serde_json::json!({"value": "hello"}),
                )],
            },
            tool_msg("call_1", r#"{"value":"hello"}"#),
        ];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 3);

        let tool_msg_result = &msgs[2];
        assert_eq!(
            tool_msg_result.get("role").unwrap().as_str().unwrap(),
            "user"
        );

        let content = tool_msg_result.get("content").unwrap().as_array().unwrap();
        assert_eq!(content.len(), 1);

        let tool_result = &content[0];
        assert_eq!(
            tool_result.get("type").unwrap().as_str().unwrap(),
            "tool_result"
        );
        assert_eq!(
            tool_result.get("tool_use_id").unwrap().as_str().unwrap(),
            "call_1"
        );
        assert_eq!(
            tool_result.get("content").unwrap().as_str().unwrap(),
            r#"{"value":"hello"}"#
        );
    }

    #[test]
    fn test_tool_result_id_matches_assistant_tool_call_id() {
        use codegg::provider::openai::OpenAiConfig;
        use codegg::provider::openai::OpenAiProvider;

        let config = OpenAiConfig::default();
        let provider = OpenAiProvider::new(config);

        let tool_call_id_str = "call_abc123";

        let messages = vec![
            Message::User {
                content: vec![text_content("Test")],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![tc(
                    tool_call_id_str,
                    "echo_args",
                    serde_json::json!({"value": "test"}),
                )],
            },
            tool_msg(tool_call_id_str, r#"{"status":"ok"}"#),
        ];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        let assistant_tool_calls = msgs[1].get("tool_calls").unwrap().as_array().unwrap();
        assert_eq!(
            assistant_tool_calls[0].get("id").unwrap().as_str().unwrap(),
            tool_call_id_str
        );

        let tool_msg_result = &msgs[2];
        assert_eq!(
            tool_msg_result
                .get("tool_call_id")
                .unwrap()
                .as_str()
                .unwrap(),
            tool_call_id_str
        );
    }

    #[test]
    fn test_compaction_preserves_assistant_tool_call_and_tool_result_pair() {
        use codegg::agent::compaction::{compact_messages, CompactionStrategy};

        let messages = vec![
            Message::User {
                content: vec![text_content("Hello")],
            },
            Message::Assistant {
                content: vec![text_content("I'll help")],
                tool_calls: vec![tc(
                    "call_1",
                    "echo_args",
                    serde_json::json!({"value": "test"}),
                )],
            },
            tool_msg("call_1", r#"{"value":"test"}"#),
        ];

        let result = compact_messages(messages, CompactionStrategy::DropMiddleMessages);

        let assistant_idx = result
            .iter()
            .position(|m| matches!(m, Message::Assistant { .. }));
        let tool_idx = result
            .iter()
            .position(|m| matches!(m, Message::Tool { .. }));

        assert!(assistant_idx.is_some(), "Should preserve assistant message");
        assert!(tool_idx.is_some(), "Should preserve tool message");

        if let (Some(a), Some(t)) = (assistant_idx, tool_idx) {
            assert!(a < t, "Assistant should come before tool result");
        }

        if let Message::Assistant { tool_calls, .. } = &result[assistant_idx.unwrap()] {
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].id.as_ref(), "call_1");
        }

        if let Message::Tool { tool_call_id, .. } = &result[tool_idx.unwrap()] {
            assert_eq!(tool_call_id.as_ref(), "call_1");
        }
    }

    #[test]
    fn test_compaction_preserves_tool_ids() {
        use codegg::agent::compaction::compact_messages;
        use codegg::agent::compaction::CompactionStrategy;

        let messages = vec![
            Message::User {
                content: vec![text_content("Hello")],
            },
            Message::Assistant {
                content: vec![text_content("I'll help")],
                tool_calls: vec![tc(
                    "call_xyz",
                    "echo_args",
                    serde_json::json!({"value": "test"}),
                )],
            },
            tool_msg("call_xyz", r#"{"value":"test"}"#),
        ];

        let result = compact_messages(messages, CompactionStrategy::TruncateToolOutputs);

        let mut found_tool_id = false;
        for m in &result {
            match m {
                Message::Assistant { tool_calls, .. } => {
                    for tc in tool_calls {
                        if tc.id.as_ref() == "call_xyz" {
                            found_tool_id = true;
                        }
                    }
                }
                Message::Tool { tool_call_id, .. } => {
                    if tool_call_id.as_ref() == "call_xyz" {
                        found_tool_id = true;
                    }
                }
                _ => {}
            }
        }
        assert!(
            found_tool_id,
            "Tool ID 'call_xyz' should survive compaction"
        );
    }

    #[test]
    fn test_openai_serialize_assistant_text_only() {
        use codegg::provider::openai::OpenAiConfig;
        use codegg::provider::openai::OpenAiProvider;

        let config = OpenAiConfig::default();
        let provider = OpenAiProvider::new(config);

        let messages = vec![Message::Assistant {
            content: vec![text_content("Hello, I'm an assistant")],
            tool_calls: vec![],
        }];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 1);
        let assistant_msg = &msgs[0];
        assert_eq!(
            assistant_msg.get("role").unwrap().as_str().unwrap(),
            "assistant"
        );
        assert_eq!(
            assistant_msg.get("content").unwrap().as_str().unwrap(),
            "Hello, I'm an assistant"
        );
        assert!(assistant_msg.get("tool_calls").is_none());
    }

    #[test]
    fn test_anthropic_serialize_assistant_text_only() {
        use codegg::provider::anthropic::AnthropicProvider;

        let provider = AnthropicProvider::new("test-key".to_string());

        let messages = vec![Message::Assistant {
            content: vec![text_content("Hello, I'm an assistant")],
            tool_calls: vec![],
        }];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 1);
        let assistant_msg = &msgs[0];
        assert_eq!(
            assistant_msg.get("role").unwrap().as_str().unwrap(),
            "assistant"
        );

        let content = assistant_msg.get("content").unwrap().as_array().unwrap();
        assert_eq!(content.len(), 1);
        let text_part = &content[0];
        assert_eq!(text_part.get("type").unwrap().as_str().unwrap(), "text");
        assert_eq!(
            text_part.get("text").unwrap().as_str().unwrap(),
            "Hello, I'm an assistant"
        );
    }

    #[test]
    fn test_openai_serialize_assistant_tool_call_only() {
        use codegg::provider::openai::OpenAiConfig;
        use codegg::provider::openai::OpenAiProvider;

        let config = OpenAiConfig::default();
        let provider = OpenAiProvider::new(config);

        let messages = vec![Message::Assistant {
            content: vec![],
            tool_calls: vec![tc(
                "call_1",
                "echo_args",
                serde_json::json!({"value": "test"}),
            )],
        }];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 1);
        let assistant_msg = &msgs[0];
        assert_eq!(
            assistant_msg.get("role").unwrap().as_str().unwrap(),
            "assistant"
        );
        assert_eq!(
            assistant_msg.get("content").unwrap().as_str().unwrap(),
            ""
        );

        let tool_calls = assistant_msg.get("tool_calls").unwrap().as_array().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].get("id").unwrap().as_str().unwrap(), "call_1");
        assert_eq!(
            tool_calls[0].get("type").unwrap().as_str().unwrap(),
            "function"
        );
        let func = tool_calls[0].get("function").unwrap();
        assert_eq!(func.get("name").unwrap().as_str().unwrap(), "echo_args");
        let args: serde_json::Value = serde_json::from_str(
            func.get("arguments").unwrap().as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(
            args.get("value").unwrap().as_str().unwrap(),
            "test"
        );
    }

    #[test]
    fn test_minimax_serialize_assistant_tool_call_only() {
        use codegg::provider::openai_compatible::{
            OpenAiCompatibleConfig, OpenAiCompatibleProvider,
        };

        let provider = OpenAiCompatibleProvider::new(
            "minimax",
            "MiniMax",
            OpenAiCompatibleConfig {
                api_key: "test-key".to_string(),
                base_url: "https://api.minimax.io/v1".to_string(),
                auth_header: "Authorization".to_string(),
                extra_headers: Vec::new(),
                models: Vec::new(),
                tool_choice_auto: false,
            },
        );

        let messages = vec![Message::Assistant {
            content: vec![],
            tool_calls: vec![tc(
                "call_1",
                "echo_args",
                serde_json::json!({"value": "test"}),
            )],
        }];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 1);
        let assistant_msg = &msgs[0];
        assert_eq!(
            assistant_msg.get("role").unwrap().as_str().unwrap(),
            "assistant"
        );
        assert_eq!(
            assistant_msg.get("content").unwrap().as_str().unwrap(),
            ""
        );

        let tool_calls = assistant_msg.get("tool_calls").unwrap().as_array().unwrap();
        assert_eq!(tool_calls.len(), 1);
        let func = tool_calls[0].get("function").unwrap();
        assert_eq!(func.get("name").unwrap().as_str().unwrap(), "echo_args");
        let args: serde_json::Value = serde_json::from_str(
            func.get("arguments").unwrap().as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(args.get("value").unwrap().as_str().unwrap(), "test");
    }

    #[test]
    fn test_anthropic_serialize_assistant_tool_call_only() {
        use codegg::provider::anthropic::AnthropicProvider;

        let provider = AnthropicProvider::new("test-key".to_string());

        let messages = vec![Message::Assistant {
            content: vec![],
            tool_calls: vec![tc(
                "call_1",
                "echo_args",
                serde_json::json!({"value": "test"}),
            )],
        }];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 1);
        let assistant_msg = &msgs[0];
        assert_eq!(
            assistant_msg.get("role").unwrap().as_str().unwrap(),
            "assistant"
        );

        let content = assistant_msg.get("content").unwrap().as_array().unwrap();
        assert_eq!(content.len(), 1);
        let tool_use_part = &content[0];
        assert_eq!(
            tool_use_part.get("type").unwrap().as_str().unwrap(),
            "tool_use"
        );
        assert_eq!(tool_use_part.get("id").unwrap().as_str().unwrap(), "call_1");
        assert_eq!(
            tool_use_part.get("name").unwrap().as_str().unwrap(),
            "echo_args"
        );
        assert_eq!(
            tool_use_part
                .get("input")
                .unwrap()
                .get("value")
                .unwrap()
                .as_str()
                .unwrap(),
            "test"
        );
    }

    #[test]
    fn test_openai_serialize_multiple_tool_calls() {
        use codegg::provider::openai::OpenAiConfig;
        use codegg::provider::openai::OpenAiProvider;

        let config = OpenAiConfig::default();
        let provider = OpenAiProvider::new(config);

        let messages = vec![Message::Assistant {
            content: vec![text_content("Running two tools")],
            tool_calls: vec![
                tc("call_1", "echo_args", serde_json::json!({"value": "a"})),
                tc("call_2", "echo_args", serde_json::json!({"value": "b"})),
            ],
        }];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        let assistant_msg = &msgs[0];
        let tool_calls = assistant_msg.get("tool_calls").unwrap().as_array().unwrap();
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0].get("id").unwrap().as_str().unwrap(), "call_1");
        assert_eq!(
            tool_calls[0]
                .get("function")
                .unwrap()
                .get("name")
                .unwrap()
                .as_str()
                .unwrap(),
            "echo_args"
        );
        assert_eq!(tool_calls[1].get("id").unwrap().as_str().unwrap(), "call_2");
        assert_eq!(
            tool_calls[1]
                .get("function")
                .unwrap()
                .get("name")
                .unwrap()
                .as_str()
                .unwrap(),
            "echo_args"
        );
    }

    #[test]
    fn test_anthropic_serialize_multiple_tool_calls() {
        use codegg::provider::anthropic::AnthropicProvider;

        let provider = AnthropicProvider::new("test-key".to_string());

        let messages = vec![Message::Assistant {
            content: vec![text_content("Running two tools")],
            tool_calls: vec![
                tc("call_1", "echo_args", serde_json::json!({"value": "a"})),
                tc("call_2", "echo_args", serde_json::json!({"value": "b"})),
            ],
        }];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        let assistant_msg = &msgs[0];
        let content = assistant_msg.get("content").unwrap().as_array().unwrap();
        assert!(content.len() >= 3);
        let tool_use_parts: Vec<_> = content
            .iter()
            .filter(|p| p.get("type").unwrap().as_str().unwrap() == "tool_use")
            .collect();
        assert_eq!(tool_use_parts.len(), 2);
        assert_eq!(
            tool_use_parts[0].get("id").unwrap().as_str().unwrap(),
            "call_1"
        );
        assert_eq!(
            tool_use_parts[1].get("id").unwrap().as_str().unwrap(),
            "call_2"
        );
    }

    #[test]
    fn test_openai_serialize_multiple_tool_results() {
        use codegg::provider::openai::OpenAiConfig;
        use codegg::provider::openai::OpenAiProvider;

        let config = OpenAiConfig::default();
        let provider = OpenAiProvider::new(config);

        let messages = vec![
            Message::User {
                content: vec![text_content("Run two tools")],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![
                    tc("call_1", "echo_args", serde_json::json!({"value": "a"})),
                    tc("call_2", "echo_args", serde_json::json!({"value": "b"})),
                ],
            },
            tool_msg("call_1", "result_a"),
            tool_msg("call_2", "result_b"),
        ];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 4);
        let tool_msg1 = &msgs[2];
        assert_eq!(tool_msg1.get("role").unwrap().as_str().unwrap(), "tool");
        assert_eq!(
            tool_msg1.get("tool_call_id").unwrap().as_str().unwrap(),
            "call_1"
        );
        assert_eq!(
            tool_msg1.get("content").unwrap().as_str().unwrap(),
            "result_a"
        );

        let tool_msg2 = &msgs[3];
        assert_eq!(
            tool_msg2.get("tool_call_id").unwrap().as_str().unwrap(),
            "call_2"
        );
        assert_eq!(
            tool_msg2.get("content").unwrap().as_str().unwrap(),
            "result_b"
        );
    }

    #[test]
    fn test_anthropic_serialize_multiple_tool_results() {
        use codegg::provider::anthropic::AnthropicProvider;

        let provider = AnthropicProvider::new("test-key".to_string());

        let messages = vec![
            Message::User {
                content: vec![text_content("Run two tools")],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![
                    tc("call_1", "echo_args", serde_json::json!({"value": "a"})),
                    tc("call_2", "echo_args", serde_json::json!({"value": "b"})),
                ],
            },
            tool_msg("call_1", "result_a"),
            tool_msg("call_2", "result_b"),
        ];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 4);
        let tool_msg1 = &msgs[2];
        assert_eq!(tool_msg1.get("role").unwrap().as_str().unwrap(), "user");
        let content1 = tool_msg1.get("content").unwrap().as_array().unwrap();
        let tool_result1 = &content1[0];
        assert_eq!(
            tool_result1.get("type").unwrap().as_str().unwrap(),
            "tool_result"
        );
        assert_eq!(
            tool_result1.get("tool_use_id").unwrap().as_str().unwrap(),
            "call_1"
        );

        let tool_msg2 = &msgs[3];
        let content2 = tool_msg2.get("content").unwrap().as_array().unwrap();
        let tool_result2 = &content2[0];
        assert_eq!(
            tool_result2.get("tool_use_id").unwrap().as_str().unwrap(),
            "call_2"
        );
    }

    #[test]
    fn test_openai_serialize_denied_tool_result() {
        use codegg::provider::openai::OpenAiConfig;
        use codegg::provider::openai::OpenAiProvider;

        let config = OpenAiConfig::default();
        let provider = OpenAiProvider::new(config);

        let messages = vec![
            Message::Assistant {
                content: vec![],
                tool_calls: vec![tc(
                    "call_1",
                    "echo_args",
                    serde_json::json!({"value": "test"}),
                )],
            },
            Message::Tool {
                tool_call_id: Arc::new("call_1".to_string()),
                content: Arc::new("".to_string()),
            },
        ];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 2);
        let tool_msg = &msgs[1];
        assert_eq!(tool_msg.get("role").unwrap().as_str().unwrap(), "tool");
        assert_eq!(
            tool_msg.get("tool_call_id").unwrap().as_str().unwrap(),
            "call_1"
        );
        assert_eq!(tool_msg.get("content").unwrap().as_str().unwrap(), "");
    }

    #[test]
    fn test_anthropic_serialize_denied_tool_result() {
        use codegg::provider::anthropic::AnthropicProvider;

        let provider = AnthropicProvider::new("test-key".to_string());

        let messages = vec![
            Message::Assistant {
                content: vec![],
                tool_calls: vec![tc(
                    "call_1",
                    "echo_args",
                    serde_json::json!({"value": "test"}),
                )],
            },
            Message::Tool {
                tool_call_id: Arc::new("call_1".to_string()),
                content: Arc::new("".to_string()),
            },
        ];

        let request = make_chat_request(messages);
        let body = provider.build_body(&request);
        let msgs = body.get("messages").unwrap().as_array().unwrap();

        assert_eq!(msgs.len(), 2);
        let tool_msg = &msgs[1];
        assert_eq!(tool_msg.get("role").unwrap().as_str().unwrap(), "user");
        let content = tool_msg.get("content").unwrap().as_array().unwrap();
        let tool_result = &content[0];
        assert_eq!(
            tool_result.get("type").unwrap().as_str().unwrap(),
            "tool_result"
        );
        assert_eq!(
            tool_result.get("tool_use_id").unwrap().as_str().unwrap(),
            "call_1"
        );
        assert_eq!(tool_result.get("content").unwrap().as_str().unwrap(), "");
    }
}
