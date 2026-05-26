# Implementation Plan

**Status**: POST-CONSOLIDATION
**Last Updated**: 2026-05-26

---

## Executive Summary

All implementation waves (0-3) have been completed via 33+ PRs. The codebase has undergone significant hardening through security fixes, performance optimizations, and new features.

**Completed Waves**: Wave 0 (Quick Wins), Wave 1 (Critical Security), Wave 2 (High-Priority Infrastructure), Wave 3 (Medium-Priority Groups)

**Remaining Items**: Deferred features classified as large refactors, TUI enhancements, agent capability features, and documentation work.

---

## Completed Implementation (April-May 2026 Sprint)

### Security Fixes
- IPv6 ULA (fc00::/7) and multicast (ff00::/8) blocking in SSRF module.
- WASM fuel tracking with proper return after execution.
- SSRF protection for `webfetch`, `websearch`, `codesearch`.
- Symlink validation before canonicalization.
- `env_clear()` and hardcoded minimal safe `PATH` in subprocess invocations.
- No information leakage in `AppError` responses.
- AES-256-GCM encryption module (`src/crypto/mod.rs`).
- Write tool TOCTOU fix - validate parent path before `create_dir_all()`.
- Error redaction for LLM safety - `redact_local_paths()`.
- `#![deny(unsafe_code)]` in lib.rs.
- Upgrade module - semver validation, env_clear, direct curl.
- WASM fuel bug fixed - `return_fuel()` uses `MAX_PLUGIN_FUEL_BUDGET`.
- Critical unwrap removed in plugin execution.

### Async/Mutex
- `TaskStore` uses `tokio::sync::Mutex` throughout.
- LSP `DiagnosticsCollector` uses `tokio::sync::Mutex`.
- `parking_lot::Mutex` replaced with `tokio::sync::Mutex` in `src/server/http.rs`.
- `parking_lot::Mutex` replaced with `tokio::sync::Mutex` in `src/server/ws.rs`.

### Performance
- HTTP client timeouts (60s request, 10s connect).
- Database `busy_timeout` (5s WAL).
- Per-tool timeouts in `bash`, `terminal`, `git` tools.
- Token caching via `ModelDiscoveryService`.
- Model-specific token estimation with `TokenizerType` (Claude: 1.4x, Gemini: 1.2x).
- `ToolRegistry` lazy initialization via `once_cell::Lazy` (`default_registry()`).
- `#[tracing::instrument]` added to `AgentLoop::run()`, `execute_tool_calls()`, and `CircuitBreaker::call()`.

### Agent Capabilities
- Context compaction (adaptive truncation/summarization).
- `SubAgentPool` with bounded concurrency (5).
- Background task scheduling with SQLite persistence.
- `denied_tools` enforcement - `ToolRegistry::filter_out()`.
- `/compact` command wired to `TuiCommand::CompactSession`.
- Subagent `max_depth` configuration with recursion limits (default: 3).

### TUI Features
- Background tasks UI via `/loop`, `/tasks`, `/task-del`.
- Vim mode keybindings (hjkl navigation).
- Diff output colorization.
- Shift+Tab toggles Plan/Build mode.
- `/compact`, `/unshare`, `/export`, `/fork`, `/rename` commands properly wired.

### TUI Input/Scrolling/Message Flow (Completed May 2026)
- Shift-modified printable characters insert correctly.
- Paste updates completion state, dialog paste isolation.
- Scrolling fixes: `set_visible_height`, `total_rendered_lines()`, `is_at_bottom()` sentinel.
- Navigate/scroll key separation.
- Thinking tag parsing, color-coded message bars, mode-based coloring.

### Waves 0-3 Summary
| Wave | Items | Status |
|------|-------|--------|
| Wave 0: Quick Wins | QW-3 through QW-15 (15 items) | ✅ COMPLETE |
| Wave 1: Critical Security | CRIT-1 through CRIT-6 (6 items) | ✅ COMPLETE |
| Wave 2: High-Priority Infrastructure | HIGH-1 through HIGH-7 (7 items) | ✅ COMPLETE |
| Wave 3: Medium-Priority Groups | GROUP-A through GROUP-I (40+ items) | ✅ COMPLETE |
| **Total PRs** | **33+ PRs** | ✅ |

---

## Deferred Items

### Wave 4: Large Refactors (2+ weeks each)

These are large efforts requiring significant rewrites. Deferred unless absolutely necessary.

#### LARGE-1: Virtual Scrolling for Messages
- **Files**: `src/tui/components/messages.rs`
- **Effort**: 12-16 hours
- **Action**: Pre-calculate line heights, binary search for visible range, cache rendered lines, add virtual list widget

#### LARGE-2: String Interning System
- **Files**: `src/provider/mod.rs`, `src/agent/`, `src/tool/`
- **Effort**: 2-3 days
- **Action**: Create `StringInterner` using `DashMap`, apply to repeated strings

---

### TUI Enhancement Features (Future)

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
- **Action**: Implement global Tab/Shift+Tab handler, create `src/util/a11y.rs`

---

### Agent Capability Features (Future)

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

### Mode/Exec Features

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

### Model & Git Features (Future)

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

### Documentation (Future)

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

8. **Tool Path Validation**: `validate_path()` in `src/tool/util.rs` checks symlinks and verifies paths. `check_path_for_symlinks()` rejects symlink components.

9. **Write Tool TOCTOU Fix**: Parent path validated BEFORE `create_dir_all()`.

10. **Token Estimation**: `estimate_tokens_sync()` uses `TokenizerType` for model-specific multipliers. Claude: 1.4x, Gemini: 1.2x.

### Implementation Patterns

- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT use `await` when calling these.

- **Registry Limitations**: Permission IDs are in format `{tool_call_id}-{tool_name}`, not `{session_id}-...`. `get_pending_permissions_for_session()` and `get_pending_questions_for_session()` cannot properly filter by session_id.

- **Registration-before-publish pattern**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event.

### Testing Commands

```bash
cargo build --all-features
cargo clippy --all-features -- -D warnings
cargo test --all-features
cargo test tui::input
cargo test tui
cargo test messages
```

---

## Consolidated Statistics

| Metric | Value |
|--------|-------|
| Waves 0-3 Completed | ✅ All via 33+ PRs |
| Wave 4 (Large Refactors) | ⏳ DEFERRED |
| TUI Enhancement | ⏳ MOSTLY DEFERRED |
| Agent Capabilities | ⏳ PARTIAL (4/8 complete) |
| Mode/Exec Features | ✅ Complete (MODE-1, EXEC-1) |
| Documentation | ⏳ FUTURE |

---

*(End of file)*
