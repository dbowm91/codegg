# Implementation Plan

**Status**: ACTIVE - Wave 1 complete, Waves 2-5 remaining items
**Last Updated**: 2026-05-27

---

## Wave 2: Graphical/Visual (Independent, 3-5 hours each)

### TUI-3: Image Attachment Support
- **Files**: `Cargo.toml`, `src/tui/components/image.rs`, `src/tui/components/image_preview.rs` (new)
- **Status**: NOT IMPLEMENTED - ImageViewer methods are stubs
- **Prerequisites**: Optional `image` crate with `png`, `jpeg`, `gif`, `webp` features
- **Implementation**:
  1. If `image` feature disabled, ImageViewer shows placeholder text explaining images require full features
  2. If enabled:
     - Create `image_preview.rs` widget that calls `image::open()` then renders via `ratatui::widgets::Widget`
     - Handle image URLs (download via `reqwest`), base64 (decode), or local paths
     - Support inline preview in message thread with aspect-ratio preservation
     - Add mouse scroll zoom in preview mode
- **Parallel**: Can proceed independently
- **Test**: `cargo test --features image tui`

### AGENT-5: Image Generation
- **Files**: `src/tool/image.rs` (new)
- **Status**: NOT IMPLEMENTED
- **Implementation**:
  1. Create `ImageTool` struct implementing `Tool` trait
  2. Accept prompt (required), model (default "dall-e-3"), size (default 1024x1024), quality (default "standard")
  3. Wire to OpenAI `/v1/images/generations` endpoint (GPT Image API)
  4. Return URL or base64 data depending on response format
  5. Add `image` tool to `ToolRegistry::with_defaults()`
- **Parallel**: Independent of TUI-3 but shares conceptually
- **Test**: `cargo test tool`

---

## Wave 3: External Integrations (3-6 hours each)

### AGENT-6: GitHub Integration
- **Files**: `src/command/github/`, `src/mcp/mod.rs`
- **Status**: NOT IMPLEMENTED
- **Implementation**:
  1. Create GitHub MCP server configuration in `src/config/schema.rs`
  2. Add `/pr` slash command: list PRs, view PR diff, post comments
  3. Add `/issue` slash command: list issues, create issue, view issue details
  4. Wire MCP connection to GitHub MCP server (mcp.github.com or self-hosted)
  5. Handle OAuth for GitHub API if required
- **Parallel**: Can proceed independently but MCP must be working first
- **Test**: `cargo test command`

### EXEC-2: Session Analytics & Cost Tracking
- **Files**: `src/session/mod.rs`, `src/config/schema.rs`, `src/command/exec.rs`
- **Status**: NOT IMPLEMENTED
- **Implementation**:
  1. Add DB migrations for `usage` table: session_id, provider, model, input_tokens, output_tokens, cached_tokens, cost_usd, timestamp
  2. Modify `AgentLoop::process_finish()` to emit usage to DB
  3. Refactor pricing to service in `src/util/pricing.rs` (hardcoded rates per provider)
  4. Add `/stats` command showing session costs, token counts, provider breakdown
  5. Add `/usage` command for detailed usage over time
- **Parallel**: Can proceed independently
- **Test**: `cargo test session`

---

## Wave 4: Large Refactors (DEFERRED - 12-16+ hours each)

### LARGE-1: Virtual Scrolling for Messages
- **Files**: `src/tui/components/messages.rs`
- **Status**: DEFERRED
- **Implementation**:
  1. Pre-calculate line heights using `measure_text()` returns
  2. Binary search for visible range given scroll position
  3. Cache rendered lines in `HashMap<usize, Vec<Line>>`
  4. Replace current scroll implementation with virtual list widget
  5. Handle dynamic content changes (insert mid-list)
- **Notes**: Performance-critical for sessions with 10k+ messages
- **Parallel**: Standalone, high-risk refactor

### LARGE-2: String Interning System
- **Files**: `src/provider/mod.rs`, `src/agent/`, `src/tool/`
- **Status**: DEFERRED
- **Implementation**:
  1. Create `StringInterner` using `DashMap<String, u64>` with reverse `Vec<String>`
  2. Apply to repeated strings: system prompts, tool definitions, common errors
  3. Add `pub fn intern(&self, s: &str) -> u64` and `pub fn get(&self, id: u64) -> &str`
  4. Profile first to identify high-frequency string allocations
- **Notes**: Reduces memory allocations for repeated constant strings
- **Parallel**: Standalone, architectural change

---

## Wave 5: Git Integration (DEFERRED - 4-6 hours)

### GIT-1: Enhanced Git Integration
- **Files**: `src/git/mod.rs` (new)
- **Status**: NOT IMPLEMENTED
- **Implementation**:
  1. Create `src/git/mod.rs` with `GitSession` struct
  2. Inject branch name and git status into system prompt
  3. Implement `/checkpoint` command - create `@checkpoint/2024-01-15-10:30` references
  4. Auto-worktree per session: detect git worktrees, create/cleanup per session id
  5. Integrate with existing `worktree/` module
- **Parallel**: Independent but builds on worktree knowledge
- **Test**: `cargo test worktree`

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

### Verified Codebase Facts

| Item | Value | Location |
|------|-------|----------|
| Tool count | 26 | `src/tool/mod.rs:89-119` |
| LSP server count | 39 | `src/lsp/server.rs:27-383` |
|InprocCoreClient fields | All wrapped in `Option<Arc<...>>` | `src/core/mod.rs:22-28` |
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
| Built-in command count | 42 (includes /tts) | `src/tui/command.rs:79-165` |
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
| Wave 2 | TUI-3 (Image Support), AGENT-5 (Image Generation) | 6-10 hours | Ready |
| Wave 3 | AGENT-6 (GitHub), EXEC-2 (Analytics) | 6-12 hours | Ready |
| Wave 4 | LARGE-1 (Virtual Scroll), LARGE-2 (String Interning) | 24-32 hours | Deferred |
| Wave 5 | GIT-1 (Enhanced Git) | 4-6 hours | Ready |
| Completed | TTS auto-stop, /tts command | N/A | ✅ DONE |

*(End of file)*
