# Client Architecture Review Findings

## Verified Claims

- **Module exports**: `src/client/mod.rs` only exports `run_attach` - verified at line 4
- **run_attach signature**: `async fn run_attach(url: &str, token: Option<&str>) -> Result<(), ClientError>` - confirmed at `src/client/attach.rs:14`
- **URL building**: `build_tui_ws_url()` and `build_http_url()` confirmed at lines 137-159
- **Health check 10s timeout**: Confirmed in `src/client/sdk.rs:26` via `connect_timeout(Duration::from_secs(10))` and at line 40 `timeout(Duration::from_secs(10))`
- **WebSocket 30s timeout**: Confirmed at `src/client/attach.rs:43` with `timeout(Duration::from_secs(30), connect_async(...))`
- **3 retries with exponential backoff**: Confirmed at `src/client/attach.rs:36-66` - max_attempts=3, delays of 2s, 4s via `saturating_pow((attempt - 1) as u32)`
- **Resume handshake**: Confirmed at `src/client/attach.rs:73-75` - sends `TuiMessage::Resume { from_event_seq: 0 }` immediately after connect
- **catch_unwind in event_task**: Confirmed at `src/client/attach.rs:86` - wrapped in `std::panic::catch_unwind`
- **Two background tasks**: event_task (lines 85-117) and send_task (lines 119-127) - verified
- **RemoteClient struct**: `base_url: String, http: Client` - confirmed at `src/client/sdk.rs:7-10`
- **ClientError enum**: Connection, Unreachable, Rpc, WebSocket, Auth - confirmed at `src/error.rs:504-519`
- **TuiMessage protocol**: Uses `#[serde(tag = "type")]` - verified at `src/protocol/tui.rs:2-3`
- **EventEnvelope**: Contains `event_seq: u64` and `payload: Box<TuiMessage>` - verified at `src/protocol/tui.rs:4-7`
- **Server→Client variants**: TextDelta, PermissionPending, QuestionPending, SessionInfo, SessionEnded, ToolCallStarted, ToolResult, Error, ResyncRequired - all present in `src/protocol/tui.rs`
- **Client→Server variants**: Input, KeyDown, MouseClick, Resize, Resume, PermissionResponse, QuestionResponse, RenderFrame - all present in `src/protocol/tui.rs`

## Stale Information

- **Protocol document reference**: `architecture/client.md` says TuiMessage is "from `src/protocol/tui.rs`" - this is correct, but the file path in the "See Also" section says `src/protocol/tui.rs` - VERIFIED

## Bugs Found

- None identified

## Improvements Suggested

- The documentation correctly describes the entire client module. No significant improvements needed.

## Cross-Module Issues

- **handle_remote_event location**: Document says "in `src/tui/app/mod.rs`" which is correct, but this is a TUI handler, not a client handler. This is cross-module documentation that is accurate.
