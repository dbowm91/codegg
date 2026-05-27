# Implementation Plan

**Status**: ACTIVE - Implementation in progress
**Last Updated**: 2026-05-27

---

## Deferred Items (Complex Refactors)

### TUI-5: Accessibility Improvements
- **Files**: `src/util/a11y.rs` (new), `src/tui/components/component/`, `src/tui/app/mod.rs`
- **Status**: DEFERRED - Requires significant FocusManager architectural change
- **Current Architecture Issues**:
  - `FocusManager` (`src/tui/components/component/focus.rs:14-108`) is purely **modal** - only top component receives key events
  - Tab key is consumed/ignored in most dialog contexts (`src/tui/app/mod.rs:2075-2088`)
  - Tab key maps to `InputAction::SwitchAgent` (line 219) and `InputAction::TogglePermissionMode` (line 221) but events don't bubble
- **Implementation Steps**:
  1. **Create `src/util/a11y.rs`**:
     ```rust
     pub struct A11yFocusManager {
         elements: Vec<FocusableElement>,
         current_index: usize,
     }
     pub struct FocusableElement {
         pub id: String,
         pub component_type: String,
         pub bounds: Rect,
     }
     ```
  2. **Add focusable element methods to `Component` trait** (`src/tui/components/component.rs:82-103`):
     ```rust
     fn focusable_elements(&self) -> Vec<FocusableElement> { vec![] }
     fn set_focus(&mut self, _element_id: &str) {}
     ```
  3. **Modify `FocusManager`** to support sequential Tab navigation:
     - Add `a11y_manager: A11yFocusManager` field
     - Add `tab_next(&mut self) -> Option<TuiMsg>` and `tab_prev()`
  4. **Replace Tab handling in `handle_dialog_key()`** (`src/tui/app/mod.rs:2064`):
     - Before current dialog-specific handling, check for global Tab
     - Delegate to `focus_manager.tab_next()` / `tab_prev()`
  5. **Implement `focusable_elements()` in each dialog** - each dialog reports its focusable children
  6. **Add visual focus indicators** - each component renders focus rings when focused
- **Architectural Challenges**:
  - Modal (dialogs) vs Sequential (Tab) navigation conflict
  - Focus boundaries: Tab cycles within dialog or across entire UI?
  - Nested component focus (dialogs containing sub-components)
  - Screen reader support for terminal
- **Note**: This is a complex refactor that would benefit from a design doc first
- **Test**: `cargo test tui -- input`

### LARGE-1: Virtual Scrolling for Messages
- **Files**: `src/tui/components/messages.rs`, `src/tui/components/messages/layout.rs` (new)
- **Status**: DEFERRED - High risk refactor
- **Current Issues**:
  - Linear scan O(n) for visible range (lines 934-947)
  - `total_rendered_lines()` recalculates all heights every scroll
  - No caching of rendered lines - full re-render on every frame
  - `estimate_msg_lines()` (lines 159-200) called O(n) times per render
- **Implementation Steps**:
  1. **Create `src/tui/components/messages/layout.rs`**:
     - `struct MessageLayout { msg_idx, total_lines, part_offsets, rendered_cache }`
     - `struct MessageLayoutCache` with `get_or_compute(msg_idx, width) -> Vec<Line>`
     - `fn binary_search_visible(cumulative: &[usize], scroll: usize, visible: usize) -> Range<usize>`
     - `fn invalidate_message(msg_idx)` to clear cache
  2. **Add cache fields to `MessagesWidget`**:
     ```rust
     layout_cache: RefCell<Option<MessageLayoutCache>>,
     last_width: Cell<Option<u16>>,
     height_cache: RefCell<HashMap<usize, usize>>,
     ```
  3. **Modify `render()` method** (lines 900-1267) to use binary search instead of linear scan
  4. **Add invalidation calls** in:
     - `add_user_message()` (line 242)
     - `add_assistant_text()` (line 256)
     - `update_tool_call()` (line 417)
     - `toggle_reasoning()` (line 461)
     - `toggle_selected_tool_call_expanded()` (line 572)
  5. **Cache markdown rendering** - `render_markdown()` (lines 1270-1378) is expensive
  6. **Handle terminal resize** - cache invalidation on width change
- **Cache key**: `(msg_idx, width, expansion_state_hash)` to handle dynamic content
- **Consider LRU eviction** for sessions with 10k+ messages
- **Risk**: HIGH - Scroll behavior deeply integrated with selection, search highlighting, streaming state
- **Test Strategy**: Create test with 1000+ messages, verify 60fps scroll performance
- **Alternative**: Feature flag `virtual-scroll` for incremental rollout, maintain current impl as fallback

### LARGE-2: String Interning System
- **Files**: `src/util/interner.rs` (new), `src/provider/mod.rs`, `src/agent/`, `src/tool/`
- **Status**: DEFERRED - High risk architectural change
- **Current State**: `Message` already uses `Arc<String>` for content, but `ToolDefinition` uses owned `String`
- **Implementation Steps**:
  1. **Create `src/util/interner.rs`**:
     ```rust
     pub struct StringInterner {
         forward: DashMap<String, u64>,
         backward: Vec<String>,
     }
     impl StringInterner {
         pub fn intern(&mut self, s: &str) -> u64 { ... }
         pub fn get(&self, id: u64) -> Option<&str> { ... }
     }
     ```
  2. **Verify DashMap dependency** - `src/plugin/loader.rs` already uses it; check Cargo.toml
  3. **Apply to ToolDefinition first** (`src/tool/mod.rs`):
     - Modify `ToolDefinition` to use `Arc<String>` for name, description
     - Add interning in `tool/mod.rs:definitions()` method
  4. **Profile first** - Add metrics to identify highest frequency allocations:
     - Track `system_prompt`, `tool_definition`, `tool_name` intern calls
     - Measure hit rate vs misses
  5. **Apply to system prompts** (`src/agent/prompt.rs`):
     - Static `SYSTEM_PROMPT_INTERNER: LazyLock<StringInterner>`
     - Intern repeated prompt segments
  6. **Add metrics** via existing `src/util/metrics.rs` system
  7. **Handle cache invalidation** - interner must not grow unbounded
- **Key Challenge**: Global state lifetime management; DashMap overhead (~48 bytes/entry)
- **Expected Benefit**: Reduced clone overhead, allocation pressure, GC pauses for 26 tools x 2-3 strings per turn
- **Risk**: HIGH - Lifetime complexity, static initialization order, memory leaks if unbounded
- **Test**: Run session with 100+ turns, verify memory reduction via metrics

---

## Known Code Issues (Deferred/Low Priority)

| Issue | Location | Priority |
|-------|----------|----------|
| Snapshot hash inconsistency | `src/snapshot/mod.rs:431` uses MD5 | MEDIUM |
| ToolExecutor exists but unused | `src/tool/executor.rs:8` | MEDIUM |
| Static CANONICAL_PATHS_CACHE never clears | `src/security/sandbox.rs:237` | MEDIUM |
| TTS init() ignores providers | `src/tts/mod.rs:45-49` | LOW |
| Worktree symlink detection | `src/worktree/mod.rs:69-88` | LOW |
| OAuth replay protection TOCTOU | `src/mcp/auth.rs:318-332` | MEDIUM |
| PermissionResponse unused | `src/permission/mod.rs:1141-1145` | LOW |
| check_external_directory unused | `src/permission/mod.rs:1237-1248` | LOW |

---

## Notes for Future Agents

### Critical Implementation Notes

1. **WASM Plugin Fuel**: Fuel is consumed per-hook execution. Unused fuel is returned after execution. Check `module_cache::CACHE` in `src/plugin/loader.rs`. `return_fuel()` uses `MAX_PLUGIN_FUEL_BUDGET`.

2. **Async in TUI**: Command handlers are sync but use `TuiCommand` pattern to bridge to async handlers. Use `tui_cmd_tx.try_send(TuiCommand::YourCommand { ... })`.

3. **Plan/Build Mode**: Controlled by `agent_state.plan_mode` in TUI and `state.plan_mode` in AgentLoop. Toggle via markers, `/plan` tool, or Shift+Tab.

4. **LSP Diagnostics**: `DiagnosticsCollector` uses async mutex. `should_debounce()` is async.

5. **Subagent Tasks**: Tasks are persisted to SQLite. `TaskStore` manages in-memory state. Task IDs are atomic u64 counters. Subagent `max_depth` limit (default: 3) prevents infinite recursion.

6. **Adding TuiCommand variants**: Add to enum in `src/tui/app/mod.rs`, add async handler in `src/tui/mod.rs`, use `tui_cmd_tx.try_send()` from sync handlers.

7. **Crypto Module**: `src/crypto/mod.rs` provides AES-256-GCM encryption (`encrypt_to_string`, `decrypt_from_string`).

8. **Tool Path Validation**: `validate_path()` in `src/tool/util.rs` checks symlinks and verifies path components. `check_path_for_symlinks()` rejects symlink paths.

9. **Write Tool TOCTOU Fix**: Parent path validated BEFORE `create_dir_all()`.

10. **Token Estimation**: `estimate_tokens_sync()` uses `TokenizerType` for model-specific multipliers. Claude: 1.4x, Gemini: 1.2x.

11. **Exec Mode Question Handling**: `src/exec.rs:121` has no question_rx handler - question tool returns "[question not supported in exec mode]" instead of deadlocking.

12. **TTS Module**: Located at `src/tts/mod.rs`. Uses macOS `say` command. TTS auto-stops when agent finishes. Toggle with `/tts` or `/voice` command.

### Implementation Patterns

- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT use `await` when calling these.

- **Registry Limitations**: Permission IDs are in format `{tool_call_id}-{tool_name}`, not `{session_id}-...`. `get_pending_permissions_for_session()` and `get_pending_questions_for_session()` cannot properly filter by session_id without code changes.

- **Registration-before-publish pattern**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event.

### Completed Implementation Items

| Item | Location | Completed |
|------|----------|-----------|
| TUI-3: Image Attachment Support | `src/tui/components/image.rs`, `src/tui/components/messages.rs` | 2026-05-27 |
| AGENT-5: Image Generation | `src/tool/image.rs` | 2026-05-27 |
| AGENT-6: GitHub Integration | `/pr` and `/issue` commands added | 2026-05-27 |
| EXEC-2: Session Analytics & Cost Tracking | `src/util/pricing.rs`, `src/session/` | 2026-05-27 |
| GIT Enhancement: GitHub MCP | `src/git/mod.rs`, prompt injection | 2026-05-27 |

### Verified Codebase Facts

| Item | Value | Location |
|------|-------|----------|
| Tool count | 27 | `src/tool/mod.rs:89-119` (now includes ImageTool) |
| LSP server count | 39 | `src/lsp/server.rs:27-383` |
| InprocCoreClient fields | All wrapped in `Option<Arc<...>>` | `src/core/mod.rs:22-28` |
| ToolExecutor | DEPRECATED - exists but unused | `src/tool/executor.rs:8` |
| Plugin fuel logic | Fixed - all early returns correctly return fuel | `src/plugin/loader.rs` |
| CoreEvent mapping | Complete - all events including Subagent* properly mapped | `src/core/mod.rs` |
| CommandRegistry location | Line 72 | `src/tui/command.rs:72` |
| UiState fields | 26 fields | `src/tui/app/state/ui.rs:27-76` |
| Subagent event types | SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed | `src/bus/events.rs:120-141` |
| CoreEvent has subagent variants | SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed | `src/protocol/core.rs:244-268` |
| map_app_event_to_core_event | All Subagent events mapped | `src/core/mod.rs` |
| SessionCompacting hook | IS dispatched in AgentLoop::compact_if_needed() | `src/agent/loop.rs:1216-1220` |
| hook_timeout vs WASM_HOOK_TIMEOUT | Outer 5s, inner 30s | `src/plugin/service.rs:18`, `src/plugin/loader.rs:14` |
| Backoff formula | `2^i` (no jitter) | `src/provider/fallback.rs:107` |
| Client backoff formula | 1s, 2s, 4s (attempt 1,2,3) | `src/client/attach.rs:39` |
| Protocol version | 1 | `src/protocol/core.rs:3` |
| AppEvent count | 36 | `src/bus/events.rs:5-147` |
| Built-in command count | 45 (includes /tts, /pr, /issue, /checkpoint) | `src/tui/command.rs:79-165` |
| ToolDefCache | `(Option<String>, bool, bool, usize, u64, Vec<ToolDefinition>)` | `src/agent/loop.rs:60-67` |
| Timeline fields location | `timeline_visible` and `timeline_selected` in `UiState` | `src/tui/app/state/ui.rs:62-63` |
| Snapshot hash | Uses MD5 in `collect_files_sync` (line 431), SHA256 elsewhere | `src/snapshot/mod.rs:431` |
| TTS stop() | Fixed - returns Err on pkill failure | `src/tts/mod.rs:85-107` |

### Security Notes

- **Auth middleware allows requests without token when none configured**: At `src/server/middleware/auth.rs:37-39`, when `expected_token` is `None`, requests are allowed through. This may be intentional for development but should be reviewed for production.

### CoreRequest Handler Attention Points

- `CoreRequest` enum in `src/protocol/core.rs:50-175`
- InprocCoreClient handlers at `src/core/mod.rs:52-355` handle: TurnSubmit, SessionMessagesLoad, SessionMessageCounts, SessionCreate, SessionLoad, SessionAttach, etc.
- Variants falling through to `Ack`: Initialize, TurnCancel, TurnSteer, AgentSelect, ModelSelect - verify if TUI actually sends these before implementing meaningful responses.

### Testing Commands

```bash
# Always run before/after changes
cargo build --all-features
cargo clippy --all-features -- -D warnings
cargo test --all-features

# Specific feature testing
cargo test --all-features -- --test-threads=1  # For integration tests

# TUI tests
cargo test tui::input
cargo test tui
cargo test messages

# Run specific module tests
cargo test --package codegg -- <module>_test_pattern
```

---

## Summary

| Wave | Items | Time Estimate | Status |
|------|-------|---------------|--------|
| Wave 2 | TUI-3 (Image Support), AGENT-5 (Image Generation) | 6-10 hours | ✅ DONE |
| Wave 3 | AGENT-6 (GitHub), EXEC-2 (Analytics) | 6-12 hours | ✅ DONE |
| Wave 4 | LARGE-1 (Virtual Scroll), LARGE-2 (String Interning) | 24-32 hours | DEFERRED |
| Wave 5 | GIT-1 (Enhanced Git) | 4-6 hours | ✅ DONE |
| Wave 6 | TUI-5 (Accessibility) | 4-6 hours | DEFERRED |

*(End of file)*
