# Implementation Plan

**Status**: IN PROGRESS
**Last Updated**: 2026-05-26

---

## Overview

This plan consolidates remaining actionable items from architecture review of 31 module plan files. Items are organized into waves for parallel implementation where possible.

**Key Finding from Review**: Many "bugs" in review files were actually correctly implemented - always verify claims against code before implementing.

---

## Implementation Waves

### Wave 1: Critical Bugs (Sequential - Must Fix First)

#### W1-1: Plugin Fuel Leaks (CONFIRMED BUG)
**Location**: `src/plugin/loader.rs:255-285`

**Problem**: When WASM plugin execution fails early (metadata read failure, size check failure, compilation failure), the reserved fuel is NOT returned to the plugin's budget.

**Fuel leaks at these locations**:
- Line 259: `metadata.read` failure → returns without `return_fuel`
- Line 270: size check exceeds MAX_WASM_SIZE → returns without `return_fuel`
- Line 285: module cache get/compile fails → returns without `return_fuel`

**Compare with correct handling**: Lines 327, 336, 351, 369, 384, 503, 508 all correctly call `module_cache::CACHE.return_fuel(plugin_id, fuel_reserved)` before returning.

**Fix Required**: Add `module_cache::CACHE.return_fuel(plugin_id, fuel_reserved)` before each early return at lines 259, 270, 285.

**Testing**: `cargo test plugin`

---

#### W1-2: CoreEvent Mapping Incomplete (CONFIRMED BUG)
**Location**: `src/core/mod.rs:728-797` (`map_app_event_to_core_event`)

**Problem**: Many AppEvent variants are mapped to `None` and dropped (line 795: `_ => None`).

**Events NOT mapped** (verified at lines 120-141, 184-187 in events.rs):
- `SubagentStarted` (line 120)
- `SubagentProgress` (line 127)
- `SubagentCompleted` (line 134)
- `SubagentFailed` (line 141)

**Impact**: In-process subscribers (via `subscribe()`) miss subagent events that are visible to SSE clients via `/api/event`.

**Fix Required**: Add mapping for Subagent events in `map_app_event_to_core_event()`:
```rust
AppEvent::SubagentStarted { session_id, agent_id, task } => 
    Some(CoreEvent::SubagentStarted { session_id, agent_id, task }),
// etc.
```

**Verification**: SSE clients see subagent events but in-process subscribe() does not.

---

#### W1-3: UiState Fields (CORRECTED - No Fix Needed)
**Location**: `src/tui/app/state/ui.rs`

**Verification Complete**: UiState DOES have all the documented fields:
- `tts: Tts` (line 67)
- `tts_enabled: bool` (line 69)
- `fullscreen: bool` (line 71)
- `dirty_regions: Vec<Rect>` (line 73)
- `render_panic_count: usize` (line 64)
- `last_render_error: Option<String>` (line 65)

**Status**: No fix needed - architecture documentation was accurate.

**Note**: `fullscreen: bool` at line 71 manages DEC 1049 alternate screen buffer.

---

### Wave 2: Documentation Fixes (Parallel - 6 agents)

All items in Wave 2 are independent documentation fixes that can be done in parallel.

#### W2-1: Create architecture/protocol.md
**Priority**: HIGH
**Reason**: `src/protocol/` module exists with CoreRequest, CoreResponse, TuiMessage types but has no dedicated architecture doc.

**Content should include**:
- CoreRequest enum variants (`src/protocol/core.rs:50-175`)
- CoreResponse enum variants
- TuiMessage enum variants (`src/protocol/tui.rs`)
- Protocol version (currently 1)
- Request/response flow diagrams

---

#### W2-2: Fix Permission Built-in Modes Table
**Location**: `architecture/permission.md` (lines 198-202)

**Problem**: Table shows "skill" in all three built-in modes' allowed_tools, but source (`src/permission/modes.rs`) shows skill is NOT in any built-in mode.

**Fix**: Remove "skill" from allowed_tools column in built-in modes table.

---

#### W2-3: Clarify Provider Auto-registration
**Location**: `architecture/provider.md`

**Problem**:
1. SAP AI Core, Zenmux, Kilo, Vercel AI Gateway listed as auto-registered but only `codegg_go` is actually auto-registered
2. "Discovery Providers" section title is misleading - these don't auto-discover

**Fix**:
1. Update table to clarify which providers are auto-registered vs config-only
2. Rename "Discovery Providers" to something like "Additional OpenAI-Compatible Providers"

---

#### W2-4: Remove Line Number References
**Priority**: MEDIUM
**Reason**: Line numbers in architecture docs frequently drift.

**Approach**: Replace specific line number references (e.g., "loop.rs:1777") with method names or describe behavior instead.

**Files to update**:
- `architecture/agent.md:296` - ToolExecuteBefore hook reference
- `architecture/compaction.md:116` - compact_if_needed reference
- All architecture docs with line number references

---

#### W2-5: Document Hook Timeout Distinction
**Location**: `architecture/plugin.md`

**Problem**: Documentation says "5s per hook dispatch, 30s for WASM execution" but this is misleading.

**Clarification needed**:
- Outer `execute_hook_with_timeout` uses 5s (hook_timeout)
- Inner WASM execution loop uses 30s (WASM_HOOK_TIMEOUT)

---

#### W2-6: Clarify Backoff Formula
**Location**: `architecture/resilience.md` (line 148)

**Problem**: "Exponential backoff: 2^i seconds, capped at 30s" is ambiguous.

**Fix**: Clarify that formula includes jitter and cap description.

---

#### W2-7: Document CoreRequest Fallthrough Behavior
**Location**: `architecture/core.md`

**Problem**: Initialize, Subscribe, Resume variants fall through to Ack but this isn't explicitly documented.

**Fix**: Add explicit note about which CoreRequest variants are handled vs fall through to Ack.

---

### Wave 3: Cross-Module Fixes (Parallel - 4 agents)

#### W3-1: Subagent Events Not Flowing to CoreEvent
**Locations**: `src/core/mod.rs:728-797`, `src/bus/events.rs`

**Problem**: `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` published to GlobalEventBus (visible to SSE) but don't appear in CoreEvent (in-process subscribe).

**Fix**: Complete `map_app_event_to_core_event()` to include subagent events.

---

#### W3-2: Hash Algorithm Inconsistency
**Locations**: `src/session/checkpoint.rs:compute_checksum` vs `src/snapshot/mod.rs:142`

**Problem**:
- checkpoint.rs uses SHA256 for working file verification
- snapshot/mod.rs uses MD5 for file snapshot hashing

**Fix**: Standardize to one algorithm (recommend SHA256 for consistency) OR document why different algorithms are used.

---

#### W3-3: PermissionRegistry Session Filtering
**Locations**: `src/bus/mod.rs`, `src/permission/mod.rs:65`

**Problem**: Permission IDs are format `{tool_call_id}-{tool_name}`, NOT `{session_id}-...`. This means `get_pending_permissions_for_session()` cannot properly filter.

**Status**: Known limitation - documented in AGENTS.md. May require architectural change to fix properly.

**Workaround**: Document the limitation clearly; future fix would need to extend registry key format.

---

#### W3-4: Rename stat_core.rs to metrics.rs
**Location**: `src/util/stat_core.rs`

**Problem**: Filename "stat_core" is misleading - file contains metrics code, not file stats.

**Fix**: Rename to `metrics.rs` and update all references.

---

### Wave 4: Lower Priority Items

#### W4-1: Add EventProcessor Documentation
**Location**: `src/agent/processor.rs` (not fully documented in architecture)

**Problem**: The `processor.rs` file handles ChatEvent processing (TextDelta, ReasoningDelta, ToolCall, Finish, Error) but isn't fully documented.

**Fix**: Document EventProcessor in architecture/agent.md or create dedicated section.

---

#### W4-2: Verify CompactionConfig Schema Location
**Location**: `src/agent/compaction.rs:579-590`

**Problem**: Magic numbers (2000 char threshold, 6 messages for TruncateToolOutputs, 8 for SummarizeOldTurns) aren't configurable via CompactionConfig.

**Fix**: Verify CompactionConfig location in schema.rs and consider making thresholds configurable.

---

#### W4-3: Fuzzy Match Example Fix
**Location**: `architecture/util.md` (line 109)

**Problem**: Example shows `fuzzy_match("hel", &candidates)` being used like `fuzzy_score` - second element is score, not index.

**Fix**: Update example to properly iterate over results.

---

## Verification Summary

| Wave | Items | Status |
|------|-------|--------|
| W1 (Critical) | 3 | Pending |
| W2 (Docs) | 7 | Pending |
| W3 (Cross-module) | 4 | Pending |
| W4 (Low priority) | 3 | Pending |

---

## Testing Commands

After any changes, run:

```bash
# Build verification
cargo build --all-features

# Lint
cargo clippy --all-features -- -D warnings

# Test
cargo test --all-features

# Specific module tests
cargo test plugin
cargo test core
cargo test session
```

---

## Implementation Notes for Future Agents

### Wave Assignment Strategy

```
Wave 1 (Sequential - Dependencies):
  - W1-1 must complete before W1-2 (both are in different files but both critical)

Wave 2 (Parallel - Documentation):
  - 6 agents can work simultaneously on W2-1 through W2-7
  - Each agent takes one sub-item

Wave 3 (Parallel - Cross-module):
  - 4 agents can work simultaneously on W3-1 through W3-4
  - W3-1 depends on W1-2 completion

Wave 4 (Parallel - Low priority):
  - 3 agents can work simultaneously on W4-1 through W4-3
```

### Pre-Implementation Verification

1. **Always verify before implementing** - Many "bugs" claimed in plan files were actually correctly implemented
2. **Read architecture doc first** - Check current state before making changes
3. **Count from source** - Don't trust line numbers or counts in documentation
4. **Use method names** - Avoid line number references as they drift

### Verification Checklist

- [ ] Plugin fuel leaks fixed - verify with test
- [ ] CoreEvent mapping complete - verify events flow to in-process subscribers
- [ ] UiState fields match documentation OR documentation updated
- [ ] architecture/protocol.md created
- [ ] Permission built-in modes table corrected
- [ ] Provider auto-registration clarified
- [ ] Line numbers replaced with method names
- [ ] Hook timeout distinction clarified
- [ ] Backoff formula includes jitter note
- [ ] CoreRequest fallthrough documented
- [ ] Subagent events flow to CoreEvent (after W1-2)
- [ ] Hash algorithm standardized or documented
- [ ] PermissionRegistry limitation documented
- [ ] stat_core.rs renamed to metrics.rs

---

## See Also

- [AGENTS.md](../AGENTS.md) - Root index file with module quick reference
- [AGENTS.override.md](../AGENTS.override.md) - Override file with verified facts
- `architecture/` - Architecture documentation per module
- `.opencode/skills/` - Module-specific skill guides
- `plans/consolidated.md` - Batch review summary
- `plans/stale_items.md` - Stale item identification

*(End of file)*