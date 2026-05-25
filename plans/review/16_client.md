# Client Architecture Review (2026-05-26)

## Overview

Reviewed `architecture/client.md` against `src/client/` and `src/protocol/` source files.

## Verified Correct Items

1. **mod.rs**: Public API correctly exports `run_attach` (line 4)
2. **attach.rs**:
   - `run_attach(url: &str, token: Option<&str>)` signature matches
   - `build_tui_ws_url()` and `build_http_url()` functions exist
   - Health check with 10s timeout (sdk.rs:40)
   - WebSocket: 30s timeout, 3 retries, exponential backoff (2s, 4s) (lines 35-66)
   - Resume handshake `TuiMessage::Resume { from_event_seq: 0 }` (lines 72-75)
   - Channel setup with `event_tx/rx` and `out_tx/rx` (lines 79-80)
   - Two background tasks: `event_task` (lines 85-117) and `send_task` (lines 119-127)
   - TUI initialization via `tui::App::new_remote()` (line 77)
   - Cleanup: both tasks aborted on return (lines 131-132)
   - `catch_unwind` for panic handling (line 86)
3. **sdk.rs**:
   - `RemoteClient` struct with `base_url` and `http` fields (lines 7-10)
   - `health()` uses `GET /health` with 10s timeout (lines 35-52)
   - Returns `Err(ClientError::Unreachable)` on non-success (lines 47-50)
4. **Protocol**: `TuiMessage` enum with `#[serde(tag = "type")]` (protocol/tui.rs:2)
5. **Client→Server messages**: All variants match (Input, KeyDown, MouseClick, Resize, Resume, RenderFrame, PermissionResponse, QuestionResponse)
6. **Server→Client messages**: All variants match (EventEnvelope, TextDelta, PermissionPending, QuestionPending, SessionInfo, SessionEnded, ToolCallStarted, ToolResult, Error, ResyncRequired)
7. **ClientError enum**: All 5 variants correct (Connection, Unreachable, Rpc, WebSocket, Auth) at error.rs:504-518

## Incorrect/Stale Items

1. **Line 108**: `handle_remote_event()` is incorrectly attributed to the client module
   - **Actual location**: `src/tui/app/mod.rs:794`
   - This is TUI-side event handling, not client-side
   - The client module sends raw JSON events to the TUI which then dispatches them
   - **Fix**: Remove or reword to clarify this is in `tui/app/mod.rs`

2. **Line 108**: Wording "replayed events and live events share the same handler path"
   - The EventEnvelope unwrapping happens in `handle_remote_event()` at line 799
   - Recursive call handles the unwrapped payload
   - This is correct but the description location is wrong

## Minor Corrections

1. **Line 43**: "immediately sends" - technically sends synchronously after connect (line 73-75)
2. **Line 48**: "uses `catch_unwind`" - correct description but could note it's on the async block itself
3. **Line 77 in attach.rs**: `tui::App::new_remote(url.to_string())` takes project_dir, but in remote mode the URL is passed (not a project path). Minor naming confusion but functionally correct.

## Summary

The architecture document is **95% accurate**. The main issue is `handle_remote_event()` being incorrectly attributed to the client module when it's actually in `tui/app/mod.rs`. All protocol definitions, error types, and connection flows are correct.

**Lines needing updates**:
- Line 108: Clarify `handle_remote_event()` is in `src/tui/app/mod.rs`, not client module
