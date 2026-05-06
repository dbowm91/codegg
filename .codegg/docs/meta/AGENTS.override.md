# Meta Override

Cross-cutting concerns that span multiple modules.

## Recent Updates (2026-05-01)

### Wave 0: Quick Wins - COMPLETE
- QW-3: Added configurable `debounce_duration_ms` to `WatcherConfig` schema
- QW-4: Added audit log (`tracing::info!`) to `filter_out()` in `tool/mod.rs`
- QW-6: Added `doomloop_threshold` to `PermissionConfig` schema
- QW-7: Added content hash before reload in `config/watcher.rs`

### Wave 1: Critical Security - COMPLETE
- CRIT-3: Removed duplicate SSRF code from `webfetch.rs`, now uses centralized `security/ssrf` module

### Wave 2: High-Priority Infrastructure - COMPLETE
- HIGH-3: Refactored `SubAgentSpawner::send()` to use async `.send()` instead of `blocking_send()`
- A-4: Restricted CORS methods in `server/http.rs` to GET, POST, PUT, DELETE

### Wave 3: Medium-Priority Groups - Partial COMPLETE
- A-2: Added `${}`, `$VAR`, `<(` patterns to `BLOCKED_PATTERNS` in `bash.rs`
- A-4: Restricted CORS methods in `server/http.rs`
- F-1: Added `BLOCKED_PATTERNS` to `terminal.rs` for command security
- F-2: Fixed `check_command_security()` in `bash.rs` to check entire command string
- B-4: Created `provider/cache.rs` for LLM response caching
- D-3: Created `tool/catalog.rs` for on-demand tool discovery

---

## Implementation Roadmap (Waves)

All Waves 0-8 are complete. See `plans/plan.md` for detailed status:

- **Wave 0 (Build & Stability)**: ✅ Unsafe fix (socket2 in mdns.rs), DoomLoop consecutive logic, Redis logic fix, dead code removal (render.rs), DB pool standardization
- **Wave 1 (Security & Infra)**: ✅ API key encryption, SSRF unification (mcp/remote.rs uses security/ssrf), MCP auto-reconnect with connection manager, Google Header Auth (x-goog-api-key), Config debounce (500ms)
- **Wave 2 (Performance)**: ✅ TUI Render Debouncing (60fps), Regex pre-compilation verified, SQLite PRAGMA tuning, String Arc Migration (✅ COMPLETED - ToolCall, Message, ContentPart, ChatEvent now use Arc<String>)
- **Wave 3 (Capabilities)**: ✅ LLM summarization (async with fallback), Model Auto-Routing classifier, Multi-agent teams via file-based inbox, Review tool, On-Demand Tool Search (deferred)
- **Wave 4 (TUI & UX)**: ✅ DiffViewer widget, Desktop notifications (notify-rust), Image support (feature-gated stub)
- **Wave 5 (Advanced Features)**: ✅ Mode system (Review/Debug/Docs), Exec mode (`opencode exec --json`), Landlock sandboxing
- **Wave 6 (Docs & Quality)**: ✅ Documentation (LSP, MCP, Plugins, Troubleshooting), Integration tests (unit tests exist, e2e resolved - non-functional tests removed, exec mode for non-interactive CI)
- **Wave 7 (TUI Architecture)**: ✅ COMPLETE - TuiMsg message bus, Component trait, FocusManager, explicit TuiMsg contracts for all dialogs, FocusManager helper methods (push_dialog, close_dialog, replace_dialog). HelpDialog and InfoDialog migrated to Component pattern.
- **Wave 8 (TUI Usability)**: ✅ COMPLETE - All packets 1-9 implemented: Unified Modal Ownership, Prompt Editor Upgrade, Footer Redesign, Search Auto-Scroll, Safer Destructive Actions, Tool Call Summaries, Permission Dialog Risk Clarity, Workspace Awareness, TUI Test Harness.
- **Agent Harness (2026-05-01)**: ✅ ALL 11 PACKETS COMPLETE - AgentLoop harness tests (ScriptedProvider, echo_args/slow_echo tools), Message::Assistant now carries tool_calls, tool result ordering by original index, SubAgentSpawner send_async(), SubAgentPool proper task spawning, permission/question paths tested, retry semantics tested, follow-up contract tested, provider transcript golden tests, compaction safety tests, event bus observability tests, harness documentation. New helpers: assert_messages_have_roles(), assert_assistant_has_tool_call(), assert_tool_result_with_id(), assert_assistant_tool_call_precedes_result(), assert_no_orphan_tool_results().

## Testing Guidance

### Test Types
- **Unit Tests**: Individual functions/components, inline `#[cfg(test)]` modules or `src/<module>/tests/`
- **Integration Tests**: Cross-module tests in `tests/` directory (156+ TUI tests passing)
- **E2E Tests**: Resolved via non-interactive `exec` mode (see `exec` skill) for CI/CD; TUI E2E with PTY deferred due to infrastructure complexity

### Running Tests
- All tests: `cargo test`
- Specific test: `cargo test --test <test_name>`
- Non-interactive CI test: `opencode exec --json '{"prompt": "hello"}' --json-output`

## Code Quality (2026-05-01)

- **Clippy**: All warnings resolved (`cargo clippy --all-features -- -D warnings` passes)
- **Build**: `cargo build --all-features` passes
- **Tests**: All key test suites pass (agent_loop_harness, provider_transcripts, subagent, compaction)