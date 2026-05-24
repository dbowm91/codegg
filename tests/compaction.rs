#[cfg(test)]
mod tests {
    use codegg::agent::compaction::{
        auto_compact, compact_messages, detect_overflow, prune_tool_outputs, CompactionStrategy,
        ContextTracker,
    };
    use codegg::provider::{ContentPart, Message};
    use std::sync::Arc;

    #[test]
    fn test_compaction_trigger_under_threshold() {
        let mut tracker = ContextTracker::new(128_000, 0.85);
        tracker.add_message(&Message::System {
            content: Arc::new("You are a helpful assistant".to_string()),
        });
        tracker.add_message(&Message::User {
            content: vec![ContentPart::Text {
                text: Arc::new("Hello".to_string()),
            }],
        });
        assert!(!tracker.needs_compaction());
    }

    #[test]
    fn test_compaction_trigger_over_threshold() {
        let mut tracker = ContextTracker::new(100, 0.8);
        tracker.add_message(&Message::System {
            content: Arc::new("x".repeat(900)),
        });
        assert!(tracker.needs_compaction());
    }

    #[test]
    fn test_detect_overflow_exact_boundary() {
        let messages = vec![Message::System {
            content: Arc::new("x".repeat(118_000)),
        }];
        assert!(!detect_overflow(&messages, 128_000, 10_000));
    }

    #[test]
    fn test_auto_compact_prunes_long_outputs() {
        let long = "x".repeat(50_000);
        let messages = vec![
            Message::System {
                content: Arc::new("sys".to_string()),
            },
            Message::Tool {
                tool_call_id: Arc::new("1".to_string()),
                content: Arc::new(long),
            },
        ];
        let result = auto_compact(&messages, 1000, 0.5, true);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_compact_messages_preserves_system() {
        let messages = vec![
            Message::System {
                content: Arc::new("important system prompt".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("hi there".to_string()),
                }],
                tool_calls: vec![],
            },
        ];
        let result = compact_messages(messages.clone(), CompactionStrategy::DropMiddleMessages);
        assert!(result.iter().any(|m| matches!(m, Message::System { .. })));
    }

    #[test]
    fn test_compact_messages_truncate_tool_outputs() {
        let long = "x".repeat(1000);
        let messages = vec![
            Message::System {
                content: Arc::new("sys".to_string()),
            },
            Message::Tool {
                tool_call_id: Arc::new("1".to_string()),
                content: Arc::new(long.clone()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hi".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".to_string()),
                }],
                tool_calls: vec![],
            },
        ];
        let result = compact_messages(messages, CompactionStrategy::TruncateToolOutputs);
        assert_eq!(result.len(), 4);
        if let Message::Tool { content, .. } = &result[1] {
            assert!(content.len() < long.len());
        }
    }

    #[test]
    fn test_compact_messages_summarize_old_turns() {
        let messages: Vec<Message> = (0..10)
            .map(|i| {
                if i % 2 == 0 {
                    Message::User {
                        content: vec![ContentPart::Text {
                            text: Arc::new(format!("user {i}")),
                        }],
                    }
                } else {
                    Message::Assistant {
                        content: vec![ContentPart::Text {
                            text: Arc::new(format!("assistant {i}")),
                        }],
                        tool_calls: vec![],
                    }
                }
            })
            .collect();
        let result = compact_messages(messages, CompactionStrategy::SummarizeOldTurns);
        assert!(!result.is_empty());
        assert!(matches!(&result[0], Message::System { .. }));
    }

    #[test]
    fn test_compact_messages_drop_middle() {
        let messages: Vec<Message> = (0..8)
            .map(|i| {
                if i % 2 == 0 {
                    Message::User {
                        content: vec![ContentPart::Text {
                            text: Arc::new(format!("user {i}")),
                        }],
                    }
                } else {
                    Message::Assistant {
                        content: vec![ContentPart::Text {
                            text: Arc::new(format!("assistant {i}")),
                        }],
                        tool_calls: vec![],
                    }
                }
            })
            .collect();
        let result = compact_messages(messages, CompactionStrategy::DropMiddleMessages);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_auto_compact_no_prune_no_compact() {
        let messages = vec![
            Message::System {
                content: Arc::new("sys".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hi".to_string()),
                }],
            },
        ];
        let result = auto_compact(&messages, 128_000, 0.85, false);
        assert_eq!(result.len(), messages.len());
    }

    #[test]
    fn test_auto_compact_with_prune() {
        let long = "x".repeat(100_000);
        let messages = vec![
            Message::System {
                content: Arc::new("sys".to_string()),
            },
            Message::Tool {
                tool_call_id: Arc::new("1".to_string()),
                content: Arc::new(long),
            },
        ];
        let result = auto_compact(&messages, 1000, 0.5, true);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_context_tracker_needs_compaction() {
        let mut tracker = ContextTracker::new(100, 0.8);
        tracker.add_message(&Message::System {
            content: Arc::new("x".repeat(900)),
        });
        assert!(tracker.needs_compaction());
    }

    #[test]
    fn test_context_tracker_needs_overflow_protection() {
        let mut tracker = ContextTracker::new(100, 0.8);
        tracker.add_message(&Message::System {
            content: Arc::new("x".repeat(950)),
        });
        assert!(tracker.needs_overflow_protection(100));
    }

    #[test]
    fn test_prune_tool_outputs_short_content() {
        let messages = vec![Message::Tool {
            tool_call_id: Arc::new("1".to_string()),
            content: Arc::new("short".to_string()),
        }];
        let result = prune_tool_outputs(&messages, 100);
        assert_eq!(result.len(), 1);
        if let Message::Tool { content, .. } = &result[0] {
            assert_eq!(content.as_ref(), "short");
        } else {
            panic!("expected tool message");
        }
    }

    #[test]
    fn test_prune_tool_outputs_long_content() {
        let long = "x".repeat(100_000);
        let messages = vec![Message::Tool {
            tool_call_id: Arc::new("1".to_string()),
            content: Arc::new(long.clone()),
        }];
        let result = prune_tool_outputs(&messages, 100);
        assert_eq!(result.len(), 1);
        if let Message::Tool { content, .. } = &result[0] {
            assert!(content.len() < long.len());
            assert!(content.contains("truncated"));
        }
    }

    #[tokio::test]
    async fn test_auto_compact_async_fallback_when_no_provider() {
        use codegg::agent::compaction::auto_compact_async;

        let messages = vec![
            Message::System {
                content: Arc::new("sys".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".to_string()),
                }],
            },
        ];

        let result = auto_compact_async(&messages, 128_000, 0.85, false, None, None).await;
        assert_eq!(result.len(), messages.len());
    }

    #[test]
    fn test_compact_messages_preserves_system_with_summarize() {
        let messages: Vec<Message> = (0..10)
            .map(|i| {
                if i % 2 == 0 {
                    Message::User {
                        content: vec![ContentPart::Text {
                            text: Arc::new(format!("user {i}")),
                        }],
                    }
                } else {
                    Message::Assistant {
                        content: vec![ContentPart::Text {
                            text: Arc::new(format!("assistant {i}")),
                        }],
                        tool_calls: vec![],
                    }
                }
            })
            .collect();

        let result = compact_messages(messages.clone(), CompactionStrategy::SummarizeOldTurns);
        assert!(!result.is_empty());
        assert!(matches!(&result[0], Message::System { .. }));
    }

    // Packet 8: Truncating long tool output keeps IDs intact
    #[test]
    fn test_truncate_tool_output_preserves_id() {
        let long = "x".repeat(1000);
        let original_id = "test_tool_call_1".to_string();
        let messages = vec![Message::Tool {
            tool_call_id: Arc::new(original_id.clone()),
            content: Arc::new(long),
        }];
        let result = compact_messages(messages.clone(), CompactionStrategy::TruncateToolOutputs);
        assert_eq!(result.len(), 1);
        if let Message::Tool { tool_call_id, .. } = &result[0] {
            assert_eq!(tool_call_id.as_ref(), &original_id);
        } else {
            panic!("Expected tool message");
        }
    }

    // Packet 8: Drop-middle preserves assistant/tool pairs
    #[test]
    fn test_drop_middle_preserves_tool_pairs() {
        let messages: Vec<Message> = vec![
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("u1".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("a1".to_string()),
                }],
                tool_calls: vec![codegg::provider::ToolCall {
                    id: Arc::new("tc1".to_string()),
                    name: Arc::new("bash".to_string()),
                    arguments: serde_json::json!({}),
                }],
            },
            Message::Tool {
                tool_call_id: Arc::new("tc1".to_string()),
                content: Arc::new("result1".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("u2".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("a2".to_string()),
                }],
                tool_calls: vec![],
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("u3".to_string()),
                }],
            },
        ];

        let result = compact_messages(messages.clone(), CompactionStrategy::DropMiddleMessages);
        // Check no orphan tools: every tool has matching assistant
        let assistant_tool_ids: Vec<String> = result
            .iter()
            .filter_map(|m| match m {
                Message::Assistant { tool_calls, .. } => Some(
                    tool_calls
                        .iter()
                        .map(|tc| tc.id.as_ref().clone())
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .flatten()
            .collect();
        for m in &result {
            if let Message::Tool { tool_call_id, .. } = m {
                assert!(
                    assistant_tool_ids.contains(&tool_call_id.as_ref().to_string()),
                    "Orphan tool result with id {}",
                    tool_call_id
                );
            }
        }
    }

    // Packet 8: Summarization fallback no orphan tool results
    #[test]
    fn test_summarization_fallback_no_orphans() {
        let messages: Vec<Message> = vec![
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("u1".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("a1".to_string()),
                }],
                tool_calls: vec![codegg::provider::ToolCall {
                    id: Arc::new("tc1".to_string()),
                    name: Arc::new("bash".to_string()),
                    arguments: serde_json::json!({}),
                }],
            },
            Message::Tool {
                tool_call_id: Arc::new("tc1".to_string()),
                content: Arc::new("result1".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("u2".to_string()),
                }],
            },
        ];
        // Use SummarizeOldTurns which falls back to placeholder if no provider
        let result = compact_messages(messages.clone(), CompactionStrategy::SummarizeOldTurns);
        // Check no orphan tools
        let assistant_tool_ids: Vec<String> = result
            .iter()
            .filter_map(|m| match m {
                Message::Assistant { tool_calls, .. } => Some(
                    tool_calls
                        .iter()
                        .map(|tc| tc.id.as_ref().clone())
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .flatten()
            .collect();
        for m in &result {
            if let Message::Tool { tool_call_id, .. } = m {
                assert!(
                    assistant_tool_ids.contains(&tool_call_id.as_ref().to_string()),
                    "Orphan tool result after summarization: {}",
                    tool_call_id
                );
            }
        }
    }

    // Packet 8: Multiple tool calls in one assistant message
    #[test]
    fn test_multiple_tool_calls_preserve_ids_order() {
        let messages: Vec<Message> = vec![
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("a1".to_string()),
                }],
                tool_calls: vec![
                    codegg::provider::ToolCall {
                        id: Arc::new("tc1".to_string()),
                        name: Arc::new("bash".to_string()),
                        arguments: serde_json::json!({}),
                    },
                    codegg::provider::ToolCall {
                        id: Arc::new("tc2".to_string()),
                        name: Arc::new("read".to_string()),
                        arguments: serde_json::json!({}),
                    },
                ],
            },
            Message::Tool {
                tool_call_id: Arc::new("tc1".to_string()),
                content: Arc::new("result1".to_string()),
            },
            Message::Tool {
                tool_call_id: Arc::new("tc2".to_string()),
                content: Arc::new("result2".to_string()),
            },
        ];
        // Test truncation (preserves IDs)
        let truncated = compact_messages(messages.clone(), CompactionStrategy::TruncateToolOutputs);
        assert_eq!(truncated.len(), 3);
        // Check IDs are preserved
        if let Message::Tool { tool_call_id, .. } = &truncated[1] {
            assert_eq!(tool_call_id.as_ref(), "tc1");
        }
        if let Message::Tool { tool_call_id, .. } = &truncated[2] {
            assert_eq!(tool_call_id.as_ref(), "tc2");
        }
        // Test order preserved
        let tool_ids: Vec<String> = truncated
            .iter()
            .filter_map(|m| match m {
                Message::Tool { tool_call_id, .. } => Some(tool_call_id.as_ref().clone()),
                _ => None,
            })
            .collect();
        assert_eq!(tool_ids, vec!["tc1", "tc2"]);
    }
}
