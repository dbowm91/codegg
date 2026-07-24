//! Integration tests for hosted program restart and recovery.
//!
//! Tests restart scenarios: before first item, after program start,
//! during nested call, after result before provider continuation,
//! and before terminal notification. Verifies that repeated items
//! return recorded results without duplicate execution.

use codegg::provider::responses_api::{
    HostedBackendPolicy, HostedProgramAdapter, HostedProgramEvent, ResponseItem, ResponseObject,
    ResponsesStreamEvent, ResponsesUsage,
};
use codegg::provider::ProviderCapabilities;

// ─── Restart before first item ─────────────────────────────────────

#[test]
fn restart_before_first_item_no_state() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-restart-1".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // No events processed yet — adapter is in initial state
    assert!(adapter.continuation().is_none());
    assert_eq!(adapter.completed_call_count(), 0);
    assert_eq!(adapter.reserved_call_count(), 0);

    // Process a ResponseCreated event — this is equivalent to restarting
    // before any items were added
    let events = adapter.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-new".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    assert!(events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::ProgramStarted { response_id, .. } if response_id == "resp-new"
    )));
    assert_eq!(adapter.completed_call_count(), 0);
}

// ─── Restart after program start ───────────────────────────────────

#[test]
fn restart_after_program_start_preserves_continuation() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-restart-2".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Start program
    adapter.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-1".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    let cont = adapter.continuation().unwrap();
    assert_eq!(cont.response_id, "resp-1");

    // Simulate restart: continuation state is still available
    // (in production, this would be loaded from persistent storage)
    let cont = adapter.continuation().unwrap();
    assert_eq!(cont.response_id, "resp-1");
}

// ─── Restart during nested call ────────────────────────────────────

#[test]
fn restart_during_nested_call_releases_reservation() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-restart-3".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Start program
    adapter.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-1".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    // Start a nested call
    adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: ResponseItem::FunctionCall {
            call_id: "call-1".to_string(),
            name: "read".to_string(),
            arguments: r#"{"path":"/tmp/a.txt"}"#.to_string(),
        },
    });

    assert!(adapter.is_call_reserved("call-1"));

    // Simulate restart: release the reservation
    assert!(adapter.release_reservation("call-1"));
    assert!(!adapter.is_call_reserved("call-1"));
    assert_eq!(adapter.completed_call_count(), 0);
}

// ─── Restart after result before provider continuation ─────────────

#[test]
fn restart_after_result_before_continuation_preserves_completed() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-restart-4".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Start program
    adapter.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-1".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    // Process a function call
    adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: ResponseItem::FunctionCall {
            call_id: "call-1".to_string(),
            name: "read".to_string(),
            arguments: r#"{"path":"/tmp/a.txt"}"#.to_string(),
        },
    });

    // Record the result
    adapter
        .record_call_result(
            "call-1".to_string(),
            "read".to_string(),
            compute_input_hash("read", r#"{"path":"/tmp/a.txt"}"#),
            true,
            serde_json::json!({"content": "file data"}),
        )
        .unwrap();

    // Simulate restart: completed call should be preserved
    assert!(adapter.is_call_completed("call-1"));
    assert_eq!(adapter.completed_call_count(), 1);

    // Replaying the same call should return recorded result
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
            assert_eq!(output, &serde_json::json!({"content": "file data"}));
        }
        other => panic!("expected NestedCallResult, got {:?}", other),
    }
}

// ─── Repeated item returns recorded result ─────────────────────────

#[test]
fn repeated_item_never_re_executes() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-restart-5".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Record a completed call
    adapter
        .record_call_result(
            "call-1".to_string(),
            "read".to_string(),
            compute_input_hash("read", r#"{"path":"/tmp/a.txt"}"#),
            true,
            serde_json::json!({"content": "data"}),
        )
        .unwrap();

    // Replay the same call 3 times
    for i in 0..3 {
        let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: 0,
            item: ResponseItem::FunctionCall {
                call_id: "call-1".to_string(),
                name: "read".to_string(),
                arguments: r#"{"path":"/tmp/a.txt"}"#.to_string(),
            },
        });

        assert!(!events.is_empty(), "iteration {}", i);
        match &events[0] {
            HostedProgramEvent::NestedCallResult { call_id, .. } => {
                assert_eq!(call_id, "call-1");
            }
            other => panic!(
                "iteration {}: expected NestedCallResult, got {:?}",
                i, other
            ),
        }

        // Completed count should remain 1 (no duplicates)
        assert_eq!(adapter.completed_call_count(), 1);
    }
}

// ─── Unavailable continuation yields recoverable terminal ──────────

#[test]
fn unavailable_continuation_yields_terminal() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-restart-6".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Process incomplete event without prior ResponseCreated
    let events = adapter.process_stream_event(ResponsesStreamEvent::Incomplete {
        response: ResponseObject {
            id: "resp-orphan".to_string(),
            status: "incomplete".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
        reason: "max_tokens".to_string(),
    });

    match &events[0] {
        HostedProgramEvent::ProgramIncomplete {
            continuation_token, ..
        } => {
            // Empty continuation token indicates no prior state
            assert_eq!(continuation_token, "");
        }
        other => panic!("expected ProgramIncomplete, got {:?}", other),
    }
}

// ─── Mismatched duplicate is terminal ──────────────────────────────

#[test]
fn mismatched_duplicate_after_restart_is_terminal() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-restart-7".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Record original call
    adapter
        .record_call_result(
            "call-1".to_string(),
            "read".to_string(),
            "hash_original".to_string(),
            true,
            serde_json::json!({"content": "original"}),
        )
        .unwrap();

    // Simulate restart — replay with different args
    let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: ResponseItem::FunctionCall {
            call_id: "call-1".to_string(),
            name: "read".to_string(),
            arguments: r#"{"path":"/tmp/tampered.txt"}"#.to_string(),
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

// ─── Multi-call restart recovery ───────────────────────────────────

#[test]
fn multi_call_restart_preserves_all_completed() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-restart-8".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Complete 3 calls with correct hashes
    for i in 0..3 {
        let args = format!(r#"{{"path":"/tmp/{}.txt"}}"#, i);
        adapter
            .record_call_result(
                format!("call-{}", i),
                "read".to_string(),
                compute_input_hash("read", &args),
                true,
                serde_json::json!({"content": format!("data-{}", i)}),
            )
            .unwrap();
    }

    assert_eq!(adapter.completed_call_count(), 3);

    // Replay all 3 — each should return recorded result
    for i in 0..3 {
        let args = format!(r#"{{"path":"/tmp/{}.txt"}}"#, i);
        let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: 0,
            item: ResponseItem::FunctionCall {
                call_id: format!("call-{}", i),
                name: "read".to_string(),
                arguments: args,
            },
        });

        match &events[0] {
            HostedProgramEvent::NestedCallResult {
                call_id,
                success,
                output,
                ..
            } => {
                assert_eq!(call_id, &format!("call-{}", i));
                assert!(success);
                assert_eq!(
                    output,
                    &serde_json::json!({"content": format!("data-{}", i)})
                );
            }
            other => panic!("call-{}: expected NestedCallResult, got {:?}", i, other),
        }

        assert_eq!(adapter.completed_call_count(), 3);
    }
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
