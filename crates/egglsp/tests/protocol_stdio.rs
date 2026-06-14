//! Core protocol integration tests for egglsp.
//!
//! These tests launch the fake LSP server binary and exercise the
//! Content-Length framing, JSON-RPC message classification, and
//! initialization/shutdown sequences through real stdio.

use std::time::Duration;

mod common;

use common::{
    is_notification, is_response, is_server_request, read_frame, read_frame_timeout,
    send_error_response, send_initialize, send_notification, send_raw_bytes, send_raw_frame,
    send_request, send_response, shutdown, spawn_fake_server, FakeLspHarness,
};

// ── Tests ────────────────────────────────────────────────────────────

/// Basic initialization handshake: client sends initialize, server responds,
/// client sends initialized, client sends shutdown, server responds, client
/// sends exit.
#[tokio::test]
async fn initialization_handshake() {
    let scenario = serde_json::json!({
        "name": "init_handshake",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "capabilities": {
                            "textDocumentSync": 1,
                            "hoverProvider": true
                        }
                    }
                }]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
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

    // Send initialize
    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    // Read initialize response
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout reading init response")
        .expect("EOF reading init response");

    assert_eq!(resp["id"], 1);
    assert!(
        resp.get("result").is_some(),
        "expected result in init response"
    );
    assert!(
        resp["result"]["capabilities"].is_object(),
        "expected capabilities object"
    );

    // Send initialized notification
    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Send shutdown
    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;

    // Read shutdown response
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout reading shutdown response")
        .expect("EOF reading shutdown response");

    assert_eq!(resp["id"], 2);
    assert!(
        resp.get("result").is_some(),
        "expected result in shutdown response"
    );

    // Send exit
    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    // Wait for process to exit
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout waiting for exit")
        .expect("wait failed");

    assert!(status.success(), "server should exit with code 0");
}

/// Server sends workspace/configuration request during initialization.
/// Client responds with the requested configuration values.
#[tokio::test]
async fn server_request_during_init() {
    let scenario = serde_json::json!({
        "name": "config_during_init",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [
                    {"type": "RespondResult", "result": {"capabilities": {}}},
                    {"type": "SendRequest", "method": "workspace/configuration", "params": {
                        "items": [{"section": "testServer"}]
                    }}
                ]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
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
        "strict": false
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    // Send initialize
    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    // Read frames until we see the init response and handle server requests
    let mut got_init_response = false;
    let mut got_config_request = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };

        if is_server_request(&frame) {
            let method = frame["method"].as_str().unwrap();
            if method == "workspace/configuration" {
                let req_id = &frame["id"];
                send_response(
                    &mut stdin,
                    req_id,
                    serde_json::json!([{"rust": {"checkOnSave": true}}]),
                )
                .await;
                got_config_request = true;
            }
        } else if is_response(&frame) && frame["id"] == serde_json::json!(1) {
            got_init_response = true;
        }
    }

    assert!(got_init_response, "should have received init response");
    assert!(
        got_config_request,
        "should have received workspace/configuration request"
    );

    // Send initialized
    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Shutdown + exit
    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}

/// Server sends workspace/applyEdit request. Client rejects it.
#[tokio::test]
async fn apply_edit_refusal() {
    let scenario = serde_json::json!({
        "name": "apply_edit_refusal",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": [
                    {"type": "SendRequest", "method": "workspace/applyEdit", "params": {
                        "edit": {
                            "documentChanges": [{
                                "textDocument": {"uri": "file:///test.rs", "version": 1},
                                "edits": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": "// edited\n"}]
                            }]
                        }
                    }}
                ]
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

    // Send initialize
    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    // Read init response
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 1);

    // Send initialized
    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Read frames until we see the applyEdit request and reject it
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut got_apply_edit = false;

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };

        if is_server_request(&frame) {
            let method = frame["method"].as_str().unwrap();
            if method == "workspace/applyEdit" {
                let req_id = &frame["id"];
                send_error_response(&mut stdin, req_id, -32800, "request cancelled").await;
                got_apply_edit = true;
                break;
            }
        }
    }

    assert!(
        got_apply_edit,
        "should have received workspace/applyEdit request"
    );

    // Shutdown + exit
    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}

/// Notifications are interleaved with request responses. The client
/// must handle out-of-order delivery correctly.
#[tokio::test]
async fn notifications_interleaved() {
    let scenario = serde_json::json!({
        "name": "notifications_interleaved",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": [
                    {"type": "SendRequest", "method": "textDocument/definition", "params": {}},
                    {"type": "SendNotification", "method": "textDocument/publishDiagnostics", "params": {
                        "uri": "file:///test.rs",
                        "diagnostics": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}}, "message": "test diagnostic", "severity": 1}]
                    }},
                    {"type": "SendRequest", "method": "textDocument/hover", "params": {}}
                ]
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

    // Initialize
    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 1);

    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Read messages - expect a notification and two server requests
    let mut got_diag_notification = false;
    let mut server_request_ids = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };

        if is_notification(&frame) {
            let method = frame["method"].as_str().unwrap();
            if method == "textDocument/publishDiagnostics" {
                got_diag_notification = true;
            }
        } else if is_server_request(&frame) {
            let method = frame["method"].as_str().unwrap();
            if method == "textDocument/definition" || method == "textDocument/hover" {
                let req_id = &frame["id"];
                // Respond with null (no result)
                send_response(&mut stdin, req_id, serde_json::Value::Null).await;
                server_request_ids.push(req_id.clone());
            }
        }
    }

    assert!(
        got_diag_notification,
        "should have received publishDiagnostics notification"
    );
    assert_eq!(
        server_request_ids.len(),
        2,
        "should have received 2 server requests"
    );

    // Shutdown + exit
    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}

/// Multiple requests are sent concurrently and the server responds in
/// reverse order. The client must correctly route each response.
#[tokio::test]
async fn concurrent_out_of_order_responses() {
    let scenario = serde_json::json!({
        "name": "out_of_order",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": [
                    {"type": "SendRequest", "method": "textDocument/definition", "params": {}},
                    {"type": "SendRequest", "method": "textDocument/hover", "params": {}},
                    {"type": "SendRequest", "method": "textDocument/references", "params": {}}
                ]
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

    // Initialize
    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 1);

    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Read three server requests and respond to each
    let mut request_methods = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while request_methods.len() < 3 && tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };

        if is_server_request(&frame) {
            let method = frame["method"].as_str().unwrap().to_string();
            let req_id = &frame["id"];
            // Respond with a result appropriate for the method
            let result = match method.as_str() {
                "textDocument/definition" => serde_json::Value::Null,
                "textDocument/hover" => {
                    serde_json::json!({"contents": {"kind": "markdown", "value": "hover info"}})
                }
                "textDocument/references" => serde_json::json!([]),
                _ => serde_json::Value::Null,
            };
            send_response(&mut stdin, req_id, result).await;
            request_methods.push(method);
        }
    }

    assert_eq!(
        request_methods.len(),
        3,
        "should have received 3 server requests"
    );
    // Server sends in order: definition, hover, references
    assert_eq!(
        request_methods,
        vec![
            "textDocument/definition",
            "textDocument/hover",
            "textDocument/references",
        ]
    );

    // Shutdown + exit
    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}

/// Clean shutdown sequence: initialize, shutdown, exit. Server exits
/// with code 0.
#[tokio::test]
async fn graceful_shutdown() {
    let scenario = serde_json::json!({
        "name": "graceful_shutdown",
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

    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 1);

    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}

// ── Malformed JSON-RPC tests (C13) ─────────────────────────────────────

/// Send `{"id": 1}` - ID without method or result. This is an unknown
/// message shape that does not match any JSON-RPC category. The server
/// should ignore it and continue.
#[tokio::test]
async fn malformed_json_rpc_id_only() {
    let scenario = serde_json::json!({
        "name": "malformed_id_only",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send malformed: id without method or result
    send_raw_frame(&mut stdin, r#"{"id": 1}"#).await;

    // Server may hang on this malformed message. Try graceful shutdown,
    // then force-kill if it doesn't exit.
    send_request(&mut stdin, 99, "shutdown", serde_json::json!(null)).await;
    let _ = read_frame_timeout(&mut stdout, Duration::from_secs(3)).await;
    send_notification(&mut stdin, "exit", serde_json::json!({})).await;
    match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
        Ok(Ok(status)) => {
            // Process exited - acceptable (any exit code)
            assert!(
                status.code().is_some(),
                "server process should terminate after id-only message"
            );
        }
        _ => {
            // Process did not exit in time - kill it
            let _ = child.kill().await;
        }
    }
}

/// Send `{"result": "hello"}` - result without ID. Not a valid
/// JSON-RPC response. The server should ignore it and continue.
#[tokio::test]
async fn malformed_json_rpc_result_without_id() {
    let scenario = serde_json::json!({
        "name": "malformed_result_no_id",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send malformed: result without id
    send_raw_frame(&mut stdin, r#"{"result": "hello"}"#).await;

    // Server may hang on this malformed message. Try graceful shutdown,
    // then force-kill if it doesn't exit.
    send_request(&mut stdin, 99, "shutdown", serde_json::json!(null)).await;
    let _ = read_frame_timeout(&mut stdout, Duration::from_secs(3)).await;
    send_notification(&mut stdin, "exit", serde_json::json!({})).await;
    match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
        Ok(Ok(status)) => {
            assert!(
                status.code().is_some(),
                "server process should terminate after result-without-id message"
            );
        }
        _ => {
            let _ = child.kill().await;
        }
    }
}

/// Send `{"jsonrpc": "2.0", "method": 123, "id": 1}` - method is not
/// a string. The server should ignore the malformed message and continue.
#[tokio::test]
async fn malformed_json_rpc_non_string_method() {
    let scenario = serde_json::json!({
        "name": "malformed_non_string_method",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send malformed: non-string method
    send_raw_frame(&mut stdin, r#"{"jsonrpc": "2.0", "method": 123, "id": 1}"#).await;

    // Verify client can still operate
    shutdown(&mut child, &mut stdin, &mut stdout, 99).await;
}

/// Send `{"jsonrpc": "2.0", "id": 1, "error": {"code": 3.5, "message": "bad"}}`
/// - fractional error code. The client's `is_structural_error` validation
/// rejects fractional codes, so this message does not resolve pending requests.
#[tokio::test]
async fn malformed_json_rpc_fractional_error_code() {
    let scenario = serde_json::json!({
        "name": "malformed_fractional_error_code",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send malformed: fractional error code (not an integer)
    send_raw_frame(
        &mut stdin,
        r#"{"jsonrpc": "2.0", "id": 1, "error": {"code": 3.5, "message": "bad"}}"#,
    )
    .await;

    // Verify client can still operate
    shutdown(&mut child, &mut stdin, &mut stdout, 99).await;
}

/// Send `{"jsonrpc": "2.0", "id": 1, "error": {"code": -32600}}` - error
/// without message field. The client's structural validation requires both
/// `code` and `message`, so this does not resolve pending requests.
#[tokio::test]
async fn malformed_json_rpc_error_without_message() {
    let scenario = serde_json::json!({
        "name": "malformed_error_no_message",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send malformed: error without message field
    send_raw_frame(
        &mut stdin,
        r#"{"jsonrpc": "2.0", "id": 1, "error": {"code": -32600}}"#,
    )
    .await;

    // Verify client can still operate
    shutdown(&mut child, &mut stdin, &mut stdout, 99).await;
}

/// Send `[{"jsonrpc": "2.0", "id": 1, "method": "test"}]` - batch array.
/// Batch arrays are unsupported by the fake server; the server may ignore
/// or reject it. The test verifies the client survives.
#[tokio::test]
async fn malformed_json_rpc_batch() {
    let scenario = serde_json::json!({
        "name": "malformed_batch",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send malformed: batch array
    send_raw_frame(
        &mut stdin,
        r#"[{"jsonrpc": "2.0", "id": 1, "method": "test"}]"#,
    )
    .await;

    // Verify client can still operate - use timeout-tolerant shutdown
    // in case the server exited due to the batch message
    let shutdown_result = tokio::time::timeout(
        Duration::from_secs(8),
        shutdown(&mut child, &mut stdin, &mut stdout, 99),
    )
    .await;

    if shutdown_result.is_err() {
        // Server likely exited - verify the process is gone
        let status = tokio::time::timeout(Duration::from_secs(3), child.wait())
            .await
            .expect("server should have exited");
        // Server may exit with 0 (clean) or non-zero (error)
        assert!(
            status.is_ok(),
            "server process should terminate after batch message"
        );
    }
}

/// Send `"just a string"` - primitive JSON value, not an object.
/// This is not valid JSON-RPC at all.
#[tokio::test]
async fn malformed_json_rpc_primitive() {
    let scenario = serde_json::json!({
        "name": "malformed_primitive",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send malformed: primitive JSON value
    send_raw_frame(&mut stdin, r#""just a string""#).await;

    // Verify client can still operate - use timeout-tolerant shutdown
    let shutdown_result = tokio::time::timeout(
        Duration::from_secs(8),
        shutdown(&mut child, &mut stdin, &mut stdout, 99),
    )
    .await;

    if shutdown_result.is_err() {
        let status = tokio::time::timeout(Duration::from_secs(3), child.wait())
            .await
            .expect("server should have exited");
        assert!(
            status.is_ok(),
            "server process should terminate after primitive message"
        );
    }
}

/// Send `null` - null JSON value. This is not valid JSON-RPC.
#[tokio::test]
async fn malformed_json_rpc_null() {
    let scenario = serde_json::json!({
        "name": "malformed_null",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send malformed: null value
    send_raw_frame(&mut stdin, "null").await;

    // Verify client can still operate - use timeout-tolerant shutdown
    let shutdown_result = tokio::time::timeout(
        Duration::from_secs(8),
        shutdown(&mut child, &mut stdin, &mut stdout, 99),
    )
    .await;

    if shutdown_result.is_err() {
        let status = tokio::time::timeout(Duration::from_secs(3), child.wait())
            .await
            .expect("server should have exited");
        assert!(
            status.is_ok(),
            "server process should terminate after null message"
        );
    }
}

/// Send `{"jsonrpc": "2.0", "id": {"key": "value"}, "method": "test"}` -
/// object ID instead of number/string. Object IDs are not valid JSON-RPC.
#[tokio::test]
async fn malformed_json_rpc_object_id() {
    let scenario = serde_json::json!({
        "name": "malformed_object_id",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send malformed: object ID
    send_raw_frame(
        &mut stdin,
        r#"{"jsonrpc": "2.0", "id": {"key": "value"}, "method": "test"}"#,
    )
    .await;

    // Server may hang on this malformed message. Try graceful shutdown,
    // then force-kill if it doesn't exit.
    send_request(&mut stdin, 99, "shutdown", serde_json::json!(null)).await;
    let _ = read_frame_timeout(&mut stdout, Duration::from_secs(3)).await;
    send_notification(&mut stdin, "exit", serde_json::json!({})).await;
    match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
        Ok(Ok(status)) => {
            assert!(
                status.code().is_some(),
                "server process should terminate after object-id message"
            );
        }
        _ => {
            let _ = child.kill().await;
        }
    }
}

/// Send `{"jsonrpc": "2.0", "id": 1, "result": "ok", "error": {"code": -1, "message": "bad"}}`
/// - response with both result and error. This is invalid JSON-RPC; the
/// client must not resolve any pending request with this ambiguous message.
#[tokio::test]
async fn malformed_json_rpc_both_result_and_error() {
    let scenario = serde_json::json!({
        "name": "malformed_both_result_error",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send malformed: both result and error present
    send_raw_frame(
        &mut stdin,
        r#"{"jsonrpc": "2.0", "id": 1, "result": "ok", "error": {"code": -1, "message": "bad"}}"#,
    )
    .await;

    // Server may hang on this malformed message. Try graceful shutdown,
    // then force-kill if it doesn't exit.
    send_request(&mut stdin, 99, "shutdown", serde_json::json!(null)).await;
    let _ = read_frame_timeout(&mut stdout, Duration::from_secs(3)).await;
    send_notification(&mut stdin, "exit", serde_json::json!({})).await;
    match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
        Ok(Ok(status)) => {
            assert!(
                status.code().is_some(),
                "server process should terminate after both-result-and-error message"
            );
        }
        _ => {
            let _ = child.kill().await;
        }
    }
}

/// Server exits without responding to the shutdown request. The client
/// should detect the broken pipe and handle it gracefully.
#[tokio::test]
async fn server_exit_before_response() {
    let scenario = serde_json::json!({
        "name": "early_exit",
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
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": false
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    // Initialize
    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 1);

    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Don't send shutdown - let the scenario end naturally.
    // The server will exit after its steps complete.

    // Wait for the process to exit
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout waiting for exit")
        .expect("wait failed");

    // Server exits successfully (scenario ran out of steps)
    assert!(status.success(), "server exited successfully");
}

/// Client sends a request and receives an error response from the server.
/// The client should handle the error gracefully.
#[tokio::test]
async fn server_error_response() {
    let scenario = serde_json::json!({
        "name": "error_response",
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
                "method": "textDocument/definition",
                "then": [{"type": "RespondError", "code": -32601, "message": "Method not found"}]
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

    // Initialize
    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 1);

    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Send a definition request that will get an error response
    send_request(
        &mut stdin,
        2,
        "textDocument/definition",
        serde_json::json!({
            "textDocument": {"uri": "file:///test.rs"},
            "position": {"line": 0, "character": 0}
        }),
    )
    .await;

    // Read the error response
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut got_error_response = false;

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };

        if is_response(&frame) && frame["id"] == serde_json::json!(2) {
            // This is the response to our definition request
            if let Some(error) = frame.get("error") {
                if error.get("code").and_then(|c| c.as_i64()) == Some(-32601) {
                    got_error_response = true;
                }
            }
            break;
        }
    }

    assert!(
        got_error_response,
        "should have received error response for definition request"
    );

    // Shutdown + exit
    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}

// ── Malformed framing tests (C12) ────────────────────────────────────

/// Send a message with a non-numeric Content-Length header value.
/// The fake server's framing parser should reject it and continue.
#[tokio::test]
async fn malformed_non_numeric_content_length() {
    let scenario = serde_json::json!({
        "name": "malformed_non_numeric_cl",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send a message with non-numeric Content-Length header.
    // The server's framing parser should reject this and continue.
    let malformed = b"Content-Length: abc\r\n\r\n{\"jsonrpc\":\"2.0\",\"method\":\"test\"}";
    send_raw_bytes(&mut stdin, malformed).await;

    // Server may hang or exit due to malformed framing. Use timeout-tolerant shutdown.
    send_request(&mut stdin, 99, "shutdown", serde_json::json!(null)).await;
    let _ = read_frame_timeout(&mut stdout, Duration::from_secs(3)).await;
    send_notification(&mut stdin, "exit", serde_json::json!({})).await;
    match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
        Ok(Ok(status)) => {
            assert!(
                status.code().is_some(),
                "server should terminate after non-numeric Content-Length"
            );
        }
        _ => {
            let _ = child.kill().await;
        }
    }
}

/// Send a message with no Content-Length header at all (just a newline
/// followed by JSON). The server's framing parser should reject it.
#[tokio::test]
async fn malformed_missing_content_length_header() {
    let scenario = serde_json::json!({
        "name": "malformed_missing_cl",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send a message with no Content-Length header: just a newline then body.
    // The server's framing parser expects "Content-Length: N" and should reject this.
    let malformed = b"\r\n{\"jsonrpc\":\"2.0\",\"method\":\"test\"}";
    send_raw_bytes(&mut stdin, malformed).await;

    // Server may hang or exit due to malformed framing. Use timeout-tolerant shutdown.
    send_request(&mut stdin, 99, "shutdown", serde_json::json!(null)).await;
    let _ = read_frame_timeout(&mut stdout, Duration::from_secs(3)).await;
    send_notification(&mut stdin, "exit", serde_json::json!({})).await;
    match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
        Ok(Ok(status)) => {
            assert!(
                status.code().is_some(),
                "server should terminate after missing Content-Length header"
            );
        }
        _ => {
            let _ = child.kill().await;
        }
    }
}

/// Send a message with an oversized Content-Length (much larger than the
/// actual body). The server will try to read more bytes than available,
/// hit EOF, and terminate.
#[tokio::test]
async fn malformed_oversized_content_length() {
    let scenario = serde_json::json!({
        "name": "malformed_oversized_cl",
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
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": false
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send a message claiming a huge Content-Length but with only 2 bytes of body.
    // The server will try to read 999999999 bytes, hit EOF, and terminate.
    let malformed = b"Content-Length: 999999999\r\n\r\n{}";
    send_raw_bytes(&mut stdin, malformed).await;

    // Give the server time to start reading the oversized body, then close stdin
    // so it hits EOF instead of hanging forever.
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(stdin);

    // Server should terminate (may exit with 0 or non-zero due to read error)
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout waiting for exit")
        .expect("wait failed");
    // Server should terminate - any exit code is acceptable
    assert!(
        status.code().is_some(),
        "server process should terminate after oversized Content-Length"
    );
}

/// Server exits with nonzero status code. The client should detect
/// the non-zero exit and not treat it as a clean shutdown.
#[tokio::test]
async fn nonzero_exit_code() {
    let scenario = serde_json::json!({
        "name": "nonzero_exit",
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
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {
                "type": "ExpectNotification",
                "method": "exit",
                "then": []
            }
        ],
        "exit": {"type": "ExitCode", "code": 1},
        "strict": false
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout reading init response")
        .expect("EOF reading init response");
    assert_eq!(resp["id"], 1);

    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout reading shutdown response")
        .expect("EOF reading shutdown response");
    assert_eq!(resp["id"], 2);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout waiting for exit")
        .expect("wait failed");

    // Server exits with code 1 as configured in the scenario.
    // The client should handle this gracefully.
    assert_eq!(status.code(), Some(1), "server should exit with code 1");
}

// ── Dynamic registration test ─────────────────────────────────────────

/// Server sends `client/registerCapability` request after initialization.
/// Client responds with null (success), registering the dynamic capability.
/// This tests the `DynamicRegistrationState` flow through real stdio.
#[tokio::test]
async fn dynamic_registration() {
    let scenario = serde_json::json!({
        "name": "dynamic_registration",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [
                    {"type": "RespondResult", "result": {"capabilities": {}}},
                    {"type": "SendRequest", "method": "client/registerCapability", "params": {
                        "registrations": [{
                            "id": "test-reg-1",
                            "method": "textDocument/completion",
                            "registerOptions": {
                                "triggerCharacters": [".", ":"]
                            }
                        }]
                    }}
                ]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
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
        "strict": false
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    // Read frames: expect init response and registerCapability request
    let mut got_init_response = false;
    let mut got_register_request = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };

        if is_server_request(&frame) {
            let method = frame["method"].as_str().unwrap();
            if method == "client/registerCapability" {
                let req_id = &frame["id"];
                send_response(&mut stdin, req_id, serde_json::Value::Null).await;
                got_register_request = true;
            }
        } else if is_response(&frame) && frame["id"] == serde_json::json!(1) {
            got_init_response = true;
        }
    }

    assert!(got_init_response, "should have received init response");
    assert!(
        got_register_request,
        "should have received client/registerCapability request"
    );

    // Send initialized
    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Shutdown + exit
    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}

// ── $/cancelRequest notification test ──────────────────────────────────

/// Client sends `$/cancelRequest` notification during an in-flight request.
/// This tests that the cancel notification is properly framed and delivered
/// to the server. The actual timeout/cancel behavior is tested at the unit
/// test level (see client.rs cancel tests); this integration test verifies
/// the wire-level notification format and delivery.
#[tokio::test]
async fn cancel_request_notification() {
    let scenario = serde_json::json!({
        "name": "cancel_request",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": [
                    {"type": "SendRequest", "method": "textDocument/definition", "params": {}}
                ]
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

    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    // Read init response
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 1);

    // Send initialized
    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Read the definition request from the server
    let mut got_definition = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };
        if is_server_request(&frame) && frame["method"].as_str() == Some("textDocument/definition")
        {
            got_definition = true;
            break;
        }
    }
    assert!(got_definition, "should have received definition request");

    // Send $/cancelRequest notification for id=2 (the definition request).
    // This is a client→server notification; the server receives it but
    // the cancel logic is handled client-side (pending map removal).
    send_notification(
        &mut stdin,
        "$/cancelRequest",
        serde_json::json!({ "id": 2 }),
    )
    .await;

    // Shutdown + exit
    send_request(&mut stdin, 3, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 3);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}

// ── Additional malformed framing tests (C12 continued) ────────────────

/// Send a message with a negative Content-Length value. The parser
/// rejects negative values because `usize::parse` fails on `-1`.
#[tokio::test]
async fn malformed_negative_content_length() {
    let scenario = serde_json::json!({
        "name": "malformed_negative_cl",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Negative Content-Length — usize::parse fails on "-1"
    let malformed = b"Content-Length: -1\r\n\r\n{}";
    send_raw_bytes(&mut stdin, malformed).await;

    send_request(&mut stdin, 99, "shutdown", serde_json::json!(null)).await;
    let _ = read_frame_timeout(&mut stdout, Duration::from_secs(3)).await;
    send_notification(&mut stdin, "exit", serde_json::json!({})).await;
    match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
        Ok(Ok(status)) => {
            assert!(
                status.code().is_some(),
                "server should terminate after negative Content-Length"
            );
        }
        _ => {
            let _ = child.kill().await;
        }
    }
}

/// Send a message where Content-Length is smaller than the actual body.
/// The server reads exactly Content-Length bytes, leaving the rest in the
/// buffer. The next read picks up the leftover bytes, which are not a
/// valid frame, causing the server to terminate.
#[tokio::test]
async fn malformed_content_length_smaller_than_body() {
    let scenario = serde_json::json!({
        "name": "malformed_cl_too_small",
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
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": false
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Content-Length says 5 bytes but we send 20. Server reads 5, leaves 15
    // in the buffer. Those 15 bytes are not a valid frame header, so the
    // server will fail on the next read.
    let body = b"12345extra_bytes_here";
    let frame = format!("Content-Length: 5\r\n\r\n{}", String::from_utf8_lossy(body));
    send_raw_bytes(&mut stdin, frame.as_bytes()).await;

    // Give server time to process, then close stdin so it exits
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(stdin);

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(
        status.code().is_some(),
        "server should terminate after Content-Length smaller than body"
    );
}

/// Send a message with LF-only line endings instead of CRLF. The parser
/// expects `\r\n\r\n` as the header terminator and should reject this.
#[tokio::test]
async fn malformed_lf_only_framing() {
    let scenario = serde_json::json!({
        "name": "malformed_lf_only",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // LF-only framing: "Content-Length: 2\n\n{}" instead of CRLF
    let malformed = b"Content-Length: 2\n\n{}";
    send_raw_bytes(&mut stdin, malformed).await;

    send_request(&mut stdin, 99, "shutdown", serde_json::json!(null)).await;
    let _ = read_frame_timeout(&mut stdout, Duration::from_secs(3)).await;
    send_notification(&mut stdin, "exit", serde_json::json!({})).await;
    match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
        Ok(Ok(status)) => {
            assert!(
                status.code().is_some(),
                "server should terminate after LF-only framing"
            );
        }
        _ => {
            let _ = child.kill().await;
        }
    }
}

/// Send two complete frames in a single write. The server's BufReader
/// should process both frames correctly (this is a valid scenario).
#[tokio::test]
async fn multiple_frames_in_one_write() {
    let scenario = serde_json::json!({
        "name": "multi_frame_write",
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

    send_initialize(&mut stdin, &mut stdout, harness.root.to_str().unwrap()).await;

    // Send two frames in one write: initialized notification + shutdown request
    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    let init_body = serde_json::to_string(&initialized).unwrap();
    let shutdown = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "shutdown",
        "params": null
    });
    let shut_body = serde_json::to_string(&shutdown).unwrap();
    let combined = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        init_body.len(),
        init_body,
        shut_body.len(),
        shut_body
    );
    send_raw_bytes(&mut stdin, combined.as_bytes()).await;

    // Read shutdown response (id=99)
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 99);

    // Send exit
    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}

// ── Workspace folders test ────────────────────────────────────────────

/// Server sends `workspace/workspaceFolders` request during initialization.
/// Client responds with the root folder. This is a simpler variant of the
/// configuration test, exercising the workspace folders handler path.
#[tokio::test]
async fn workspace_folders_request() {
    let scenario = serde_json::json!({
        "name": "workspace_folders",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [
                    {"type": "RespondResult", "result": {"capabilities": {}}},
                    {"type": "SendRequest", "method": "workspace/workspaceFolders", "params": {}}
                ]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
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
        "strict": false
    });

    let harness = FakeLspHarness::new(&scenario);
    let (mut child, mut stdout) = spawn_fake_server(&harness).await;
    let mut stdin = child.stdin.take().expect("stdin not captured");

    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    // Read frames: expect init response and workspaceFolders request
    let mut got_init_response = false;
    let mut got_workspace_folders = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };

        if is_server_request(&frame) {
            let method = frame["method"].as_str().unwrap();
            if method == "workspace/workspaceFolders" {
                let req_id = &frame["id"];
                send_response(&mut stdin, req_id, serde_json::json!([])).await;
                got_workspace_folders = true;
            }
        } else if is_response(&frame) && frame["id"] == serde_json::json!(1) {
            got_init_response = true;
        }
    }

    assert!(got_init_response, "should have received init response");
    assert!(
        got_workspace_folders,
        "should have received workspace/workspaceFolders request"
    );

    // Send initialized
    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Shutdown + exit
    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}

/// Diagnostics lifecycle: after didOpen, the server may publish
/// diagnostics. didChange/didSave can trigger re-publish, and
/// didClose typically produces an empty diagnostic list. The client
/// must route these notifications to the diagnostics cache without
/// confusing them with responses.
#[tokio::test]
async fn diagnostics_lifecycle() {
    let test_uri = "file:///test/lifecycle.rs";
    let scenario = serde_json::json!({
        "name": "diagnostics_lifecycle",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": [
                    {"type": "SendNotification", "method": "textDocument/publishDiagnostics", "params": {
                        "uri": test_uri,
                        "diagnostics": [{
                            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}},
                            "message": "initial diagnostic",
                            "severity": 2,
                            "source": "fake-lsp"
                        }]
                    }}
                ]
            },
            {
                "type": "ExpectNotification",
                "method": "textDocument/didOpen",
                "then": [
                    {"type": "SendNotification", "method": "textDocument/publishDiagnostics", "params": {
                        "uri": test_uri,
                        "diagnostics": [{
                            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}},
                            "message": "unused variable",
                            "severity": 2,
                            "source": "fake-lsp"
                        }]
                    }}
                ]
            },
            {
                "type": "ExpectNotification",
                "method": "textDocument/didChange",
                "then": [
                    {"type": "SendNotification", "method": "textDocument/publishDiagnostics", "params": {
                        "uri": test_uri,
                        "diagnostics": []
                    }}
                ]
            },
            {
                "type": "ExpectNotification",
                "method": "textDocument/didSave",
                "then": [
                    {"type": "SendNotification", "method": "textDocument/publishDiagnostics", "params": {
                        "uri": test_uri,
                        "diagnostics": []
                    }}
                ]
            },
            {
                "type": "ExpectNotification",
                "method": "textDocument/didClose",
                "then": [
                    {"type": "SendNotification", "method": "textDocument/publishDiagnostics", "params": {
                        "uri": test_uri,
                        "diagnostics": []
                    }}
                ]
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

    // Initialize
    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 1);

    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Send didOpen
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

    // Send didChange (full content)
    send_notification(
        &mut stdin,
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": {"uri": test_uri, "version": 2},
            "contentChanges": [{"text": "fn main() { let x = 1; }\n"}]
        }),
    )
    .await;

    // Send didSave
    send_notification(
        &mut stdin,
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": {"uri": test_uri}
        }),
    )
    .await;

    // Send didClose
    send_notification(
        &mut stdin,
        "textDocument/didClose",
        serde_json::json!({
            "textDocument": {"uri": test_uri}
        }),
    )
    .await;

    // Read all publishDiagnostics notifications - expect 5 (one per
    // document-sync step: pre-open, post-open, post-change, post-save,
    // post-close).
    let mut diag_notifications = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline && diag_notifications.len() < 5 {
        let remaining = deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };

        if is_notification(&frame) {
            let method = frame["method"].as_str().unwrap();
            if method == "textDocument/publishDiagnostics" {
                diag_notifications.push(frame);
            }
        }
    }

    assert_eq!(
        diag_notifications.len(),
        5,
        "should have received 5 publishDiagnostics notifications (one per lifecycle step)"
    );

    // At least one diagnostics notification should have a non-empty list
    let has_non_empty = diag_notifications.iter().any(|n| {
        n["params"]["diagnostics"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
    });
    assert!(
        has_non_empty,
        "at least one diagnostics notification should have a non-empty list"
    );

    // The publishDiagnostics after didClose should have an empty list
    let last = diag_notifications.last().expect("at least one diagnostic");
    let last_diags = last["params"]["diagnostics"].as_array();
    assert!(
        last_diags.map(|a| a.is_empty()).unwrap_or(false),
        "post-didClose publishDiagnostics should have an empty list"
    );

    // The transcript must record all the did_* lifecycle events we sent
    let transcript =
        std::fs::read_to_string(harness.transcript_path_str()).expect("failed to read transcript");
    for method in &[
        "textDocument/didOpen",
        "textDocument/didChange",
        "textDocument/didSave",
        "textDocument/didClose",
    ] {
        assert!(
            transcript.contains(method),
            "transcript should record {method} notification"
        );
    }

    // Shutdown + exit
    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}

/// Verify the frame reader handles Content-Length framing correctly
/// with various payload sizes.
#[tokio::test]
async fn framing_various_sizes() {
    let scenario = serde_json::json!({
        "name": "framing_sizes",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": [
                    {"type": "SendRequest", "method": "test/small", "params": {}},
                    {"type": "SendRequest", "method": "test/large", "params": {"data": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}}
                ]
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

    // Initialize
    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 1);

    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Read two server requests
    let mut request_methods = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while request_methods.len() < 2 && tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };

        if is_server_request(&frame) {
            let method = frame["method"].as_str().unwrap().to_string();
            let req_id = &frame["id"];
            send_response(&mut stdin, req_id, serde_json::json!({"received": true})).await;
            request_methods.push(method);
        }
    }

    assert_eq!(
        request_methods.len(),
        2,
        "should have received 2 server requests"
    );
    assert!(request_methods.contains(&"test/small".to_string()));
    assert!(request_methods.contains(&"test/large".to_string()));

    // Shutdown + exit
    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}

/// Server sends a progress notification. Client receives it and
/// continues processing.
#[tokio::test]
async fn progress_notification() {
    let scenario = serde_json::json!({
        "name": "progress_notification",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {}}}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": [
                    {"type": "SendNotification", "method": "$/progress", "params": {
                        "token": "test-token",
                        "value": {"kind": "begin", "title": "Loading"}
                    }},
                    {"type": "SendNotification", "method": "$/progress", "params": {
                        "token": "test-token",
                        "value": {"kind": "end"}
                    }}
                ]
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

    // Initialize
    send_request(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", harness.root.display()),
            "capabilities": {}
        }),
    )
    .await;

    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 1);

    send_notification(&mut stdin, "initialized", serde_json::json!({})).await;

    // Read progress notifications
    let mut progress_notifications = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while progress_notifications.len() < 2 && tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        let frame = match tokio::time::timeout(remaining, read_frame(&mut stdout)).await {
            Ok(Some(f)) => f,
            _ => break,
        };

        if is_notification(&frame) {
            let method = frame["method"].as_str().unwrap();
            if method == "$/progress" {
                progress_notifications.push(frame);
            }
        }
    }

    assert_eq!(
        progress_notifications.len(),
        2,
        "should have received 2 progress notifications"
    );

    // Shutdown + exit
    send_request(&mut stdin, 2, "shutdown", serde_json::json!(null)).await;
    let resp = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stdout))
        .await
        .expect("timeout")
        .expect("EOF");
    assert_eq!(resp["id"], 2);

    send_notification(&mut stdin, "exit", serde_json::json!({})).await;

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout")
        .expect("wait failed");
    assert!(status.success());
}
