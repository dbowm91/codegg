//! Integration tests for the hosted program adapter.
//!
//! Tests the full lifecycle of `HostedProgramAdapter`: capability
//! negotiation, backend selection, stream event processing, broker
//! integration steps 3-8, deduplication, continuation, and
//! native-hosted equivalence.

use codegg::provider::responses_api::{
    filter_artifacts_for_provider, minimize_input_items, validate_arguments, validate_call_count,
    validate_result_size, ArtifactRef, HostedBackendPolicy, HostedProgramAdapter,
    HostedProgramEvent, InputValidation, ResponseItem, ResponseObject, ResponsesStreamEvent,
    ResponsesTransport, ResponsesTransportConfig, ResponsesUsage, MAX_RESULT_SIZE,
};
use codegg::provider::ProviderCapabilities;

// ─── Capability negotiation ────────────────────────────────────────

#[test]
fn openai_capabilities_host_full_support() {
    let caps = ProviderCapabilities::for_provider("openai");
    assert!(caps.supports_responses_api);
    assert!(caps.supports_hosted_programs);
    assert!(caps.supports_client_owned_nested_calls);
    assert!(caps.supports_hosted_continuation);
    assert!(caps.can_host_programs());
    assert!(caps.full_hosted_support());
    assert!(caps.supports_output_schema);
    assert!(!caps.requires_fingerprint);
    assert!(caps.max_result_size.is_some());
    assert!(caps.max_tool_calls_per_program.is_some());
}

#[test]
fn non_openai_capabilities_no_host_support() {
    for provider in &["anthropic", "google", "openrouter", "unknown"] {
        let caps = ProviderCapabilities::for_provider(provider);
        assert!(!caps.supports_responses_api, "{}", provider);
        assert!(!caps.supports_hosted_programs, "{}", provider);
        assert!(!caps.can_host_programs(), "{}", provider);
        assert!(!caps.full_hosted_support(), "{}", provider);
    }
}

// ─── Backend selection ─────────────────────────────────────────────

#[test]
fn hosted_preferred_resolves_to_hosted_for_openai() {
    let caps = ProviderCapabilities::for_provider("openai");
    let policy = HostedBackendPolicy::HostedPreferred;
    assert_eq!(
        policy.resolve(&caps),
        codegg::provider::responses_api::ResolvedBackend::Hosted
    );
}

#[test]
fn hosted_preferred_resolves_to_native_for_anthropic() {
    let caps = ProviderCapabilities::for_provider("anthropic");
    let policy = HostedBackendPolicy::HostedPreferred;
    assert_eq!(
        policy.resolve(&caps),
        codegg::provider::responses_api::ResolvedBackend::Native
    );
}

#[test]
fn hosted_required_fails_for_anthropic() {
    let caps = ProviderCapabilities::for_provider("anthropic");
    let policy = HostedBackendPolicy::HostedRequired;
    match policy.resolve(&caps) {
        codegg::provider::responses_api::ResolvedBackend::Failed { reason } => {
            assert!(reason.contains("does not support"));
        }
        other => panic!("expected Failed, got {:?}", other),
    }
}

// ─── Full lifecycle ────────────────────────────────────────────────

#[test]
fn full_lifecycle_program_started_to_terminal() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-lifecycle-1".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    let mut all_events = Vec::new();

    // 1. ResponseCreated → ProgramStarted
    all_events.extend(
        adapter.process_stream_event(ResponsesStreamEvent::ResponseCreated {
            response: ResponseObject {
                id: "resp-abc".to_string(),
                status: "in_progress".to_string(),
                output: vec![],
                usage: None,
                incomplete_details: None,
            },
        }),
    );

    // 2. Function call → NestedCall
    all_events.extend(
        adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: 0,
            item: ResponseItem::FunctionCall {
                call_id: "call-1".to_string(),
                name: "read".to_string(),
                arguments: r#"{"path":"/tmp/a.txt"}"#.to_string(),
            },
        }),
    );

    // 3. Record result → CompletedHostedCall
    adapter
        .record_call_result(
            "call-1".to_string(),
            "read".to_string(),
            compute_input_hash("read", r#"{"path":"/tmp/a.txt"}"#),
            true,
            serde_json::json!({"content": "file data"}),
        )
        .unwrap();

    // 4. ResponseCompleted → Terminal + Usage
    all_events.extend(
        adapter.process_stream_event(ResponsesStreamEvent::ResponseCompleted {
            response: ResponseObject {
                id: "resp-abc".to_string(),
                status: "completed".to_string(),
                output: vec![ResponseItem::FunctionCallOutput {
                    call_id: "call-1".to_string(),
                    output: serde_json::json!({"content": "file data"}),
                }],
                usage: Some(ResponsesUsage {
                    input_tokens: 50,
                    output_tokens: 100,
                    total_tokens: 150,
                    reasoning_tokens: None,
                }),
                incomplete_details: None,
            },
        }),
    );

    // Verify events
    assert!(all_events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::ProgramStarted { response_id, .. } if response_id == "resp-abc"
    )));
    assert!(all_events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::NestedCall { call_id, tool_name, .. }
            if call_id == "call-1" && tool_name == "read"
    )));
    assert!(all_events.iter().any(
        |e| matches!(e, HostedProgramEvent::Terminal { status, .. } if status == "completed")
    ));
    assert!(all_events
        .iter()
        .any(|e| matches!(e, HostedProgramEvent::Usage(_))));

    // Verify state
    assert_eq!(adapter.completed_call_count(), 1);
    assert!(adapter.is_call_completed("call-1"));
    let cont = adapter.continuation().unwrap();
    assert_eq!(cont.response_id, "resp-abc");
}

// ─── Deduplication ─────────────────────────────────────────────────

#[test]
fn duplicate_call_with_matching_args_returns_recorded_result() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-dedup-1".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    let hash = compute_input_hash("read", r#"{"path":"/tmp/a.txt"}"#);

    adapter
        .record_call_result(
            "call-1".to_string(),
            "read".to_string(),
            hash,
            true,
            serde_json::json!({"content": "original"}),
        )
        .unwrap();

    // Duplicate with same args
    let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: ResponseItem::FunctionCall {
            call_id: "call-1".to_string(),
            name: "read".to_string(),
            arguments: r#"{"path":"/tmp/a.txt"}"#.to_string(),
        },
    });

    match &events[0] {
        HostedProgramEvent::NestedCallResult {
            call_id,
            success,
            output,
            ..
        } => {
            assert_eq!(call_id, "call-1");
            assert!(success);
            assert_eq!(output, &serde_json::json!({"content": "original"}));
        }
        other => panic!("expected NestedCallResult, got {:?}", other),
    }
}

#[test]
fn duplicate_call_with_mismatched_args_is_terminal_error() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-dedup-2".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    adapter
        .record_call_result(
            "call-1".to_string(),
            "read".to_string(),
            "hash_original".to_string(),
            true,
            serde_json::json!({"content": "original"}),
        )
        .unwrap();

    // Duplicate with different args
    let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: ResponseItem::FunctionCall {
            call_id: "call-1".to_string(),
            name: "read".to_string(),
            arguments: r#"{"path":"/tmp/different.txt"}"#.to_string(),
        },
    });

    match &events[0] {
        HostedProgramEvent::Error { code, message } => {
            assert_eq!(code.as_deref(), Some("call_identity_mismatch"));
            assert!(message.contains("different arguments"));
        }
        other => panic!("expected Error, got {:?}", other),
    }
}

// ─── Broker integration steps 3-8 ─────────────────────────────────

#[test]
fn broker_step_3_tool_validation_rejects_denied_tool() {
    let caps = ProviderCapabilities::for_provider("openai");
    let adapter = HostedProgramAdapter::new(
        "tp-broker-1".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    )
    .with_denied_tools(vec!["bash".to_string(), "write".to_string()]);

    assert!(adapter
        .validate_tool_call("read", &serde_json::json!({}))
        .is_ok());
    assert!(adapter
        .validate_tool_call("bash", &serde_json::json!({}))
        .is_err());
    assert!(adapter
        .validate_tool_call("write", &serde_json::json!({}))
        .is_err());
}

#[test]
fn broker_step_3_tool_validation_rejects_outside_allowlist() {
    let caps = ProviderCapabilities::for_provider("openai");
    let adapter = HostedProgramAdapter::new(
        "tp-broker-2".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    )
    .with_allowed_tools(vec!["read".to_string(), "glob".to_string()]);

    assert!(adapter
        .validate_tool_call("read", &serde_json::json!({}))
        .is_ok());
    assert!(adapter
        .validate_tool_call("write", &serde_json::json!({}))
        .is_err());
}

#[test]
fn broker_step_3_tool_validation_rejects_invalid_arguments() {
    let caps = ProviderCapabilities::for_provider("openai");
    let adapter = HostedProgramAdapter::new(
        "tp-broker-3".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Non-JSON arguments
    let result = adapter.validate_tool_call("read", &serde_json::json!("not an object"));
    assert!(result.is_err());
}

#[test]
fn broker_step_4_reserve_and_release() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-broker-4".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    let normalized = adapter.reserve_call("c1", "read", "h1").unwrap();
    assert!(!normalized.is_empty());
    assert!(adapter.is_call_reserved("c1"));
    assert_eq!(adapter.reserved_call_count(), 1);

    assert!(adapter.release_reservation("c1"));
    assert!(!adapter.is_call_reserved("c1"));
}

#[test]
fn broker_step_4_reserve_rejects_exceeding_limit() {
    let caps = ProviderCapabilities {
        max_nested_calls: Some(1),
        ..ProviderCapabilities::for_provider("openai")
    };
    let mut adapter = HostedProgramAdapter::new(
        "tp-broker-5".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    adapter.reserve_call("c1", "read", "h1").unwrap();
    let result = adapter.reserve_call("c2", "read", "h2");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("exceeds maximum"));
}

#[test]
fn broker_step_6_record_result_validates_size() {
    let caps = ProviderCapabilities {
        max_result_size: Some(100),
        ..ProviderCapabilities::for_provider("openai")
    };
    let mut adapter = HostedProgramAdapter::new(
        "tp-broker-6".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Small result: OK
    let result = adapter.record_call_result(
        "c1".to_string(),
        "read".to_string(),
        "h1".to_string(),
        true,
        serde_json::json!({"ok": true}),
    );
    assert!(result.is_ok());

    // Large result: rejected
    let big = "x".repeat(200);
    let result = adapter.record_call_result(
        "c2".to_string(),
        "read".to_string(),
        "h2".to_string(),
        true,
        serde_json::json!({"data": big}),
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("exceeds maximum"));
}

#[test]
fn broker_step_7_bounded_result_output() {
    // Small result: passed through
    let small = serde_json::json!({"content": "hello"});
    let output = HostedProgramAdapter::build_call_output("c1".to_string(), &small);
    match output {
        ResponseItem::FunctionCallOutput { output, .. } => {
            assert_eq!(output, small);
        }
        _ => panic!("expected FunctionCallOutput"),
    }
}

#[test]
fn broker_step_8_continuation_state_persisted() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-broker-8".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    assert!(adapter.continuation().is_none());

    adapter.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-xyz".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    let cont = adapter.continuation().unwrap();
    assert_eq!(cont.response_id, "resp-xyz");

    // Process incomplete event
    adapter.process_stream_event(ResponsesStreamEvent::Incomplete {
        response: ResponseObject {
            id: "resp-xyz".to_string(),
            status: "incomplete".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
        reason: "max_tokens".to_string(),
    });

    // Continuation state should still be available
    let cont = adapter.continuation().unwrap();
    assert_eq!(cont.response_id, "resp-xyz");
}

// ─── Call count tracking ───────────────────────────────────────────

#[test]
fn total_result_bytes_tracks_across_calls() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-bytes-1".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    assert_eq!(adapter.total_result_bytes(), 0);

    adapter
        .record_call_result(
            "c1".to_string(),
            "read".to_string(),
            "h1".to_string(),
            true,
            serde_json::json!({"content": "hello"}),
        )
        .unwrap();

    let bytes_after_first = adapter.total_result_bytes();
    assert!(bytes_after_first > 0);

    adapter
        .record_call_result(
            "c2".to_string(),
            "read".to_string(),
            "h2".to_string(),
            true,
            serde_json::json!({"content": "world"}),
        )
        .unwrap();

    assert!(adapter.total_result_bytes() > bytes_after_first);
}

// ─── Incomplete / error events ─────────────────────────────────────

#[test]
fn incomplete_event_emits_program_incomplete() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-incomplete-1".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    adapter.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-1".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    let events = adapter.process_stream_event(ResponsesStreamEvent::Incomplete {
        response: ResponseObject {
            id: "resp-1".to_string(),
            status: "incomplete".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
        reason: "max_tokens".to_string(),
    });

    match &events[0] {
        HostedProgramEvent::ProgramIncomplete {
            reason,
            continuation_token,
            ..
        } => {
            assert_eq!(reason, "max_tokens");
            assert_eq!(continuation_token, "resp-1");
        }
        other => panic!("expected ProgramIncomplete, got {:?}", other),
    }
}

#[test]
fn error_event_passthrough() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-error-1".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    let events = adapter.process_stream_event(ResponsesStreamEvent::Error {
        code: Some("rate_limit".to_string()),
        message: "too many requests".to_string(),
    });

    match &events[0] {
        HostedProgramEvent::Error { code, message } => {
            assert_eq!(code.as_deref(), Some("rate_limit"));
            assert_eq!(message, "too many requests");
        }
        other => panic!("expected Error, got {:?}", other),
    }
}

// ─── Security: body minimization ───────────────────────────────────

#[test]
fn minimize_input_items_truncates_large_outputs() {
    let mut items = vec![
        ResponseItem::FunctionCallOutput {
            call_id: "c1".to_string(),
            output: serde_json::json!({"data": "small"}),
        },
        ResponseItem::FunctionCallOutput {
            call_id: "c2".to_string(),
            output: serde_json::json!({"data": "x".repeat(2000)}),
        },
    ];

    minimize_input_items(&mut items, 512);

    // First item should be untouched
    match &items[0] {
        ResponseItem::FunctionCallOutput { output, .. } => {
            assert!(output.get("data").is_some());
            assert!(output.get("truncated").is_none());
        }
        _ => panic!("expected FunctionCallOutput"),
    }

    // Second item should be truncated
    match &items[1] {
        ResponseItem::FunctionCallOutput { output, .. } => {
            assert!(output.get("truncated").unwrap().as_bool().unwrap());
            assert!(output.get("original_size").is_some());
        }
        _ => panic!("expected FunctionCallOutput"),
    }
}

#[test]
fn artifact_filtering_only_selected() {
    let artifacts = vec![
        ArtifactRef {
            id: "a1".to_string(),
            path: "/tmp/a.txt".to_string(),
            size: 100,
            selected_by_call: true,
        },
        ArtifactRef {
            id: "a2".to_string(),
            path: "/tmp/b.txt".to_string(),
            size: 200,
            selected_by_call: false,
        },
        ArtifactRef {
            id: "a3".to_string(),
            path: "/tmp/c.txt".to_string(),
            size: 300,
            selected_by_call: true,
        },
    ];

    let filtered = filter_artifacts_for_provider(&artifacts);
    assert_eq!(filtered.len(), 2);
    assert!(filtered.iter().all(|a| a.selected_by_call));
}

// ─── Transport configuration ───────────────────────────────────────

#[test]
fn transport_config_custom_timeout() {
    let config = ResponsesTransportConfig {
        request_timeout: std::time::Duration::from_secs(30),
        stream_idle_timeout: std::time::Duration::from_secs(10),
        max_sse_buffer_size: 1024 * 1024,
    };

    let transport = ResponsesTransport::with_config(
        "https://api.openai.com/v1".to_string(),
        "test-key".to_string(),
        config,
    );

    assert!(!transport.is_cancelled());
    transport.cancel();
    assert!(transport.is_cancelled());
    transport.reset_cancel();
    assert!(!transport.is_cancelled());
}

// ─── Input validation ──────────────────────────────────────────────

#[test]
fn validate_arguments_rejects_non_object() {
    assert!(matches!(
        validate_arguments(r#"[1, 2, 3]"#),
        InputValidation::Invalid { .. }
    ));
    assert!(matches!(
        validate_arguments(r#""string""#),
        InputValidation::Invalid { .. }
    ));
    assert!(matches!(
        validate_arguments("42"),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn validate_arguments_rejects_invalid_json() {
    assert!(matches!(
        validate_arguments("not json at all"),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn validate_arguments_accepts_valid_object() {
    assert_eq!(
        validate_arguments(r#"{"key": "value", "nested": {"a": 1}}"#),
        InputValidation::Valid
    );
}

#[test]
fn validate_result_size_accepts_small() {
    assert_eq!(
        validate_result_size(&serde_json::json!({"ok": true}), None),
        InputValidation::Valid
    );
}

#[test]
fn validate_result_size_rejects_oversized() {
    let big = "x".repeat(MAX_RESULT_SIZE + 1);
    let val = serde_json::json!({"data": big});
    assert!(matches!(
        validate_result_size(&val, None),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn validate_call_count_within_and_exceeding() {
    assert_eq!(validate_call_count(5, Some(10)), InputValidation::Valid);
    assert_eq!(validate_call_count(9, Some(10)), InputValidation::Valid);
    // At the limit (count == max) is rejected — you can have at most max-1
    assert!(matches!(
        validate_call_count(10, Some(10)),
        InputValidation::Invalid { .. }
    ));
}

// ─── Helper ────────────────────────────────────────────────────────

fn compute_input_hash(tool_name: &str, arguments: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    tool_name.hash(&mut h);
    arguments.hash(&mut h);
    format!("{:016x}", h.finish())
}
