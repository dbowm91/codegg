use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

fn server_bin() -> &'static str {
    env!("CARGO_BIN_EXE_egglsp-test-server")
}

fn write_scenario_file(scenario: &serde_json::Value) -> PathBuf {
    let dir = tempdir().expect("failed to create tempdir");
    let path = dir.path().join("scenario.json");
    std::fs::write(&path, serde_json::to_string_pretty(scenario).unwrap())
        .expect("failed to write scenario");
    std::mem::forget(dir);
    path
}

async fn spawn_server(
    scenario: &serde_json::Value,
) -> (Child, ChildStdin, BufReader<ChildStdout>, PathBuf) {
    let scenario_path = write_scenario_file(scenario);
    let transcript_path = scenario_path.parent().unwrap().join("transcript.jsonl");

    let mut child = Command::new(server_bin())
        .env("CODEGG_FAKE_LSP_SCENARIO", &scenario_path)
        .env("CODEGG_FAKE_LSP_TRANSCRIPT", &transcript_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn fake server");

    let stdin = child.stdin.take().expect("stdin not captured");
    let stdout = child.stdout.take().expect("stdout not captured");
    (child, stdin, BufReader::new(stdout), transcript_path)
}

async fn write_frame(stdin: &mut ChildStdin, value: serde_json::Value) {
    let body = serde_json::to_string(&value).unwrap();
    let frame = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    stdin.write_all(frame.as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();
}

async fn read_frame(stdout: &mut BufReader<ChildStdout>) -> Option<serde_json::Value> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = stdout.read_line(&mut line).await.ok()?;
        if n == 0 {
            return None;
        }
        let line = line.trim();
        if line.is_empty() {
            break;
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

async fn wait_for_exit(child: &mut Child) -> std::process::ExitStatus {
    tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("timeout waiting for server exit")
        .expect("wait failed")
}

#[tokio::test]
async fn strict_request_param_mismatch_records_diagnostic_and_exits() {
    let scenario = serde_json::json!({
        "name": "strict-param-mismatch",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "processId": {"type": "Number"},
                        "rootUri": {"type": "String"}
                    }
                },
                "then": [{"type": "RespondResult", "result": {"capabilities": {}}}]
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let (mut child, mut stdin, mut stdout, transcript_path) = spawn_server(&scenario).await;

    write_frame(
        &mut stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": std::process::id(),
                "rootUri": 123
            }
        }),
    )
    .await;

    let status = wait_for_exit(&mut child).await;
    assert!(!status.success(), "strict mismatch should exit nonzero");

    let response = tokio::time::timeout(Duration::from_millis(250), read_frame(&mut stdout))
        .await
        .ok()
        .flatten();
    assert!(response.is_none(), "mismatch should not produce a response");

    let transcript = std::fs::read_to_string(&transcript_path).expect("failed to read transcript");
    assert!(
        transcript.contains("StepMismatch"),
        "transcript should record the mismatch"
    );
    assert!(
        transcript.contains("rootUri"),
        "transcript should include the mismatched field"
    );
    assert!(
        transcript.contains("request params did not match"),
        "transcript should include the mismatch reason"
    );
}

#[tokio::test]
async fn raw_bytes_action_emits_unframed_stdout() {
    let raw_tail = b"\x01RAW-TAIL\x7f";
    let scenario = serde_json::json!({
        "name": "raw-output",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [
                    {"type": "RespondResult", "result": {"capabilities": {}}},
                    {"type": "SendRawBytes", "bytes_base64": base64::engine::general_purpose::STANDARD.encode(raw_tail)},
                    {"type": "Exit", "code": 0}
                ]
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let (mut child, mut stdin, mut stdout, transcript_path) = spawn_server(&scenario).await;

    write_frame(
        &mut stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    )
    .await;

    let init = read_frame(&mut stdout)
        .await
        .expect("missing initialize response");
    assert_eq!(init["id"], 1);
    assert!(init.get("result").is_some(), "expected initialize result");

    let mut tail = Vec::new();
    stdout
        .read_to_end(&mut tail)
        .await
        .expect("failed to read raw tail");

    assert!(
        tail.windows(raw_tail.len())
            .any(|window| window == raw_tail),
        "raw bytes should be present in the stdout tail"
    );

    let status = wait_for_exit(&mut child).await;
    assert!(status.success(), "raw-output scenario should exit cleanly");

    let transcript = std::fs::read_to_string(&transcript_path).expect("failed to read transcript");
    assert!(
        transcript.contains("SendRawBytes"),
        "transcript should record the raw bytes action"
    );
}

#[tokio::test]
async fn grouped_frames_action_writes_multiple_frames() {
    let scenario = serde_json::json!({
        "name": "grouped-frames",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [
                    {"type": "RespondResult", "result": {"capabilities": {}}},
                    {"type": "SendFramesTogether", "messages": [
                        {"jsonrpc": "2.0", "method": "$/progress", "params": {"token": 1}},
                        {"jsonrpc": "2.0", "method": "$/progress", "params": {"token": 2}}
                    ]},
                    {"type": "Exit", "code": 0}
                ]
            }
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let (mut child, mut stdin, mut stdout, transcript_path) = spawn_server(&scenario).await;

    write_frame(
        &mut stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    )
    .await;

    let init = read_frame(&mut stdout)
        .await
        .expect("missing initialize response");
    assert_eq!(init["id"], 1);

    let first = read_frame(&mut stdout)
        .await
        .expect("missing first grouped frame");
    let second = read_frame(&mut stdout)
        .await
        .expect("missing second grouped frame");
    assert_eq!(first["method"], "$/progress");
    assert_eq!(first["params"]["token"], 1);
    assert_eq!(second["method"], "$/progress");
    assert_eq!(second["params"]["token"], 2);

    let status = wait_for_exit(&mut child).await;
    assert!(
        status.success(),
        "grouped-frame scenario should exit cleanly"
    );

    let transcript = std::fs::read_to_string(&transcript_path).expect("failed to read transcript");
    assert!(
        transcript.contains("SendFramesTogether"),
        "transcript should record the grouped frame action"
    );
}
