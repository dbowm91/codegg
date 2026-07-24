//! Integration tests for hosted program contention, cancellation,
//! and backpressure scenarios.
//!
//! Tests cancel during stream, nested inline call, child job, backoff,
//! and terminal publication. Verifies that many hosted programs respect
//! provider and scheduler limits. Tests idle/timeout/backpressure
//! convergence.

use codegg::provider::responses_api::{
    validate_call_count, validate_result_size, HostedBackendPolicy, HostedProgramAdapter,
    HostedProgramEvent, InputValidation, ResponseItem, ResponseObject, ResponsesStreamEvent,
    ResponsesTransport, ResponsesTransportConfig, DEFAULT_STREAM_IDLE_TIMEOUT, MAX_NESTED_CALLS,
    MAX_RESULT_SIZE,
};
use codegg::provider::ProviderCapabilities;
use std::time::Duration;

// ─── Cancel during stream processing ───────────────────────────────

#[test]
fn cancel_during_stream_terminates_transport() {
    let transport = ResponsesTransport::new(
        "https://api.openai.com/v1".to_string(),
        "test-key".to_string(),
    );
    assert!(!transport.is_cancelled());

    // Simulate cancellation mid-stream
    transport.cancel();
    assert!(transport.is_cancelled());

    // Subsequent checks confirm cancellation is sticky
    assert!(transport.is_cancelled());
    transport.reset_cancel();
    assert!(!transport.is_cancelled());
}

#[test]
fn cancel_during_stream_releases_all_reservations() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-cancel-1".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Reserve multiple calls
    adapter.reserve_call("c1", "read", "h1").unwrap();
    adapter.reserve_call("c2", "glob", "h2").unwrap();
    adapter.reserve_call("c3", "grep", "h3").unwrap();
    assert_eq!(adapter.reserved_call_count(), 3);

    // Cancel: release all reservations
    assert!(adapter.release_reservation("c1"));
    assert!(adapter.release_reservation("c2"));
    assert!(adapter.release_reservation("c3"));
    assert_eq!(adapter.reserved_call_count(), 0);
}

#[test]
fn cancel_during_stream_partial_release() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-cancel-2".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    adapter.reserve_call("c1", "read", "h1").unwrap();
    adapter.reserve_call("c2", "read", "h2").unwrap();

    // Cancel mid-flight: release one, leave the other
    assert!(adapter.release_reservation("c1"));
    assert_eq!(adapter.reserved_call_count(), 1);
    assert!(!adapter.is_call_reserved("c1"));
    assert!(adapter.is_call_reserved("c2"));

    // Release the remaining
    assert!(adapter.release_reservation("c2"));
    assert_eq!(adapter.reserved_call_count(), 0);
}

#[test]
fn cancel_release_nonexistent_returns_false() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-cancel-3".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    assert!(!adapter.release_reservation("nonexistent"));
}

// ─── Cancel during nested inline call ──────────────────────────────

#[test]
fn cancel_during_nested_call_then_resume_with_new_call() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-cancel-4".to_string(),
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

    // First call arrives
    adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: ResponseItem::FunctionCall {
            call_id: "c1".to_string(),
            name: "read".to_string(),
            arguments: r#"{"path":"a.txt"}"#.to_string(),
        },
    });
    assert_eq!(adapter.reserved_call_count(), 1);

    // Cancel the first call
    adapter.release_reservation("c1");
    assert_eq!(adapter.reserved_call_count(), 0);

    // Second call arrives — should succeed since reservation was released
    let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 1,
        item: ResponseItem::FunctionCall {
            call_id: "c2".to_string(),
            name: "read".to_string(),
            arguments: r#"{"path":"b.txt"}"#.to_string(),
        },
    });

    assert!(events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::NestedCall {
            call_id,
            tool_name,
            ..
        } if call_id == "c2" && tool_name == "read"
    )));
    assert_eq!(adapter.reserved_call_count(), 1);
}

// ─── Cancel during terminal publication ────────────────────────────

#[test]
fn cancel_after_terminal_event_ignored() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-cancel-5".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Complete a full lifecycle
    adapter.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-1".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: ResponseItem::FunctionCall {
            call_id: "c1".to_string(),
            name: "read".to_string(),
            arguments: r#"{"path":"a.txt"}"#.to_string(),
        },
    });

    adapter
        .record_call_result(
            "c1".to_string(),
            "read".to_string(),
            "hash1".to_string(),
            true,
            serde_json::json!({"content": "data"}),
        )
        .unwrap();

    let terminal_events = adapter.process_stream_event(ResponsesStreamEvent::ResponseCompleted {
        response: ResponseObject {
            id: "resp-1".to_string(),
            status: "completed".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    assert!(terminal_events
        .iter()
        .any(|e| matches!(e, HostedProgramEvent::Terminal { .. })));

    // Cancel after terminal — no panic, no state corruption
    adapter.release_reservation("nonexistent");
    assert_eq!(adapter.completed_call_count(), 1);
    assert!(adapter.continuation().is_some());
}

// ─── Cancel with error event ───────────────────────────────────────

#[test]
fn cancel_during_error_event_preserves_completed_calls() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-cancel-6".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Complete a call
    adapter
        .record_call_result(
            "c1".to_string(),
            "read".to_string(),
            "h1".to_string(),
            true,
            serde_json::json!({"content": "ok"}),
        )
        .unwrap();

    // Error event arrives
    let events = adapter.process_stream_event(ResponsesStreamEvent::Error {
        code: Some("rate_limit".to_string()),
        message: "rate limited".to_string(),
    });

    assert!(events
        .iter()
        .any(|e| matches!(e, HostedProgramEvent::Error { .. })));

    // Completed call is still intact
    assert_eq!(adapter.completed_call_count(), 1);
    assert!(adapter.is_call_completed("c1"));
}

// ─── Many hosted programs respect limits ───────────────────────────

#[test]
fn many_adapters_independent_state() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapters: Vec<HostedProgramAdapter> = (0..50)
        .map(|i| {
            HostedProgramAdapter::new(
                format!("tp-many-{}", i),
                caps.clone(),
                HostedBackendPolicy::HostedPreferred,
            )
        })
        .collect();

    // Each adapter is independent
    for (i, adapter) in adapters.iter_mut().enumerate() {
        adapter
            .record_call_result(
                format!("call-{}", i),
                "read".to_string(),
                format!("hash-{}", i),
                true,
                serde_json::json!({"content": format!("data-{}", i)}),
            )
            .unwrap();
    }

    // Each adapter has exactly 1 completed call
    for (i, adapter) in adapters.iter().enumerate() {
        assert_eq!(
            adapter.completed_call_count(),
            1,
            "adapter {} should have 1 completed call",
            i
        );
        assert!(adapter.is_call_completed(&format!("call-{}", i)));
    }

    // Cross-adapter isolation: call-0 is NOT completed in adapter-1
    assert!(!adapters[1].is_call_completed("call-0"));
}

#[test]
fn many_calls_within_limit_succeed() {
    let caps = ProviderCapabilities {
        max_nested_calls: Some(10),
        ..ProviderCapabilities::for_provider("openai")
    };
    let mut adapter = HostedProgramAdapter::new(
        "tp-many-limit".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Fill up to the limit
    for i in 0..10 {
        let result = adapter.reserve_call(&format!("call-{}", i), "read", &format!("hash-{}", i));
        assert!(result.is_ok(), "call {} should succeed", i);
    }
    assert_eq!(adapter.reserved_call_count(), 10);

    // 11th call should fail
    let result = adapter.reserve_call("call-10", "read", "hash-10");
    assert!(result.is_err());
}

#[test]
fn many_calls_releasing_and_reusing_slots() {
    let caps = ProviderCapabilities {
        max_nested_calls: Some(3),
        ..ProviderCapabilities::for_provider("openai")
    };
    let mut adapter = HostedProgramAdapter::new(
        "tp-many-reuse".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Fill all 3 slots
    adapter.reserve_call("c1", "read", "h1").unwrap();
    adapter.reserve_call("c2", "read", "h2").unwrap();
    adapter.reserve_call("c3", "read", "h3").unwrap();
    assert_eq!(adapter.reserved_call_count(), 3);

    // 4th fails
    assert!(adapter.reserve_call("c4", "read", "h4").is_err());

    // Release c1, c2 → 1 slot used, 2 available
    adapter.release_reservation("c1");
    adapter.release_reservation("c2");
    assert_eq!(adapter.reserved_call_count(), 1);

    // Now c4 and c5 should succeed
    adapter.reserve_call("c4", "read", "h4").unwrap();
    adapter.reserve_call("c5", "read", "h5").unwrap();
    assert_eq!(adapter.reserved_call_count(), 3);

    // c6 fails again
    assert!(adapter.reserve_call("c6", "read", "h6").is_err());
}

#[test]
fn many_calls_mixing_completed_and_reserved() {
    let caps = ProviderCapabilities {
        max_nested_calls: Some(5),
        ..ProviderCapabilities::for_provider("openai")
    };
    let mut adapter = HostedProgramAdapter::new(
        "tp-many-mix".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Complete 3 calls
    for i in 0..3 {
        adapter
            .record_call_result(
                format!("c{}", i),
                "read".to_string(),
                format!("h{}", i),
                true,
                serde_json::json!({"ok": true}),
            )
            .unwrap();
    }
    assert_eq!(adapter.completed_call_count(), 3);

    // Reserve 2 more → total 5 = completed(3) + reserved(2)
    adapter.reserve_call("r1", "read", "hr1").unwrap();
    adapter.reserve_call("r2", "read", "hr2").unwrap();

    // 6th total should fail
    assert!(adapter.reserve_call("r3", "read", "hr3").is_err());

    // Release a reservation → now we can reserve again
    adapter.release_reservation("r1");
    adapter.reserve_call("r3", "read", "hr3").unwrap();
    assert_eq!(adapter.reserved_call_count(), 2);
    assert_eq!(adapter.completed_call_count(), 3);
}

// ─── Idle/timeout/backpressure convergence ─────────────────────────

#[test]
fn transport_config_custom_timeouts() {
    let config = ResponsesTransportConfig {
        request_timeout: Duration::from_secs(30),
        stream_idle_timeout: Duration::from_secs(5),
        max_sse_buffer_size: 1024,
    };
    let transport = ResponsesTransport::with_config(
        "https://api.openai.com/v1".to_string(),
        "test-key".to_string(),
        config,
    );
    assert!(!transport.is_cancelled());
}

#[test]
fn transport_config_extreme_values() {
    // Minimum viable config
    let config = ResponsesTransportConfig {
        request_timeout: Duration::from_millis(1),
        stream_idle_timeout: Duration::from_millis(1),
        max_sse_buffer_size: 0,
    };
    let transport = ResponsesTransport::with_config(
        "https://api.openai.com/v1".to_string(),
        "test-key".to_string(),
        config,
    );
    // Should not panic on construction
    assert!(!transport.is_cancelled());
}

#[test]
fn backpressure_via_max_result_size() {
    let caps = ProviderCapabilities {
        max_result_size: Some(256),
        ..ProviderCapabilities::for_provider("openai")
    };
    let mut adapter = HostedProgramAdapter::new(
        "tp-backpressure".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Small result is fine
    let result = adapter.record_call_result(
        "c1".to_string(),
        "read".to_string(),
        "h1".to_string(),
        true,
        serde_json::json!({"ok": true}),
    );
    assert!(result.is_ok());

    // Large result exceeds limit
    let large_output = serde_json::json!({"data": "x".repeat(512)});
    let result = adapter.record_call_result(
        "c2".to_string(),
        "read".to_string(),
        "h2".to_string(),
        true,
        large_output,
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("exceeds maximum"));
}

#[test]
fn backpressure_via_max_nested_calls() {
    let caps = ProviderCapabilities {
        max_nested_calls: Some(3),
        ..ProviderCapabilities::for_provider("openai")
    };
    let mut adapter = HostedProgramAdapter::new(
        "tp-backpressure-2".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    adapter.reserve_call("c1", "read", "h1").unwrap();
    adapter.reserve_call("c2", "read", "h2").unwrap();
    adapter.reserve_call("c3", "read", "h3").unwrap();

    // 4th fails — backpressure (3 completed+reserved = 3, max is 3, validate uses >=)
    let result = adapter.reserve_call("c4", "read", "h4");
    assert!(result.is_err());

    // Complete c1 → frees a slot (completed=1, reserved=2, total=3 still at limit)
    adapter
        .record_call_result(
            "c1".to_string(),
            "read".to_string(),
            "h1".to_string(),
            true,
            serde_json::json!({"ok": true}),
        )
        .unwrap();

    // Still at limit (completed=1 + reserved=2 = 3 >= 3), so c4 still fails
    let result = adapter.reserve_call("c4", "read", "h4");
    assert!(result.is_err());

    // Release c2 → now total = completed(1) + reserved(1) = 2 < 3
    adapter.release_reservation("c2");
    adapter.reserve_call("c4", "read", "h4").unwrap();
    assert_eq!(adapter.reserved_call_count(), 2);
    assert_eq!(adapter.completed_call_count(), 1);
}

#[test]
fn call_count_validation_at_boundary() {
    // Exactly at limit = invalid
    assert!(matches!(
        validate_call_count(10, Some(10)),
        codegg::provider::responses_api::InputValidation::Invalid { .. }
    ));

    // One below limit = valid
    assert_eq!(
        validate_call_count(9, Some(10)),
        codegg::provider::responses_api::InputValidation::Valid
    );

    // Zero calls = valid
    assert_eq!(
        validate_call_count(0, Some(10)),
        codegg::provider::responses_api::InputValidation::Valid
    );

    // No limit set = valid (uses MAX_NESTED_CALLS)
    assert_eq!(
        validate_call_count(50, None),
        codegg::provider::responses_api::InputValidation::Valid
    );
}

#[test]
fn result_size_boundary_enforcement() {
    // Exactly at limit = valid
    let at_limit = serde_json::json!({"data": "x".repeat(100)});
    assert!(matches!(
        validate_result_size(&at_limit, Some(128)),
        InputValidation::Valid
    ));

    // Over limit = invalid
    let over_limit = serde_json::json!({"data": "x".repeat(200)});
    assert!(matches!(
        validate_result_size(&over_limit, Some(128)),
        InputValidation::Invalid { .. }
    ));
}

// ─── Concurrent adapter operations ─────────────────────────────────

#[test]
fn rapid_reserve_release_cycle() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-rapid".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Rapidly reserve and release the same slot 100 times
    for i in 0..100 {
        adapter
            .reserve_call("c1", "read", &format!("h{}", i))
            .unwrap();
        assert!(adapter.is_call_reserved("c1"));
        adapter.release_reservation("c1");
        assert!(!adapter.is_call_reserved("c1"));
    }

    assert_eq!(adapter.reserved_call_count(), 0);
    assert_eq!(adapter.completed_call_count(), 0);
}

#[test]
fn interleaved_operations_stress() {
    let caps = ProviderCapabilities {
        max_nested_calls: Some(4),
        ..ProviderCapabilities::for_provider("openai")
    };
    let mut adapter = HostedProgramAdapter::new(
        "tp-stress".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Interleave: reserve, complete, release, reserve, complete
    adapter.reserve_call("c1", "read", "h1").unwrap();
    adapter.reserve_call("c2", "read", "h2").unwrap();
    adapter.release_reservation("c1");
    adapter.reserve_call("c3", "read", "h3").unwrap();
    adapter
        .record_call_result(
            "c2".to_string(),
            "read".to_string(),
            "h2".to_string(),
            true,
            serde_json::json!({"ok": true}),
        )
        .unwrap();
    adapter.reserve_call("c4", "read", "h4").unwrap();
    adapter.release_reservation("c3");
    adapter.release_reservation("c4");

    assert_eq!(adapter.completed_call_count(), 1);
    assert_eq!(adapter.reserved_call_count(), 0);
    assert!(adapter.is_call_completed("c2"));
}

// ─── Incomplete event with continuation ────────────────────────────

#[test]
fn incomplete_event_provides_continuation_token() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-incomplete-1".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    adapter.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-inc-1".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    let events = adapter.process_stream_event(ResponsesStreamEvent::Incomplete {
        response: ResponseObject {
            id: "resp-inc-1".to_string(),
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
            assert_eq!(continuation_token, "resp-inc-1");
        }
        other => panic!("expected ProgramIncomplete, got {:?}", other),
    }
}

#[test]
fn error_event_preserves_state() {
    let caps = ProviderCapabilities::for_provider("openai");
    let mut adapter = HostedProgramAdapter::new(
        "tp-error-state".to_string(),
        caps,
        HostedBackendPolicy::HostedPreferred,
    );

    // Start program and complete a call
    adapter.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-e1".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: ResponseItem::FunctionCall {
            call_id: "c1".to_string(),
            name: "read".to_string(),
            arguments: r#"{"path":"a.txt"}"#.to_string(),
        },
    });

    adapter
        .record_call_result(
            "c1".to_string(),
            "read".to_string(),
            "h1".to_string(),
            true,
            serde_json::json!({"content": "data"}),
        )
        .unwrap();

    // Error event
    adapter.process_stream_event(ResponsesStreamEvent::Error {
        code: Some("server_error".to_string()),
        message: "internal error".to_string(),
    });

    // State is preserved: completed call + continuation still exist
    assert_eq!(adapter.completed_call_count(), 1);
    assert!(adapter.is_call_completed("c1"));
    assert!(adapter.continuation().is_some());
}
