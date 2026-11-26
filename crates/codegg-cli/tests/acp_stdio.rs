use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acp_initialize_and_shutdown_are_protocol_pure() {
    let binary = env!("CARGO_BIN_EXE_codegg");
    let mut child = Command::new(binary)
        .arg("acp")
        .env("RUST_LOG", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn codegg acp");
    let mut input = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut lines = BufReader::new(stdout).lines();

    input
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":1}}\n",
        )
        .await
        .expect("initialize write");
    input
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"shutdown\",\"params\":{}}\n")
        .await
        .expect("shutdown write");
    input.shutdown().await.expect("close stdin");

    let initialize = lines
        .next_line()
        .await
        .expect("read initialize")
        .expect("initialize frame");
    let shutdown = lines
        .next_line()
        .await
        .expect("read shutdown")
        .expect("shutdown frame");
    let initialize: serde_json::Value =
        serde_json::from_str(&initialize).expect("valid initialize JSON");
    let shutdown: serde_json::Value = serde_json::from_str(&shutdown).expect("valid shutdown JSON");
    assert_eq!(initialize["id"], 1);
    assert_eq!(initialize["result"]["protocolVersion"], 1);
    assert_eq!(shutdown["id"], 2);
    assert!(shutdown.get("error").is_none());

    let output = child.wait_with_output().await.expect("wait for ACP");
    assert!(
        output.status.success(),
        "ACP stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
