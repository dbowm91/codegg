# Architecture Review - Consolidated Findings

**Review Date**: 2026-05-26
**Batches Reviewed**: 7 batches (30 modules)
**Status**: Phase 3 Complete

---

## Executive Summary

Overall documentation quality is **Good** (85% accuracy). Most discrepancies are minor line number drift or documentation clarity issues rather than actual bugs. One critical bug class was identified (plugin fuel leaks).

| Batch | Modules | Quality | Critical Issues |
|-------|---------|---------|-----------------|
| 1 | agent, bus, core, command, compaction | Good | 1 (core event mapping) |
| 2 | permission, security, crypto | Good | 0 |
| 3 | session, storage, memory, snapshot | Good | 0 |
| 4 | tui, client, ide | Good | 0 |
| 5 | server, mcp, lsp, exec | Excellent | 0 |
| 6 | plugin, skills, hooks, upgrade | Good | 3 (fuel leaks) |
| 7 | provider, tool, resilience, util, tts, pty_session, worktree | Good | 0 |

---

## Critical Bugs Found

### 1. Plugin Fuel Leaks (CRITICAL)

**Location**: `src/plugin/loader.rs:255-285`

When WASM plugin execution fails early (metadata read failure, size check failure, compilation failure), the reserved fuel is NOT returned to the plugin's budget. This causes permanent fuel loss from plugin budgets.

```rust
// Line 259 - fuel leak
return HookResult::ok(ctx.input);  // Missing return_fuel!

// Line 270 - fuel leak
return HookResult::ok(ctx.input);  // Missing return_fuel!

// Line 285 - fuel leak
return HookResult::ok(ctx.input);  // Missing return_fuel!
```

**Fix Required**: Add `module_cache::CACHE.return_fuel(plugin_id, fuel_reserved)` before each early return.

### 2. Core Event Mapping Incomplete

**Location**: `src/core/mod.rs:728-797` (`map_app_event_to_core_event`)

Many AppEvent variants are dropped (mapped to `None`) rather than being converted to CoreEvent:
- `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` not mapped
- `SnapshotSession`, `SnapshotWorkspace`, `SnapshotModels` not handled
- `TurnStarted`, `TurnFailed` not handled

This creates inconsistent event visibility between SSE clients and in-process subscribers.

### 3. UiState Missing Fields

**Location**: `src/tui/app/state/ui.rs`

The architecture documentation shows `tts`, `tts_enabled`, `fullscreen`, `dirty_regions`, `render_panic_count`, `last_render_error` fields in UiState, but these are NOT present in the actual source code.

---

## Cross-Module Issues

### 1. Event Type Inconsistency

- `TextDelta` uses `Arc<str>` for session_id and delta
- `ReasoningDelta` uses owned `String` for delta

This inconsistency may affect performance for high-frequency reasoning events.

### 2. PermissionRegistry Session Filtering

Both architecture docs and AGENTS.md note that `PermissionRegistry` and `QuestionRegistry` don't store `session_id` in their keys. Permission IDs are format `{tool_call_id}-{tool_name}`.

This means `get_pending_permissions_for_session()` and `get_pending_questions_for_session()` cannot properly filter by session_id.

### 3. Hash Algorithm Inconsistency

- `checkpoint.rs:compute_checksum` uses SHA256 for working file verification
- `snapshot/mod.rs:142` uses MD5 for file snapshot hashing

Both are cryptographic hashes but different algorithms could cause confusion.

### 4. Subagent Events Not Flowing to CoreEvent

`SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` are published to GlobalEventBus (visible to SSE clients via `/api/event`) but don't appear in CoreEvent (used by in-process `subscribe()`).

### 5. CoreRequest Handler Gaps

`Initialize`, `Subscribe`, `Resume` variants in `CoreRequest` are not handled in `InprocCoreClient` and fall through to `CoreResponse::Ack` silently. This may cause issues for remote TUI resume functionality.

---

## Stale Information by Module

### permission.md
- Table at lines 198-202 shows "skill" in all three built-in mode's allowed_tools, but source shows skill is NOT in any built-in mode

### provider.md
- Lines 36-49 list SAP AI Core, Zenmux, Kilo, Vercel AI Gateway as auto-registered but only codegg_go is auto-registered
- "Discovery Providers" section title is misleading

### core.md
- TurnSubmit documentation mentions only `text` and `plan_mode` but actual has additional fields

### plugin.md
- Hook dispatch timeout documentation misleading: 5s outer, 30s inner WASM

### resilience.md
- Exponential backoff formula description ambiguous; actual includes jitter

### session.md
- Hash algorithm (SHA256 vs MD5) inconsistency with snapshot module

---

## Documentation Quality Issues

### Line Numbers Are Fragile

Most architecture documents reference specific line numbers (e.g., "loop.rs:1777"). These frequently drift as code is modified. Recommendation: Reference method names or describe behavior instead.

### Counts Should Be Verified

Module counts in documentation should be verified against actual source:
- LSP servers: Documentation varies between 39 and 40
- Tools: 26 verified
- Built-in commands: 41 verified

### Missing Cross-References

Several modules document integration points but don't reference the actual code locations in other modules:
- AgentLoop hooks not cross-referenced in plugin.md
- Snapshot table schema defined in session module
- ToolExecutor exists but not integrated (documented but easy to miss)

---

## Verified Correct Items

The following items were verified as correctly documented and implemented:

| Item | Value | Location |
|------|-------|----------|
| Tool count | 26 | `src/tool/mod.rs:89-119` |
| LSP server count | 39 | `src/lsp/server.rs:27-385` |
| PermissionResponse | `{level: PermissionLevel, persist: bool}` | `src/permission/mod.rs:1142-1145` |
| InprocCoreClient fields | All wrapped in `Option<Arc<...>>` | `src/core/mod.rs:22-28` |
| Plugin fuel logic | Returns early when exhausted | `src/plugin/loader.rs:262-266` |
| InlineScript | Deprecated, non-functional | `src/hooks/mod.rs:180-184` |
| CommandRegistry location | Line 72 | `src/tui/command.rs:72` |
| ProviderError::Auth | is_retryable = true | `src/error.rs:169` |
| Memory frequency_bonus | `(count - 1) * 2.0` | `src/memory/patterns.rs:232` |
| Session events published | SessionCreated, MessageAdded | `src/bus/events.rs:7,21` |
| GlobalEventBus capacity | 2048 | `src/bus/global.rs:13` |
| PermissionRegistry TTL | 300s | `src/bus/mod.rs:59` |

---

## Recommendations

### High Priority

1. **Fix plugin fuel leaks** at `loader.rs:255-285`
2. **Complete CoreEvent mapping** at `map_app_event_to_core_event()`
3. **Update UiState** to match documentation or fix documentation

### Medium Priority

4. **Remove "skill" from permission built-in modes table** in documentation
5. **Clarify provider registration** - which are auto-registered vs config-only
6. **Document CoreRequest fallthrough** behavior explicitly

### Low Priority

7. **Remove specific line numbers** from architecture docs, use method names
8. **Standardize hash algorithm** (SHA256 vs MD5) for consistency
9. **Add jitter to backoff formula** documentation clarification
10. **Rename "stat_core.rs"** to "metrics.rs" to avoid confusion

---

## Files Reviewed

| Module | Plan File | Status |
|--------|-----------|--------|
| agent | plans/agent.md | Complete |
| bus | plans/bus.md | Complete |
| core | plans/core.md | Complete |
| command | plans/command.md | Complete |
| compaction | plans/compaction.md | Complete |
| permission | plans/permission.md | Complete |
| security | plans/security.md | Complete |
| crypto | plans/crypto.md | Complete |
| session | plans/session.md | Complete |
| storage | plans/storage.md | Complete |
| memory | plans/memory.md | Complete |
| snapshot | plans/snapshot.md | Complete |
| tui | plans/tui.md | Complete |
| client | plans/client.md | Complete |
| ide | plans/ide.md | Complete |
| server | plans/server.md | Complete |
| mcp | plans/mcp.md | Complete |
| lsp | plans/lsp.md | Complete |
| exec | plans/exec.md | Complete |
| plugin | plans/plugin.md | Complete |
| skills | plans/skills.md | Complete |
| hooks | plans/hooks.md | Complete |
| upgrade | plans/upgrade.md | Complete |
| provider | plans/provider.md | Complete |
| tool | plans/tool.md | Complete |
| resilience | plans/resilience.md | Complete |
| util | plans/util.md | Complete |
| tts | plans/tts.md | Complete |
| pty_session | plans/pty_session.md | Complete |
| worktree | plans/worktree.md | Complete |
