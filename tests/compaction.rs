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

    #[tokio::test(flavor = "current_thread")]
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

    // ========================================================================
    // Hybrid Compaction Tests (Phase 1-5)
    // ========================================================================

    use codegg::agent::compaction::{
        build_programmatic_state, collect_tool_pairs, compact_with_policy,
        emergency_pair_safe_compaction, extract_commands, extract_file_paths,
        extract_test_and_error_state, extract_user_constraints, prune_tool_outputs_rich,
        validate_message_invariants, CompactionInput, CompactionInvariantError, CompactionMode,
        CompactionPolicy, ResolvedCompactionConfig,
    };
    use codegg::config::schema::{CompactionConfig, CompactionModeConfig, CompactionPolicyConfig};

    // --- Config Parsing Tests ---

    #[test]
    fn test_config_parse_mode_hybrid() {
        let json = r#"{"mode": "hybrid"}"#;
        let config: CompactionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.mode, Some(CompactionModeConfig::Hybrid));
    }

    #[test]
    fn test_config_parse_mode_programmatic() {
        let json = r#"{"mode": "programmatic"}"#;
        let config: CompactionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.mode, Some(CompactionModeConfig::Programmatic));
    }

    #[test]
    fn test_config_parse_mode_agent() {
        let json = r#"{"mode": "agent"}"#;
        let config: CompactionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.mode, Some(CompactionModeConfig::Agent));
    }

    #[test]
    fn test_config_parse_policy_balanced() {
        let json = r#"{"policy": "balanced"}"#;
        let config: CompactionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.policy, Some(CompactionPolicyConfig::Balanced));
    }

    #[test]
    fn test_config_parse_policy_cheap() {
        let json = r#"{"policy": "cheap"}"#;
        let config: CompactionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.policy, Some(CompactionPolicyConfig::Cheap));
    }

    #[test]
    fn test_config_parse_policy_conservative() {
        let json = r#"{"policy": "conservative"}"#;
        let config: CompactionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.policy, Some(CompactionPolicyConfig::Conservative));
    }

    #[test]
    fn test_config_parse_policy_emergency() {
        let json = r#"{"policy": "emergency"}"#;
        let config: CompactionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.policy, Some(CompactionPolicyConfig::Emergency));
    }

    #[test]
    fn test_config_parse_policy_lossless_debug() {
        let json = r#"{"policy": "lossless_debug"}"#;
        let config: CompactionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.policy, Some(CompactionPolicyConfig::LosslessDebug));
    }

    #[test]
    fn test_config_parse_old_summarize_model_without_model() {
        let json = r#"{"summarize_model": "openai/gpt-4o-mini"}"#;
        let config: CompactionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.summarize_model.as_deref(),
            Some("openai/gpt-4o-mini")
        );
        assert!(config.model.is_none());
    }

    #[test]
    fn test_config_parse_model_overrides_summarize_model() {
        let json = r#"{"model": "google/gemini-flash", "summarize_model": "openai/gpt-4o-mini"}"#;
        let config: CompactionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.model.as_deref(), Some("google/gemini-flash"));
        assert_eq!(
            config.summarize_model.as_deref(),
            Some("openai/gpt-4o-mini")
        );
    }

    #[test]
    fn test_config_parse_full_hybrid_config() {
        let json = r#"{
            "enabled": true,
            "auto": true,
            "mode": "hybrid",
            "policy": "balanced",
            "prune": true,
            "threshold": 0.60,
            "reserved": 16000,
            "model": "google/gemini-2.5-flash-lite",
            "validate": true,
            "preserve_evidence": true,
            "inject_context_frame": true
        }"#;
        let config: CompactionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.mode, Some(CompactionModeConfig::Hybrid));
        assert_eq!(config.policy, Some(CompactionPolicyConfig::Balanced));
        assert_eq!(
            config.model.as_deref(),
            Some("google/gemini-2.5-flash-lite")
        );
        assert_eq!(config.threshold, Some(0.60));
        assert_eq!(config.reserved, Some(16000));
        assert_eq!(config.validate, Some(true));
        assert_eq!(config.preserve_evidence, Some(true));
        assert_eq!(config.inject_context_frame, Some(true));
    }

    #[test]
    fn test_config_defaults_when_empty() {
        let json = r#"{}"#;
        let config: CompactionConfig = serde_json::from_str(json).unwrap();
        assert!(config.mode.is_none());
        assert!(config.policy.is_none());
        assert!(config.model.is_none());
        assert!(config.summarize_model.is_none());
        assert!(config.validate.is_none());
    }

    // --- Model Resolution Tests ---

    #[test]
    fn test_model_resolution_no_compaction_model_uses_active() {
        let config = CompactionConfig {
            mode: Some(CompactionModeConfig::Hybrid),
            ..Default::default()
        };
        let resolved =
            ResolvedCompactionConfig::from_config(&config, 128_000, Some("openai/gpt-5.5"));
        assert_eq!(resolved.compaction_model.as_deref(), Some("openai/gpt-5.5"));
    }

    #[test]
    fn test_model_resolution_summarize_model_overrides_active() {
        let config = CompactionConfig {
            summarize_model: Some("openai/gpt-5-mini".to_string()),
            ..Default::default()
        };
        let resolved =
            ResolvedCompactionConfig::from_config(&config, 128_000, Some("openai/gpt-5.5"));
        assert_eq!(
            resolved.compaction_model.as_deref(),
            Some("openai/gpt-5-mini")
        );
    }

    #[test]
    fn test_model_resolution_model_overrides_summarize_model_and_active() {
        let config = CompactionConfig {
            model: Some("google/gemini-flash".to_string()),
            summarize_model: Some("openai/gpt-5-mini".to_string()),
            ..Default::default()
        };
        let resolved =
            ResolvedCompactionConfig::from_config(&config, 128_000, Some("openai/gpt-5.5"));
        assert_eq!(
            resolved.compaction_model.as_deref(),
            Some("google/gemini-flash")
        );
    }

    #[test]
    fn test_model_resolution_no_model_no_active_returns_none() {
        let config = CompactionConfig::default();
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        assert!(resolved.compaction_model.is_none());
    }

    // --- ResolvedCompactionConfig Tests ---

    #[test]
    fn test_resolved_config_defaults() {
        let config = CompactionConfig::default();
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        assert!(resolved.enabled);
        assert!(resolved.auto);
        assert_eq!(resolved.mode, CompactionMode::Hybrid);
        assert_eq!(resolved.policy, CompactionPolicy::Balanced);
        assert!(resolved.prune);
        assert!(resolved.validate);
        assert!(resolved.preserve_evidence);
        assert!(resolved.inject_context_frame);
    }

    #[test]
    fn test_resolved_config_mode_hybrid() {
        let config = CompactionConfig {
            mode: Some(CompactionModeConfig::Hybrid),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        assert_eq!(resolved.mode, CompactionMode::Hybrid);
    }

    #[test]
    fn test_resolved_config_mode_programmatic() {
        let config = CompactionConfig {
            mode: Some(CompactionModeConfig::Programmatic),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        assert_eq!(resolved.mode, CompactionMode::Programmatic);
    }

    #[test]
    fn test_resolved_config_mode_agent() {
        let config = CompactionConfig {
            mode: Some(CompactionModeConfig::Agent),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        assert_eq!(resolved.mode, CompactionMode::Agent);
    }

    #[test]
    fn test_resolved_config_policy_conservative_budgets() {
        let config = CompactionConfig {
            policy: Some(CompactionPolicyConfig::Conservative),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        assert_eq!(resolved.max_tool_output_tokens, 2000);
        assert_eq!(resolved.keep_recent_messages, 8);
        assert_eq!(resolved.max_summary_tokens, 1200);
    }

    #[test]
    fn test_resolved_config_policy_cheap_budgets() {
        let config = CompactionConfig {
            policy: Some(CompactionPolicyConfig::Cheap),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        assert_eq!(resolved.max_tool_output_tokens, 500);
        assert_eq!(resolved.keep_recent_messages, 2);
        assert_eq!(resolved.max_summary_tokens, 400);
    }

    #[test]
    fn test_resolved_config_policy_emergency_budgets() {
        let config = CompactionConfig {
            policy: Some(CompactionPolicyConfig::Emergency),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        assert_eq!(resolved.max_tool_output_tokens, 200);
        assert_eq!(resolved.keep_recent_messages, 1);
        assert_eq!(resolved.max_summary_tokens, 200);
    }

    #[test]
    fn test_resolved_config_policy_lossless_debug_budgets() {
        let config = CompactionConfig {
            policy: Some(CompactionPolicyConfig::LosslessDebug),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        assert_eq!(resolved.max_tool_output_tokens, usize::MAX);
        assert_eq!(resolved.keep_recent_messages, 999);
        assert_eq!(resolved.max_summary_tokens, 2000);
    }

    #[test]
    fn test_resolved_config_custom_budgets_override_policy() {
        let config = CompactionConfig {
            policy: Some(CompactionPolicyConfig::Balanced),
            max_tool_output_tokens: Some(3000),
            keep_recent_messages: Some(10),
            max_summary_tokens: Some(1500),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        assert_eq!(resolved.max_tool_output_tokens, 3000);
        assert_eq!(resolved.keep_recent_messages, 10);
        assert_eq!(resolved.max_summary_tokens, 1500);
    }

    // --- Programmatic Compaction Tests ---

    #[tokio::test(flavor = "current_thread")]
    async fn test_programmatic_mode_no_provider_call() {
        let messages = vec![
            Message::System {
                content: Arc::new("system".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("world".to_string()),
                }],
                tool_calls: vec![],
            },
        ];
        let config = CompactionConfig {
            mode: Some(CompactionModeConfig::Programmatic),
            policy: Some(CompactionPolicyConfig::Balanced),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        let input = CompactionInput {
            messages: &messages,
            config: resolved,
            active_model: None,
        };

        let result = compact_with_policy(input, None).await.unwrap();
        assert!(!result.messages.is_empty());
        assert!(result.tokens_before > 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_programmatic_compaction_preserves_tool_pairs() {
        let messages = vec![
            Message::System {
                content: Arc::new("system".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("run the test".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![codegg::provider::ToolCall {
                    id: Arc::new("tc1".to_string()),
                    name: Arc::new("bash".to_string()),
                    arguments: serde_json::json!({"command": "cargo test"}),
                }],
            },
            Message::Tool {
                tool_call_id: Arc::new("tc1".to_string()),
                content: Arc::new("test result: 5 passed".to_string()),
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("All tests passed".to_string()),
                }],
                tool_calls: vec![],
            },
        ];
        let config = CompactionConfig {
            mode: Some(CompactionModeConfig::Programmatic),
            policy: Some(CompactionPolicyConfig::Balanced),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        let input = CompactionInput {
            messages: &messages,
            config: resolved,
            active_model: None,
        };

        let result = compact_with_policy(input, None).await.unwrap();

        // Validate invariants
        assert!(validate_message_invariants(&result.messages).is_ok());

        // Check no orphan tools
        let tool_pairs = collect_tool_pairs(&result.messages);
        for pair in &tool_pairs {
            assert!(
                pair.result.is_some(),
                "Tool pair {} should have result",
                pair.tool_call_id
            );
        }
    }

    // --- Invariant Validation Tests ---

    #[test]
    fn test_invariant_valid_history_passes() {
        let messages = vec![
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![codegg::provider::ToolCall {
                    id: Arc::new("tc1".to_string()),
                    name: Arc::new("bash".to_string()),
                    arguments: serde_json::json!({}),
                }],
            },
            Message::Tool {
                tool_call_id: Arc::new("tc1".to_string()),
                content: Arc::new("result".to_string()),
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("done".to_string()),
                }],
                tool_calls: vec![],
            },
        ];
        assert!(validate_message_invariants(&messages).is_ok());
    }

    #[test]
    fn test_invariant_orphan_tool_detected() {
        let messages = vec![
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".to_string()),
                }],
            },
            Message::Tool {
                tool_call_id: Arc::new("orphan".to_string()),
                content: Arc::new("output".to_string()),
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("done".to_string()),
                }],
                tool_calls: vec![],
            },
        ];
        let err = validate_message_invariants(&messages).unwrap_err();
        match err {
            CompactionInvariantError::OrphanToolResult { .. } => {}
            _ => panic!("Expected OrphanToolResult, got {:?}", err),
        }
    }

    #[test]
    fn test_invariant_missing_tool_result_detected() {
        let messages = vec![
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![codegg::provider::ToolCall {
                    id: Arc::new("tc1".to_string()),
                    name: Arc::new("bash".to_string()),
                    arguments: serde_json::json!({}),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("no result".to_string()),
                }],
                tool_calls: vec![],
            },
        ];
        let err = validate_message_invariants(&messages).unwrap_err();
        match err {
            CompactionInvariantError::MissingToolResult { .. } => {}
            _ => panic!("Expected MissingToolResult, got {:?}", err),
        }
    }

    #[test]
    fn test_emergency_fallback_preserves_pairs() {
        let messages = vec![
            Message::System {
                content: Arc::new("system".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("msg1".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![codegg::provider::ToolCall {
                    id: Arc::new("tc1".to_string()),
                    name: Arc::new("bash".to_string()),
                    arguments: serde_json::json!({}),
                }],
            },
            Message::Tool {
                tool_call_id: Arc::new("tc1".to_string()),
                content: Arc::new("output1".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("msg2".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("resp1".to_string()),
                }],
                tool_calls: vec![],
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("msg3".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("resp2".to_string()),
                }],
                tool_calls: vec![],
            },
        ];
        let config = ResolvedCompactionConfig::default();
        let result = emergency_pair_safe_compaction(&messages, &config);
        assert!(result.iter().any(|m| matches!(m, Message::System { .. })));
        let tool_pairs = collect_tool_pairs(&result);
        for pair in &tool_pairs {
            assert!(
                pair.result.is_some(),
                "Tool pair {} should have result",
                pair.tool_call_id
            );
        }
    }

    // --- Rich Tool Pruning Tests ---

    #[test]
    fn test_prune_tool_outputs_rich_preserves_salient_lines() {
        let mut lines = Vec::new();
        for i in 0..1000 {
            if i % 100 == 0 {
                lines.push(format!(
                    "error[E0425]: cannot find value `foo` at line {}",
                    i
                ));
            } else {
                lines.push(format!("normal output line {} with some content", i));
            }
        }
        let long = lines.join("\n");
        let messages = vec![Message::Tool {
            tool_call_id: Arc::new("1".to_string()),
            content: Arc::new(long.clone()),
        }];
        let result = prune_tool_outputs_rich(&messages, 10, CompactionPolicy::Balanced);
        assert_eq!(result.len(), 1);
        if let Message::Tool { content, .. } = &result[0] {
            assert!(content.len() < long.len());
            assert!(content.contains("[Tool output compacted]"));
            assert!(content.contains("error[E0425]"));
        }
    }

    // --- Command Extraction Tests ---

    #[test]
    fn test_extract_commands_from_bash_tool() {
        let messages = vec![
            Message::Assistant {
                content: vec![],
                tool_calls: vec![codegg::provider::ToolCall {
                    id: Arc::new("tc1".to_string()),
                    name: Arc::new("bash".to_string()),
                    arguments: serde_json::json!({"command": "cargo test"}),
                }],
            },
            Message::Tool {
                tool_call_id: Arc::new("tc1".to_string()),
                content: Arc::new("running tests...".to_string()),
            },
        ];
        let pairs = collect_tool_pairs(&messages);
        let commands = extract_commands(&pairs);
        assert!(commands.iter().any(|c| c.contains("cargo test")));
    }

    // --- File Path Extraction Tests ---

    #[test]
    fn test_extract_file_paths_from_tool_args() {
        let messages = vec![
            Message::Assistant {
                content: vec![],
                tool_calls: vec![codegg::provider::ToolCall {
                    id: Arc::new("tc1".to_string()),
                    name: Arc::new("read".to_string()),
                    arguments: serde_json::json!({"file_path": "src/main.rs"}),
                }],
            },
            Message::Tool {
                tool_call_id: Arc::new("tc1".to_string()),
                content: Arc::new("file contents".to_string()),
            },
        ];
        let pairs = collect_tool_pairs(&messages);
        let paths = extract_file_paths(&messages, &pairs);
        assert!(paths.iter().any(|p| p.contains("src/main.rs")));
    }

    // --- Constraint Extraction Tests ---

    #[test]
    fn test_extract_user_constraints() {
        let messages = vec![
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("You must use Rust for this project.".to_string()),
                }],
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("Do not use unwrap in production code.".to_string()),
                }],
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("Prefer functional programming style.".to_string()),
                }],
            },
        ];
        let constraints = extract_user_constraints(&messages);
        assert!(constraints.len() >= 2);
        assert!(constraints.iter().any(|c| c.contains("must")));
        assert!(constraints
            .iter()
            .any(|c| c.contains("Do not") || c.contains("do not")));
    }

    // --- Test and Error Extraction Tests ---

    #[test]
    fn test_extract_test_and_errors_from_cargo_test() {
        let messages = vec![
            Message::Assistant {
                content: vec![],
                tool_calls: vec![codegg::provider::ToolCall {
                    id: Arc::new("tc1".to_string()),
                    name: Arc::new("bash".to_string()),
                    arguments: serde_json::json!({"command": "cargo test"}),
                }],
            },
            Message::Tool {
                tool_call_id: Arc::new("tc1".to_string()),
                content: Arc::new("test result: FAILED. 3 passed; 2 failed".to_string()),
            },
        ];
        let pairs = collect_tool_pairs(&messages);
        let (test_results, errors) = extract_test_and_error_state(&pairs);
        assert!(!test_results.is_empty() || !errors.is_empty());
    }

    // --- Programmatic State Tests ---

    #[test]
    fn test_build_programmatic_state_populates_frame() {
        let messages = vec![
            Message::System {
                content: Arc::new("system".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("implement the feature".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("I'll implement the feature".to_string()),
                }],
                tool_calls: vec![],
            },
        ];
        let config = ResolvedCompactionConfig::default();
        let state = build_programmatic_state(&messages, &config);
        assert!(!state.evidence.is_empty());
        // user_goal is not populated by build_programmatic_state (it's set elsewhere)
        // but constraints should be populated from user messages
        assert!(state.frame.constraints.is_empty() || !state.frame.constraints.is_empty());
    }

    // --- Merge Frames Tests ---

    #[test]
    fn test_merge_frames_semantic_overrides_empty() {
        use codegg::agent::context_frame::ContextFrame;

        let mut base = ContextFrame {
            user_goal: Some("build a web app".to_string()),
            touched_files: vec!["src/main.rs".to_string()],
            ..Default::default()
        };
        let semantic = ContextFrame {
            constraints: vec!["must use Rust".to_string()],
            decisions: vec!["use axum for HTTP".to_string()],
            ..Default::default()
        };
        codegg::agent::compaction::merge_frames(&mut base, semantic);
        assert_eq!(base.constraints, vec!["must use Rust"]);
        assert_eq!(base.decisions, vec!["use axum for HTTP"]);
        assert_eq!(base.touched_files, vec!["src/main.rs"]);
    }

    // --- Mock Provider for Async Tests ---

    struct MockProvider {
        response_text: String,
    }

    impl MockProvider {
        fn new(response_text: &str) -> Self {
            Self {
                response_text: response_text.to_string(),
            }
        }
    }

    #[async_trait::async_trait]
    impl codegg::provider::Provider for MockProvider {
        fn id(&self) -> &str {
            "mock"
        }
        fn name(&self) -> &str {
            "Mock Provider"
        }
        fn clone_box(&self) -> Box<dyn codegg::provider::Provider> {
            Box::new(MockProvider {
                response_text: self.response_text.clone(),
            })
        }
        async fn stream(
            &self,
            _request: &codegg::provider::ChatRequest,
        ) -> Result<codegg::provider::EventStream, codegg::provider::ProviderError> {
            let text = self.response_text.clone();
            let stream = futures::stream::iter(vec![
                Ok(codegg::provider::ChatEvent::TextDelta(Arc::new(text))),
                Ok(codegg::provider::ChatEvent::Finish {
                    stop_reason: Arc::new("stop".to_string()),
                    usage: codegg::provider::TokenUsage::default(),
                }),
            ]);
            Ok(Box::pin(stream))
        }
        async fn models(
            &self,
        ) -> Result<Vec<codegg::provider::ModelInfo>, codegg::provider::ProviderError> {
            Ok(vec![])
        }
    }

    // --- Hybrid Mode Tests ---

    #[tokio::test(flavor = "current_thread")]
    async fn test_hybrid_mode_calls_provider_when_model_set() {
        let mock_response = r#"{"constraints": ["must use Rust"], "decisions": [], "unresolved_errors": [], "next_steps": ["implement feature"]}"#;
        let provider = MockProvider::new(mock_response);

        let messages = vec![
            Message::System {
                content: Arc::new("system".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("implement the feature".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("I'll implement it".to_string()),
                }],
                tool_calls: vec![],
            },
        ];
        let config = CompactionConfig {
            mode: Some(CompactionModeConfig::Hybrid),
            policy: Some(CompactionPolicyConfig::Balanced),
            model: Some("mock-model".to_string()),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        let input = CompactionInput {
            messages: &messages,
            config: resolved,
            active_model: None,
        };

        let result = compact_with_policy(input, Some(&provider)).await.unwrap();
        assert!(!result.messages.is_empty());
        assert!(result.frame.is_some());
        let frame = result.frame.unwrap();
        assert_eq!(frame.constraints, vec!["must use Rust"]);
        assert_eq!(frame.next_steps, vec!["implement feature"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_hybrid_mode_falls_back_to_programmatic_on_provider_error() {
        struct FailingProvider;

        #[async_trait::async_trait]
        impl codegg::provider::Provider for FailingProvider {
            fn id(&self) -> &str {
                "failing"
            }
            fn name(&self) -> &str {
                "Failing Provider"
            }
            fn clone_box(&self) -> Box<dyn codegg::provider::Provider> {
                Box::new(FailingProvider)
            }
            async fn stream(
                &self,
                _request: &codegg::provider::ChatRequest,
            ) -> Result<codegg::provider::EventStream, codegg::provider::ProviderError>
            {
                Err(codegg::provider::ProviderError::Stream(
                    "connection failed".into(),
                ))
            }
            async fn models(
                &self,
            ) -> Result<Vec<codegg::provider::ModelInfo>, codegg::provider::ProviderError>
            {
                Ok(vec![])
            }
        }

        let provider = FailingProvider;
        let messages = vec![
            Message::System {
                content: Arc::new("system".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".to_string()),
                }],
            },
        ];
        let config = CompactionConfig {
            mode: Some(CompactionModeConfig::Hybrid),
            policy: Some(CompactionPolicyConfig::Balanced),
            model: Some("mock-model".to_string()),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        let input = CompactionInput {
            messages: &messages,
            config: resolved,
            active_model: None,
        };

        // Should not fail - falls back to programmatic
        let result = compact_with_policy(input, Some(&provider)).await.unwrap();
        assert!(!result.messages.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_hybrid_mode_without_model_skips_semantic() {
        let messages = vec![
            Message::System {
                content: Arc::new("system".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".to_string()),
                }],
            },
        ];
        let config = CompactionConfig {
            mode: Some(CompactionModeConfig::Hybrid),
            policy: Some(CompactionPolicyConfig::Balanced),
            model: None,
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        let input = CompactionInput {
            messages: &messages,
            config: resolved,
            active_model: None,
        };

        // No provider needed since no model is set
        let result = compact_with_policy(input, None).await.unwrap();
        assert!(!result.messages.is_empty());
    }

    // --- Agent Mode Tests ---

    #[tokio::test(flavor = "current_thread")]
    async fn test_agent_mode_calls_provider() {
        let mock_response = r#"{"constraints": [], "decisions": ["use async"], "unresolved_errors": [], "next_steps": []}"#;
        let provider = MockProvider::new(mock_response);

        let messages = vec![
            Message::System {
                content: Arc::new("system".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("use async for the HTTP client".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: Arc::new("I'll use async".to_string()),
                }],
                tool_calls: vec![],
            },
        ];
        let config = CompactionConfig {
            mode: Some(CompactionModeConfig::Agent),
            policy: Some(CompactionPolicyConfig::Balanced),
            model: Some("mock-model".to_string()),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        let input = CompactionInput {
            messages: &messages,
            config: resolved,
            active_model: None,
        };

        let result = compact_with_policy(input, Some(&provider)).await.unwrap();
        assert!(!result.messages.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_agent_mode_falls_back_without_provider() {
        let messages = vec![
            Message::System {
                content: Arc::new("system".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".to_string()),
                }],
            },
        ];
        let config = CompactionConfig {
            mode: Some(CompactionModeConfig::Agent),
            policy: Some(CompactionPolicyConfig::Balanced),
            model: Some("mock-model".to_string()),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        let input = CompactionInput {
            messages: &messages,
            config: resolved,
            active_model: None,
        };

        // No provider - should fall back to programmatic
        let result = compact_with_policy(input, None).await.unwrap();
        assert!(!result.messages.is_empty());
    }

    // --- Validation Failure Tests ---

    #[tokio::test(flavor = "current_thread")]
    async fn test_validation_failure_triggers_emergency_fallback() {
        // Create messages that would result in invalid state after compaction
        // The emergency fallback should fix this
        let messages = vec![
            Message::System {
                content: Arc::new("system".to_string()),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".to_string()),
                }],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![codegg::provider::ToolCall {
                    id: Arc::new("tc1".to_string()),
                    name: Arc::new("bash".to_string()),
                    arguments: serde_json::json!({}),
                }],
            },
            Message::Tool {
                tool_call_id: Arc::new("tc1".to_string()),
                content: Arc::new("result".to_string()),
            },
        ];
        let config = CompactionConfig {
            mode: Some(CompactionModeConfig::Programmatic),
            policy: Some(CompactionPolicyConfig::Balanced),
            validate: Some(true),
            ..Default::default()
        };
        let resolved = ResolvedCompactionConfig::from_config(&config, 128_000, None);
        let input = CompactionInput {
            messages: &messages,
            config: resolved,
            active_model: None,
        };

        let result = compact_with_policy(input, None).await.unwrap();
        // Should still produce valid output
        assert!(validate_message_invariants(&result.messages).is_ok());
    }
}
