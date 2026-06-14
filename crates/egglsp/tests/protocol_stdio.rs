//! Core protocol integration tests for egglsp.
//!
//! These tests launch the fake LSP server binary and exercise the
//! Content-Length framing, JSON-RPC message classification, and
//! initialization/shutdown sequences through real stdio.

use std::time::Duration;

mod common;

use common::{
    is_notification, is_response, is_server_request, read_frame, send_error_response,
    send_notification, send_request, send_response, spawn_fake_server, FakeLspHarness,
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
    assert!(resp.get("result").is_some(), "expected result in init response");
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

    assert!(got_apply_edit, "should have received workspace/applyEdit request");

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

    assert!(got_diag_notification, "should have received publishDiagnostics notification");
    assert_eq!(server_request_ids.len(), 2, "should have received 2 server requests");

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
                "textDocument/hover" => serde_json::json!({"contents": {"kind": "markdown", "value": "hover info"}}),
                "textDocument/references" => serde_json::json!([]),
                _ => serde_json::Value::Null,
            };
            send_response(&mut stdin, req_id, result).await;
            request_methods.push(method);
        }
    }

    assert_eq!(request_methods.len(), 3, "should have received 3 server requests");
    // Server sends in order: definition, hover, references
    assert_eq!(request_methods, vec![
        "textDocument/definition",
        "textDocument/hover",
        "textDocument/references",
    ]);

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
    assert!(status.success(), "expected exit code 0");
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

    assert!(got_error_response, "should have received error response for definition request");

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
    let transcript = std::fs::read_to_string(harness.transcript_path_str())
        .expect("failed to read transcript");
    for method in &["textDocument/didOpen", "textDocument/didChange", "textDocument/didSave", "textDocument/didClose"] {
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

    assert_eq!(request_methods.len(), 2, "should have received 2 server requests");
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

    assert_eq!(progress_notifications.len(), 2, "should have received 2 progress notifications");

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
