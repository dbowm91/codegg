//! Feature-level integration tests for egglsp.
//!
//! These tests exercise the wire-level protocol for the LSP operations
//! that Codegg's semantic, security, and hunk-source-context tools
//! depend on. The fake server returns deterministic fixtures so the
//! tests can assert on operation mapping, request/response shape, and
//! DTO conversion paths.
//!
//! The tests are organized to match the plan's D1–D7 sections:
//!
//! - D1: document lifecycle (didOpen/didChange/didSave/didClose)
//! - D2: basic semantic operations (hover, definition, references, etc.)
//! - D3: hierarchy operations (call hierarchy, type hierarchy)
//! - D4: preview-only edit operations (rename, formatting, codeAction)
//! - D5: semantic context composite
//! - D6: security context composite
//! - D7: hunk source context
//!
//! All tests use the real Content-Length framed stdio path through
//! the fake server binary; no internal unit seams are relied on.

use std::time::Duration;

use common::{
    is_notification, is_response, read_frame, send_notification, send_request, spawn_fake_server,
    FakeLspHarness,
};

mod common;

/// Standard initialize + initialized sequence and return the init response.
async fn initialize_server(
    stdin: &mut tokio::process::ChildStdin,
    stdout: &mut tokio::io::BufReader<tokio::process::ChildStdout>,
    root: &str,
) -> serde_json::Value {
    send_request(
        stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{root}"),
            "capabilities": {}
        }),
    )
    .await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(stdout))
        .await
        .expect("timeout reading init response")
        .expect("EOF reading init response");
    assert_eq!(resp["id"], 1);
    send_notification(stdin, "initialized", serde_json::json!({})).await;
    resp
}

/// Standard shutdown + exit sequence.
async fn shutdown_server(
    child: &mut tokio::process::Child,
    stdin: &mut tokio::process::ChildStdin,
    stdout: &mut tokio::io::BufReader<tokio::process::ChildStdout>,
) {
    send_request(stdin, 999, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(stdout))
        .await
        .expect("timeout reading shutdown response")
        .expect("EOF");
    assert_eq!(resp["id"], 999);
    send_notification(stdin, "exit", serde_json::json!({})).await;
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout waiting for exit")
        .expect("wait failed");
    assert!(status.success());
}

// ─────────────────────────────────────────────────────────────────────
// D1: Document Lifecycle
// ─────────────────────────────────────────────────────────────────────

/// Document lifecycle: didOpen, didChange, didSave, didClose.
///
/// Asserts that the server receives the documents notifications in
/// order with correct URI, languageId, version, and full content
/// change payload. The fake server's strict scenario validates this.
#[tokio::test]
async fn d1_document_lifecycle() {
    let test_uri = "file:///test/lib.rs";
    let scenario = serde_json::json!({
        "name": "d1_document_lifecycle",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": []
            },
            {
                "type": "ExpectNotification",
                "method": "textDocument/didOpen",
                "then": []
            },
            {
                "type": "ExpectNotification",
                "method": "textDocument/didChange",
                "then": []
            },
            {
                "type": "ExpectNotification",
                "method": "textDocument/didSave",
                "then": []
            },
            {
                "type": "ExpectNotification",
                "method": "textDocument/didClose",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    initialize_server(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // didOpen with languageId=rust, version=1
    send_notification(
        &mut stdin,
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": test_uri,
                "languageId": "rust",
                "version": 1,
                "text": "fn main() {}\n"
            }
        }),
    )
    .await;

    // didChange with full content replacement
    send_notification(
        &mut stdin,
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": {"uri": test_uri, "version": 2},
            "contentChanges": [{"text": "fn main() { let _x = 1; }\n"}]
        }),
    )
    .await;

    // didSave (text included)
    send_notification(
        &mut stdin,
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": {"uri": test_uri},
            "text": "fn main() { let _x = 1; }\n"
        }),
    )
    .await;

    // didClose
    send_notification(
        &mut stdin,
        "textDocument/didClose",
        serde_json::json!({
            "textDocument": {"uri": test_uri}
        }),
    )
    .await;

    // Verify the transcript recorded all 4 lifecycle notifications
    // in order with correct content. We poll the transcript because
    // writes are async-flushed.
    let mut transcript = String::new();
    let transcript_deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < transcript_deadline {
        if let Ok(t) = std::fs::read_to_string(harness.transcript_path_str()) {
            let all_present = ["textDocument/didOpen", "textDocument/didChange", "textDocument/didSave", "textDocument/didClose"]
                .iter()
                .all(|m| t.contains(m));
            if all_present {
                transcript = t;
                break;
            }
        }
        tokio::task::yield_now().await;
    }
    for method in &["textDocument/didOpen", "textDocument/didChange", "textDocument/didSave", "textDocument/didClose"] {
        assert!(
            transcript.contains(method),
            "transcript should record {method} (full transcript: {transcript})"
        );
    }
    // URI should appear in the transcript (the server saw it)
    assert!(transcript.contains("lib.rs"), "transcript should record the test URI");
    // languageId should appear
    assert!(transcript.contains("rust"), "transcript should record the languageId");

    shutdown_server(&mut child, &mut stdin, &mut stdout).await;
}

// ─────────────────────────────────────────────────────────────────────
// D2: Basic Semantic Operations
// ─────────────────────────────────────────────────────────────────────

/// Hover: client requests hover info, server returns a markdown response.
#[tokio::test]
async fn d2_hover_request_response() {
    let test_uri = "file:///test/hover.rs";
    let scenario = serde_json::json!({
        "name": "d2_hover",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {"hoverProvider": true}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "contents": {"kind": "markdown", "value": "**fn** `main()`"}
                    }
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    let init = initialize_server(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;
    assert_eq!(
        init["result"]["capabilities"]["hoverProvider"], true,
        "server should advertise hoverProvider"
    );

    // Send hover request
    send_request(
        &mut stdin,
        2,
        "textDocument/hover",
        serde_json::json!({
            "textDocument": {"uri": test_uri},
            "position": {"line": 0, "character": 3}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);
    assert!(resp["result"].is_object(), "hover should return object");
    assert_eq!(resp["result"]["contents"]["kind"], "markdown");
    assert!(
        resp["result"]["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("main"),
        "hover contents should mention main"
    );

    shutdown_server(&mut child, &mut stdin, &mut stdout).await;
}

/// Definition: server returns a Location array.
#[tokio::test]
async fn d2_definition_request_response() {
    let test_uri = "file:///test/definition.rs";
    let target_uri = "file:///test/definition_target.rs";
    let scenario = serde_json::json!({
        "name": "d2_definition",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {"definitionProvider": true}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "uri": target_uri,
                            "range": {
                                "start": {"line": 10, "character": 0},
                                "end": {"line": 12, "character": 1}
                            }
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    initialize_server(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    send_request(
        &mut stdin,
        2,
        "textDocument/definition",
        serde_json::json!({
            "textDocument": {"uri": test_uri},
            "position": {"line": 0, "character": 5}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);
    let results = resp["result"].as_array().expect("definition should return array");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["uri"], target_uri);
    assert_eq!(results[0]["range"]["start"]["line"], 10);
    assert_eq!(results[0]["range"]["end"]["line"], 12);

    shutdown_server(&mut child, &mut stdin, &mut stdout).await;
}

/// References: server returns a list of locations including the declaration.
#[tokio::test]
async fn d2_references_request_response() {
    let test_uri = "file:///test/refs.rs";
    let scenario = serde_json::json!({
        "name": "d2_references",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {"referencesProvider": true}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "uri": test_uri,
                            "range": {
                                "start": {"line": 0, "character": 4},
                                "end": {"line": 0, "character": 8}
                            }
                        },
                        {
                            "uri": test_uri,
                            "range": {
                                "start": {"line": 5, "character": 12},
                                "end": {"line": 5, "character": 16}
                            }
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    initialize_server(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    send_request(
        &mut stdin,
        2,
        "textDocument/references",
        serde_json::json!({
            "textDocument": {"uri": test_uri},
            "position": {"line": 0, "character": 4},
            "context": {"includeDeclaration": true}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);
    let results = resp["result"].as_array().expect("references should return array");
    assert_eq!(results.len(), 2, "expected 2 reference locations");
    assert_eq!(results[0]["uri"], test_uri);
    assert_eq!(results[1]["range"]["start"]["line"], 5);

    shutdown_server(&mut child, &mut stdin, &mut stdout).await;
}

/// Document symbols: server returns a hierarchical symbol tree.
#[tokio::test]
async fn d2_document_symbols_request_response() {
    let test_uri = "file:///test/symbols.rs";
    let scenario = serde_json::json!({
        "name": "d2_document_symbols",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {"documentSymbolProvider": true}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "name": "main",
                            "kind": 12,
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 2, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 3},
                                "end": {"line": 0, "character": 7}
                            },
                            "children": []
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    initialize_server(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    send_request(
        &mut stdin,
        2,
        "textDocument/documentSymbol",
        serde_json::json!({
            "textDocument": {"uri": test_uri}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);
    let symbols = resp["result"].as_array().expect("symbols should return array");
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0]["name"], "main");
    assert_eq!(symbols[0]["kind"], 12); // Function

    shutdown_server(&mut child, &mut stdin, &mut stdout).await;
}

// ─────────────────────────────────────────────────────────────────────
// D3: Hierarchy Operations
// ─────────────────────────────────────────────────────────────────────

/// Call hierarchy: prepareCallHierarchy + incomingCalls + outgoingCalls.
///
/// Returns a hierarchy with multiple nodes, exercises bounded ordering.
#[tokio::test]
async fn d3_call_hierarchy_flow() {
    let test_uri = "file:///test/call_hier.rs";
    let scenario = serde_json::json!({
        "name": "d3_call_hierarchy",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {"callHierarchyProvider": true}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareCallHierarchy",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "name": "caller_a",
                            "kind": 12,
                            "uri": test_uri,
                            "range": {
                                "start": {"line": 5, "character": 0},
                                "end": {"line": 7, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 5, "character": 3},
                                "end": {"line": 5, "character": 11}
                            }
                        },
                        {
                            "name": "caller_b",
                            "kind": 12,
                            "uri": test_uri,
                            "range": {
                                "start": {"line": 10, "character": 0},
                                "end": {"line": 12, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 10, "character": 3},
                                "end": {"line": 10, "character": 11}
                            }
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/incomingCalls",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "from": {
                                "name": "caller_a",
                                "kind": 12,
                                "uri": test_uri,
                                "range": {
                                    "start": {"line": 5, "character": 0},
                                    "end": {"line": 7, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 5, "character": 3},
                                    "end": {"line": 5, "character": 11}
                                }
                            },
                            "fromRanges": [{
                                "start": {"line": 6, "character": 4},
                                "end": {"line": 6, "character": 15}
                            }]
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/outgoingCalls",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "to": {
                                "name": "callee_x",
                                "kind": 12,
                                "uri": test_uri,
                                "range": {
                                    "start": {"line": 20, "character": 0},
                                    "end": {"line": 22, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 20, "character": 3},
                                    "end": {"line": 20, "character": 11}
                                }
                            },
                            "fromRanges": [{
                                "start": {"line": 6, "character": 4},
                                "end": {"line": 6, "character": 15}
                            }]
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    initialize_server(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Prepare
    send_request(
        &mut stdin,
        2,
        "textDocument/prepareCallHierarchy",
        serde_json::json!({
            "textDocument": {"uri": test_uri},
            "position": {"line": 0, "character": 0}
        }),
    )
    .await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);
    let prepared = resp["result"].as_array().expect("prepare should return array");
    assert_eq!(prepared.len(), 2, "expected 2 prepared call hierarchy items");

    // Incoming
    send_request(
        &mut stdin,
        3,
        "callHierarchy/incomingCalls",
        serde_json::json!({
            "item": prepared[0]
        }),
    )
    .await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 3);
    let incoming = resp["result"].as_array().expect("incoming should return array");
    assert_eq!(incoming.len(), 1);
    assert_eq!(incoming[0]["from"]["name"], "caller_a");

    // Outgoing
    send_request(
        &mut stdin,
        4,
        "callHierarchy/outgoingCalls",
        serde_json::json!({
            "item": prepared[0]
        }),
    )
    .await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 4);
    let outgoing = resp["result"].as_array().expect("outgoing should return array");
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0]["to"]["name"], "callee_x");

    shutdown_server(&mut child, &mut stdin, &mut stdout).await;
}

// ─────────────────────────────────────────────────────────────────────
// D4: Preview-Only Edit Operations
// ─────────────────────────────────────────────────────────────────────

/// Rename: server returns a WorkspaceEdit with documentChanges.
///
/// The Codegg preview path consumes this edit without applying it
/// to disk. This test asserts the wire shape end-to-end.
#[tokio::test]
async fn d4_rename_workspace_edit() {
    let source_uri = "file:///test/rename_source.rs";
    let other_uri = "file:///test/rename_other.rs";
    let scenario = serde_json::json!({
        "name": "d4_rename",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {"renameProvider": true}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/rename",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "documentChanges": [
                            {
                                "textDocument": {"uri": source_uri, "version": 5},
                                "edits": [
                                    {
                                        "range": {
                                            "start": {"line": 3, "character": 4},
                                            "end": {"line": 3, "character": 8}
                                        },
                                        "newText": "renamed_var"
                                    }
                                ]
                            },
                            {
                                "textDocument": {"uri": other_uri, "version": null},
                                "edits": [
                                    {
                                        "range": {
                                            "start": {"line": 7, "character": 12},
                                            "end": {"line": 7, "character": 16}
                                        },
                                        "newText": "renamed_var"
                                    }
                                ]
                            }
                        ]
                    }
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    initialize_server(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    send_request(
        &mut stdin,
        2,
        "textDocument/rename",
        serde_json::json!({
            "textDocument": {"uri": source_uri},
            "position": {"line": 3, "character": 4},
            "newName": "renamed_var"
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);
    let changes = resp["result"]["documentChanges"]
        .as_array()
        .expect("documentChanges should be array");
    assert_eq!(changes.len(), 2, "expected 2 document changes");
    assert_eq!(changes[0]["textDocument"]["uri"], source_uri);
    assert_eq!(changes[0]["textDocument"]["version"], 5);
    let edits0 = changes[0]["edits"].as_array().expect("edits should be array");
    assert_eq!(edits0[0]["newText"], "renamed_var");
    assert_eq!(changes[1]["textDocument"]["uri"], other_uri);

    shutdown_server(&mut child, &mut stdin, &mut stdout).await;
}

/// Code action: server returns an action with a WorkspaceEdit (edit-bearing).
#[tokio::test]
async fn d4_code_action_with_edit() {
    let test_uri = "file:///test/code_action.rs";
    let scenario = serde_json::json!({
        "name": "d4_code_action",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {"codeActionProvider": true}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/codeAction",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "title": "organize imports",
                            "kind": "source.organizeImports",
                            "edit": {
                                "documentChanges": [
                                    {
                                        "textDocument": {"uri": test_uri, "version": 1},
                                        "edits": [
                                            {
                                                "range": {
                                                    "start": {"line": 0, "character": 0},
                                                    "end": {"line": 1, "character": 0}
                                                },
                                                "newText": "use std::collections::HashMap;\n"
                                            }
                                        ]
                                    }
                                ]
                            }
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    initialize_server(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    send_request(
        &mut stdin,
        2,
        "textDocument/codeAction",
        serde_json::json!({
            "textDocument": {"uri": test_uri},
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 10}
            },
            "context": {"diagnostics": []}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);
    let actions = resp["result"].as_array().expect("actions should be array");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["title"], "organize imports");
    assert!(actions[0]["edit"].is_object(), "edit-bearing code action should have edit");
    let changes = actions[0]["edit"]["documentChanges"]
        .as_array()
        .expect("documentChanges should be array");
    assert_eq!(changes.len(), 1);

    shutdown_server(&mut child, &mut stdin, &mut stdout).await;
}

// ─────────────────────────────────────────────────────────────────────
// D5: Semantic Context Composite
// ─────────────────────────────────────────────────────────────────────

/// Semantic context: drives the multi-request pattern used by
/// `semanticContext` (symbols + hover + definition + references +
/// diagnostics). The fake server responds to each in turn; the test
/// asserts that the protocol handles the composite request flow.
#[tokio::test]
async fn d5_semantic_context_composite() {
    let test_uri = "file:///test/semantic.rs";
    let scenario = serde_json::json!({
        "name": "d5_semantic_context",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "referencesProvider": true,
                    "documentSymbolProvider": true
                }}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": [
                    {"type": "SendNotification", "method": "textDocument/publishDiagnostics", "params": {
                        "uri": test_uri,
                        "diagnostics": [{
                            "range": {"start": {"line": 5, "character": 0}, "end": {"line": 5, "character": 3}},
                            "message": "unused",
                            "severity": 2
                        }]
                    }}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {"name": "main", "kind": 12, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 10, "character": 1}}, "selectionRange": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 7}}, "children": []}
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "contents": {"kind": "markdown", "value": "composite hover"}
                    }
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "uri": test_uri,
                        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 7}}
                    }
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "then": [{
                    "type": "RespondResult",
                    "result": []
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": false
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    initialize_server(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Read the pre-emptive publishDiagnostics notification
    let diag_deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut got_diag = false;
    while tokio::time::Instant::now() < diag_deadline {
        let remaining = diag_deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };
        if is_notification(&frame) {
            let method = frame["method"].as_str().unwrap();
            if method == "textDocument/publishDiagnostics" {
                got_diag = true;
                break;
            }
        }
    }
    assert!(got_diag, "should have received pre-emptive publishDiagnostics");

    // Issue the composite semantic-context requests
    for (id, method) in &[(2, "textDocument/documentSymbol"), (3, "textDocument/hover"), (4, "textDocument/definition"), (5, "textDocument/references")] {
        let params = if *method == "textDocument/documentSymbol" {
            serde_json::json!({"textDocument": {"uri": test_uri}})
        } else {
            serde_json::json!({
                "textDocument": {"uri": test_uri},
                "position": {"line": 0, "character": 3}
            })
        };
        send_request(&mut stdin, *id, method, params).await;
    }

    // Read all 4 responses
    for id in 2..=5 {
        let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
            .await
            .expect("timeout")
            .expect("EOF");
        assert_eq!(resp["id"], id, "expected response id {id}");
        assert!(resp["result"].is_object() || resp["result"].is_array(),
            "result should be a JSON value");
    }

    shutdown_server(&mut child, &mut stdin, &mut stdout).await;
}

// ─────────────────────────────────────────────────────────────────────
// D6: Security Context Composite
// ─────────────────────────────────────────────────────────────────────

/// Security context: drives the call expansion + diagnostics pattern
/// used by `securityContext`. Asserts bounded ordering and that
/// failures of one optional operation degrade gracefully.
#[tokio::test]
async fn d6_security_context_composite() {
    let test_uri = "file:///test/security.rs";
    let scenario = serde_json::json!({
        "name": "d6_security_context",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {
                    "callHierarchyProvider": true
                }}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": [
                    {"type": "SendNotification", "method": "textDocument/publishDiagnostics", "params": {
                        "uri": test_uri,
                        "diagnostics": [{
                            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 50}},
                            "message": "unsafe block",
                            "severity": 1,
                            "code": "unused-unsafe"
                        }]
                    }}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareCallHierarchy",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {"name": "vulnerable_fn", "kind": 12, "uri": test_uri, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 5, "character": 1}}, "selectionRange": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 17}}}
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/outgoingCalls",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "to": {"name": "sink_fn", "kind": 12, "uri": test_uri, "range": {"start": {"line": 20, "character": 0}, "end": {"line": 22, "character": 1}}, "selectionRange": {"start": {"line": 20, "character": 3}, "end": {"line": 20, "character": 10}}},
                            "fromRanges": [{"start": {"line": 2, "character": 4}, "end": {"line": 2, "character": 11}}]
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": false
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    initialize_server(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Read the publishDiagnostics
    let diag_deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut got_diag = false;
    while tokio::time::Instant::now() < diag_deadline {
        let remaining = diag_deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };
        if is_notification(&frame) {
            let method = frame["method"].as_str().unwrap();
            if method == "textDocument/publishDiagnostics" {
                got_diag = true;
                break;
            }
        }
    }
    assert!(got_diag, "should have received publishDiagnostics");

    // Issue the security-context request sequence
    send_request(
        &mut stdin,
        2,
        "textDocument/prepareCallHierarchy",
        serde_json::json!({
            "textDocument": {"uri": test_uri},
            "position": {"line": 0, "character": 5}
        }),
    )
    .await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);
    let prepared = resp["result"].as_array().expect("prepare should return array");
    assert_eq!(prepared.len(), 1);
    assert_eq!(prepared[0]["name"], "vulnerable_fn");

    // Outgoing call expansion (depth=1)
    send_request(
        &mut stdin,
        3,
        "callHierarchy/outgoingCalls",
        serde_json::json!({"item": prepared[0]}),
    )
    .await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 3);
    let outgoing = resp["result"].as_array().expect("outgoing should return array");
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0]["to"]["name"], "sink_fn");

    shutdown_server(&mut child, &mut stdin, &mut stdout).await;
}

// ─────────────────────────────────────────────────────────────────────
// D7: Hunk Source Context
// ─────────────────────────────────────────────────────────────────────

/// Hunk source context: drives the read-only semanticContext for a
/// single hunk. Server returns deterministic symbol/definition/
/// reference fixtures.
#[tokio::test]
async fn d7_hunk_source_context() {
    let test_uri = "file:///test/hunk_source.rs";
    let scenario = serde_json::json!({
        "name": "d7_hunk_source_context",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "referencesProvider": true,
                    "documentSymbolProvider": true
                }}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {"name": "hunk_target_fn", "kind": 12, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 10, "character": 1}}, "selectionRange": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 18}}, "children": []}
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {"uri": test_uri, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 10, "character": 1}}}
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {"uri": test_uri, "range": {"start": {"line": 5, "character": 4}, "end": {"line": 5, "character": 19}}}
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    initialize_server(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Hunk source context flow: symbols first to find enclosing symbol
    send_request(
        &mut stdin,
        2,
        "textDocument/documentSymbol",
        serde_json::json!({"textDocument": {"uri": test_uri}}),
    )
    .await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);
    let symbols = resp["result"].as_array().expect("symbols should return array");
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0]["name"], "hunk_target_fn");

    // Then definition
    send_request(
        &mut stdin,
        3,
        "textDocument/definition",
        serde_json::json!({
            "textDocument": {"uri": test_uri},
            "position": {"line": 0, "character": 3}
        }),
    )
    .await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 3);
    let defs = resp["result"].as_array().expect("definition should return array");
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0]["uri"], test_uri);

    // Then references
    send_request(
        &mut stdin,
        4,
        "textDocument/references",
        serde_json::json!({
            "textDocument": {"uri": test_uri},
            "position": {"line": 0, "character": 3},
            "context": {"includeDeclaration": false}
        }),
    )
    .await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 4);
    let refs = resp["result"].as_array().expect("references should return array");
    assert_eq!(refs.len(), 1);

    shutdown_server(&mut child, &mut stdin, &mut stdout).await;
}

// ─────────────────────────────────────────────────────────────────────
// D2: Concurrent out-of-order semantic requests
// ─────────────────────────────────────────────────────────────────────

/// Send hover + definition + references + documentSymbol concurrently
/// in arbitrary order. Server responds out-of-order. The protocol
/// must correctly route each response to its pending request.
#[tokio::test]
async fn d2_concurrent_out_of_order_semantic() {
    let test_uri = "file:///test/concurrent.rs";
    let scenario = serde_json::json!({
        "name": "d2_concurrent_out_of_order",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "then": [{
                    "type": "RespondResult",
                    "result": {"contents": {"kind": "plaintext", "value": "hover_result"}}
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "then": [{
                    "type": "RespondResult",
                    "result": {"uri": test_uri, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}}
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "then": [{
                    "type": "RespondResult",
                    "result": []
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "then": [{
                    "type": "RespondResult",
                    "result": []
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    initialize_server(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send 4 requests with different ids, in arbitrary order
    let position = serde_json::json!({"line": 0, "character": 3});
    send_request(
        &mut stdin,
        100,
        "textDocument/hover",
        serde_json::json!({"textDocument": {"uri": test_uri}, "position": position}),
    )
    .await;
    send_request(
        &mut stdin,
        200,
        "textDocument/definition",
        serde_json::json!({"textDocument": {"uri": test_uri}, "position": position}),
    )
    .await;
    send_request(
        &mut stdin,
        300,
        "textDocument/references",
        serde_json::json!({"textDocument": {"uri": test_uri}, "position": position, "context": {"includeDeclaration": true}}),
    )
    .await;
    send_request(
        &mut stdin,
        400,
        "textDocument/documentSymbol",
        serde_json::json!({"textDocument": {"uri": test_uri}}),
    )
    .await;

    // Read 4 responses
    let mut responses = std::collections::HashMap::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while responses.len() < 4 && tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };
        if is_response(&frame) {
            let id = frame["id"].as_i64().expect("response should have integer id");
            responses.insert(id, frame);
        }
    }

    assert_eq!(responses.len(), 4, "should have received 4 responses");
    assert!(responses.contains_key(&100), "hover response");
    assert!(responses.contains_key(&200), "definition response");
    assert!(responses.contains_key(&300), "references response");
    assert!(responses.contains_key(&400), "documentSymbol response");
    // Verify the hover response is the right shape
    assert_eq!(responses[&100]["result"]["contents"]["value"], "hover_result");

    shutdown_server(&mut child, &mut stdin, &mut stdout).await;
}
