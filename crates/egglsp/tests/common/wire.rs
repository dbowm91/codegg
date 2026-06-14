//! Shared wire-level LSP test helpers.
//!
//! These helpers build, send, and receive Content-Length framed
//! JSON-RPC messages against a spawned fake server child process.
//! They are intentionally low-level: the integration tests use them
//! to drive deterministic scripted scenarios and assert on the
//! exact byte-level protocol behavior.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::common::FakeLspHarness;

/// Spawn the fake LSP server with the given scenario.
pub async fn spawn_fake_server(harness: &FakeLspHarness) -> (Child, BufReader<ChildStdout>) {
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

/// Read a single Content-Length framed JSON-RPC message from stdout.
///
/// Returns `None` on EOF.
pub async fn read_frame(stdout: &mut BufReader<ChildStdout>) -> Option<serde_json::Value> {
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

/// Read a frame with a timeout.
#[allow(dead_code)]
pub async fn read_frame_timeout(
    stdout: &mut BufReader<ChildStdout>,
    timeout: Duration,
) -> Option<serde_json::Value> {
    tokio::time::timeout(timeout, read_frame(stdout))
        .await
        .ok()
        .flatten()
}

/// Write a Content-Length framed message body to stdin.
async fn write_framed(stdin: &mut ChildStdin, body: &str) {
    let content = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    stdin.write_all(content.as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();
}

/// Send a framed JSON-RPC request.
pub async fn send_request(
    stdin: &mut ChildStdin,
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
    write_framed(stdin, &body).await;
}

/// Send a framed JSON-RPC request with a string id.
#[allow(dead_code)]
pub async fn send_request_str(
    stdin: &mut ChildStdin,
    id: &str,
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
    write_framed(stdin, &body).await;
}

/// Send a framed JSON-RPC notification.
pub async fn send_notification(stdin: &mut ChildStdin, method: &str, params: serde_json::Value) {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    let body = serde_json::to_string(&msg).unwrap();
    write_framed(stdin, &body).await;
}

/// Send a framed JSON-RPC response (success).
#[allow(dead_code)]
pub async fn send_response(
    stdin: &mut ChildStdin,
    id: &serde_json::Value,
    result: serde_json::Value,
) {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    let body = serde_json::to_string(&msg).unwrap();
    write_framed(stdin, &body).await;
}

/// Send a framed JSON-RPC error response.
#[allow(dead_code)]
pub async fn send_error_response(
    stdin: &mut ChildStdin,
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
    write_framed(stdin, &body).await;
}

/// Check if a frame is a server request (has both `id` and `method`).
#[allow(dead_code)]
pub fn is_server_request(frame: &serde_json::Value) -> bool {
    frame.get("id").is_some() && frame.get("method").is_some()
}

/// Check if a frame is a response (has `id` but no `method`, and has `result` or `error`).
pub fn is_response(frame: &serde_json::Value) -> bool {
    frame.get("id").is_some()
        && frame.get("method").is_none()
        && (frame.get("result").is_some() || frame.get("error").is_some())
}

/// Check if a frame is a notification (no `id`, has `method`).
pub fn is_notification(frame: &serde_json::Value) -> bool {
    frame.get("id").is_none() && frame.get("method").is_some()
}

/// Send the initialize request and read the response.
///
/// Returns the response frame.
#[allow(dead_code)]
pub async fn send_initialize(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    root_path: &str,
) -> serde_json::Value {
    send_request(
        stdin,
        1,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{root_path}"),
            "capabilities": {}
        }),
    )
    .await;

    let resp = read_frame_timeout(stdout, Duration::from_secs(5))
        .await
        .expect("timeout reading init response");
    assert_eq!(resp["id"], 1, "expected init response with id=1");
    resp
}

/// Shutdown the fake server cleanly.
///
/// Sends shutdown request, awaits response, sends exit, awaits process exit.
#[allow(dead_code)]
pub async fn shutdown(
    child: &mut Child,
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    shutdown_id: i64,
) {
    send_request(stdin, shutdown_id, "shutdown", serde_json::json!(null)).await;
    let resp = read_frame_timeout(stdout, Duration::from_secs(5))
        .await
        .expect("timeout reading shutdown response");
    assert_eq!(resp["id"], shutdown_id);
    assert!(
        resp.get("result").is_some(),
        "expected result in shutdown response"
    );
    send_notification(stdin, "exit", serde_json::json!({})).await;
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout waiting for exit")
        .expect("wait failed");
    assert!(status.success(), "server should exit with code 0");
}
