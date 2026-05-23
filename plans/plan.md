# Implementation Plan - Documentation Review Consolidation

**Status**: IN PROGRESS
**Last Updated**: 2026-05-23
**Goal**: Consolidate all architecture review findings into a single actionable plan with parallelization waves.

---

## Executive Summary

This plan consolidates findings from 28 module architecture reviews conducted in May 2026. The goal is to fix bugs, correct documentation inconsistencies, and create missing architecture/skills documentation.

### Summary Statistics

| Metric | Count |
|--------|-------|
| Total Items | 27 |
| High Priority (Bug Fixes) | 2 |
| Medium Priority (Documentation) | 23 |
| Low Priority | 2 |
| New Files to Create | 3 |
| Already Verified Fixed | 6 |

### Verification Notes
- Most issues are **documentation gaps**, not implementation bugs
- Implementation is generally correct; docs were out of sync
- Wave 0 items are independent and can be done in parallel by multiple agents
- Wave 1+ items have dependencies and should be done with consideration for order

---

## Wave 0: Quick Documentation Fixes (Under 30 min each)

All items in this wave are independent and can be done in parallel by multiple agents.

### DOC-1: Fix event count in event-bus.md
- **File**: `architecture/event-bus.md:83`
- **Issue**: Claims "Other Events (9)" but only 8 events are listed (AgentFinished counted twice)
- **Fix**: Change "Other Events (9)" to "Other Events (8)"
- **Status**: pending

### DOC-2: Sync event-bus skill line 84
- **File**: `.opencode/skills/event-bus/SKILL.md:84`
- **Issue**: Says "Other (8)" but lists 9 items
- **Fix**: Remove duplicate AgentFinished entry
- **Status**: pending

### DOC-3: Remove duplicate command table
- **File**: `architecture/command.md`
- **Issue**: Duplicate built-in commands table (lines 161-205)
- **Fix**: Remove duplicate table
- **Status**: pending

### DOC-4: Fix RenderFrame table placement
- **File**: `architecture/client.md`
- **Issue**: RenderFrame incorrectly in Server→Client table
- **Fix**: Move RenderFrame from Server→Client note to Client→Server table
- **Status**: pending

### DOC-5: Fix TUI theme count
- **File**: `architecture/tui.md`
- **Issue**: Claims "42 themes" but only 31 themes exist
- **Fix**: Change "42 themes" to "31 themes"
- **Status**: pending

### DOC-6: Update TUI line counts
- **File**: `architecture/tui.md`
- **Issue**: app/mod.rs line count outdated
- **Fix**: Update to current line count (5814)
- **Status**: pending

---

## Wave 1: New Documentation Files (1-2 hrs each)

### DOC-7: Create architecture/compaction.md
- **File**: `architecture/compaction.md` (NEW)
- **Priority**: HIGH
- **Issue**: No architecture documentation exists for the compaction module
- **Context**: Compaction is a critical feature for context window management
- **Reference**: See `src/agent/compaction.rs` (902 lines) and `src/agent/loop.rs` for integration
- **Status**: pending

### DOC-8: Create .opencode/skills/compaction/SKILL.md
- **File**: `.opencode/skills/compaction/SKILL.md` (NEW)
- **Priority**: HIGH
- **Issue**: No skill documentation for compaction module
- **Context**: Follow the module naming convention (agent-loop has agent-loop/SKILL.md, etc.)
- **Status**: pending

### DOC-9: Create .opencode/skills/hooks/SKILL.md
- **File**: `.opencode/skills/hooks/SKILL.md` (NEW)
- **Priority**: MEDIUM
- **Issue**: No skill documentation for hooks module
- **Context**: Document hook system, ShellCommandHook, Plugin hooks, execution order
- **Status**: pending

---

## Wave 2: Bug Fixes Requiring Code Changes

### BUG-6: FocusManager pop_dialog index bug - REAL BUG
- **File**: `src/tui/components/component/focus.rs:33-46`
- **Priority**: HIGH
- **Issue**: `pop_dialog()` reverses the index before removal, causing wrong dialog to be removed
- **Details**:
  - `position()` returns index from front (0 = first)
  - Code computes `idx_rev = stack.len() - 1 - idx` and removes `idx_rev`
  - Example: stack `[A,B,C,D,E]`, searching for `B` gives `idx=1`, `idx_rev=3`, removes `D` not `B`
- **Fix**: Use `pos` directly instead of `idx_rev`
  ```rust
  pub fn pop_dialog(&mut self, dialog_type: DialogType) -> Option<Box<dyn Component>> {
      let pos = self.stack.iter().position(|c| c.dialog_type() == dialog_type);
      if let Some(idx) = pos {
          return self.stack.remove(idx);  // Use idx, not idx_rev
      }
      None
  }
  ```
- **Status**: pending

---

## Wave 3: Snapshot & Resilience Documentation

### DOC-10: Fix snapshot capture flow documentation
- **File**: `architecture/snapshot.md`
- **Priority**: HIGH
- **Issue**: Doc shows "capture AFTER tool execution" but actual:
  - `capture_snapshot_if_needed()` is called BEFORE (loop.rs:1655)
  - `capture_incremental_snapshot_if_needed()` is called AFTER (loop.rs:1853)
- **Fix**: Update documentation to reflect actual two-phase capture flow
- **Status**: pending

### DOC-11: Add path validation to snapshot restore()
- **File**: `src/snapshot/mod.rs`
- **Priority**: HIGH
- **Issue**: `restore()` does NOT check if files escape project_root, but `restore_to_path()` does
- **Fix**: Add path validation to `restore()` using canonicalize() check
- **Context**: `restore_to_path()` at mod.rs:305-332 shows the correct pattern
- **Status**: pending

### DOC-12: Document Resilience half_open fields
- **File**: `architecture/resilience.md`
- **Priority**: MEDIUM
- **Issue**: `half_open_start_time` field and `max_half_open_duration` (30s) not documented
- **Fix**: Document both fields and the half-open timeout mechanism
- **Context**: `is_available()` sets `half_open_start_time` when transitioning Open→HalfOpen
- **Status**: pending

### DOC-13: Update LSP request_id type
- **File**: `architecture/lsp.md`
- **Priority**: MEDIUM
- **Issue**: Shows `AtomicI64` but actual is `AtomicU64` (unsigned avoids overflow)
- **Fix**: Update to `AtomicU64`
- **Status**: pending

---

## Wave 4: Architecture Corrections

### DOC-14: Add PROVIDER_NOT_FOUND to exec error codes
- **File**: `architecture/exec.md`
- **Priority**: MEDIUM
- **Issue**: `PROVIDER_NOT_FOUND` exists in source (exec.rs:217-218) but missing from doc
- **Fix**: Add to Error Codes table
- **Status**: pending

### DOC-15: Document SSE methods
- **Files**: `architecture/server.md`, `.opencode/skills/server/SKILL.md`
- **Priority**: MEDIUM
- **Issue**: `connect_sse()`, `connect_sse_stream()`, `take_sse_events()` undocumented
- **Fix**: Document these methods in server architecture and skill
- **Status**: pending

### DOC-16: Document IdeServer::run_socket()
- **File**: `architecture/ide.md`
- **Priority**: MEDIUM
- **Issue**: Socket-based transport mode exists but not documented
- **Fix**: Document run_socket() async I/O method
- **Status**: pending

---

## Wave 5: Skills, Crypto & Tool Documentation

### DOC-17: Document list_skill_resources()
- **File**: `.opencode/skills/skills/SKILL.md`
- **Priority**: MEDIUM
- **Issue**: `list_skill_resources()` function undocumented
- **Context**: Scans skill directory for additional resource files (excluding SKILL.md)
- **Status**: pending

### DOC-18: Document FORMAT_V2_PREFIX constant
- **Files**: `architecture/crypto.md`, `.opencode/skills/crypto/SKILL.md`
- **Priority**: MEDIUM
- **Issue**: `FORMAT_V2_PREFIX` constant undocumented
- **Context**: Public constant `"v2:"` used by `config/encryption.rs`
- **Status**: pending

### DOC-19: Document TTS speaking type and stop()
- **File**: `.opencode/skills/tts/SKILL.md`
- **Priority**: MEDIUM
- **Issue**: Tts.speaking type shown as `AtomicBool` but actual is `Mutex<AtomicBool>`
- **Fix**: Update to show `Mutex<AtomicBool>` wrapper
- **Context**: This is required for Clone implementation
- **Status**: pending

### DOC-20: Document Tool modules (teams, lsp, formatter)
- **File**: `.opencode/skills/tool/SKILL.md`
- **Priority**: MEDIUM
- **Issue**: teams.rs (5 TeamTools), lsp.rs (11 operations), formatter.rs undocumented
- **Fix**: Add documentation for these tool modules
- **Status**: pending

### DOC-21: Fix tool count claim
- **File**: `architecture/tool.md`
- **Priority**: LOW
- **Issue**: Claims "26 tools in with_defaults()" but table shows 28 entries
- **Fix**: Correct count and clarify team tools require separate registration
- **Status**: pending

---

## Wave 6: TUI & Util Documentation Updates

### DOC-22: Add handle_paste to Component trait
- **File**: `.opencode/skills/tui/SKILL.md`
- **Priority**: MEDIUM
- **Issue**: `handle_paste` default method not documented in Component trait
- **Fix**: Add documentation for the default method
- **Status**: pending

### DOC-23: Document UiState fields
- **File**: `architecture/tui.md`
- **Priority**: MEDIUM
- **Issue**: `dirty_regions`, `render_panic_count`, `last_render_error` undocumented
- **Fix**: Add these fields to UiState documentation
- **Status**: pending

### DOC-24: Document fuzzy_match vs fuzzy_score
- **File**: `.opencode/skills/util/SKILL.md`
- **Priority**: MEDIUM
- **Issue**: `fuzzy_match` returns distance (lower=better) but `fuzzy_score` returns similarity (higher=better)
- **Fix**: Add clarifying note about distance vs similarity metric
- **Status**: pending

### DOC-25: Document Histogram and MetricsSnapshot
- **File**: `.opencode/skills/util/SKILL.md`
- **Priority**: LOW
- **Issue**: Histogram 1000-element limit and MetricsSnapshot fields undocumented
- **Fix**: Document both
- **Status**: pending

### DOC-26: Document Worktree error handling
- **File**: `.opencode/skills/worktree/SKILL.md`
- **Priority**: MEDIUM
- **Issue**: Functions return `AppError::Worktree` but not documented
- **Fix**: Add error handling section for public functions
- **Status**: pending

---

## Verified Already Fixed (No Action Needed)

The following items were marked as bugs but verification confirms they are **already fixed**:

### BUG-1: IDE line range slicing fix - ✅ ALREADY FIXED
- Verified: `open_diff_generic()` now correctly receives sliced content at line 91
- No action needed

### BUG-2: Remove external_directory from PERMISSION_TYPES - ✅ ALREADY FIXED
- Verified: `external_directory` is NOT in PERMISSION_TYPES (lines 70-87)
- `check_external_directory` is marked `#[allow(dead_code)]` at line 1236
- No action needed

### BUG-3: TTS duplicate store(false) - ✅ ALREADY FIXED
- Verified: No duplicate `speaking.store(false)` in speak()
- Different execution paths (error vs success), not duplicates
- No action needed

### BUG-4: TTS stop() guard - ✅ ALREADY FIXED
- Verified: `stop()` correctly checks `is_speaking()` at line 85-88
- Returns `Ok(())` early if not speaking before pkill
- No action needed

### BUG-5: TTS init() error handling - ✅ ALREADY FIXED
- Verified: `TtsProvider` enum only has `None` variant (exhaustive match)
- Properly handles `TtsProvider::None` via match
- No action needed

### PTY location path - ✅ ALREADY FIXED
- Verified: Module is at `src/pty_session/`, `src/pty/` does not exist
- Architecture doc was updated in Wave 4 (May 26)
- No action needed

---

## Dependencies Graph

```
Wave 0 (Documentation fixes - all independent)
├── DOC-1 to DOC-6: All can run in parallel
└── Each targets a different file/section

Wave 1 (New files - can run in parallel after source research)
├── DOC-7: Create architecture/compaction.md
├── DOC-8: Create .opencode/skills/compaction/SKILL.md
└── DOC-9: Create .opencode/skills/hooks/SKILL.md

Wave 2 (Bug Fixes)
└── BUG-6: FocusManager pop_dialog - investigate first

Wave 3 (Snapshot/Resilience)
├── DOC-10: Snapshot flow - depends on loop.rs:1655, 1853
├── DOC-11: Snapshot restore path - depends on restore_to_path()
├── DOC-12: Resilience half_open - depends on circuit.rs:88, 114-127
└── DOC-13: LSP request_id - verify client.rs:42

Wave 4-6 (Documentation additions)
└── All can run after source verification
```

---

## Parallelization Strategy

### Items That Can Run in Parallel (Same Agent or Different Agents)

**Agent A**: Wave 0 items (DOC-1 through DOC-6) - all independent
**Agent B**: Wave 1 items (DOC-7, DOC-8, DOC-9) - all create new files
**Agent C**: FocusManager bug (BUG-6) - standalone fix
**Agent D**: Wave 3-6 documentation - all independent after verification

### Items That Should Run Sequentially
- BUG-6 (FocusManager): Should be verified and fixed before closing dialog-related work
- DOC-7 (compaction arch): Should be created before DOC-8 (compaction skill) for reference

---

## Verification Commands

After implementing changes, run:

```bash
# Build verification
cargo build --all-features

# Clippy check
cargo clippy --all-features -- -D warnings

# Module-specific tests
cargo test tts
cargo test tui
cargo test provider
cargo test session

# Focus test (after BUG-6 fix)
cargo test focus
```

---

## Status Summary

| Wave | Items | High | Medium | Low | Status |
|------|-------|------|--------|-----|--------|
| Wave 0 | 6 | 0 | 6 | 0 | pending |
| Wave 1 | 3 | 2 | 1 | 0 | pending |
| Wave 2 | 1 | 1 | 0 | 0 | pending |
| Wave 3 | 4 | 2 | 2 | 0 | pending |
| Wave 4 | 3 | 0 | 3 | 0 | pending |
| Wave 5 | 5 | 0 | 4 | 1 | pending |
| Wave 6 | 5 | 0 | 4 | 1 | pending |
| Already Fixed | 6 | 0 | 0 | 0 | ✅ COMPLETED |

### Remaining Work: 27 items

---

## Historical Context

The main `plan.md` in the root contains implementation status from previous sprint cycles (April-May 2026). This file contains the latest architecture review findings from the May 2026 module review sessions.

### Completed from Previous Reviews (May 2026)
- Wave 1-3 implementation: All completed via 25+ PRs
- Config module: Documentation accurate
- Agent module: All 41 claims verified
- Provider module: Architecture accurate
- Plugin module: Architecture accurate

---

## Notes for Future Agents

1. **Architecture docs vs actual code**: When implementing fixes, always verify against actual source
2. **Module naming**: Skills follow module naming (e.g., `compaction` skill at `.opencode/skills/compaction/SKILL.md`)
3. **TTS is macOS-only**: Uses hardcoded `say` command
4. **Permissions are synchronous**: `PermissionRegistry::respond()` is `fn`, not `async fn`
5. **WASM plugins use fuel tracking**: `return_fuel()` uses `MAX_PLUGIN_FUEL_BUDGET`
6. **FocusManager dialog stack**: Uses `VecDeque`, `pop_dialog()` by dialog_type removes wrong item if index reversal bug not fixed

---

## Implementation Phases (For Subagents)

### Phase 1: Quick Wins (4 agents in parallel)
- Agent 1: DOC-1, DOC-2 (event-bus fixes)
- Agent 2: DOC-3, DOC-4 (command/client fixes)
- Agent 3: DOC-5, DOC-6 (TUI fixes)
- Agent 4: DOC-7, DOC-8, DOC-9 (new documentation files)

### Phase 2: Critical Bug Fix (1 agent)
- Agent 5: BUG-6 (FocusManager pop_dialog)

### Phase 3: Documentation Updates (2-3 agents in parallel)
- Agent 6: DOC-10 through DOC-13 (snapshot, resilience, LSP)
- Agent 7: DOC-14 through DOC-16 (server, IDE)
- Agent 8: DOC-17 through DOC-21 (skills, crypto, tool)
- Agent 9: DOC-22 through DOC-26 (TUI, util, worktree)

---

*(Last updated: 2026-05-23)*