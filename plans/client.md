# Client Architecture Review

## Architecture Document
- Path: architecture/client.md

## Source Code Location
- src/client/

## Verification Summary
Partial

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| mod.rs exports `run_attach` as public API | Pass | Line 4: `pub use attach::run_attach;` |
| `run_attach(url: &str, token: Option<&str>) -> Result<(), ClientError>` | Pass | attach.rs line 14 matches exactly |
| `build_tui_ws_url()` converts URLs to `/tui` WebSocket endpoint | Pass | attach.rs lines 132-143 |
| `build_http_url()` converts WS/WSS to HTTP/HTTPS | Pass | attach.rs lines 145-154 |
| Health check creates `RemoteClient` and calls `health()` | Pass | attach.rs lines 18, 22 |
| WebSocket connection uses 30-second timeout | Pass | attach.rs line 43: `timeout(Duration::from_secs(30), connect_async(...))` |
| `event_tx/rx` receives JSON events from server | Pass | attach.rs line 74 |
| `out_tx/rx` sends TuiMessage to server | Pass | attach.rs line 75 |
| Two background tasks: `event_task` and `send_task` | Pass | attach.rs lines 80-122 |
| `event_task` receives WebSocket, parses JSON, forwards to TUI | Pass | attach.rs lines 80-112 |
| `send_task` serializes TuiMessage to JSON, sends over WebSocket | Pass | attach.rs lines 114-122 |
| TUI uses `tui::App::new_remote()` with event channels | Pass | attach.rs lines 72-78 |
| Cleanup: Both tasks aborted when `run_event_loop()` returns | Pass | attach.rs lines 126-127 |
| `RemoteClient` struct with `base_url` and `http` fields | Pass | sdk.rs lines 7-10 |
| `new(base_url: &str, token: Option<&str>) -> Result<Self, ClientError>` | Pass | sdk.rs line 13 |
| `health(&self) -> Result<bool, ClientError>` | Pass | sdk.rs line 35 |
| Health check uses `GET /health` with 10-second timeout | Pass | sdk.rs lines 36, 40 |
| Returns `Err(ClientError::Unreachable)` on non-success/failure | Pass | sdk.rs lines 43, 47-50 |
| `TuiMessage` uses `#[serde(tag = "type")]` | Pass | protocol/tui.rs line 2 |
| Client → Server: Input, KeyDown, MouseClick, Resize | Pass | protocol/tui.rs lines 4-18 |
| Client → Server: PermissionResponse, QuestionResponse | Pass | protocol/tui.rs lines 19-26 |
| Server → Client: TextDelta, PermissionPending, QuestionPending | Pass | protocol/tui.rs lines 30-40 |
| Server → Client: SessionInfo, SessionEnded | Pass | protocol/tui.rs lines 42-48 |
| Server → Client: ToolCallStarted, ToolResult, Error | Pass | protocol/tui.rs lines 49-61 |
| Server → Client: ResyncRequired | Pass | protocol/tui.rs lines 62-67 |
| `RenderFrame` variant exists in protocol | Pass | protocol/tui.rs lines 27-29 |
| `RenderFrame` not actively used by server | Pass | Architecture doc accurate - RenderFrame is client-side only |
| ClientError variants: Connection, Unreachable, Rpc, WebSocket, Auth | Pass | error.rs lines 504-519 match |

## Issues Found

### Bugs
None identified - the implementation matches the architecture document.

### Inconsistencies

1. **`RenderFrame` placement in tables**: The architecture doc places `RenderFrame` in the "Server → Client Messages" table (line 98), but `RenderFrame` is actually a **client → server** message (sent from TUI to server). It should be in the Client → Server table instead.

2. **Missing `RenderFrame` in Client → Server table**: The Client → Server table lists only 6 variants but should list 7. The missing variant is `RenderFrame`.

3. **Server → Client message count**: The architecture doc shows 10 server → client variants, but the actual TuiMessage enum has 9 server → client variants (TextDelta, PermissionPending, QuestionPending, SessionInfo, SessionEnded, ToolCallStarted, ToolResult, Error, ResyncRequired). `RenderFrame` should not be in this table.

4. **`Rpc` variant unused**: `ClientError::Rpc` is defined in error.rs but never constructed in src/client/. It may be intended for future use or server-side error propagation but is currently dead code.

5. **Stale line number references**: The "Client skill line numbers updated" note mentions `new_remote()` at line 492 and `handle_remote_event()` at line 686, but these are in tui/app/mod.rs, not client. The architecture doc at architecture/client.md does not contain these line numbers, suggesting either a cross-reference error or outdated skill documentation.

### Missing Documentation

1. **WebSocket reconnection logic**: The architecture doc describes "30-second timeout using `tokio_tungstenite::connect_async()`" but does not mention the retry logic (3 attempts with exponential backoff: 1s, 2s delays on lines 35-66 of attach.rs).

2. **Panic handling in event_task**: The `event_task` uses `std::panic::catch_unwind` (attach.rs line 81) but this is not documented in the architecture.

3. **URL normalization details**: The `build_tui_ws_url()` and `build_http_url()` functions handle WS/WSS/HTTP/HTTPS but the exact transformation rules could be clearer (e.g., lines 134-142).

4. **`RenderFrame` semantics**: The `RenderFrame { content: String }` variant exists but is unused. Its purpose and intended use case should be documented if it's part of the long-term protocol design.

5. **Token handling**: The Bearer token is added to WebSocket headers (attach.rs lines 27-29) but this is not mentioned in the architecture doc's connection flow.

### Improvement Opportunities

1. **Add reconnection documentation**: The WebSocket connection includes retry logic with 3 attempts and exponential backoff that should be documented.

2. **Clarify `RenderFrame` status**: Either remove `RenderFrame` from the protocol documentation if it's deprecated, or document its intended purpose.

3. **Document channel buffer sizes**: The `unbounded_channel()` calls don't specify buffer sizes (attach.rs lines 74-75). For production use, explicit buffer sizes may be beneficial.

4. **Add error propagation details**: When `tui::run_event_loop()` returns an error, it's converted to `ClientError::Connection` (line 129) but the reasoning for this mapping isn't documented.

5. **Consider adding `Rpc` usage or removing**: If `Rpc` error variant is intentionally part of the API for future server-side error propagation, this should be documented. Otherwise, consider removing dead code.

## Recommendations

1. **Fix `RenderFrame` table placement**: Move `RenderFrame` from the Server → Client table to the Client → Server table in the protocol section.

2. **Update Server → Client variant count**: Correct the table to show 9 variants instead of 10, removing the reference to `RenderFrame`.

3. **Document WebSocket retry logic**: Add a section describing the reconnection behavior with 3 attempts and exponential backoff.

4. **Document `catch_unwind` behavior**: The panic handling in `event_task` is a resilience feature worth documenting.

5. **Audit `Rpc` variant**: Either implement `ClientError::Rpc` usage or remove the dead code variant.
