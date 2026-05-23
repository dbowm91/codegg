# Client Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| mod.rs re-exports `run_attach` as public API | VERIFIED | `src/client/mod.rs:4` - `pub use attach::run_attach;` |
| `run_attach(url: &str, token: Option<&str>) -> Result<(), ClientError>` signature | VERIFIED | `src/client/attach.rs:13` |
| `build_tui_ws_url()` converts base URL to `{base}/tui` WebSocket endpoint | VERIFIED | `src/client/attach.rs:98-109` |
| `build_http_url()` converts WS/WSS to HTTP/HTTPS | VERIFIED | `src/client/attach.rs:111-120` |
| Health Check with 10s timeout | VERIFIED | `src/client/sdk.rs:39` - `.timeout(Duration::from_secs(10))` |
| WebSocket connection with 30s timeout | VERIFIED | `src/client/attach.rs:33` - `timeout(Duration::from_secs(30), connect_async(ws_request))` |
| Channel setup: `event_tx/rx` for server→TUI, `out_tx/rx` for TUI→server | VERIFIED | `src/client/attach.rs:45-46` |
| Two background tasks: `event_task` and `send_task` | VERIFIED | `attach.rs:51-78` (event_task), `attach.rs:80-88` (send_task) |
| TUI initialization via `tui::App::new_remote()` | VERIFIED | `src/client/attach.rs:43` |
| Cleanup: Both tasks aborted when `run_event_loop()` returns | VERIFIED | `src/client/attach.rs:92-93` |
| `RemoteClient` struct with `base_url` and `http` fields | VERIFIED | `src/client/sdk.rs:7-10` |
| `RemoteClient::new(base_url, token) -> Result<Self, ClientError>` | VERIFIED | `src/client/sdk.rs:13` |
| `RemoteClient::health(&self) -> Result<bool, ClientError>` | VERIFIED | `src/client/sdk.rs:34` |
| Health check uses `GET /health` | VERIFIED | `src/client/sdk.rs:35` - `format!("{}/health", self.base_url)` |
| Returns `Err(ClientError::Unreachable)` on non-success | VERIFIED | `src/client/sdk.rs:46-49` |
| TuiMessage uses `#[serde(tag = "type")]` | VERIFIED | `src/protocol/tui.rs:2` |
| `RenderFrame` variant exists but not actively used by server | VERIFIED | `src/protocol/tui.rs:27` - comment in arch doc and RenderFrame in tui.rs |
| ClientError enum with Connection, Unreachable, Rpc, WebSocket, Auth | VERIFIED | `src/error.rs:482-497` |

## Bugs Found

### High

**1. Missing auth callback on WebSocket connection failure**
- **Location**: `src/client/attach.rs:33-37`
- **Issue**: When WebSocket connection fails (timeout, connection refused, etc.), the error message does not include the URL being connected to, making debugging difficult. The `health()` call at line 21 already succeeded, so failure here is specifically WebSocket-level.
- **Example**: `"WebSocket connection timed out"` - no URL context

**2. No retry logic for WebSocket connection**
- **Location**: `src/client/attach.rs:33`
- **Issue**: Single attempt with 30s timeout. Network glitches, temporary server unavailability, or brief connectivity issues cause immediate failure.
- **Impact**: Poor user experience on unreliable networks

**3. `RenderFrame` is only sent by client, never received**
- **Location**: `src/protocol/tui.rs:27-29`
- **Issue**: `RenderFrame { content: String }` variant exists in TuiMessage but is never handled in `handle_remote_event()` (`src/tui/app/mod.rs:686-756`). Server sends it but client ignores it.
- **Impact**: The server can send render frames to the client, but the client discards them silently.

**4. `build_http_url()` doesn't normalize URLs with schemes other than WS/WS**
- **Location**: `src/client/attach.rs:111-120`
- **Issue**: If URL is already `https://` or `http://` (not `wss://` or `ws://`), the function returns it unchanged. Combined with `build_tui_ws_url()` which would also return unchanged for HTTPS (line 102-103), the HTTP URL would be passed as-is to `RemoteClient::new()`.
- **Impact**: An `https://example.com` URL would pass through unchanged, but if someone passed `wss://example.com` expecting to connect to HTTP health endpoint, it would correctly convert. The asymmetry is confusing but not a functional bug per se.

### Medium

**5. No connection timeout on health check beyond the 10s read timeout**
- **Location**: `src/client/sdk.rs:34-50`
- **Issue**: The health check has a read timeout but no connect timeout. On macOS/Darwin, TCP connection establishment has its own timeout (~75s) that is independent of the request timeout.
- **Impact**: A firewall that blocks connections silently causes the health check to hang for up to 75s before failing.

**6. Silent message drop in `send_task`**
- **Location**: `src/client/attach.rs:80-88`
- **Issue**: If `serde_json::to_string()` fails (should rarely happen), the message is silently dropped with no error logging or propagation.
- **Impact**: Data loss on serialization errors, hard to debug.

**7. `event_task` doesn't propagate panics from spawned task**
- **Location**: `src/client/attach.rs:51-78`
- **Issue**: The `event_task` is spawned but its abort is called at line 92. If the task panics, the panic is not caught or logged before abort. The task itself does proper error handling for WebSocket messages, but a panic in `ws_rx.next()` or the parsing code would propagate.
- **Impact**: Unhandled panic could crash the application.

### Low

**8. No user-agent or client identification in HTTP requests**
- **Location**: `src/client/sdk.rs:14-27`
- **Issue**: No `User-Agent` header is set on the HTTP client. Server cannot distinguish client requests from other HTTP traffic.
- **Impact**: Makes server-side analytics/debugging harder.

**9. Redundant string conversion in `RemoteClient::new`**
- **Location**: `src/client/sdk.rs:29`
- **Issue**: `base_url.trim_end_matches('/').to_string()` creates a temporary String. Could use `base_url.trim_end_matches('/').to_owned()` for clarity.
- **Impact**: Minor - single allocation difference.

## Improvement Suggestions

### Performance

1. **Keepalive connections**: The HTTP client in `sdk.rs` doesn't configure keepalive. Adding keepalive would reduce latency for health checks if clients reconnect frequently.

2. **WebSocket message batching**: The `send_task` processes messages one at a time. For high-throughput scenarios, batching could reduce the number of WebSocket frames.

### Correctness

1. **Handle `Message::Binary` in WebSocket**: The `event_task` only handles `Message::Text` (line 54). `Message::Binary` is silently ignored (falls through to `_ => {}` at line 75). If server ever sends binary, it would be lost.

2. **Validate incoming JSON before sending to TUI**: Currently raw JSON from server is parsed as `serde_json::Value` and forwarded. A malformed message causes a `warn!` log but doesn't affect the connection. Consider validating message shape.

3. **Connection closure detection**: When server closes the WebSocket, `event_task` logs "Server closed connection" (line 68) and breaks. But `run_event_loop()` continues until user exits. The client remains in a disconnected state but doesn't inform the user or attempt reconnection.

### Maintainability

1. **Extract WebSocket URL building into separate function**: The URL building logic in `build_tui_ws_url` and `build_http_url` could benefit from a shared URL parsing utility that handles all cases (HTTP, HTTPS, WS, WSS) consistently.

2. **Add integration tests**: The client module has no integration tests. Testing the full connection flow with a mock server would catch issues like silent message drops.

3. **Document the two-task architecture**: The relationship between `event_task`, `send_task`, and the TUI's `run_event_loop()` is not documented. Adding comments would help future maintainers understand the channel ownership and shutdown sequence.

## Priority Actions (top 5 items to fix)

1. **[High] Handle `RenderFrame` in `handle_remote_event()`** - The server can send `RenderFrame` messages but the client silently discards them. Either implement rendering support or log a warning.

2. **[High] Add retry logic for WebSocket connection** - Implement exponential backoff with max retries for transient connection failures.

3. **[Medium] Add connect timeout to health check** - Configure the HTTP client with a connect timeout to avoid 75s hangs on blocked connections.

4. **[Medium] Improve error messages with URL context** - Include the target URL in connection failure errors for easier debugging.

5. **[Medium] Handle WebSocket connection closure gracefully** - When server closes the WebSocket, inform the user instead of silently continuing in disconnected state.

---

*Review completed: 2026-05-23*