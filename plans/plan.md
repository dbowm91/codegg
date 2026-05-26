# Implementation Plan

**Status**: IN PROGRESS
**Last Updated**: 2026-05-26

---

## Overview

This plan consolidates remaining actionable items from architecture review of 31 module plan files. Items are organized into waves for parallel implementation where possible.

**Key Finding from Review**: Many "bugs" in review files were actually correctly implemented - always verify claims against code before implementing.

---

## Implementation Waves

### Wave 1: Critical Bugs (Parallel - 2 agents)

Both W1-1 and W1-2 are independent code fixes that can be done in parallel.

#### W1-1: Plugin Fuel Leaks (CONFIRMED BUG)
**Location**: `src/plugin/loader.rs:255-285`

**Problem**: When WASM plugin execution fails early (metadata read failure, size check failure, compilation failure), the reserved fuel is NOT returned to the plugin's budget.

**Fuel leaks at these locations**:
- Line ~259: `metadata.read` failure → returns without `return_fuel`
- Line ~270: size check exceeds MAX_WASM_SIZE → returns without `return_fuel`
- Line ~285: module cache get/compile fails → returns without `return_fuel`
- Line ~403: "hook function returned no value" → returns without `return_fuel`

**Compare with correct handling**: Lines ~327, 336, 351, 369, 384, 503, 508 all correctly call `module_cache::CACHE.return_fuel(plugin_id, fuel_reserved)` before returning.

**Fix Required**: Add `module_cache::CACHE.return_fuel(plugin_id, fuel_reserved)` before each early return at lines ~259, ~270, ~285, ~403.

**Testing**: `cargo test plugin`

---

#### W1-2: CoreEvent Mapping Incomplete (CONFIRMED BUG)
**Location**: `src/core/mod.rs:728-797` (`map_app_event_to_core_event`)

**Problem**: Many AppEvent variants are mapped to `None` and dropped (line ~795: `_ => None`).

**Events NOT mapped** (verified at lines 120-141 in `src/bus/events.rs`):
- `SubagentStarted` (line 120)
- `SubagentProgress` (line 127)
- `SubagentCompleted` (line 134)
- `SubagentFailed` (line 141)

**Impact**: In-process subscribers (via `subscribe()`) miss subagent events that are visible to SSE clients via `/api/event`.

**Fix Required**: Add mapping for Subagent events in `map_app_event_to_core_event()`:
```rust
AppEvent::SubagentStarted { session_id, task_id, agent, description } => 
    Some(CoreEvent::SubagentStarted { session_id, task_id, agent, description }),
AppEvent::SubagentProgress { session_id, task_id, agent, message } => 
    Some(CoreEvent::SubagentProgress { session_id, task_id, agent, message }),
AppEvent::SubagentCompleted { session_id, task_id, agent, result_summary } => 
    Some(CoreEvent::SubagentCompleted { session_id, task_id, agent, result_summary }),
AppEvent::SubagentFailed { session_id, task_id, agent, error } => 
    Some(CoreEvent::SubagentFailed { session_id, task_id, agent, error }),
```

**Note**: W3-1 is the same issue (subagent events not flowing to CoreEvent) - W1-2 fix addresses both.

**Verification**: SSE clients see subagent events but in-process subscribe() does not.

---

### Wave 2: Documentation Fixes (Parallel - 7 agents)

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

**Implementation guidance**: Look at `src/protocol/core.rs` for CoreRequest/CoreResponse definitions, `src/protocol/tui.rs` for TuiMessage. Document each variant's purpose and fields. Include flow diagrams showing how requests traverse through InprocCoreClient.

---

#### W2-2: Fix Permission Built-in Modes Table
**Location**: `architecture/permission.md` (lines 198-202)

**Problem**: Table shows "skill" in all three built-in modes' allowed_tools, but source (`src/permission/modes.rs`) shows skill is NOT in any built-in mode.

**Fix**: Remove "skill" from allowed_tools column in built-in modes table (review, debug, docs modes).

**Verification**: Check `src/permission/modes.rs` to confirm no built-in mode includes "skill" in their allowed_tools list.

---

#### W2-3: Clarify Provider Auto-registration
**Location**: `architecture/provider.md`

**Problem**:
1. SAP AI Core, Zenmux, Kilo, Vercel AI Gateway listed as auto-registered but only `codegg_go` is actually auto-registered
2. "Discovery Providers" section title is misleading - these don't auto-discover

**Fix**:
1. Update table to clarify which providers are auto-registered vs config-only
2. Rename "Discovery Providers" to something like "Additional OpenAI-Compatible Providers"
3. Reference `src/provider/mod.rs:register_builtin_with_config()` to verify auto-registration

---

#### W2-4: Remove Line Number References
**Priority**: MEDIUM
**Reason**: Line numbers in architecture docs frequently drift.

**Approach**: Replace specific line number references (e.g., "loop.rs:1777") with method names or describe behavior instead.

**Files to update** (search for patterns like `:\d+` in architecture docs):
- `architecture/agent.md:296` - ToolExecuteBefore hook reference
- `architecture/compaction.md:116` - compact_if_needed reference
- All architecture docs with line number references

**Implementation**: Use grep to find `:line_number` patterns in architecture docs, replace with method name references like `AgentLoop::execute_tool_calls()`.

---

#### W2-5: Document Hook Timeout Distinction
**Location**: `architecture/plugin.md`

**Problem**: Documentation says "5s per hook dispatch, 30s for WASM execution" but this is misleading.

**Clarification needed**:
- Outer `execute_hook_with_timeout` uses 5s (`hook_timeout`) at `src/plugin/service.rs:18`
- Inner WASM execution loop uses 30s (`WASM_HOOK_TIMEOUT`) at `src/plugin/loader.rs:14`

**Fix**: Clarify in docs that 5s is the outer dispatch timeout and 30s is the inner WASM execution timeout.

---

#### W2-6: Clarify Backoff Formula
**Location**: `architecture/resilience.md` (line 148)

**Problem**: "Exponential backoff: 2^i seconds, capped at 30s" is ambiguous.

**Fix**: 
1. Clarify formula is `2^(i-1) * jitter` not `2^i`
2. Include jitter factor description
3. Document HalfOpen→Open timeout (at `src/resilience/circuit.rs:114-127`)

**Implementation**: Check `src/resilience/backoff.rs` for actual formula implementation.

---

#### W2-7: Document CoreRequest Fallthrough Behavior
**Location**: `architecture/core.md`

**Problem**: Initialize, Subscribe, Resume variants fall through to Ack but this isn't explicitly documented.

**Fix**: Add explicit note about which CoreRequest variants are handled vs fall through to Ack.

**Handled variants**: TurnSubmit, SessionMessagesLoad, SessionMessageCounts, SessionCreate, SessionLoad, SessionAttach

**Fallthrough variants** (return Ack without doing anything):
- Initialize
- TurnCancel
- TurnSteer
- AgentSelect
- ModelSelect

---

### Wave 3: Cross-Module Fixes (Parallel - 3 agents)

#### W3-1: Subagent Events Not Flowing to CoreEvent
**Locations**: `src/core/mod.rs:728-797`, `src/bus/events.rs`

**Status**: ALREADY ADDRESSED by W1-2 fix. The CoreEvent mapping fix will resolve this issue.

**Verification**: After W1-2 is implemented, verify subagent events flow to in-process subscribers.

---

#### W3-2: Hash Algorithm Inconsistency
**Locations**: `src/session/checkpoint.rs:150-153` vs `src/snapshot/mod.rs:142`

**Problem**:
- checkpoint.rs uses SHA256 for working file verification
- snapshot/mod.rs uses MD5 for file snapshot hashing

**Fix**: Standardize to SHA256 for consistency.

**Implementation**:
1. Change `src/snapshot/mod.rs:142` from `md5::compute()` to `Sha256`
2. Update any tests or snapshots that rely on MD5 checksums
3. Verify both produce lowercase hex output

**Testing**: `cargo test snapshot`

---

#### W3-3: PermissionRegistry Session Filtering
**Locations**: `src/bus/mod.rs`, `src/permission/mod.rs:65`

**Problem**: Permission IDs are format `{tool_call_id}-{tool_name}`, NOT `{session_id}-...`. This means `get_pending_permissions_for_session()` cannot properly filter.

**Status**: Known limitation - documented in AGENTS.md. 

**Fix (lower priority)**: Document the limitation clearly in architecture/permission.md. Future fix would need to extend registry key format to include session_id.

---

#### W3-4: Rename stat_core.rs to metrics.rs
**Location**: `src/util/stat_core.rs`

**Problem**: Filename "stat_core" is misleading - file contains metrics code (Counter, Gauge, Histogram structs), not file stats.

**Fix**: 
1. Rename `src/util/stat_core.rs` to `src/util/metrics.rs`
2. Update `src/util/mod.rs` to reference `metrics` module
3. Update all internal references to `stat_core`

**Testing**: `cargo build` to verify no breaking changes

---

### Wave 4: Additional Items from Module Reviews (Parallel - 4 agents)

#### W4-1: Server Permission Route Documentation
**Location**: `architecture/server.md`

**Problem**: Permission routes table shows two separate paths (GET and POST) but actual route is single path with multiple methods.

**Fix**: Update permission route entry from:
```
/api/permission/:session_id/submit  GET, POST (submit permission response)
```

To:
```
/api/permission/:session_id  GET (pending permissions), POST (submit response)
```

**Verification**: Check `src/server/routes.rs` for actual route definition.

---

#### W4-2: TTS Keybinding Verification
**Location**: `src/tts/mod.rs`, `src/tui/app/mod.rs`

**Problem**: tts.md notes Ctrl+Y toggle and Ctrl+Shift+Y stop keybindings that need verification in TUI.

**Fix**: 
1. Check `src/tui/app/mod.rs` for TTS keybinding handlers
2. Verify Ctrl+Y and Ctrl+Shift+Y are correctly bound
3. Document actual keybindings in tts.md or architecture/tts.md

---

#### W4-3: Resilience HalfOpen→Open Timeout Documentation
**Location**: `architecture/resilience.md`

**Problem**: State transition diagram missing HalfOpen→Open timeout trigger documentation.

**Fix**: Document that HalfOpen→Open transition occurs when `max_half_open_duration` (default 30s) elapses without failure, per `src/resilience/circuit.rs:66`.

---

#### W4-4: Upgrade Command Behavior Documentation
**Location**: `architecture/upgrade.md`

**Problem**: Unclear what `codegg upgrade` CLI command actually does.

**Fix**: 
1. Verify actual upgrade command behavior by checking `src/upgrade/mod.rs`
2. Document that it fetches latest release from GitHub
3. Clarify configuration settings for upgrade behavior

---

### Wave 5: Lower Priority Items

#### W5-1: Add EventProcessor Documentation
**Location**: `src/agent/processor.rs` (not fully documented in architecture)

**Problem**: The `processor.rs` file handles ChatEvent processing (TextDelta, ReasoningDelta, ToolCall, Finish, Error) but isn't fully documented.

**Fix**: Document EventProcessor in `architecture/agent.md` or create dedicated section.

**Implementation**: EventProcessor takes a `Stream<Item=ChatEvent>` and processes TextDelta/ReasoningDelta into string builders, handles ToolCall/Finish/Error. Used in exec mode and agent loop.

---

#### W5-2: Verify CompactionConfig Schema Location
**Location**: `src/agent/compaction.rs:579-590` vs `src/config/schema.rs`

**Problem**: Magic numbers (2000 char threshold, 6 messages for TruncateToolOutputs, 8 for SummarizeOldTurns) aren't configurable via CompactionConfig.

**Fix**: 
1. Verify CompactionConfig location in schema.rs
2. Consider making thresholds configurable OR document them as hardcoded constants

---

#### W5-3: Fuzzy Match Example Fix
**Location**: `architecture/util.md` (line 109)

**Problem**: Example shows `fuzzy_match("hel", &candidates)` being used like `fuzzy_score` - second element is score, not index.

**Fix**: Update example to properly iterate over results with `(name, score)` tuple destructuring.

---

#### W5-4: InlineScript Deprecation Visibility
**Location**: `src/hooks/mod.rs:180-184`

**Problem**: InlineScript is deprecated but skip just uses `continue` without logging.

**Fix**: Add `tracing::warn!("Skipping deprecated InlineScript")` before `continue` for visibility.

---

## Verification Summary

| Wave | Items | Status |
|------|-------|--------|
| W1 (Critical) | 2 | Pending |
| W2 (Docs) | 7 | Pending |
| W3 (Cross-module) | 4 (W3-1 = W1-2) | Pending |
| W4 (Additional) | 4 | Pending |
| W5 (Low priority) | 4 | Pending |

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
cargo test snapshot
```

---

## Implementation Notes for Future Agents

### Wave Assignment Strategy

```
Wave 1 (Parallel - 2 agents):
  - W1-1: Plugin fuel leaks (loader.rs:259,270,285,403)
  - W1-2: CoreEvent mapping (core/mod.rs:728-797)
  - W1-1 and W1-2 are independent - can run in parallel

Wave 2 (Parallel - 7 agents):
  - W2-1 through W2-7 - each agent takes one item
  - All are independent documentation fixes

Wave 3 (Parallel - 3 agents):
  - W3-1 is addressed by W1-2 - skip or verify
  - W3-2: Hash algorithm (2 files to change)
  - W3-3: PermissionRegistry - documentation only
  - W3-4: Rename stat_core.rs (file rename + references)

Wave 4 (Parallel - 4 agents):
  - W4-1 through W4-4 - each agent takes one item

Wave 5 (Parallel - 4 agents):
  - W5-1 through W5-4 - each agent takes one item
```

### Pre-Implementation Verification

1. **Always verify before implementing** - Many "bugs" claimed in plan files were actually correctly implemented
2. **Read architecture doc first** - Check current state before making changes
3. **Count from source** - Don't trust line numbers or counts in documentation
4. **Use method names** - Avoid line number references as they drift
5. **Search before adding** - Don't duplicate existing documentation

### Verification Checklist

- [ ] W1-1: Plugin fuel leaks fixed - verify with `cargo test plugin`
- [ ] W1-2: CoreEvent mapping complete - verify events flow to in-process subscribers
- [ ] W2-1: architecture/protocol.md created
- [ ] W2-2: Permission built-in modes table corrected
- [ ] W2-3: Provider auto-registration clarified
- [ ] W2-4: Line numbers replaced with method names
- [ ] W2-5: Hook timeout distinction clarified
- [ ] W2-6: Backoff formula includes jitter and HalfOpen→Open note
- [ ] W2-7: CoreRequest fallthrough documented
- [ ] W3-2: Hash algorithm standardized to SHA256
- [ ] W3-3: PermissionRegistry limitation documented
- [ ] W3-4: stat_core.rs renamed to metrics.rs
- [ ] W4-1: Server permission route table corrected
- [ ] W4-2: TTS keybinding verification completed
- [ ] W4-3: HalfOpen→Open timeout documented
- [ ] W4-4: Upgrade command behavior documented
- [ ] W5-1: EventProcessor documented
- [ ] W5-2: CompactionConfig thresholds verified
- [ ] W5-3: Fuzzy match example fixed
- [ ] W5-4: InlineScript deprecation logging added

---

## See Also

- [AGENTS.md](../AGENTS.md) - Root index file with module quick reference
- [AGENTS.override.md](../AGENTS.override.md) - Override file with verified facts
- `architecture/` - Architecture documentation per module
- `.opencode/skills/` - Module-specific skill guides

*(End of file)*