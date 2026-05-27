# Implementation Plan

**Status**: REVISION 2 - CONSOLIDATED AND CORRECTED
**Last Updated**: 2026-05-27

---

## Executive Summary

All implementation waves (0-3) completed via 33+ PRs. The codebase has undergone significant hardening through security fixes, performance optimizations, and new features.

**Completed Waves**: Wave 0 (Quick Wins), Wave 1 (Critical Security), Wave 2 (High-Priority Infrastructure), Wave 3 (Medium-Priority Groups)

**Wave 4 (Large Refactors)**: DEFERRED - requires significant rewrites (12-16+ hours each)

**Wave 5 (Documentation & Minor Fixes)**: IN PROGRESS - see below

---

## Completed Implementation (April-May 2026 Sprint)

### Security Fixes
- IPv6 ULA (fc00::/7) and multicast (ff00::/8) blocking in SSRF module
- WASM fuel tracking with proper return after execution
- SSRF protection for `webfetch`, `websearch`, `codesearch`
- Symlink validation before canonicalization
- `env_clear()` and hardcoded minimal safe `PATH` in subprocess invocations
- No information leakage in `AppError` responses
- AES-256-GCM encryption module (`src/crypto/mod.rs`)
- Write tool TOCTOU fix - validate parent path before `create_dir_all()`
- Error redaction for LLM safety - `redact_local_paths()`
- `#![deny(unsafe_code)]` in lib.rs
- Upgrade module - semver validation, env_clear, direct curl
- WASM fuel bug fixed - `return_fuel()` uses `MAX_PLUGIN_FUEL_BUDGET`
- Critical unwrap removed in plugin execution

### Async/Mutex
- `TaskStore` uses `tokio::sync::Mutex` throughout
- LSP `DiagnosticsCollector` uses `tokio::sync::Mutex`
- `parking_lot::Mutex` replaced with `tokio::sync::Mutex` in `src/server/http.rs`
- `parking_lot::Mutex` replaced with `tokio::sync::Mutex` in `src/server/ws.rs`

### Performance
- HTTP client timeouts (60s request, 10s connect)
- Database `busy_timeout` (5s WAL)
- Per-tool timeouts in `bash`, `terminal`, `git` tools
- Token caching via `ModelDiscoveryService`
- Model-specific token estimation with `TokenizerType` (Claude: 1.4x, Gemini: 1.2x)
- `ToolRegistry` lazy initialization via `once_cell::Lazy` (`default_registry()`)
- `#[tracing::instrument]` added to `AgentLoop::run()`, `execute_tool_calls()`, and `CircuitBreaker::call()`

### Agent Capabilities
- Context compaction (adaptive truncation/summarization)
- `SubAgentPool` with bounded concurrency (5)
- Background task scheduling with SQLite persistence
- `denied_tools` enforcement - `ToolRegistry::filter_out()`
- `/compact` command wired to `TuiCommand::CompactSession`
- Subagent `max_depth` configuration with recursion limits (default: 3)

### TUI Features
- Background tasks UI via `/loop`, `/tasks`, `/task-del`
- Vim mode keybindings (hjkl navigation)
- Diff output colorization
- Shift+Tab toggles Plan/Build mode
- `/compact`, `/unshare`, `/export`, `/fork`, `/rename` commands properly wired

### TUI Input/Scrolling/Message Flow
- Shift-modified printable characters insert correctly
- Paste updates completion state, dialog paste isolation
- Scrolling fixes: `set_visible_height`, `total_rendered_lines()`, `is_at_bottom()` sentinel
- Navigate/scroll key separation
- Thinking tag parsing, color-coded message bars, mode-based coloring

---

## Wave 5: Documentation & Minor Fixes (IN PROGRESS)

### Implementation Waves (Parallelizable)

#### W5-Phase 1: Independent Code Fixes (3 parallel agents)

| ID | Issue | Location | Action |
|----|-------|----------|--------|
| W5-2 | Session exports missing | `src/session/mod.rs:28` | Add `compute_checksum`, `create_working_file`, `verify_file` to `pub use` |
| W5-5 | TUI theme count mismatch | `src/tui/theme.rs:8` | Change comment from "31" to "33" (actual theme count) |
| W5-3 | Snapshot hash inconsistency | `src/snapshot/mod.rs:431` | Change MD5 to SHA256 in `collect_files_sync()` |

#### W5-Phase 2: ToolExecutor Investigation (1 agent)

| ID | Issue | Location | Action |
|----|-------|----------|--------|
| W5-4 | ToolExecutor exists but unused | `src/tool/executor.rs:8` | Investigate why created but not used; decide to integrate or deprecate |

#### W5-Phase 3: Critical Bug Fix (1 agent - MUST BE DONE FIRST)

| ID | Issue | Location | Details |
|----|-------|----------|---------|
| W5-1 | Question tool deadlocks in exec mode | `src/exec.rs:121` | No handler for `question_rx` responses. AgentLoop waits indefinitely if question tool invoked in exec mode. Add timeout or handler. Requires understanding of `AgentLoop::setup_question_channel()`. |

### Priority 2: Documentation Fixes

#### W5-Phase 4: Architecture Doc Corrections - Core/Protocol/Error (3 parallel agents)

| ID | File | Issue | Location |
|----|------|-------|----------|
| W5-6 | `architecture/core.md` | InprocCoreClient fields wrapped in `Option<Arc<T>>` | `src/core/mod.rs:22-28` |
| W5-7 | `architecture/core.md` | Add note: Snapshot events defined but not published via `map_app_event_to_core_event` | `src/core/mod.rs:728-841` |
| W5-8 | `architecture/error.md` | Line numbers incorrect; missing `ServerRuntimeError IntoResponse`, `ProviderError::api()` docs | `src/error.rs` |
| W5-9 | `architecture/permission.md` | Mode tool fix: `write` is in `restricted_tools` but docs incorrectly list it as allowed | `modes.rs:171` |
| W5-10 | `architecture/permission.md` | `PermissionResponse` at lines 1141-1145 (not 61-71) | `src/permission/mod.rs:1141-1145` |
| W5-11 | `architecture/protocol.md` | CoreEvent count: 20 → 21 | `src/protocol/core.rs:179` |
| W5-12 | `architecture/protocol.md` | Turn events: 5 → 7 (add `TurnReasoningDelta`, `TurnCompleted`) | `src/protocol/core.rs` |
| W5-13 | `architecture/protocol.md` | Server-to-Client count: 9 → 10 | `src/protocol/core.rs` |
| W5-14 | `architecture/command.md` | Stale bugs table contradicts historical notes; line numbers 203-205 | `src/command/` |

#### W5-Phase 5: Architecture Doc Corrections - TUI/Overview/LSP (2 parallel agents)

| ID | File | Issue | Location |
|----|------|-------|----------|
| W5-15 | `architecture/overview.md` | Tool count: "29" → "26" | `src/tool/mod.rs:89-119` |
| W5-16 | `architecture/overview.md` | Remove "multiedit" from tool list - tool exists but NOT registered in `with_defaults()` | `src/tool/mod.rs:89-119` |
| W5-36 | `architecture/overview.md` | `pty_session/` → `shell_session/` in module references | `src/shell_session/` |
| W5-37 | `architecture/overview.md` | Dialog::Info doesn't exist in Dialog enum | `src/tui/app/types.rs:2-25` |
| W5-38 | `architecture/tui.md` | UiState code block shows 21 fields, actual is 25 | `src/tui/app/state/ui.rs:27-74` |
| W5-18 | `architecture/lsp.md` | Server count: 39 → 40 (verified at `src/lsp/server.rs:27-375`) | `src/lsp/server.rs:27-375` |
| W5-19 | `architecture/lsp.md` | Extension count: "50+" → "~80" | `src/lsp/` |

#### W5-Phase 6: Architecture Doc Corrections - Provider/MCP/Skills (2 parallel agents)

| ID | File | Issue | Location |
|----|------|-------|----------|
| W5-17 | `architecture/provider.md` | HashMap vs DashMap: `catalog.rs` uses `HashMap`, not `DashMap` | `src/provider/` |
| W5-20 | `architecture/mcp.md` | JSON field is `type`, not `server_type` | `src/mcp/` |
| W5-39 | `architecture/skills.md` | Document `resources` field in SkillTool output, `SkillIndex` Default impl, `SkillFrontmatter` struct | `src/tool/skill.rs` |
| W5-28 | `architecture/skills.md` | Create missing snapshot skill guide | `.opencode/skills/snapshot/SKILL.md` |

#### W5-Phase 7: Architecture Doc Corrections - Remaining Modules (2 parallel agents)

| ID | File | Issue | Location |
|----|------|-------|----------|
| W5-21 | `architecture/resilience.md` | Fix state transition diagram wording | `circuit.rs:114-127` |
| W5-40 | `architecture/resilience.md` | Document missing HalfOpen timeout check in `call()` method | `circuit.rs:114-127` |
| W5-22 | `architecture/server.md` | mDNS module undocumented | `src/server/mdns.rs` |
| W5-23 | `architecture/server.md` | Clarify `RenderFrame` direction (Client→Server) | `src/server/` |
| W5-24 | `architecture/session.md` | Field order note for `timestamp` vs `session_id` | `src/session/` |
| W5-29 | `architecture/compaction.md` | Threshold clarity: ">6 messages" → "7 or more" | `src/agent/compaction.rs:584` |
| W5-30 | `architecture/config.md` | Field reference: `compaction_threshold` → `compaction.threshold` | `schema.rs:374` |
| W5-31 | `architecture/util.md` | `stat_core.rs` → `metrics.rs` | `src/util/` |
| W5-32 | `architecture/worktree.md` | Add `is_git_file()` to See Also | `workspace.rs:36,56` |
| W5-33 | `architecture/pty_session.md` | Rename to `architecture/shell_session.md`, update `Pty*` → `Shell*` references | `src/shell_session/` |
| W5-34 | `architecture/ide.md` | `run_stdio()` line numbers 125-130 → 78-119 | `src/ide/ide_server.rs:78-119` |
| W5-35 | `architecture/ide.md` | `run_socket()` line numbers 138-149 → 121-144; document `handle_connection()` and `clone_for_connection()` | `src/ide/ide_server.rs:121-194` |

#### W5-Phase 8: SKILL.md Corrections (1 agent)

| ID | File | Issue | Location |
|----|------|-------|----------|
| W5-27 | `.opencode/skills/exec/SKILL.md` | Timeout claim incorrect (no 300s timeout exists in exec mode) | `src/exec.rs:121` |
| W5-41 | `.opencode/skills/hooks/SKILL.md` | Document `WASM_HOOK_TIMEOUT` (outer 5s, inner 30s); error format is in `service.rs` not `hooks.rs` | `src/plugin/service.rs:18`, `src/plugin/loader.rs:14` |

#### W5-Phase 9: AGENTS.md Corrections (can be done alongside documentation)

| ID | File | Issue | Location |
|----|------|-------|----------|
| W5-25 | `AGENTS.md` | LSP count: 39 → 40 (ALREADY FIXED per verified facts) | `src/lsp/server.rs:27-375` |
| W5-26 | `AGENTS.md` | Module naming: `pty_session/` → `shell_session/` in Quick Reference | `src/shell_session/` |

### Wave 5 Implementation Notes

- **W5-Phase 1** (W5-2, W5-5, W5-3) - 3 parallel agents, independent fixes
- **W5-Phase 3** (W5-1) - CRITICAL BUG, do first before parallel work
- **W5-Phase 4-9** - Documentation fixes, 9 parallel phases with 2-3 agents each
- **W5-Phase 2** (W5-4) - Requires research, may produce additional code fix items

### Wave 4: Large Refactors (DEFERRED - 12-16+ hours each)

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
| Snapshot hash inconsistency | `src/snapshot/mod.rs:431` | MEDIUM |
| ToolExecutor exists but unused | `src/tool/executor.rs:8` | MEDIUM |
| Static CANONICAL_PATHS_CACHE never clears | `src/security/sandbox.rs:237` | MEDIUM |
| TTS stop() returns Ok on failure | `src/tts/mod.rs:85-103` | LOW |
| TTS init() ignores providers | `src/tts/mod.rs:45-49` | LOW |
| Histogram unbounded memory | `src/util/metrics.rs:122-124` | LOW |
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
| LSP server count | 40 | `src/lsp/server.rs:27-375` |
| InprocCoreClient fields | All wrapped in `Option<Arc<...>>` | `src/core/mod.rs:22-28` |
| ToolExecutor | NOT integrated - exists but unused | `src/tool/executor.rs:8` |
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
| Built-in command count | 39 | `src/tui/command.rs:79-161` |
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

## Consolidated Statistics

| Metric | Value |
|--------|-------|
| Waves 0-3 Completed | ✅ All via 33+ PRs |
| Wave 4 (Large Refactors) | ⏳ DEFERRED |
| Wave 5 (Docs & Minor Fixes) | ⏳ IN PROGRESS (43 items) |
| TUI Enhancement | ⏳ MOSTLY DEFERRED |
| Agent Capabilities | ⏳ PARTIAL (4/8 complete) |
| Mode/Exec Features | ✅ Complete (MODE-1, EXEC-1) |
| Documentation | ⏳ FUTURE |

---

*(End of file)*