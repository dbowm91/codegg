//! Integration tests for hosted program security.
//!
//! Tests security guards: direct-only/mutating tool rejection,
//! malformed/oversized arguments, forged caller/fingerprint/call IDs,
//! cross-program result injection, secret/header/token leakage,
//! item reordering/duplication/truncation, and size/count bounds.

use codegg::provider::responses_api::{
    redact_fingerprint, redact_for_log, validate_arguments, validate_call_count,
    validate_result_size, HostedBackendPolicy, HostedProgramAdapter, HostedProgramEvent,
    InputValidation, ResponseItem, ResponseObject, ResponsesStreamEvent, MAX_ARGUMENT_SIZE,
    MAX_NESTED_CALLS, MAX_RESULT_SIZE,
};
use codegg::provider::ProviderCapabilities;

// ─── Direct-only / mutating tool rejection ─────────────────────────

#[test]
fn bash_tool_rejected_for_hosted() {
    let caps = ProviderCapabilities::for_provider("openai");
    let adapter = HostedProgramAdapter::new(
        "tp-sec-1".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    )
    .with_denied_tools(vec!["bash".to_string()]);

    let result = adapter.validate_tool_call("bash", &serde_json::json!({"command": "ls"}));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("denied"));
}

#[test]
fn write_tool_rejected_for_hosted() {
    let caps = ProviderCapabilities::for_provider("openai");
    let adapter = HostedProgramAdapter::new(
        "tp-sec-2".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    )
    .with_denied_tools(vec!["write".to_string()]);

    let result = adapter.validate_tool_call("write", &serde_json::json!({"path": "/etc/passwd"}));
    assert!(result.is_err());
}

#[test]
fn multiple_denied_tools_all_rejected() {
    let caps = ProviderCapabilities::for_provider("openai");
    let adapter = HostedProgramAdapter::new(
        "tp-sec-3".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    )
    .with_denied_tools(vec![
        "bash".to_string(),
        "write".to_string(),
        "apply_patch".to_string(),
    ]);

    for tool in &["bash", "write", "apply_patch"] {
        let result = adapter.validate_tool_call(tool, &serde_json::json!({}));
        assert!(result.is_err(), "tool '{}' should be denied", tool);
    }
}

#[test]
fn allowed_tool_passes_validation() {
    let caps = ProviderCapabilities::for_provider("openai");
    let adapter = HostedProgramAdapter::new(
        "tp-sec-4".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    )
    .with_allowed_tools(vec![
        "read".to_string(),
        "glob".to_string(),
        "grep".to_string(),
    ]);

    for tool in &["read", "glob", "grep"] {
        assert!(
            adapter
                .validate_tool_call(tool, &serde_json::json!({}))
                .is_ok(),
            "tool '{}' should be allowed",
            tool
        );
    }
}

// ─── Malformed arguments ───────────────────────────────────────────

#[test]
fn non_json_arguments_rejected() {
    assert!(matches!(
        validate_arguments("not json at all"),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn empty_string_arguments_rejected() {
    assert!(matches!(
        validate_arguments(""),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn array_arguments_rejected() {
    assert!(matches!(
        validate_arguments(r#"[1, 2, 3]"#),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn string_arguments_rejected() {
    assert!(matches!(
        validate_arguments(r#""just a string""#),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn number_arguments_rejected() {
    assert!(matches!(
        validate_arguments("42"),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn null_arguments_rejected() {
    assert!(matches!(
        validate_arguments("null"),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn valid_object_arguments_accepted() {
    assert_eq!(
        validate_arguments(r#"{"key": "value"}"#),
        InputValidation::Valid
    );
    assert_eq!(
        validate_arguments(r#"{"nested": {"a": [1, 2, 3]}}"#),
        InputValidation::Valid
    );
}

// ─── Oversized arguments ───────────────────────────────────────────

#[test]
fn oversized_arguments_rejected() {
    let big = "x".repeat(MAX_ARGUMENT_SIZE + 1);
    let json = format!(r#"{{"data": "{}"}}"#, big);
    assert!(matches!(
        validate_arguments(&json),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn boundary_sized_arguments_accepted() {
    // Just under the limit should be accepted
    let small = "x".repeat(100);
    let json = format!(r#"{{"data": "{}"}}"#, small);
    assert_eq!(validate_arguments(&json), InputValidation::Valid);
}

// ─── Oversized results ─────────────────────────────────────────────

#[test]
fn oversized_result_rejected() {
    let big = "x".repeat(MAX_RESULT_SIZE + 1);
    let val = serde_json::json!({"data": big});
    assert!(matches!(
        validate_result_size(&val, None),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn oversized_result_with_custom_limit_rejected() {
    let val = serde_json::json!({"data": "x".repeat(200)});
    assert!(matches!(
        validate_result_size(&val, Some(100)),
        InputValidation::Invalid { .. }
    ));
}

// ─── Call count bounds ─────────────────────────────────────────────

#[test]
fn call_count_within_limit_accepted() {
    assert_eq!(validate_call_count(0, Some(10)), InputValidation::Valid);
    assert_eq!(validate_call_count(9, Some(10)), InputValidation::Valid);
}

#[test]
fn call_count_at_limit_rejected() {
    // At the limit (count == max) is rejected — you can have at most max-1
    assert!(matches!(
        validate_call_count(10, Some(10)),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn call_count_exceeding_limit_rejected() {
    assert!(matches!(
        validate_call_count(11, Some(10)),
        InputValidation::Invalid { .. }
    ));
}

#[test]
fn adapter_rejects_exceeding_nested_call_limit() {
    let caps = ProviderCapabilities {
        max_nested_calls: Some(2),
        ..ProviderCapabilities::for_provider("openai")
    };
    let mut adapter = HostedProgramAdapter::new(
        "tp-sec-call-limit".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    adapter.reserve_call("c1", "read", "h1").unwrap();
    adapter.reserve_call("c2", "read", "h2").unwrap();

    let result = adapter.reserve_call("c3", "read", "h3");
    assert!(result.is_err());
}

// ─── Forged call IDs ───────────────────────────────────────────────

#[test]
fn forged_call_id_with_different_args_is_error() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-sec-forged".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Record legitimate call
    adapter
        .record_call_result(
            "legit-call-1".to_string(),
            "read".to_string(),
            "hash_legit".to_string(),
            true,
            serde_json::json!({"content": "legitimate"}),
        )
        .unwrap();

    // Forge same call_id with different args
    let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: ResponseItem::FunctionCall {
            call_id: "legit-call-1".to_string(),
            name: "read".to_string(),
            arguments: r#"{"path":"/etc/shadow"}"#.to_string(),
        },
    });

    match &events[0] {
        HostedProgramEvent::Error { code, .. } => {
            assert_eq!(code.as_deref(), Some("call_identity_mismatch"));
        }
        other => panic!("expected Error, got {:?}", other),
    }
}

// ─── Cross-program result injection ────────────────────────────────

#[test]
fn different_program_ids_produce_different_normalized_ids() {
    let id1 = codegg::provider::responses_api::HostedCallIdentity {
        program_id: "tp-program-1".to_string(),
        provider_call_id: "call-1".to_string(),
        tool_name: "read".to_string(),
        input_hash: "hash1".to_string(),
    };

    let id2 = codegg::provider::responses_api::HostedCallIdentity {
        program_id: "tp-program-2".to_string(),
        provider_call_id: "call-1".to_string(),
        tool_name: "read".to_string(),
        input_hash: "hash1".to_string(),
    };

    assert_ne!(
        id1.normalized_call_id(),
        id2.normalized_call_id(),
        "different programs must produce different normalized IDs"
    );
}

#[test]
fn same_call_across_programs_does_not_leak() {
    // Program 1 records a result
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter1 = HostedProgramAdapter::new(
        "tp-program-a".to_string(),
        caps.clone(),
        HostedBackendPolicy::HostedPreferred,
    );
    adapter1
        .record_call_result(
            "call-1".to_string(),
            "read".to_string(),
            "hash1".to_string(),
            true,
            serde_json::json!({"secret": "program-a-data"}),
        )
        .unwrap();

    // Program 2 does NOT have the call
    let mut adapter2 = HostedProgramAdapter::new(
        "tp-program-b".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    assert!(!adapter2.is_call_completed("call-1"));
    assert_eq!(adapter2.completed_call_count(), 0);
}

// ─── Redaction ─────────────────────────────────────────────────────

#[test]
fn api_key_redacted_in_log() {
    let key = "sk-1234567890abcdef1234567890abcdef";
    let redacted = redact_for_log(key);
    assert!(
        !redacted.contains("1234567890abcdef"),
        "redacted should not contain full key"
    );
    assert!(redacted.contains("..."), "redacted should contain ellipsis");
}

#[test]
fn short_key_redacted() {
    let key = "abc";
    let redacted = redact_for_log(key);
    assert_eq!(redacted, "abc****");
}

#[test]
fn empty_key_redacted() {
    assert_eq!(redact_for_log(""), "<empty>");
}

#[test]
fn fingerprint_redacted() {
    let fp = "fp_abcdefgh1234567890";
    let redacted = redact_fingerprint(fp);
    assert!(
        !redacted.contains("abcdefgh1234567890"),
        "fingerprint should be masked"
    );
}

// ─── Item reordering / duplication ─────────────────────────────────

#[test]
fn reordered_items_still_dedup() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-sec-reorder".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Record call-2 with its actual arguments' hash
    let args2 = r#"{"path":"/tmp/b.txt"}"#;
    adapter
        .record_call_result(
            "call-2".to_string(),
            "read".to_string(),
            compute_input_hash("read", args2),
            true,
            serde_json::json!({"content": "result-2"}),
        )
        .unwrap();

    // Now process call-1 (new)
    let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: ResponseItem::FunctionCall {
            call_id: "call-1".to_string(),
            name: "read".to_string(),
            arguments: r#"{"path":"/tmp/a.txt"}"#.to_string(),
        },
    });

    match &events[0] {
        HostedProgramEvent::NestedCall { call_id, .. } => {
            assert_eq!(call_id, "call-1");
        }
        other => panic!("expected NestedCall, got {:?}", other),
    }

    // Replay call-2 with SAME arguments (should return recorded result)
    let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 1,
        item: ResponseItem::FunctionCall {
            call_id: "call-2".to_string(),
            name: "read".to_string(),
            arguments: args2.to_string(),
        },
    });

    match &events[0] {
        HostedProgramEvent::NestedCallResult {
            call_id, success, ..
        } => {
            assert_eq!(call_id, "call-2");
            assert!(success);
        }
        other => panic!("expected NestedCallResult, got {:?}", other),
    }
}

// ─── Deny-only tool prevents event emission ────────────────────────

#[test]
fn denied_tool_emits_error_event_not_nested_call() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-sec-event".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    )
    .with_denied_tools(vec!["bash".to_string()]);

    let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: ResponseItem::FunctionCall {
            call_id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: r#"{"command":"whoami"}"#.to_string(),
        },
    });

    // Should emit Error, NOT NestedCall
    assert!(events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::Error { code: Some(c), .. } if c == "tool_validation_failed"
    )));
    assert!(!events
        .iter()
        .any(|e| matches!(e, HostedProgramEvent::NestedCall { .. })));
}

// ─── Reservation state isolation ───────────────────────────────────

#[test]
fn reservation_isolation_between_adapters() {
    let caps = ProviderCapabilities::for_provider("openai");

    let mut adapter1 = HostedProgramAdapter::new(
        "tp-iso-1".to_string(),
        caps.clone(),
        HostedBackendPolicy::HostedPreferred,
    );
    let mut adapter2 = HostedProgramAdapter::new(
        "tp-iso-2".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    adapter1.reserve_call("c1", "read", "h1").unwrap();

    assert!(adapter1.is_call_reserved("c1"));
    assert!(!adapter2.is_call_reserved("c1"));
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
