//! Shared serialized writer for LSP JSON-RPC messages.
//!
//! Wraps an `AsyncWrite` behind an `Arc<Mutex<...>>` so that multiple
//! concurrent callers (client requests, notifications, future server-request
//! responses) can write framed messages without interleaving bytes.

use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::client::JsonRpcId;
use crate::error::LspError;

/// Shared writer that serializes JSON-RPC messages into Content-Length framed
/// output on the LSP server's stdin.
pub struct LspWriter<W = tokio::process::ChildStdin> {
    writer: Arc<Mutex<Option<W>>>,
}

impl LspWriter<tokio::process::ChildStdin> {
    pub fn new(stdin: tokio::process::ChildStdin) -> Self {
        Self {
            writer: Arc::new(Mutex::new(Some(stdin))),
        }
    }
}

impl<W: tokio::io::AsyncWrite + Send + Sync + Unpin> LspWriter<W> {
    /// Construct from an existing shared writer handle.
    pub fn from_inner(writer: Arc<Mutex<Option<W>>>) -> Self {
        Self { writer }
    }

    /// Send a pre-serialized JSON value as a Content-Length framed message.
    pub async fn send_raw_message(&self, value: &serde_json::Value) -> Result<(), LspError> {
        let msg_str = serde_json::to_string(value)?;
        self.send_raw_string(&msg_str).await
    }

    /// Send a raw JSON string as a Content-Length framed message.
    pub async fn send_raw_string(&self, msg: &str) -> Result<(), LspError> {
        let content = format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
        let mut guard = self.writer.lock().await;
        let w = guard
            .as_mut()
            .ok_or_else(|| LspError::RequestFailed("writer is closed".to_string()))?;
        w.write_all(content.as_bytes())
            .await
            .map_err(|e| LspError::RequestFailed(format!("write failed: {}", e)))?;
        w.flush()
            .await
            .map_err(|e| LspError::RequestFailed(format!("flush failed: {}", e)))?;
        Ok(())
    }

    /// Send a JSON-RPC request message (has id + method).
    pub async fn send_request_message(
        &self,
        id: &JsonRpcId,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), LspError> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send_raw_message(&msg).await
    }

    /// Send a JSON-RPC notification message (has method, no id).
    pub async fn send_notification_message(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), LspError> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.send_raw_message(&msg).await
    }

    /// Send a JSON-RPC success response (has id + result).
    pub async fn send_response_result(
        &self,
        id: &JsonRpcId,
        result: serde_json::Value,
    ) -> Result<(), LspError> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        self.send_raw_message(&msg).await
    }

    /// Send a JSON-RPC error response (has id + error).
    pub async fn send_response_error(
        &self,
        id: &JsonRpcId,
        code: i64,
        message: &str,
        data: Option<serde_json::Value>,
    ) -> Result<(), LspError> {
        let mut error = serde_json::json!({
            "code": code,
            "message": message,
        });
        if let Some(d) = data {
            error["data"] = d;
        }
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": error,
        });
        self.send_raw_message(&msg).await
    }

    /// Clone the underlying writer handle (for passing to background tasks).
    pub fn clone_inner(&self) -> Arc<Mutex<Option<W>>> {
        self.writer.clone()
    }

    /// Close the underlying writer by taking ownership of it.
    ///
    /// After this call, any subsequent writes will fail with a broken-pipe
    /// error. This is used during shutdown to signal stdin EOF to the
    /// server process, which many LSP servers require before they exit.
    pub async fn close(&self) {
        let mut guard = self.writer.lock().await;
        // Take the inner writer, dropping it and closing the pipe.
        if let Some(w) = guard.take() {
            drop(w);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock writer that captures written bytes into a channel.
    struct MockWriter {
        tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    }

    unsafe impl Send for MockWriter {}
    unsafe impl Sync for MockWriter {}

    impl tokio::io::AsyncWrite for MockWriter {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            match self.tx.try_send(buf.to_vec()) {
                Ok(_) => std::task::Poll::Ready(Ok(buf.len())),
                Err(_) => std::task::Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "channel closed",
                ))),
            }
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    /// Create a mock writer and its byte receiver.
    fn make_mock() -> (LspWriter<MockWriter>, tokio::sync::mpsc::Receiver<Vec<u8>>) {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let writer = LspWriter {
            writer: Arc::new(Mutex::new(Some(MockWriter { tx }))),
        };
        (writer, rx)
    }

    /// Extract the body bytes (after the header) from a Content-Length framed message.
    fn extract_body(bytes: &[u8]) -> &[u8] {
        let header_end = bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("no header terminator");
        &bytes[header_end + 4..]
    }

    /// Extract the Content-Length value from a framed byte sequence.
    fn extract_content_length(bytes: &[u8]) -> usize {
        let header_end = bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("no header terminator");
        let header = String::from_utf8_lossy(&bytes[..header_end + 4]);
        header
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length: ")
                    .map(|v| v.trim().parse().ok())
            })
            .flatten()
            .expect("missing Content-Length")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn content_length_and_body_are_correct() {
        let (writer, mut rx) = make_mock();
        let msg = serde_json::json!({"hello": "world"});
        writer.send_raw_message(&msg).await.unwrap();

        let raw = rx.recv().await.unwrap();
        let body = extract_body(&raw);
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body_str).unwrap();
        assert_eq!(parsed["hello"], "world");

        let claimed = extract_content_length(&raw);
        assert_eq!(claimed, body.len());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_envelope_json_is_correct() {
        let (writer, mut rx) = make_mock();
        let id = JsonRpcId::Number(42);
        writer
            .send_request_message(&id, "initialize", serde_json::json!({"root": "/tmp"}))
            .await
            .unwrap();

        let raw = rx.recv().await.unwrap();
        let body_str = String::from_utf8(extract_body(&raw).to_vec()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body_str).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 42);
        assert_eq!(parsed["method"], "initialize");
        assert_eq!(parsed["params"]["root"], "/tmp");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn response_result_envelope_is_correct() {
        let (writer, mut rx) = make_mock();
        let id = JsonRpcId::Number(7);
        writer
            .send_response_result(&id, serde_json::json!({"capabilities": {}}))
            .await
            .unwrap();

        let raw = rx.recv().await.unwrap();
        let body_str = String::from_utf8(extract_body(&raw).to_vec()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body_str).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 7);
        assert_eq!(parsed["result"]["capabilities"], serde_json::json!({}));
        assert!(parsed.get("error").is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn response_error_envelope_is_correct() {
        let (writer, mut rx) = make_mock();
        let id = JsonRpcId::Number(99);
        writer
            .send_response_error(&id, -32601, "Method not found", None)
            .await
            .unwrap();

        let raw = rx.recv().await.unwrap();
        let body_str = String::from_utf8(extract_body(&raw).to_vec()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body_str).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 99);
        assert_eq!(parsed["error"]["code"], -32601);
        assert_eq!(parsed["error"]["message"], "Method not found");
        assert!(parsed["error"].get("data").is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn response_error_with_data() {
        let (writer, mut rx) = make_mock();
        let id = JsonRpcId::Number(10);
        let data = serde_json::json!({"details": "extra info"});
        writer
            .send_response_error(&id, -32600, "Invalid Request", Some(data))
            .await
            .unwrap();

        let raw = rx.recv().await.unwrap();
        let body_str = String::from_utf8(extract_body(&raw).to_vec()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body_str).unwrap();
        assert_eq!(parsed["error"]["code"], -32600);
        assert_eq!(parsed["error"]["data"]["details"], "extra info");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn notification_envelope_is_correct() {
        let (writer, mut rx) = make_mock();
        writer
            .send_notification_message("initialized", serde_json::json!({}))
            .await
            .unwrap();

        let raw = rx.recv().await.unwrap();
        let body_str = String::from_utf8(extract_body(&raw).to_vec()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body_str).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "initialized");
        assert!(parsed.get("id").is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unicode_body_length_uses_encoded_bytes() {
        let (writer, mut rx) = make_mock();
        let msg = serde_json::json!({"text": "\u{00e9}\u{00e8}\u{00ea}"});
        writer.send_raw_message(&msg).await.unwrap();

        let raw = rx.recv().await.unwrap();
        let body = extract_body(&raw);
        let claimed = extract_content_length(&raw);
        assert_eq!(claimed, body.len());
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body_str).unwrap();
        assert_eq!(parsed["text"], "\u{00e9}\u{00e8}\u{00ea}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn multiple_sequential_writes_are_frame_isolated() {
        let (writer, mut rx) = make_mock();
        for i in 0..10 {
            let msg = serde_json::json!({"seq": i});
            writer.send_raw_message(&msg).await.unwrap();
        }

        for i in 0..10 {
            let raw = rx.recv().await.unwrap();
            let body_str = String::from_utf8(extract_body(&raw).to_vec()).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&body_str).unwrap();
            assert_eq!(parsed["seq"], i);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn string_id_roundtrip() {
        let (writer, mut rx) = make_mock();
        let id = JsonRpcId::String("abc-123".to_string());
        writer
            .send_request_message(&id, "test/method", serde_json::json!({}))
            .await
            .unwrap();

        let raw = rx.recv().await.unwrap();
        let body_str = String::from_utf8(extract_body(&raw).to_vec()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body_str).unwrap();
        assert_eq!(parsed["id"], "abc-123");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn send_raw_string_wraps_correctly() {
        let (writer, mut rx) = make_mock();
        let json_str = r#"{"method":"test"}"#;
        writer.send_raw_string(json_str).await.unwrap();

        let raw = rx.recv().await.unwrap();
        let body = extract_body(&raw);
        let claimed = extract_content_length(&raw);
        assert_eq!(claimed, body.len());
        assert_eq!(body, json_str.as_bytes());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn content_length_matches_utf8_byte_count() {
        let (writer, mut rx) = make_mock();
        // "café" has 5 bytes in UTF-8 (c=1, a=1, f=1, é=2).
        let msg = serde_json::json!({"text": "café"});
        writer.send_raw_message(&msg).await.unwrap();

        let raw = rx.recv().await.unwrap();
        let body = extract_body(&raw);
        let claimed = extract_content_length(&raw);
        // The body is the JSON string, which encodes "café" as `"caf\u00e9"` (10 bytes).
        // The Content-Length must equal the actual UTF-8 byte count of the body.
        assert_eq!(claimed, body.len());
        // Verify the body is valid JSON and round-trips correctly.
        let parsed: serde_json::Value = serde_json::from_slice(body).unwrap();
        assert_eq!(parsed["text"], "caf\u{00e9}");
    }
}
