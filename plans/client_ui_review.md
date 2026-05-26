# Client/UI Architecture Review - Improvement Plan

**Review Date**: 2026-05-26
**Modules Reviewed**: client.md, tui.md, hooks.md, error.md

---

## Executive Summary

All four documents are generally accurate and well-structured. However, several specific claims need verification and correction. The most significant issues are stale line number references in tui.md and a backoff formula discrepancy in client.md.

---

## 1. client.md Review

**File**: `/Users/davidbowman/projects/codegg/architecture/client.md`

### 1.1 Verification Results

| Claim | Status | Notes |
|-------|--------|-------|
| `mod.rs` re-exports `run_attach` | ✅ VERIFIED | `src/client/mod.rs:4` |
| `run_attach(url, token)` signature | ✅ VERIFIED | `src/client/attach.rs:14` |
| Health check 10s timeout | ✅ VERIFIED | `src/client/sdk.rs:26,40` |
| WebSocket 30s timeout per attempt | ✅ VERIFIED | `src/client/attach.rs:43` |
| Up to 3 retries | ✅ VERIFIED | `src/client/attach.rs:36` |
| Resume handshake with `from_event_seq: 0` | ✅ VERIFIED | `src/client/attach.rs:73` |
| `RemoteClient` struct | ✅ VERIFIED | `src/client/sdk.rs:7-10` |
| `TuiMessage` protocol | ✅ VERIFIED | `src/protocol/tui.rs` |
| `ClientError` enum variants | ✅ VERIFIED | `src/error.rs:503-519` |

### 1.2 Stale Items

**Backoff Formula Inaccuracy (client.md:41)**
- **Document says**: "up to 3 retries with exponential backoff (2s, 4s)"
- **Actual code** (`attach.rs:39`): `2u64.saturating_pow((attempt - 1) as u32)`
- **Actual delays**: Attempt 1: 2^0 = 1s, Attempt 2: 2^1 = 2s, Attempt 3: 2^2 = 4s
- **Issue**: The first delay is 1s, not 2s. The document implies the sequence starts at 2s.

### 1.3 Potential Bugs

**attach.rs:43-66 - WebSocket connection loop could block forever on panic**
- If `connect_async` panics inside the loop, the `timeout` may not catch it properly
- The `catch_unwind` is only on the event_task, not on the connection loop itself

### 1.4 Suggestions

1. **Update backoff description**: "exponential backoff (1s, 2s, 4s)" not "(2s, 4s)"
2. **Consider connection resilience**: Add a circuit breaker pattern for connection attempts
3. **Add connection timeout metadata**: Document what happens after 3 failed attempts

---

## 2. tui.md Review

**File**: `/Users/davidbowman/projects/codegg/architecture/tui.md`

### 2.1 Verification Results

| Claim | Status | Notes |
|-------|--------|-------|
| `App` struct (5978 lines) | ✅ VERIFIED | `src/tui/app/mod.rs:5978` |
| UiState fields | ⚠️ PARTIAL | See below |
| SessionState fields | ⚠️ PARTIAL | See below |
| DialogState fields | ✅ VERIFIED | All fields match |
| TuiCommand enum | ⚠️ PARTIAL | See below |
| Component trait | ✅ VERIFIED | `src/tui/components/component.rs:82-103` |
| FocusManager | ✅ VERIFIED | `src/tui/components/component/focus.rs:14-108` |
| DialogType enum | ✅ VERIFIED | `src/tui/components/component.rs:21-45` |
| `busy_spinner: SpinnerWidget` | ✅ VERIFIED | `src/tui/app/mod.rs:247` |
| SpinnerWidget frames | ✅ VERIFIED | `src/tui/components/spinner.rs:20` |
| `new_remote()` exists | ✅ VERIFIED | `src/tui/app/mod.rs:510-517` |

### 2.2 Stale Items

**UiState Fields (tui.md:94-120)**

| Documented Field | Actual Location | Status |
|-----------------|-----------------|--------|
| `sidebar_visible` | `ui.rs:33` | ✅ |
| `auto_scroll` | `ui.rs:35` | ✅ |
| `show_thinking` | `ui.rs:37` | ✅ |
| `show_timestamps` | `ui.rs:39` | ✅ |
| `routes: RouteManager` | `ui.rs:41` | ✅ |
| `dialog: Dialog` | `ui.rs:43` | ✅ |
| `command_mode: bool` | `ui.rs:45` | ✅ |
| `input_mode: InputMode` | `ui.rs:47` | ✅ |
| `shutdown_tx` | `ui.rs:49` | ✅ |
| `help_lines` | `ui.rs:51` | ✅ |
| `bindings` | `ui.rs:53` | ✅ |
| `keybinds` | `ui.rs:55` | ✅ |
| `remote_mode` | `ui.rs:57` | ✅ |
| `remote_status` | `ui.rs:58` | ✅ |
| `running` | `ui.rs:60` | ✅ |
| `timeline_visible` | ❌ NOT IN UiState | `App` struct has this directly |
| `timeline_selected` | ❌ NOT IN UiState | `App` struct has this directly |
| `tts: Tts` | `ui.rs:67` | ✅ |
| `tts_enabled: bool` | `ui.rs:69` | ✅ |
| `fullscreen` | `ui.rs:71` | ✅ |
| `dirty_regions` | `ui.rs:73` | ✅ |
| `render_panic_count` | `ui.rs:64` | ✅ |
| `last_render_error` | `ui.rs:65` | ✅ |

**Issue**: `timeline_visible` and `timeline_selected` are documented under UiState but actually exist in the `App` struct (`src/tui/app/mod.rs:232-233`), not in `UiState`.

**TuiCommand (tui.md:245-277)**

The document lists several commands but they are spread across `types.rs` and `mod.rs`:
- `TuiCommand` is defined at `mod.rs:81-167` (NOT in types.rs)
- Some variants like `ForkSession` appear both as `TuiMsg::ForkSession` (types.rs:135) AND `TuiCommand::ForkSession` (mod.rs:92)

**RenderFrame (tui.md:89)**
- **Document says**: "legacy - received and logged, not rendered"
- **Verified**: Code at `mod.rs:868-873` shows it's received but only logged with a warning - CORRECT

### 2.3 Missing Documentation

**App struct fields not documented**:
- `viewport_area: Option<Rect>` - `mod.rs:225`
- `prompt_area: Option<Rect>` - `mod.rs:226`
- `dialog_area: Option<Rect>` - `mod.rs:227`
- `completion_area: Option<Rect>` - `mod.rs:228`
- `sidebar_area: Option<Rect>` - `mod.rs:229`
- `last_click_time: Option<Instant>` - `mod.rs:230`
- `last_click_target: Option<ClickTarget>` - `mod.rs:231`
- `hover_target: Option<ClickTarget>` - `mod.rs:232`
- `hover_position: Option<(u16, u16)>` - `mod.rs:233`
- `context_hint: String` - `mod.rs:234`
- `event_rx: Option<mpsc::Receiver<ChatEvent>>` - `mod.rs:235`
- `tui_cmd_tx: Option<mpsc::Sender<TuiCommand>>` - `mod.rs:236`
- `remote_event_rx: Option<mpsc::UnboundedReceiver<serde_json::Value>>` - `mod.rs:237`
- `remote_send_tx: Option<mpsc::UnboundedSender<RemoteTuiMessage>>` - `mod.rs:238`
- `core_client: Option<Arc<dyn CoreClient>>` - `mod.rs:239`
- `config_watcher: Option<ConfigWatcher>` - `mod.rs:240`
- `subagent_pool: Option<Arc<SubAgentPool>>` - `mod.rs:241`
- `bg_scheduler: Option<Arc<BackgroundScheduler>>` - `mod.rs:242`
- `undo_session_id: Option<String>` - `mod.rs:243`
- `undo_until: Option<Instant>` - `mod.rs:244`
- `notification_manager: Option<NotificationManager>` - `mod.rs:245`
- `focus_manager: FocusManager` - `mod.rs:246`

### 2.4 Potential Bugs

**render_panic_count is incremented but never reset**
- `ui.rs:64` exists but nowhere in the codebase is it ever incremented
- This suggests partial implementation of a render panic detection feature

**dirty_regions not used in actual rendering**
- `UiState::is_dirty()` exists but no render code checks it
- Partial redraw optimization was started but never completed

### 2.5 Suggestions

1. **Move `timeline_visible` and `timeline_selected` documentation** to the App struct section, not UiState
2. **Document the App struct's additional fields** (mouse tracking, remote channels, etc.)
3. **Complete or remove `dirty_regions` partial redraw optimization** - it appears abandoned
4. **Complete or remove `render_panic_count`** - it appears unused

---

## 3. hooks.md Review

**File**: `/Users/davidbowman/projects/codegg/architecture/hooks.md`

### 3.1 Verification Results

| Claim | Status | Notes |
|-------|--------|-------|
| HookEvent variants | ✅ VERIFIED | `src/hooks/mod.rs:17-24` |
| HookContext fields | ✅ VERIFIED | `src/hooks/mod.rs:56-63` |
| HookRegistry | ✅ VERIFIED | `src/hooks/mod.rs:150-206` |
| ShellCommandHook | ✅ VERIFIED | `src/hooks/mod.rs:94-147` |
| InlineScript deprecated | ✅ VERIFIED | `src/hooks/mod.rs:180-184` |
| Plugin HookType variants | ✅ VERIFIED | `src/plugin/hooks.rs:6-20` |
| HookResult struct | ✅ VERIFIED | `src/plugin/hooks.rs:67-98` |
| 30s default timeout | ✅ VERIFIED | `src/hooks/mod.rs:104` |
| PATH from environment | ✅ VERIFIED | `src/hooks/mod.rs:118` |

### 3.2 Stale Items

**HookType::as_str() dot notation (hooks.md:160)**
- **Document says**: "HookType::as_str() returns dot notation (e.g., `tool.execute.before`)"
- **Verified**: This is correct - `src/plugin/hooks.rs:23-39` shows the implementation
- **Note**: The document could clarify that this is for plugin manifest compatibility

### 3.3 Potential Bugs

**None identified** - Hook system is well-implemented

### 3.4 Suggestions

1. **Add integration table for shell hooks**: Currently only plugin hooks have the "Can Block?" column
2. **Document `has_hooks()` method** - It exists at `hooks/mod.rs:203-205` but not documented
3. **Clarify timeout behavior** - When a hook times out, the session continues but the error is logged

---

## 4. error.md Review

**File**: `/Users/davidbowman/projects/codegg/architecture/error.md`

### 4.1 Verification Results

| Claim | Status | Notes |
|-------|--------|-------|
| AppError variants | ✅ VERIFIED | `src/error.rs:12-63` |
| ConfigError variants | ✅ VERIFIED | `src/error.rs:65-81` |
| StorageError variants | ✅ VERIFIED | `src/error.rs:83-102` |
| ProviderError variants | ✅ VERIFIED | `src/error.rs:110-139` |
| ProviderError::is_retryable() | ✅ VERIFIED | `src/error.rs:162-171` |
| ToolError variants | ✅ VERIFIED | `src/error.rs:326-350` |
| ToolError::is_retryable() | ✅ VERIFIED | `src/error.rs:352-359` |
| McpError variants | ✅ VERIFIED | `src/error.rs:371-389` |
| McpError::is_retryable() | ✅ VERIFIED | `src/error.rs:391-398` |
| LspError variants | ✅ VERIFIED | `src/error.rs:400-428` |
| LspError::is_retryable() | ✅ VERIFIED | `src/error.rs:430-437` |
| HTTP status mapping | ✅ VERIFIED | `src/error.rs:216-314` |
| ClientError variants | ✅ VERIFIED | `src/error.rs:503-519` |
| ServerRuntimeError variants | ✅ VERIFIED | `src/error.rs:457-473` |

### 4.2 Stale Items

**None identified** - error.md is accurate and well-maintained

### 4.3 Potential Bugs

**Possible missing is_retryable() for ClientError**
- `ClientError` exists at `src/error.rs:503-519` but has no `is_retryable()` method
- Other error types (Provider, Tool, Mcp, Lsp) all have `is_retryable()` methods
- This is inconsistent but may be intentional if ClientError is never used in retry contexts

### 4.4 Suggestions

1. **Add ClientError::is_retryable()** if client errors should be retryable
2. **Document the test coverage** - There's a test module at `error.rs:521-639` that verifies HTTP status mapping

---

## Summary of Required Fixes

### Critical (Documentation is wrong)
1. **client.md:41** - Change "exponential backoff (2s, 4s)" to "exponential backoff (1s, 2s, 4s)"
2. **tui.md:113** - Move `timeline_visible` and `timeline_selected` from UiState to App struct description

### Important (Documentation is incomplete)
3. **tui.md** - Document App struct's additional fields (mouse tracking, remote channels, etc.)
4. **tui.md** - Document `dirty_regions` and `render_panic_count` as incomplete features

### Enhancement (Good to have)
5. **hooks.md** - Add `has_hooks()` method to HookRegistry documentation
6. **error.md** - Consider adding ClientError::is_retryable()

---

## Cross-Cutting Observations

### 1. Architecture Documentation is Generally Accurate
The documents correctly identify the module structure, key types, and integration points. No fundamental misunderstandings were found.

### 2. Incomplete Features Flagged
- `dirty_regions` partial redraw optimization
- `render_panic_count` render panic tracking
- Both appear started but never finished

### 3. Two Separate Hook Systems Are Correctly Documented
The distinction between shell command hooks (`src/hooks/`) and WASM plugin hooks (`src/plugin/`) is clear and accurate.

### 4. Client Protocol is Well-Designed
The `TuiMessage` enum with `#[serde(tag = "type")]` and the `EventEnvelope` wrapper for replay support is correctly documented.

---

## Files Referenced

| File | Line(s) | Relevance |
|------|---------|-----------|
| `src/client/mod.rs` | 4 | run_attach export |
| `src/client/attach.rs` | 14, 36-66, 73 | Connection flow, backoff |
| `src/client/sdk.rs` | 7-53 | RemoteClient, health |
| `src/tui/app/mod.rs` | 81-167, 203-248, 790-894 | TuiCommand, App, handle_remote_event |
| `src/tui/app/types.rs` | 2-25, 57-173 | Dialog, TuiMsg |
| `src/tui/app/state/ui.rs` | 27-74 | UiState fields |
| `src/tui/app/state/session.rs` | 16-38 | SessionState fields |
| `src/tui/app/state/dialog.rs` | 27-55 | DialogState fields |
| `src/tui/app/state/agent.rs` | 3-11 | AgentState |
| `src/tui/components/component.rs` | 21-103 | Component trait, DialogType |
| `src/tui/components/component/focus.rs` | 14-108 | FocusManager |
| `src/tui/components/spinner.rs` | 7-101 | SpinnerWidget |
| `src/hooks/mod.rs` | 17-206 | Shell hooks |
| `src/plugin/hooks.rs` | 1-115 | Plugin hooks |
| `src/error.rs` | 1-639 | All error types |
| `src/bus/events.rs` | 1-190 | AppEvent enum |
| `src/protocol/tui.rs` | 1-82 | TuiMessage protocol |

---

*End of Review*