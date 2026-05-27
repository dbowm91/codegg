# Implementation Plan

**Status**: DEFERRED ITEMS ONLY
**Last Updated**: 2026-05-27

---

## Wave 4: Large Refactors (DEFERRED - 12-16+ hours each)

#### LARGE-1: Virtual Scrolling for Messages
- **Files**: `src/tui/components/messages.rs`
- **Action**: Pre-calculate line heights, binary search for visible range, cache rendered lines, add virtual list widget

#### LARGE-2: String Interning System
- **Files**: `src/provider/mod.rs`, `src/agent/`, `src/tool/`
- **Action**: Create `StringInterner` using `DashMap`, apply to repeated strings

---

## TUI Enhancement Features (Future/Partial)

| Feature | Priority | Status |
|---------|----------|--------|
| Inline Diff Rendering | HIGH | ✅ IMPLEMENTED |
| Native Desktop Notifications | HIGH | Partial - manager exists, not wired to events |
| Image Attachment Support | HIGH | NOT IMPLEMENTED |
| Streaming UX Enhancements | MEDIUM | Partial - basic streaming exists, missing features |
| Accessibility Improvements | MEDIUM | Partial - focus indicators, missing screen reader |
| Mouse Support Enhancements | LOW | ✅ MOSTLY IMPLEMENTED |

#### TUI-2: Native Desktop Notifications
- **Files**: `Cargo.toml`, `src/tui/components/notification.rs`, `src/config/schema.rs`
- **Status**: Partial - `NotificationManager` exists but NOT wired to `AgentFinished`/`SubagentCompleted`
- **Action**: Wire `AppEvent::AgentFinished` and `AppEvent::SubagentCompleted` to `NotificationManager::send()`

#### TUI-3: Image Attachment Support
- **Files**: `Cargo.toml`, `src/tui/components/image.rs` (stub)
- **Status**: NOT IMPLEMENTED - dependency optional/feature-gated, ImageViewer is stub
- **Action**: Implement `image_preview.rs` widget, render images in messages

#### TUI-4: Streaming UX Enhancements
- **Status**: Partial - streaming state exists, newline-gated commit, resize debounce missing
- **Action**: Add 75ms resize debounce, complete newline-gated commit

#### TUI-5: Accessibility Improvements
- **Status**: Partial - focus indicators exist, global Tab handler and screen reader not implemented
- **Action**: Implement global Tab and Shift+Tab handler, create `src/util/a11y.rs`

---

## Agent Capability Features (Future)

| Feature | Priority | Status |
|---------|----------|--------|
| AGENT-1: Context Summarization | HIGH | ✅ IMPLEMENTED |
| AGENT-2: Review Command | HIGH | ✅ COMPLETE |
| AGENT-3: Multi-Agent Teams | HIGH | ✅ COMPLETE |
| AGENT-4: Tool Search | MEDIUM | ✅ COMPLETE |
| AGENT-5: Image Generation | MEDIUM | NOT IMPLEMENTED |
| AGENT-6: GitHub Integration | MEDIUM | NOT IMPLEMENTED |
| AGENT-7: Sandbox Security Modes | MEDIUM | PARTIAL - Landlock only |
| AGENT-8: TTS/Voice Integration | LOW | PARTIAL - basic speak/stop |

#### AGENT-5: Image Generation
- **Files**: `src/tool/image.rs` (new)
- **Status**: NOT IMPLEMENTED
- **Action**: Create `ImageTool` struct, integrate GPT Image API

#### AGENT-6: GitHub Integration
- **Files**: `src/command/github/` (new)
- **Status**: NOT IMPLEMENTED
- **Action**: Add GitHub MCP configuration, `/pr` and `/issue` slash commands

#### AGENT-7: Sandbox Security Modes
- **Status**: PARTIAL - Landlock only (Linux), no separate sandbox module
- **Action**: Implement three-mode sandbox (read-only, workspace-write, danger-full-access)

#### AGENT-8: TTS/Voice Integration
- **Status**: PARTIAL - only `speak()` and `stop()` using macOS `say` command
- **Action**: Hook Stop event for TTS, add STT voice input

---

## Mode/Exec Features

| Feature | Status |
|---------|--------|
| MODE-1: Extended Mode System (5 modes) | ✅ COMPLETE |
| EXEC-1: Non-Interactive Exec Mode | ✅ COMPLETE |
| EXEC-2: Session Analytics & Cost Tracking | NOT IMPLEMENTED |
| EXEC-3: Token Caching Display | NOT IMPLEMENTED |

#### EXEC-2: Session Analytics & Cost Tracking
- **Action**: Add DB migrations for usage, emit usage to DB, refactor pricing to service, add `/stats` command

#### EXEC-3: Token Caching Display
- **Action**: Parse `prompt_tokens_details.cached_tokens` (OpenAI), `cache_read_input_tokens` (Anthropic)

---

## Model & Git Features (Future)

| Feature | Priority | Status |
|---------|----------|--------|
| MODEL-1: Model Variants with Thinking | MEDIUM | PARTIAL |
| MODEL-2: Auto-Routing Model Selection | MEDIUM | ✅ COMPLETE |
| GIT-1: Enhanced Git Integration | MEDIUM | NOT IMPLEMENTED |

#### MODEL-1: Model Variants with Thinking
- **Status**: PARTIAL - basic ModelVariant exists, thinking params not implemented
- **Action**: Extend for thinking/reasoning, add Anthropic thinking param, OpenAI reasoning_effort

#### GIT-1: Enhanced Git Integration
- **Files**: `src/git/mod.rs` (new)
- **Status**: NOT IMPLEMENTED
- **Action**: Branch/status injection, checkpoint system, auto-worktree per session

---

## Documentation (Future)

See `docs/` directory for planned documentation:
- Conceptual guides (agents-vs-skills, mcp, lsp, sessions, permissions, plugins)
- Reference documentation (configuration, tools, commands, environment)
- Workflow guides (quick-start, debugging, code-review, refactoring, tdd)
- Operations & troubleshooting

---

## Known Code Issues (Deferred/Low Priority)

These issues are documented but deferred for later attention:

| Issue | Location | Priority |
|-------|----------|----------|
| Snapshot hash inconsistency | `src/snapshot/mod.rs:431` uses MD5 | MEDIUM |
| ToolExecutor exists but unused | `src/tool/executor.rs:8` | MEDIUM |
| Static CANONICAL_PATHS_CACHE never clears | `src/security/sandbox.rs:237` | MEDIUM |
| TTS stop() returns Ok on failure | `src/tts/mod.rs:85-103` | LOW |
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

11. **Exec Mode Question Handling**: `src/exec.rs:121` has no question_rx handler - question tool will deadlock in exec mode. Fix requires understanding AgentLoop::setup_question_channel().

### Implementation Patterns

- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT use `await` when calling these.

- **Registry Limitations**: Permission IDs are in format `{tool_call_id}-{tool_name}`, not `{session_id}-...`. `get_pending_permissions_for_session()` and `get_pending_questions_for_session()` cannot properly filter by session_id.

- **Registration-before-publish pattern**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event.

### Verified Codebase Facts

| Item | Value | Location |
|------|-------|----------|
| Tool count | 26 | `src/tool/mod.rs:89-119` |
| LSP server count | 39 | `src/lsp/server.rs:27-383` |
| InprocCoreClient fields | All wrapped in `Option<Arc<...>>` | `src/core/mod.rs:22-28` |
| ToolExecutor | DEPRECATED - exists but unused | `src/tool/executor.rs:8` |
| Plugin fuel logic | Fixed - all early returns correctly return fuel | `src/plugin/loader.rs` |
| CoreEvent mapping | Complete - all events including Subagent* properly mapped | `src/core/mod.rs` |
| CommandRegistry location | Line 72 | `src/tui/command.rs:72` |
| UiState fields | All documented fields present (25 fields) | `src/tui/app/state/ui.rs:27-74` |
| Subagent event types | SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed | `src/bus/events.rs:120-141` |
| CoreEvent has subagent variants | SubagentStarted, SubagentCompleted | `src/protocol/core.rs:244,256` |
| map_app_event_to_core_event | All Subagent events mapped | `src/core/mod.rs` |
| SessionCompacting hook | IS dispatched in AgentLoop::compact_if_needed() | `src/agent/loop.rs:1197-1201` |
| hook_timeout vs WASM_HOOK_TIMEOUT | Outer 5s, inner 30s | `src/plugin/service.rs:18`, `src/plugin/loader.rs:14` |
| Backoff formula | `2^i` (no jitter) | `src/provider/fallback.rs:107` |
| Client backoff formula | 1s, 2s, 4s (attempt 1,2,3) | `src/client/attach.rs:39` |
| Protocol version | 1 | `src/protocol/core.rs:3` |
| AppEvent count | 36 | `src/bus/events.rs:5-147` |
| Built-in command count | 41 | `src/tui/command.rs:79-162` |
| ToolDefCache | `(Option<String>, bool, bool, usize, u64, Vec<ToolDefinition>)` | `src/agent/loop.rs:60-67` |
| Timeline fields location | `timeline_visible` and `timeline_selected` are in `App` struct, NOT `UiState` | `src/tui/app/mod.rs:232-233` |
| Snapshot hash inconsistency | `collect_files_sync` uses MD5 for non-empty files, SHA256 elsewhere | `src/snapshot/mod.rs:431` |

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

| Category | Status |
|----------|--------|
| Wave 4 (Large Refactors) | ⏳ DEFERRED |
| TUI Enhancement | ⏳ PARTIAL (2/6 complete) |
| Agent Capabilities | ⏳ PARTIAL (4/8 complete) |
| Mode/Exec Features | ⏳ PARTIAL (2/4 complete) |
| Model & Git Features | ⏳ PARTIAL (1/3 complete) |
| Documentation | ⏳ FUTURE |

*(End of file)*
