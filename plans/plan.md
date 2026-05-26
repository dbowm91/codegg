# Implementation Plan - Phase 3: Deferred Items

**Status**: Partially Deferred (2026-05-26)
**Created**: 2026-05-26
**Consolidated from**: Review of 33 plan files across the codebase

---

## Summary

This plan consolidates 10 code bugs and ~25 documentation corrections. **All items have been verified as completed or already correct** except for 3 optional items that remain deferred.

- **Wave 1 (Code Bugs)**: COMPLETED (all 10 bugs fixed)
- **Wave 2 (Documentation)**: COMPLETED (all items correct or already fixed)
- **Wave 3 (Optional)**: DEFERRED (3 items remain)

---

## Deferred Items (Wave 3)

These items were not addressed in the Phase 3 implementation but remain as known limitations for future work.

### OPT-01: SSE Support Not Fully Integrated

- **Module**: server
- **File**: `src/mcp/remote.rs:698-764`
- **Issue**: `connect_sse()` and `connect_sse_stream()` exist but are not automatically called during remote connection setup. SSE events are collected but not processed by the agent.
- **Fix**: Requires understanding SSE event flow integration with the agent loop
- **Status**: Known limitation - requires architectural work to properly integrate SSE events into agent loop

### OPT-02: Tool Definition Cache Staleness

- **Module**: tool
- **File**: `src/agent/loop.rs:1029-1051`
- **Issue**: Using `mcp_tool_count` as proxy means if MCP tool identities change without count changing, cache may be stale. MCP service would need to expose a version/hash for more precise invalidation.
- **Current**: Code at line 1033-1037 documents this limitation in a comment
- **Status**: Known limitation - would require MCP protocol changes to expose version/hash

### OPT-03: Plugin Global Fuel Budget Dead Code

- **Module**: plugin
- **File**: `src/plugin/loader.rs:15, 24-41`
- **Issue**: Global `PLUGIN_FUEL_BUDGET` and `check_and_reset_fuel_budget()` are never called - only per-plugin fuel via `ModuleCache` is used.
- **Decision**: Keep as-is; per-plugin fuel via `ModuleCache` is what's actually used. Global budget removal would require significant refactoring without clear benefit.
- **Status**: Documented as dead code, no action planned

---

## Completed Items Reference

All completed items from Wave 1 and Wave 2 are documented in git history:

| Commit | Description |
|--------|-------------|
| `7177e1a` | fix(server): remove faulty session_id validation in permission/question routes (BUG-01, BUG-02, BUG-03) |
| `6c16c94` | fix(plugin): return fuel on all early exits in execute_wasm_hook (BUG-09, BUG-10) |
| `641f015` | docs(wave2): documentation corrections across multiple files |

**Key verification findings:**
- BUG-04 through BUG-08: TUI does NOT send Initialize, TurnCancel, TurnSteer, AgentSelect, ModelSelect requests - no action needed
- Many "documentation bugs" from review files were already correct in current docs
- Only 1 confirmed doc count error: theme count (42 vs 31) - fixed

---

*Plan consolidated from codebase review (2026-05-26)*