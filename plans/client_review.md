# Client Module Architecture Review

**Status**: STALE (architecture doc modified May 25 after previous review)

**Review Date**: 2026-05-25

---

## Summary

Verified `architecture/client.md` against `src/client/` (3 files: `mod.rs`, `attach.rs`, `sdk.rs`) and `src/protocol/tui.rs`. The architecture document is **generally accurate** with minor discrepancies in line counts and one behavioral note.

---

## File Verification

| File | Arch Doc Lines | Actual Lines | Status |
|------|-----------------|--------------|--------|
| `mod.rs` | 4 | 4 | ✅ MATCH |
| `attach.rs` | ~154 (skill) | 159 | ⚠️ DISCREPANCY |
| `sdk.rs` | ~53 (skill) | 53 | ✅ MATCH |

---

## Verified Correct

### mod.rs
- ✅ Re-exports `run_attach` from `attach` module

### attach.rs
- ✅ `run_attach(url: &str, token: Option<&str>) -> Result<(), ClientError>` signature matches
- ✅ `build_tui_ws_url()` and `build_http_url()` functions present
- ✅ Health check via `RemoteClient::health()` with 10s timeout
- ✅ WebSocket: 30s timeout per attempt, up to 3 retries with exponential backoff (2s, 4s)
- ✅ Resume handshake: `TuiMessage::Resume { from_event_seq: 0 }` sent immediately after connect
- ✅ Channel setup: `event_tx/rx` and `out_tx/rx` using `tokio::sync::mpsc::unbounded_channel`
- ✅ `event_task`: receives WS messages, parses JSON, forwards via `event_tx`
- ✅ `send_task`: receives `TuiMessage`, serializes to JSON, sends over WS
- ✅ `catch_unwind` wraps `event_task` async block
- ✅ TUI initialization: `tui::App::new_remote(url.to_string())`
- ✅ Cleanup: both tasks aborted when `run_event_loop()` returns

### sdk.rs
- ✅ `RemoteClient` struct with `base_url: String` and `http: Client`
- ✅ `new(base_url: &str, token: Option<&str>)` constructor with Bearer token header support
- ✅ `health()` method: `GET /health` with 10s timeout, returns `Err(ClientError::Unreachable)` on failure

### TuiMessage Protocol
- ✅ All documented variants present in `src/protocol/tui.rs`:
  - Client→Server: `Input`, `KeyDown`, `MouseClick`, `Resize`, `Resume`, `RenderFrame`, `PermissionResponse`, `QuestionResponse`
  - Server→Client: `EventEnvelope`, `TextDelta`, `PermissionPending`, `QuestionPending`, `SessionInfo`, `SessionEnded`, `ToolCallStarted`, `ToolResult`, `Error`, `ResyncRequired`
- ✅ `#[serde(tag = "type")]` for JSON serialization

### ClientError Enum
- ✅ All 5 variants in `src/error.rs:504-519`: `Connection`, `Unreachable`, `Rpc`, `WebSocket`, `Auth`

### TUI Integration
- ✅ `App::new_remote(project_dir: String)` at `src/tui/app/mod.rs:510`
- ✅ `handle_remote_event(event: serde_json::Value)` at `src/tui/app/mod.rs:794`
- ✅ `EventEnvelope` unwrapping with recursive call to `handle_remote_event` for payload
- ✅ `ResyncRequired` handling with toast warning and full details logged

---

## Discrepancies

### 1. SKILL.md Line Count Outdated
**File**: `.opencode/skills/client/SKILL.md`

- Line 21 claims `attach.rs` is 154 lines; actual is **159 lines**
- This is a skill documentation issue, not an architecture doc issue

### 2. SKILL.md `new_remote` Signature Inaccurate
**File**: `.opencode/skills/client/SKILL.md:157`

The skill shows:
```rust
pub fn new_remote(project_dir: String) -> Self
```

But the actual call site in `attach.rs:77` passes `url.to_string()`:
```rust
let mut app = tui::App::new_remote(url.to_string());
```

This is a documentation inaccuracy in the **skill**, not the architecture doc. The architecture doc correctly describes this as `tui::App::new_remote()` without specifying parameters.

### 3. RenderFrame Handling
**Files**: `architecture/client.md:89`, `src/protocol/tui.rs:34-36`

The architecture doc says `RenderFrame` is "received and logged, not rendered". This is **correct** - see `src/tui/app/mod.rs:868-873`:
```rust
Ok(RemoteTuiMessage::RenderFrame { content }) => {
    tracing::warn!(
        "RenderFrame received ({} bytes) but rendering not implemented",
        content.len()
    );
}
```

---

## Bugs Identified

**None.** The implementation is correct and matches the architecture documentation.

---

## Recommendations

### For Architecture Doc
1. Consider adding `attach.rs` line count (159 lines) if maintaining line counts
2. The architecture doc is accurate - no changes required

### For SKILL.md
1. Update line count for `attach.rs` from 154 to 159
2. The `new_remote(project_dir: String)` signature shown in the skill is for documentation purposes (the method does take a `String` parameter), but the call site uses `url.to_string()`. Consider clarifying this in the skill.

### For Code
No changes recommended - implementation is correct.

---

## Conclusion

The architecture document `architecture/client.md` is **accurate and up-to-date**. The implementation in `src/client/` matches all documented types, functions, and behaviors. The only discrepancies are in the **skill file** `.opencode/skills/client/SKILL.md` which has outdated line counts.
