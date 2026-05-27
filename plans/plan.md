# Implementation Plan

**Status**: WAVE 4 DEFERRED - Waves R0-R3 completed (2026-05-27)
**Last Updated**: 2026-05-27

---

## Deferred Items (Complex Refactors - Wave 4)

These items are deferred to future iterations due to their architectural complexity.

### TUI-5: Accessibility Improvements
- **Files**: `src/util/a11y.rs` (new), `src/tui/components/component/`, `src/tui/app/mod.rs`
- **Status**: DEFERRED - Requires significant FocusManager architectural change
- **Note**: This is a complex refactor that would benefit from a design doc first

### LARGE-1: Virtual Scrolling for Messages
- **Files**: `src/tui/components/messages.rs`, `src/tui/components/messages/layout.rs` (new)
- **Status**: DEFERRED - High risk refactor
- **Risk**: HIGH - Scroll behavior deeply integrated with selection, search highlighting, streaming state

### LARGE-2: String Interning System
- **Files**: `src/util/interner.rs` (new), `src/provider/mod.rs`, `src/agent/`, `src/tool/`
- **Status**: DEFERRED - High risk architectural change
- **Risk**: HIGH - Lifetime complexity, static initialization order

---

## Known Code Issues

| Issue | Location | Priority | Status |
|-------|----------|----------|--------|
| ToolExecutor deprecated | `src/tool/executor.rs:8` | MEDIUM | MARKED DEPRECATED |
| Static CANONICAL_PATHS_CACHE never clears | `src/security/sandbox.rs:237` | MEDIUM | KNOWN ISSUE |
| TTS init() ignores providers | `src/tts/mod.rs:45-49` | LOW | KNOWN ISSUE |
| Worktree symlink detection | `src/worktree/mod.rs:69-88` | LOW | KNOWN ISSUE |
| OAuth replay protection TOCTOU | `src/mcp/auth.rs:318-332` | MEDIUM | KNOWN ISSUE |
| PermissionResponse struct unused | `src/permission/mod.rs:1141-1145` | LOW | KNOWN ISSUE |
| check_external_directory function unused | `src/permission/mod.rs:1237-1248` | LOW | KNOWN ISSUE |

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
| R0: Documentation-Only (~38 items) | Various architecture docs | 2026-05-27 |
| R1: Code Fixes (Low Risk) | MCP Debug, WebSocket auth | 2026-05-27 |
| R2: Code Fixes (Medium Risk) | Snapshot atomic write, SHA256, OAuth | 2026-05-27 |
| R3: Incomplete Implementation | MCP SSE, socket documented | 2026-05-27 |
| thinking_budget/reasoning_effort fields | Agent, ChatRequest structs | 2026-05-27 |

### Verified Codebase Facts

| Item | Value | Location |
|------|-------|----------|
| Tool count | 27 | `src/tool/mod.rs:89-119` |
| LSP server count | 39 | `src/lsp/server.rs:27-383` |
| InprocCoreClient fields | All wrapped in `Option<Arc<...>>` | `src/core/mod.rs:22-28` |
| ToolExecutor | DEPRECATED | `src/tool/executor.rs:8` |
| Plugin fuel logic | Fixed - all early returns correctly return fuel | `src/plugin/loader.rs` |
| CoreEvent mapping | Complete - all Subagent events mapped | `src/core/mod.rs` |
| CommandRegistry location | Line 72 | `src/tui/command.rs:72` |
| UiState fields | 26 fields | `src/tui/app/state/ui.rs:27-76` |
| Subagent event types | SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed | `src/bus/events.rs:120-141` |
| CoreEvent has subagent variants | SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed | `src/protocol/core.rs:244-268` |
| SessionCompacting hook | IS dispatched in AgentLoop::compact_if_needed() | `src/agent/loop.rs:1216-1220` |
| hook_timeout vs WASM_HOOK_TIMEOUT | Outer 5s, inner 30s | `src/plugin/service.rs:18`, `src/plugin/loader.rs:14` |
| Backoff formula | `2^i` (no jitter) | `src/provider/fallback.rs:107` |
| Client backoff formula | 1s, 2s, 4s (attempt 1,2,3) | `src/client/attach.rs:39` |
| Protocol version | 1 | `src/protocol/core.rs:3` |
| AppEvent count | 36 | `src/bus/events.rs:5-147` |
| Built-in command count | 46 | `src/tui/command.rs:79-182` |
| ToolDefCache | `(Option<String>, bool, bool, usize, u64, Vec<ToolDefinition>)` | `src/agent/loop.rs:60-67` |
| Timeline fields location | `timeline_visible` and `timeline_selected` in UiState | `src/tui/app/state/ui.rs:62-63` |
| Snapshot hash | Uses SHA256 consistently | `src/snapshot/mod.rs` |
| TTS stop() | Fixed - returns Err on pkill failure | `src/tts/mod.rs:85-107` |
| MCP connect_sse() | Dead code - documented | `src/mcp/remote.rs:698-740` |
| MCP run_socket() | Dead code - documented | `src/mcp/ide_server.rs:121-144` |

### Security Notes

- **Auth middleware allows requests without token when none configured**: At `src/server/middleware/auth.rs:37-39`, when `expected_token` is `None`, requests are allowed through. This may be intentional for development but should be reviewed for production.

### CoreRequest Handler Attention Points

- `CoreRequest` enum in `src/protocol/core.rs:50-175`
- InprocCoreClient handlers at `src/core/mod.rs:52-355` handle: TurnSubmit, SessionMessagesLoad, SessionMessageCounts, SessionCreate, SessionLoad, SessionAttach, etc.
- Variants falling through to `Ack`: Initialize, TurnCancel, TurnSteer, AgentSelect, ModelSelect

### Testing Commands

```bash
cargo build --all-features
cargo clippy --all-features -- -D warnings
cargo test --all-features -- --test-threads=1
cargo test tui::input
cargo test tui
cargo test messages
```

---

## Architecture Review Summary (2026-05-27)

Waves R0-R3 (54 items) completed via 25+ PRs:
- **R0**: 38 documentation-only fixes
- **R1**: 4 code fixes (low risk)
- **R2**: 4 code fixes (medium risk)
- **R3**: 4 incomplete implementations documented

Wave 4 (TUI-5, LARGE-1, LARGE-2) deferred due to architectural complexity.

*(End of file)*