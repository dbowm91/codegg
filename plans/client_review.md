# Client Architecture Review

## Source Files Verified
- `src/client/mod.rs` (4 lines)
- `src/client/attach.rs` (159 lines)
- `src/client/sdk.rs` (53 lines)
- `src/protocol/tui.rs` (82 lines)
- `src/error.rs` (lines 504-519 for ClientError)
- `src/tui/app/mod.rs` (line 794 for handle_remote_event)

---

## Verification Results

### OVERVIEW (Lines 1-14)
| Claim | Status | Notes |
|-------|--------|-------|
| Location `src/client/` | ✅ VERIFIED | |
| WebSocket connection to server | ✅ VERIFIED | `attach.rs:43` uses `connect_async` |
| Remote TUI protocol | ✅ VERIFIED | `TuiMessage` in protocol/tui.rs |
| Session attach/detach | ✅ VERIFIED | `run_attach()` function |
| Server health checking | ✅ VERIFIED | `sdk.rs:35` `health()` method |

---

### MOD.RS (Lines 17-23)
| Claim | Status | Notes |
|-------|--------|-------|
| Re-exports `run_attach` as public API | ✅ VERIFIED | Line 4: `pub use attach::run_attach;` |

---

### ATTACH.RS (Lines 25-56)
| Claim | Status | Notes |
|-------|--------|-------|
| `run_attach(url, token)` signature | ✅ VERIFIED | Line 14 |
| URL building functions | ✅ VERIFIED | Lines 137-158 |
| 10s health check timeout | ✅ VERIFIED | `sdk.rs:40` |
| 30-second WebSocket timeout per attempt | ✅ VERIFIED | `attach.rs:43` |
| Up to 3 retries | ✅ VERIFIED | `attach.rs:36` `max_attempts = 3` |
| Backoff formula 1s, 2s, 4s | ⚠️ INACCURATE | See below |
| Resume handshake with `from_event_seq: 0` | ✅ VERIFIED | Line 73 |
| `event_tx/rx` channel | ✅ VERIFIED | Line 79 |
| `out_tx/rx` channel | ✅ VERIFIED | Line 80 |
| `catch_unwind` in event_task | ✅ VERIFIED | Line 86 |
| Two background tasks | ✅ VERIFIED | Lines 85-127 |
| `tui::App::new_remote()` | ✅ VERIFIED | Line 77 |
| Cleanup - tasks aborted | ✅ VERIFIED | Lines 131-132 |

**Backoff Issue**: Documentation states "1s, 2s, 4s" but code at `attach.rs:39` uses `2u64.saturating_pow((attempt - 1) as u32)`:
- Attempt 0 (first try): 2^0 = 1s delay before retry
- Attempt 1: 2^1 = 2s delay
- Attempt 2: 2^2 = 4s delay

The "1s, 2s, 4s" in docs likely refers to the delays **after** attempts 1, 2, 3 fail. The first failure sleeps 1s, second sleeps 2s, third sleeps 4s. This is technically correct but could be clearer.

---

### SDK.RS (Lines 58-74)
| Claim | Status | Notes |
|-------|--------|-------|
| `RemoteClient` struct with `base_url`, `http` | ✅ VERIFIED | Lines 7-10 |
| `new(base_url, token)` constructor | ✅ VERIFIED | Line 13 |
| `health()` returns Result | ✅ VERIFIED | Line 35 |
| 10-second timeout on health check | ✅ VERIFIED | Line 40 |
| `Err(ClientError::Unreachable)` on failure | ✅ VERIFIED | Line 43 |

---

### PROTOCOL SECTION (Lines 76-108)
| Claim | Status | Notes |
|-------|--------|-------|
| TuiMessage from `src/protocol/tui.rs` | ✅ VERIFIED | |
| `#[serde(tag = "type")]` | ✅ VERIFIED | `protocol/tui.rs:2` |

**Client → Server Messages (Input/Control)**:
| Variant | Fields | Status |
|---------|--------|--------|
| `Input` | `text: String` | ✅ VERIFIED |
| `KeyDown` | `key: String`, `modifiers: Vec<String>` | ✅ VERIFIED |
| `MouseClick` | `x: u16`, `y: u16` | ✅ VERIFIED |
| `Resize` | `w: u16`, `h: u16` | ✅ VERIFIED |
| `Resume` | `from_event_seq: u64` | ✅ VERIFIED |
| `RenderFrame` | `content: String` | ✅ VERIFIED |
| `PermissionResponse` | `id: String`, `choice: String` | ✅ VERIFIED |
| `QuestionResponse` | `id: String`, `answers: serde_json::Value` | ✅ VERIFIED |

**Server → Client Messages (Events)**:
| Variant | Fields | Status |
|---------|--------|--------|
| `EventEnvelope` | `event_seq: u64`, `payload: Box<TuiMessage>` | ✅ VERIFIED |
| `TextDelta` | `delta: String` | ✅ VERIFIED |
| `PermissionPending` | `id: String`, `tool: String`, `path: Option<String>` | ✅ VERIFIED |
| `QuestionPending` | `id: String`, `questions: Vec<QuestionSpec>` | ✅ VERIFIED |
| `SessionInfo` | `id: String`, `model: String` | ✅ VERIFIED |
| `SessionEnded` | `stop_reason: String` | ✅ VERIFIED |
| `ToolCallStarted` | `tool_name: String`, `tool_id: String`, `arguments: String` | ✅ VERIFIED |
| `ToolResult` | `tool_id: String`, `output: String`, `success: bool` | ✅ VERIFIED |
| `Error` | `message: String` | ✅ VERIFIED |
| `ResyncRequired` | `reason: Option<String>`, `pending_permissions: Vec<String>`, `pending_questions: Vec<String>` | ✅ VERIFIED |

| Claim | Status | Notes |
|-------|--------|-------|
| `handle_remote_event()` unwraps EventEnvelope | ✅ VERIFIED | `tui/app/mod.rs:799` |

---

### ERROR HANDLING (Lines 110-120)
| Claim | Status | Notes |
|-------|--------|-------|
| ClientError enum location | ❌ INCORRECT | Claims inline, actual location is `src/error.rs:504` |
| Variant names and comments | ✅ VERIFIED | All 5 variants match |

---

## Summary

| Category | Correct | Issues |
|----------|---------|--------|
| Module organization | ✅ | All components match |
| Function signatures | ✅ | All match |
| Protocol enum variants | ✅ | All 17 variants correct |
| Error types | ✅ | Variants correct, location wrong |
| Line numbers | ⚠️ | Only file-level, no internal line refs |
| Backoff description | ⚠️ | Ambiguous but technically correct |

---

## Corrections Needed

1. **Line 41**: Change "1s, 2s, 4s" to clarify:
   - "with exponential backoff of 1s, 2s, 4s between the 3 attempts"

2. **Line 112**: ClientError is defined in `src/error.rs:504`, not inline in the client module.