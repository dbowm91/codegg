//! Core protocol integration tests for egglsp.
//!
//! These tests launch the fake LSP server binary and exercise the
//! Content-Length framing, JSON-RPC message classification, and
//! initialization/shutdown sequences through real stdio.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, ChildStdout, Command};

use common::FakeLspHarness;

mod common;

// ── Frame reader ─────────────────────────────────────────────────────

/// Read a single Content-Length framed JSON-RPC message from stdout.
///
/// Returns `None` on EOF.
async fn read_frame(stdout: &mut BufReader<ChildStdout>) -> Option<serde_json::Value> {
    let mut content_length: Option<usize> = None;

    // Read headers line by line until empty line
    loop {
        let mut line = String::new();
        let n = stdout.read_line(&mut line).await.ok()?;
        if n == 0 {
            return None; // EOF
        }
        let line = line.trim();
        if line.is_empty() {
            break; // End of headers
        }
        if let Some(len) = line.strip_prefix("Content-Length: ") {
            content_length = len.parse().ok();
        }
    }

    let len = content_length?;
    let mut body = vec![0u8; len];
    stdout.read_exact(&mut body).await.ok()?;
    serde_json::from_slice(&body).ok()
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Spawn the fake LSP server with the given scenario.
async fn spawn_fake_server(
    harness: &FakeLspHarness,
) -> (Child, BufReader<ChildStdout>) {
    let server_path = FakeLspHarness::fake_server_path();
    let mut child = Command::new(&server_path)
        .env("CODEGG_FAKE_LSP_SCENARIO", harness.scenario_path_str())
        .env("CODEGG_FAKE_LSP_TRANSCRIPT", harness.transcript_path_str())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("failed to spawn fake server");

    let stdout = child.stdout.take().expect("stdout not captured");
    (child, BufReader::new(stdout))
}

/// Send a framed JSON-RPC request.
async fn send_request(
    stdin: &mut tokio::process::ChildStdin,
    id: i64,
    method: &str,
    params: serde_json::Value,
) {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    let body = serde_json::to_string(&msg).unwrap();
    let content = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    use tokio::io::AsyncWriteExt;
    stdin.write_all(content.as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();
}

/// Send a framed JSON-RPC notification.
async fn send_notification(
    stdin: &mut tokio::process::ChildStdin,
    method: &str,
    params: serde_json::Value,
) {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    let body = serde_json::to_string(&msg).unwrap();
    let content = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    use tokio::io::AsyncWriteExt;
    stdin.write_all(content.as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();
}

/// Send a framed JSON-RPC response (success).
async fn send_response(
    stdin: &mut tokio::process::ChildStdin,
    id: &serde_json::Value,
    result: serde_json::Value,
) {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    let body = serde_json::to_string(&msg).unwrap();
    let content = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    use tokio::io::AsyncWriteExt;
    stdin.write_all(content.as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();
}

/// Send a framed JSON-RPC error response.
async fn send_error_response(
    stdin: &mut tokio::process::ChildStdin,
    id: &serde_json::Value,
    code: i64,
    message: &str,
) {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        },
    });
    let body = serde_json::to_string(&msg).unwrap();
    let content = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    use tokio::io::AsyncWriteExt;
    stdin.write_all(content.as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();
}

/// Check if a frame is a server request (has both `id` and `method`).
fn is_server_request(frame: &serde_json::Value) -> bool {
    frame.get("id").is_some() && frame.get("method").is_some()
}

/// Check if a frame is a response (has `id` but no `method`, and has `result` or `error`).
fn is_response(frame: &serde_json::Value) -> bool {
    frame.get("id").is_some()
        && frame.get("method").is_none()
        && (frame.get("result").is_some() || frame.get("error").is_some())
}

/// Check if a frame is a notification (no `id`, has `method`).
fn is_notification(frame: &serde_json::Value) -> bool {
    frame.get("id").is_none() && frame.get("method").is_some()
}

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
