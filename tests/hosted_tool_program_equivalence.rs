//! Integration tests for native vs hosted program equivalence.
//!
//! Verifies that native and hosted execution paths produce equivalent
//! normalized results through the adapter's event model. Both paths
//! share the same HostedProgramEvent types, deduplication, continuation,
//! and broker integration contracts.

use codegg::provider::responses_api::{
    HostedBackendPolicy, HostedProgramAdapter, HostedProgramEvent, ResponseItem, ResponseObject,
    ResponsesStreamEvent, ResponsesUsage,
};
use codegg::provider::ProviderCapabilities;

// ─── Shared fixture helpers ────────────────────────────────────────

/// A "read file" operation represented as a nested function call.
fn read_file_call(call_id: &str, path: &str) -> ResponseItem {
    ResponseItem::FunctionCall {
        call_id: call_id.to_string(),
        name: "read".to_string(),
        arguments: serde_json::json!({"path": path}).to_string(),
    }
}

/// A "grep" operation represented as a nested function call.
fn grep_call(call_id: &str, pattern: &str, path: &str) -> ResponseItem {
    ResponseItem::FunctionCall {
        call_id: call_id.to_string(),
        name: "grep".to_string(),
        arguments: serde_json::json!({"pattern": pattern, "path": path}).to_string(),
    }
}

/// A successful result for a read_file call.
#[allow(dead_code)]
fn read_file_result(call_id: &str, content: &str) -> ResponsesStreamEvent {
    ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: ResponseItem::FunctionCallOutput {
            call_id: call_id.to_string(),
            output: serde_json::json!({"content": content}),
        },
    }
}

/// A response completed event.
fn response_completed(response_id: &str) -> ResponsesStreamEvent {
    ResponsesStreamEvent::ResponseCompleted {
        response: ResponseObject {
            id: response_id.to_string(),
            status: "completed".to_string(),
            output: vec![],
            usage: Some(ResponsesUsage {
                input_tokens: 100,
                output_tokens: 200,
                total_tokens: 300,
                reasoning_tokens: None,
            }),
            incomplete_details: None,
        },
    }
}

/// Compute the input hash the same way the adapter does.
fn compute_input_hash(tool_name: &str, arguments: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    tool_name.hash(&mut h);
    arguments.hash(&mut h);
    format!("{:016x}", h.finish())
}

// ─── Single read: native vs hosted produce same events ─────────────

#[test]
fn single_read_equivalent_events() {
    let caps = ProviderCapabilities::for_provider("openai");

    // ── Hosted path: adapter processes stream events ──
    let mut hosted = HostedProgramAdapter::new(
        "tp-equiv-1".to_string(),
        caps.clone(),
        HostedBackendPolicy::HostedPreferred,
    );

    let mut hosted_events = Vec::new();
    hosted_events.extend(
        hosted.process_stream_event(ResponsesStreamEvent::ResponseCreated {
            response: ResponseObject {
                id: "resp-1".to_string(),
                status: "in_progress".to_string(),
                output: vec![],
                usage: None,
                incomplete_details: None,
            },
        }),
    );

    hosted_events.extend(
        hosted.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: 0,
            item: read_file_call("call-1", "src/main.rs"),
        }),
    );

    let hash = compute_input_hash("read", r#"{"path":"src/main.rs"}"#);
    hosted
        .record_call_result(
            "call-1".to_string(),
            "read".to_string(),
            hash,
            true,
            serde_json::json!({"content": "fn main() {}"}),
        )
        .unwrap();

    hosted_events.extend(
        hosted.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: 1,
            item: ResponseItem::FunctionCallOutput {
                call_id: "call-1".to_string(),
                output: serde_json::json!({"content": "fn main() {}"}),
            },
        }),
    );

    hosted_events.extend(hosted.process_stream_event(response_completed("resp-1")));

    // ── Native path: adapter constructs same events manually ──
    let mut native = HostedProgramAdapter::new(
        "tp-equiv-1".to_string(),
        caps,
        HostedBackendPolicy::NativeOnly,
    );

    let mut native_events = Vec::new();
    native_events.extend(
        native.process_stream_event(ResponsesStreamEvent::ResponseCreated {
            response: ResponseObject {
                id: "resp-1".to_string(),
                status: "in_progress".to_string(),
                output: vec![],
                usage: None,
                incomplete_details: None,
            },
        }),
    );

    // Native resolves to Native backend, but the adapter events are the same
    native_events.extend(
        native.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: 0,
            item: read_file_call("call-1", "src/main.rs"),
        }),
    );

    let hash = compute_input_hash("read", r#"{"path":"src/main.rs"}"#);
    native
        .record_call_result(
            "call-1".to_string(),
            "read".to_string(),
            hash,
            true,
            serde_json::json!({"content": "fn main() {}"}),
        )
        .unwrap();

    native_events.extend(native.process_stream_event(response_completed("resp-1")));

    // ── Compare: both paths produce same event kinds ──
    // Hosted has ProgramStarted, NestedCall, NestedCallResult (from dedup on FunctionCallOutput),
    // Terminal, Usage.
    // Native has ProgramStarted, NestedCall, Terminal, Usage.
    // The key invariant: both complete the call successfully with the same output.
    assert!(hosted_events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::ProgramStarted {
            response_id,
            ..
        } if response_id == "resp-1"
    )));
    assert!(native_events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::ProgramStarted {
            response_id,
            ..
        } if response_id == "resp-1"
    )));
    assert!(hosted_events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::NestedCall {
            call_id,
            tool_name,
            ..
        } if call_id == "call-1" && tool_name == "read"
    )));
    assert!(native_events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::NestedCall {
            call_id,
            tool_name,
            ..
        } if call_id == "call-1" && tool_name == "read"
    )));
    assert!(hosted_events
        .iter()
        .any(|e| matches!(e, HostedProgramEvent::Terminal { .. })));
    assert!(native_events
        .iter()
        .any(|e| matches!(e, HostedProgramEvent::Terminal { .. })));
}

// ─── Multi-call program: same call counts ──────────────────────────

#[test]
fn multi_call_program_equivalent_call_counts() {
    let caps = ProviderCapabilities::for_provider("openai");

    // ── Hosted path ──
    let mut hosted = HostedProgramAdapter::new(
        "tp-equiv-2".to_string(),
        caps.clone(),
        HostedBackendPolicy::HostedPreferred,
    );

    hosted.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-2".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    // 3 calls
    for i in 0..3 {
        let events = hosted.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: i,
            item: read_file_call(&format!("call-{}", i), &format!("file-{}.txt", i)),
        });
        assert!(events
            .iter()
            .any(|e| matches!(e, HostedProgramEvent::NestedCall { .. })));
    }

    // Record all 3 results
    for i in 0..3 {
        let args = serde_json::json!({"path": format!("file-{}.txt", i)}).to_string();
        let hash = compute_input_hash("read", &args);
        hosted
            .record_call_result(
                format!("call-{}", i),
                "read".to_string(),
                hash,
                true,
                serde_json::json!({"content": format!("data-{}", i)}),
            )
            .unwrap();
    }

    hosted.process_stream_event(response_completed("resp-2"));

    // ── Native path ──
    let mut native = HostedProgramAdapter::new(
        "tp-equiv-2".to_string(),
        caps,
        HostedBackendPolicy::NativeOnly,
    );

    native.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-2".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    for i in 0..3 {
        native.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: i,
            item: read_file_call(&format!("call-{}", i), &format!("file-{}.txt", i)),
        });
    }

    for i in 0..3 {
        let args = serde_json::json!({"path": format!("file-{}.txt", i)}).to_string();
        let hash = compute_input_hash("read", &args);
        native
            .record_call_result(
                format!("call-{}", i),
                "read".to_string(),
                hash,
                true,
                serde_json::json!({"content": format!("data-{}", i)}),
            )
            .unwrap();
    }

    native.process_stream_event(response_completed("resp-2"));

    // ── Compare ──
    assert_eq!(hosted.completed_call_count(), 3);
    assert_eq!(native.completed_call_count(), 3);
    assert_eq!(hosted.total_result_bytes(), native.total_result_bytes());

    // Both have the same event types
    let hosted_nested: Vec<_> = hosted
        .events()
        .iter()
        .filter(|e| matches!(e, HostedProgramEvent::NestedCall { .. }))
        .collect();
    let native_nested: Vec<_> = native
        .events()
        .iter()
        .filter(|e| matches!(e, HostedProgramEvent::NestedCall { .. }))
        .collect();
    assert_eq!(hosted_nested.len(), 3);
    assert_eq!(native_nested.len(), 3);
}

// ─── Deduplication: same for both paths ────────────────────────────

#[test]
fn deduplication_equivalent_for_both_paths() {
    let caps = ProviderCapabilities::for_provider("openai");

    // ── Hosted dedup ──
    let mut hosted = HostedProgramAdapter::new(
        "tp-equiv-dedup".to_string(),
        caps.clone(),
        HostedBackendPolicy::HostedPreferred,
    );

    let hash = compute_input_hash("read", r#"{"path":"a.txt"}"#);
    hosted
        .record_call_result(
            "call-1".to_string(),
            "read".to_string(),
            hash,
            true,
            serde_json::json!({"content": "original"}),
        )
        .unwrap();

    // Duplicate call
    let hosted_events = hosted.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: read_file_call("call-1", "a.txt"),
    });

    assert!(hosted_events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::NestedCallResult {
            success: true,
            output,
            ..
        } if output == &serde_json::json!({"content": "original"})
    )));
    assert_eq!(hosted.completed_call_count(), 1);

    // ── Native dedup ──
    let mut native = HostedProgramAdapter::new(
        "tp-equiv-dedup".to_string(),
        caps,
        HostedBackendPolicy::NativeOnly,
    );

    let hash = compute_input_hash("read", r#"{"path":"a.txt"}"#);
    native
        .record_call_result(
            "call-1".to_string(),
            "read".to_string(),
            hash,
            true,
            serde_json::json!({"content": "original"}),
        )
        .unwrap();

    let native_events = native.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: read_file_call("call-1", "a.txt"),
    });

    assert!(native_events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::NestedCallResult {
            success: true,
            output,
            ..
        } if output == &serde_json::json!({"content": "original"})
    )));
    assert_eq!(native.completed_call_count(), 1);
}

// ─── Continuation state: same for both paths ───────────────────────

#[test]
fn continuation_state_equivalent_for_both_paths() {
    let caps = ProviderCapabilities::for_provider("openai");

    // ── Hosted ──
    let mut hosted = HostedProgramAdapter::new(
        "tp-equiv-cont".to_string(),
        caps.clone(),
        HostedBackendPolicy::HostedPreferred,
    );

    hosted.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-cont-1".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    let hosted_cont = hosted.continuation().unwrap();
    assert_eq!(hosted_cont.response_id, "resp-cont-1");

    // ── Native ──
    let mut native = HostedProgramAdapter::new(
        "tp-equiv-cont".to_string(),
        caps,
        HostedBackendPolicy::NativeOnly,
    );

    native.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-cont-1".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    let native_cont = native.continuation().unwrap();
    assert_eq!(native_cont.response_id, "resp-cont-1");
}

// ─── Error handling: same for both paths ───────────────────────────

#[test]
fn error_handling_equivalent_for_both_paths() {
    let caps = ProviderCapabilities::for_provider("openai");

    // ── Hosted ──
    let mut hosted = HostedProgramAdapter::new(
        "tp-equiv-err".to_string(),
        caps.clone(),
        HostedBackendPolicy::HostedPreferred,
    );

    let hosted_events = hosted.process_stream_event(ResponsesStreamEvent::Error {
        code: Some("rate_limit".to_string()),
        message: "rate limited".to_string(),
    });

    assert!(hosted_events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::Error {
            code: Some(ref c),
            ..
        } if c == "rate_limit"
    )));

    // ── Native ──
    let mut native = HostedProgramAdapter::new(
        "tp-equiv-err".to_string(),
        caps,
        HostedBackendPolicy::NativeOnly,
    );

    let native_events = native.process_stream_event(ResponsesStreamEvent::Error {
        code: Some("rate_limit".to_string()),
        message: "rate limited".to_string(),
    });

    assert!(native_events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::Error {
            code: Some(ref c),
            ..
        } if c == "rate_limit"
    )));
}

// ─── Incomplete/continuation: same for both paths ──────────────────

#[test]
fn incomplete_continuation_equivalent_for_both_paths() {
    let caps = ProviderCapabilities::for_provider("openai");

    // ── Hosted ──
    let mut hosted = HostedProgramAdapter::new(
        "tp-equiv-inc".to_string(),
        caps.clone(),
        HostedBackendPolicy::HostedPreferred,
    );

    hosted.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-inc".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    let hosted_events = hosted.process_stream_event(ResponsesStreamEvent::Incomplete {
        response: ResponseObject {
            id: "resp-inc".to_string(),
            status: "incomplete".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
        reason: "max_tokens".to_string(),
    });

    assert!(hosted_events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::ProgramIncomplete {
            reason,
            continuation_token,
            ..
        } if reason == "max_tokens" && continuation_token == "resp-inc"
    )));

    // ── Native ──
    let mut native = HostedProgramAdapter::new(
        "tp-equiv-inc".to_string(),
        caps,
        HostedBackendPolicy::NativeOnly,
    );

    native.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-inc".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    let native_events = native.process_stream_event(ResponsesStreamEvent::Incomplete {
        response: ResponseObject {
            id: "resp-inc".to_string(),
            status: "incomplete".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
        reason: "max_tokens".to_string(),
    });

    assert!(native_events.iter().any(|e| matches!(
        e,
        HostedProgramEvent::ProgramIncomplete {
            reason,
            continuation_token,
            ..
        } if reason == "max_tokens" && continuation_token == "resp-inc"
    )));
}

// ─── Mixed tool types: same normalization for both paths ───────────

#[test]
fn mixed_tool_types_equivalent_normalization() {
    let caps = ProviderCapabilities::for_provider("openai");

    // ── Hosted ──
    let mut hosted = HostedProgramAdapter::new(
        "tp-equiv-mixed".to_string(),
        caps.clone(),
        HostedBackendPolicy::HostedPreferred,
    );

    hosted.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-mix".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    // read call
    hosted.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: read_file_call("c-read", "src/main.rs"),
    });

    // grep call
    hosted.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 1,
        item: grep_call("c-grep", "fn main", "src/"),
    });

    assert_eq!(hosted.reserved_call_count(), 2);

    // Record results
    let h1 = compute_input_hash("read", r#"{"path":"src/main.rs"}"#);
    hosted
        .record_call_result(
            "c-read".to_string(),
            "read".to_string(),
            h1,
            true,
            serde_json::json!({"content": "fn main() {}"}),
        )
        .unwrap();

    let h2 = compute_input_hash("grep", r#"{"pattern":"fn main","path":"src/"}"#);
    hosted
        .record_call_result(
            "c-grep".to_string(),
            "grep".to_string(),
            h2,
            true,
            serde_json::json!({"matches": ["src/main.rs:1"]}),
        )
        .unwrap();

    hosted.process_stream_event(response_completed("resp-mix"));

    // ── Native ──
    let mut native = HostedProgramAdapter::new(
        "tp-equiv-mixed".to_string(),
        caps,
        HostedBackendPolicy::NativeOnly,
    );

    native.process_stream_event(ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-mix".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    });

    native.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 0,
        item: read_file_call("c-read", "src/main.rs"),
    });

    native.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
        output_index: 1,
        item: grep_call("c-grep", "fn main", "src/"),
    });

    let h1 = compute_input_hash("read", r#"{"path":"src/main.rs"}"#);
    native
        .record_call_result(
            "c-read".to_string(),
            "read".to_string(),
            h1,
            true,
            serde_json::json!({"content": "fn main() {}"}),
        )
        .unwrap();

    let h2 = compute_input_hash("grep", r#"{"pattern":"fn main","path":"src/"}"#);
    native
        .record_call_result(
            "c-grep".to_string(),
            "grep".to_string(),
            h2,
            true,
            serde_json::json!({"matches": ["src/main.rs:1"]}),
        )
        .unwrap();

    native.process_stream_event(response_completed("resp-mix"));

    // ── Compare ──
    assert_eq!(hosted.completed_call_count(), native.completed_call_count());
    assert_eq!(hosted.total_result_bytes(), native.total_result_bytes());

    // Both have same tool names in their completed calls
    let hosted_tools: Vec<_> = hosted
        .events()
        .iter()
        .filter_map(|e| {
            if let HostedProgramEvent::NestedCall { tool_name, .. } = e {
                Some(tool_name.as_str())
            } else {
                None
            }
        })
        .collect();
    let native_tools: Vec<_> = native
        .events()
        .iter()
        .filter_map(|e| {
            if let HostedProgramEvent::NestedCall { tool_name, .. } = e {
                Some(tool_name.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(hosted_tools, native_tools);
}

// ─── Backend resolution: native-only and hosted produce same events ─

#[test]
fn native_only_and_hosted_same_event_types() {
    let caps = ProviderCapabilities::for_provider("openai");

    let mut hosted = HostedProgramAdapter::new(
        "tp-equiv-backend".to_string(),
        caps.clone(),
        HostedBackendPolicy::HostedPreferred,
    );

    let mut native = HostedProgramAdapter::new(
        "tp-equiv-backend".to_string(),
        caps,
        HostedBackendPolicy::NativeOnly,
    );

    // Both start with the same response
    let start_event = ResponsesStreamEvent::ResponseCreated {
        response: ResponseObject {
            id: "resp-be".to_string(),
            status: "in_progress".to_string(),
            output: vec![],
            usage: None,
            incomplete_details: None,
        },
    };

    let hosted_start = hosted.process_stream_event(start_event.clone());
    let native_start = native.process_stream_event(start_event);

    assert_eq!(hosted_start.len(), native_start.len());
    assert!(matches!(
        &hosted_start[0],
        HostedProgramEvent::ProgramStarted { .. }
    ));
    assert!(matches!(
        &native_start[0],
        HostedProgramEvent::ProgramStarted { .. }
    ));

    // Both complete with the same event
    let end_event = ResponsesStreamEvent::ResponseCompleted {
        response: ResponseObject {
            id: "resp-be".to_string(),
            status: "completed".to_string(),
            output: vec![],
            usage: Some(ResponsesUsage {
                input_tokens: 50,
                output_tokens: 100,
                total_tokens: 150,
                reasoning_tokens: None,
            }),
            incomplete_details: None,
        },
    };

    let hosted_end = hosted.process_stream_event(end_event.clone());
    let native_end = native.process_stream_event(end_event);

    assert_eq!(hosted_end.len(), native_end.len());

    // Both have Terminal and Usage events
    assert!(hosted_end
        .iter()
        .any(|e| matches!(e, HostedProgramEvent::Terminal { .. })));
    assert!(native_end
        .iter()
        .any(|e| matches!(e, HostedProgramEvent::Terminal { .. })));
    assert!(hosted_end
        .iter()
        .any(|e| matches!(e, HostedProgramEvent::Usage(_))));
    assert!(native_end
        .iter()
        .any(|e| matches!(e, HostedProgramEvent::Usage(_))));
}
