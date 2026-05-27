# Implementation Plan

**Status**: ACTIVE - Implementation completed (all R0-R3 items done)
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

## Architecture Review Items (2026-05-27)

This section consolidates ~54 items identified during architecture review sessions (batches 1-5). Items are grouped by module and organized into waves for parallelization.

### Wave Structure

| Wave | Focus | Items | Type | Parallel Potential |
|------|-------|-------|------|-------------------|
| R0 | Documentation-Only Fixes | ~38 items | Docs | All fully parallel |
| R1 | Code Fixes (Low Risk) | ~6 items | Code/Docs | All fully parallel |
| R2 | Code Fixes (Medium Risk) | ~5 items | Code/Docs | By module group |
| R3 | Incomplete Implementation | ~5 items | Code | Some dependencies |

---

## Wave R0: Documentation-Only Fixes (~38 items)

All items in this wave are pure documentation fixes - no code changes required. These can be done in parallel by multiple agents.

### R0-DOCS-1: Count/Number Corrections

| ID | Item | Location | Fix |
|----|------|----------|-----|
| B1-1 | Fix LSP server count: "40 servers" → "39 servers" | `architecture/lsp.md:229` | Change number |
| B1-2 | Fix Session Lifecycle count: "(16 variants)" → "(19 variants)" | `architecture/protocol.md:69` | Change number |
| B1-3 | Fix compaction docs inequality: "7 or more" → "more than 6" | `architecture/compaction.md:91` | Change inequality |
| B3-1 | Fix built-in command count: "39" → "46" | `architecture/command.md` | Regenerate table |
| B3-2 | Add missing commands to table: `/stats`, `/tts`, `/pr`, `/issue`, `/checkpoint` | `architecture/command.md` | Regenerate table |
| B5-1 | Fix SSE Parser line numbers: "16-382" → "16-24" | `architecture/provider.md:526` | Update range |

### R0-DOCS-2: Stale Reference Fixes

| ID | Item | Location | Fix |
|----|------|----------|-----|
| B1-4 | Replace hook dispatch table line numbers with function names | `architecture/agent.md:621-628` | Use function names |
| B1-5 | Replace all line number references with function/class names | `architecture/bus.md` | General cleanup |
| B2-1 | Update app/mod.rs line count: "~5978" → "6003" | `architecture/tui.md` | Update count |
| B2-2 | Update worktree.md line references: `is_git_file()` line 36→172, `is_git_worktree()` line 56→180 | `architecture/worktree.md:117` | Update line refs |
| B4-1 | Fix stale line number references in server docs | `architecture/server.md` | Use more generic refs |
| B4-2 | Verify client timeout code location and update reference | `architecture/server.md:465-468` vs `src/client/attach.rs` | Verify and fix |

### R0-DOCS-3: Documentation Completeness

| ID | Item | Location | Fix |
|----|------|----------|-----|
| B1-6 | Mark "Dead tui_config code removed" section as historical (dated 2026-05-22) | `architecture/config.md:247-249` | Add historical note |
| B1-7 | Document ProviderConfig::merge() behavior (field-level merge vs full replace) | `architecture/config.md` | Add merge behavior docs |
| B1-8 | Add note: multiedit exists but NOT in default ToolRegistry | `architecture/agent.md:818` | Add clarifying note |
| B1-9 | Clarify shutdown sequence wording: "10x 100ms waits" → "up to 10 attempts with 100ms delays" | `architecture/agent.md:375-378` | Improve wording |
| B1-10 | Clarify PermissionChecker struct range at line 392 | `architecture/permission.md:392` | Clarify range |
| B1-11 | Document missing PermissionChecker methods (check_bash, check_git, check_with_args, always_allow_legacy, always_deny_legacy) | `architecture/permission.md:156-173` | Add to Key Methods |
| B1-12 | Add InprocCoreClient field names to docs: subagent_pool, memory_store, bg_scheduler, pool | `architecture/core.md:37` | Add field names |
| B1-13 | Explain why snapshot events are NOT mapped via map_app_event_to_core_event | `architecture/core.md` | Add explanation |
| B1-14 | Add line number ranges for plugin builtin/mod.rs in Project Structure | `architecture/plugin.md` | Add line ranges |
| B1-15 | Document path canonicalization security checks in Security table | `architecture/plugin.md:136-156,183-212` | Add security docs |
| B2-3 | Add `resize_debounce: Option<std::time::Instant>` to UiState docs | `architecture/tui.md` | Add field to UiState section |
| B2-4 | Update Component trait docs: `Send` → `Send + Any` | `architecture/tui.md:284` vs `src/tui/components/component.rs:84` | Update bound |
| B2-5 | Add `Stats` variant to Dialog enum documentation | `architecture/tui.md:189-196` vs `src/tui/app/types.rs:21` | Add variant |
| B2-6 | Add documentation for `pricing.rs`: ModelPricing struct, PricingService, calculate_cost() | `src/util/pricing.rs` | Document module |
| B3-3 | Remove stale historical note about "Removed orphaned src/tui/app/commands.rs" | `architecture/command.md:212` | Remove stale content |
| B3-4 | Document `/pr` and `/issue` use GitHub MCP templates | `architecture/command.md` | Add template docs |
| B3-5 | Expand Shell Session architecture (currently brief 80 lines) | `architecture/shell_session.md` | Expand documentation |
| B3-6 | Document memory eviction criteria (lowest importance when at limit) | `architecture/memory.md` | Add eviction policy |
| B3-7 | Document consolidate_session limitations with binary data | `architecture/memory.md` | Add limitations section |
| B4-3 | Document RateLimiter vs WsRateLimiter mutex implementation divergence | `architecture/server.md` | Add implementation note |
| B4-4 | Update skills path docs for platform-specific paths (macOS: ~/Library/Application Support/) | `architecture/skills.md:44-56` | Update platform docs |
| B4-5 | Document specific index names in session migration v1: session.project_idx, etc. | `architecture/session.md:192-199` | Add index names |
| B4-6 | Fix migrate() pattern description to reflect actual version-check implementation | `architecture/session.md:206-216` | Update description |
| B5-2 | Fix IPv6 unique local description: "fc00::/8 and fd00::/8" → "fc00::/7 (unique local: fc00::/8 and fd00::/8)" | `architecture/security.md:197` | Update range description |
| B5-3 | Add CANONICAL_PATHS_CACHE known issue to security.md (already in AGENTS.md) | `architecture/security.md` | Sync known issues |
| B5-4 | Clarify Question Channel immediate-answer behavior in exec docs | `architecture/exec.md:168-169` | Update docs |
| B5-5 | Consider documenting EncryptedData visibility intent (struct not pub but fields are) | `architecture/crypto.md` | Document design intent |
| B5-6 | Consider clarifying Argon2idParams last param is output key length | `architecture/crypto.md:63` | Add param explanation |
| B5-7 | Consider clarifying ProviderError::api() url field behavior | `architecture/error.md:106-108` | Add clarification |
| B5-8 | Note Encryption exclusion in McpError::is_retryable docs | `architecture/error.md:188-192` | Add note |

---

## Wave R1: Code Fixes - Low Risk (~6 items)

These are code fixes that are isolated, low-risk, and can be done in parallel.

### R1-CODE-1: Server Module Fixes

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B4-7 | Fix WebSocket validate_ws_auth() inconsistency: returns 500 when no token configured, but HTTP allows | `src/server/ws.rs:103-106` | Code | Make consistent |

### R1-CODE-2: MCP Module Fixes

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B3-8 | Wire up SSE event processing in agent loop (connect_sse() exists but never called) | `src/mcp/remote.rs:698-740` | Code/Docs | Wire to agent loop |
| B3-9 | run_socket() exists but not called anywhere (Unix socket server for IDE MCP) | `src/mcp/ide_server.rs:121-144` | Code | Document or wire up |
| B3-10 | McpCli Debug command is STUB only - doesn't actually test connections | `src/mcp/cli.rs:309-318` | Code | Implement properly or remove |

### R1-CODE-3: Client Module Fixes

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B3-11 | Add handle_remote_event line number to docs | `src/tui/app/mod.rs:794` | Docs | Add line ref |

### R1-CODE-4: Snapshot Module Fixes

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B2-10 | Update storage migration version in docs: "v1-v14" → "v1-v15" | `architecture/storage.md:106` | Docs | Update version |

---

## Wave R2: Code Fixes - Medium Risk (~5 items)

These involve actual code changes with moderate complexity. Grouped by module for parallelization.

### R2-CODE-1: Snapshot Module (Code Changes)

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B2-11 | Add atomic write pattern to restore() method (use temp file + rename like restore_to_path()) | `src/snapshot/mod.rs:292` | Code | Add atomic write |
| B2-12 | Decide on unified hash algorithm (MD5 at line 431 vs SHA256 elsewhere) | `src/snapshot/mod.rs:431,143` | Code | Decide and unify |

### R2-CODE-2: MCP Module (Incomplete Implementation)

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B3-12 | Implement or remove McpCli Debug command (currently just prints message) | `src/mcp/cli.rs:309-318` | Code | Implement or remove |
| B3-13 | OAuthManager sync methods unused: load_tokens_sync() and load_used_codes_sync() marked #[allow(dead_code)] | `src/mcp/auth.rs` | Code | Implement or remove |

### R2-CODE-3: Config/Provider Module

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B5-10 | Verify ProviderConfig::api_key(prefix) method exists at schema.rs | `src/config/schema.rs` | Code | Verify and document |

---

## Wave R3: Incomplete Implementation (~5 items)

These items involve incomplete implementations that may require more design work.

### R3-IMPL-1: MCP SSE Integration (High Priority) | DOCUMENTED |

| ID | Item | Location | Issue | Action |
|----|------|----------|-------|--------|
| B3-12 | connect_sse() exists but never called automatically - no consumer for take_sse_events() in agent loop | `src/mcp/remote.rs:698-740` | Dead code | Wire SSE events to agent loop OR document limitation | ✅ DOCUMENTED in architecture/mcp.md |

### R3-IMPL-2: IdeServer Socket (Medium Priority) | DOCUMENTED |

| ID | Item | Location | Issue | Action |
|----|------|----------|-------|--------|
| B3-13 | run_socket() returns Ok(()) without socket handling - Unix socket server for IDE MCP not wired up | `src/mcp/ide_server.rs:121-144` | Unused | Implement IDE integration or remove from docs | ✅ DOCUMENTED in architecture/mcp.md |

### R3-IMPL-3: MCP Debug Command (Medium Priority) | IMPLEMENTED in R1 |

| ID | Item | Location | Issue | Action |
|----|------|----------|-------|--------|
| B3-14 | McpCli Debug command just prints arguments and help message - does NOT test connections | `src/mcp/cli.rs:309-318` | Stub | Implement actual connection test OR strip from CLI | ✅ IMPLEMENTED in Wave R1 |

### R3-IMPL-4: OAuth Sync Methods (Low Priority) | IMPLEMENTED in R2 |

| ID | Item | Location | Issue | Action |
|----|------|----------|-------|--------|
| B3-15 | load_tokens_sync() called in OAuthManager::new() at auth.rs:119 but errors silently ignored via `let _` | `src/mcp/auth.rs:119` | Silent error | Handle errors properly or remove sync method | ✅ FIXED in Wave R2 |

### R3-IMPL-5: Pricing Module Documentation (Medium Priority) | COMPLETED |

| ID | Item | Location | Issue | Action |
|----|------|----------|-------|--------|
| B2-11 | pricing.rs (84 lines) completely undocumented | `src/util/pricing.rs` | Missing docs | Add comprehensive documentation | ✅ DOCUMENTED in Wave R0

---

## Summary by Module

| Module | R0-Docs | R1-Code | R2-Code | R3-Impl | Total |
|--------|---------|---------|---------|---------|-------|
| Agent | 3 | 1 | - | - | 4 |
| Bus | 1 | - | - | - | 1 |
| Client | 1 | - | - | - | 1 |
| Command | 2 | - | - | - | 2 |
| Compaction | 1 | - | - | - | 1 |
| Config | 1 | - | 1 | - | 2 |
| Core | 2 | - | - | - | 2 |
| Crypto | 2 | - | - | - | 2 |
| Error | 2 | - | - | - | 2 |
| Exec | 1 | - | 1 | - | 2 |
| Hooks | 1 | - | - | - | 1 |
| IDE/LSP | - | - | - | - | 0 |
| MCP | 1 | 3 | - | 4 | 8 |
| Memory | 2 | - | - | - | 2 |
| Permission | 2 | - | - | - | 2 |
| Plugin | 2 | - | - | - | 2 |
| Provider | 1 | - | 1 | - | 2 |
| Security | 2 | - | - | - | 2 |
| Server | 4 | 1 | 1 | - | 6 |
| Session | 2 | - | - | - | 2 |
| Shell Session | 1 | - | - | - | 1 |
| Skills | 1 | - | - | - | 1 |
| Storage | 1 | - | - | - | 1 |
| Snapshot | - | - | 2 | - | 2 |
| TTS | - | - | - | - | 0 |
| Tool | 1 | - | - | - | 1 |
| TUI | 4 | 1 | - | - | 5 |
| Upgrade | - | - | - | - | 0 |
| Util | 1 | - | - | - | 1 |
| Worktree | 1 | - | - | 1 | 2 |
| **Total** | **38** | **6** | **5** | **5** | **54** |

---

## Status Summary

| Category | Status |
|----------|--------|
| Historical Completed (Waves 0-3) | ✅ All (via 25+ PRs) |
| TUI Input Repair (Completed 2026-05-01) | ✅ |
| TUI Scrolling Fix (Completed 2026-05-06) | ✅ |
| TUI Message Flow (Completed 2026-05-05) | ✅ |
| Wave R0: Documentation-Only (~38 items) | ✅ COMPLETED 2026-05-27 |
| Wave R1: Code Fixes (Low Risk) | ✅ COMPLETED 2026-05-27 |
| Wave R2: Code Fixes (Medium Risk) | ✅ COMPLETED 2026-05-27 |
| Wave R3: Incomplete Implementation | ✅ COMPLETED 2026-05-27 |
| Wave 4: Large Refactors | ⏳ DEFERRED |

---

## Consolidated Statistics

| Metric | Value |
|--------|-------|
| Waves 0-3 Completed | ✅ All (via 25+ PRs) |
| Architecture Review Items (R0-R3) | ~54 items |
| R0: Documentation-Only | ✅ 38 items completed |
| R1: Code Fixes (Low Risk) | ✅ 4 items completed (2 moved to docs already correct) |
| R2: Code Fixes (Medium Risk) | ✅ 4 items completed (1 already correct) |
| R3: Incomplete Implementation | ✅ Documented limitations |
| PRs Created (Waves 0-3 + Features) | 35 |
| Wave 4 (Large Refactors) | ⏳ DEFERRED |

---

## Notes for Future Agents

### Architecture Review Items Guidance

1. **R0 items are pure documentation** - no code changes, safe to do in parallel
2. **R1 items are isolated code fixes** - can be done in parallel, low risk
3. **R2 items involve actual code changes** - review carefully before merging
4. **R3 items are incomplete implementations** - may need design discussion before implementation
5. **Batches 1-5 review files are source of truth** - see individual review files in plans/

### Verified Claims from Original plan.md (Still Accurate)

- **TUI-3**: Image Attachment Support - ✅ DONE
- **AGENT-5**: Image Generation - ✅ DONE
- **AGENT-6**: GitHub Integration (/pr and /issue) - ✅ DONE
- **EXEC-2**: Session Analytics & Cost Tracking - ✅ DONE
- **GIT Enhancement**: GitHub MCP - ✅ DONE

### Deferred Items (Complex Refactors)

| Item | Status | Notes |
|------|--------|-------|
| TUI-5: Accessibility Improvements | DEFERRED | Requires FocusManager architectural change |
| LARGE-1: Virtual Scrolling for Messages | DEFERRED | High risk refactor |
| LARGE-2: String Interning System | DEFERRED | High risk architectural change |

---

## Known Code Issues (Previously Documented)

| Issue | Location | Priority |
|-------|----------|----------|
| Snapshot hash inconsistency (MD5 vs SHA256) | `src/snapshot/mod.rs:431` | MEDIUM |
| ToolExecutor exists but unused | `src/tool/executor.rs:8` | MEDIUM |
| Static CANONICAL_PATHS_CACHE never clears | `src/security/sandbox.rs:237` | MEDIUM |
| TTS init() ignores providers | `src/tts/mod.rs:45-49` | LOW |
| Worktree symlink detection | `src/worktree/mod.rs:69-88` | LOW |
| OAuth replay protection TOCTOU | `src/mcp/auth.rs:318-332` | MEDIUM |
| PermissionResponse unused | `src/permission/mod.rs:1141-1145` | LOW |
| check_external_directory unused | `src/permission/mod.rs:1237-1248` | LOW |

*(End of file)*
