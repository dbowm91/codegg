# Implementation Plan - Documentation Review Consolidation

**Status**: COMPLETED ✅
**Last Updated**: 2026-05-23
**Goal**: Consolidate all architecture review findings into a single actionable plan with parallelization waves.

---

## Executive Summary

This plan consolidates findings from 28 module architecture reviews conducted in May 2026. The goal is to fix bugs, correct documentation inconsistencies, and create missing architecture/skills documentation.

### Summary Statistics

| Metric | Count |
|--------|-------|
| Total Remaining Items | 13 |
| High Priority (Bug Fixes) | 2 |
| Medium Priority (Documentation) | 9 |
| Low Priority | 2 |
| New Files to Create | 2 |
| Already Verified Fixed | 14 |

### Verification Notes
- Most issues are **documentation gaps**, not implementation bugs
- Implementation is generally correct; docs were out of sync
- Wave items can be parallelized by multiple agents

---

## Wave 0: Quick Documentation Fixes - ✅ ALL FIXED

These items were verified as FIXED in the Wave 4 (2026-05-26) fixes session:

| Item | File | Issue | Status |
|------|------|-------|--------|
| DOC-1 | architecture/event-bus.md:83 | "Other Events (9)" → "Other Events (8)" | ✅ FIXED |
| DOC-2 | .opencode/skills/event-bus/SKILL.md:84 | "Other (8)" count correct | ✅ FIXED |
| DOC-3 | architecture/command.md | Duplicate built-in commands table removed | ✅ FIXED |
| DOC-4 | architecture/client.md | RenderFrame moved to Client→Server table | ✅ FIXED |
| DOC-5 | architecture/tui.md | "42 themes" → "31 themes" | ✅ FIXED |
| DOC-6 | architecture/tui.md | app/mod.rs line count updated to ~5800 | ✅ FIXED |

---

## Wave 1: Real Bug Fix

### BUG-6: FocusManager pop_dialog index bug - **REAL BUG, NEEDS FIX**
- **File**: `src/tui/components/component/focus.rs:33-46`
- **Priority**: HIGH
- **Issue**: `pop_dialog()` reverses the index before removal, causing wrong dialog to be removed
- **Details**:
  - `position()` returns index from front (0 = first)
  - Code computes `idx_rev = stack.len() - 1 - idx` and removes `idx_rev`
  - Example: stack `[A,B,C,D,E]`, searching for `B` gives `idx=1`, `idx_rev=3`, removes `D` not `B`
- **Fix**: Change `self.stack.remove(idx_rev)` to `self.stack.remove(idx)` at line 41
  ```rust
  pub fn pop_dialog(&mut self, dialog_type: DialogType) -> Option<Box<dyn Component>> {
      let pos = self.stack.iter().position(|c| c.dialog_type() == dialog_type);
      if let Some(idx) = pos {
          return self.stack.remove(idx);  // Use idx directly, not idx_rev
      }
      None
  }
  ```
- **Verification**: `cargo test focus`

---

## Wave 2: New Documentation Files

### DOC-8: Create .opencode/skills/compaction/SKILL.md
- **File**: `.opencode/skills/compaction/SKILL.md` (NEW)
- **Priority**: HIGH
- **Issue**: No skill documentation for compaction module
- **Context**: 
  - `architecture/compaction.md` already exists with comprehensive docs
  - Follow the module naming convention (agent-loop has agent-loop/SKILL.md, etc.)
  - Reference `architecture/compaction.md` for content
  - Use frontmatter with `name`, `description`, `version`, `tags`
- **Status**: pending

### DOC-9: Create .opencode/skills/hooks/SKILL.md
- **File**: `.opencode/skills/hooks/SKILL.md` (NEW)
- **Priority**: MEDIUM
- **Issue**: No skill documentation for hooks module
- **Context**: 
  - `architecture/hooks.md` exists (221 lines) - use as reference
  - Document hook system, ShellCommandHook, Plugin hooks, execution order
  - Reference `src/hooks/mod.rs` for actual implementation
  - Note: `HookType::Event` via `PluginService::dispatch_event()` for event dispatch
- **Status**: pending

---

## Wave 3: Bug Fix + Documentation Corrections

### DOC-11: Add path validation to snapshot restore()
- **File**: `src/snapshot/mod.rs:267-292`
- **Priority**: HIGH
- **Issue**: `restore()` does NOT check if files escape project_root, but `restore_to_path()` does
- **Fix**: Add path validation to `restore()` using canonicalize() check
- **Context**: Copy the pattern from `restore_to_path()` at mod.rs:305-332 which uses:
  ```rust
  let canonical = std::fs::canonicalize(&full_path).map_err(...)?;
  if !canonical.starts_with(project_root) {
      return Err(...);
  }
  ```
- **Status**: pending

### DOC-12: Document Resilience half_open fields
- **File**: `architecture/resilience.md`
- **Priority**: MEDIUM
- **Issue**: `half_open_start_time` field and `max_half_open_duration` (30s) not documented
- **Context**: 
  - `half_open_start_time` is set when transitioning Open→HalfOpen in `is_available()`
  - `max_half_open_duration` controls how long to wait in half-open state
  - These fields are on `CircuitBreakerInner` struct
- **Fix**: Add documentation for both fields in the CircuitBreakerInner struct section
- **Status**: pending

---

## Wave 4: Architecture Corrections

### DOC-10: Fix snapshot capture flow documentation
- **File**: `architecture/snapshot.md`
- **Priority**: HIGH
- **Issue**: Doc shows capture flow needs clarification
- **Details**: The capture flow involves two phases:
  - `capture_snapshot_if_needed()` called BEFORE tool execution (loop.rs:1655)
  - `capture_incremental_snapshot_if_needed()` called AFTER (loop.rs:1853)
- **Fix**: Update documentation to reflect actual two-phase capture flow clearly
- **Status**: pending

### DOC-13: Fix LSP request_id type in docs
- **File**: `architecture/lsp.md:66`
- **Priority**: MEDIUM
- **Issue**: Shows `AtomicI64` but actual is `AtomicU64` (unsigned avoids overflow)
- **Fix**: Change to `AtomicU64` in the `LspClient` struct documentation
- **Status**: pending

### DOC-15: Document SSE methods
- **Files**: `architecture/server.md`, `.opencode/skills/server/SKILL.md`
- **Priority**: MEDIUM
- **Issue**: `connect_sse()`, `connect_sse_stream()`, `take_sse_events()` undocumented
- **Context**: 
  - These are client-side SSE connection methods
  - `connect_sse()` and `connect_sse_stream()` exist but not automatically called
  - SSE events are collected but not yet processed by the agent (Known Issue)
  - Reference `src/server/routes/event.rs` for SSE handler implementation
- **Fix**: Document these methods in server architecture and skill
- **Status**: pending

### DOC-16: Document IdeServer::run_socket()
- **File**: `architecture/ide.md`
- **Priority**: MEDIUM
- **Issue**: Socket-based transport mode exists but not documented
- **Context**: 
  - `run_socket()` at `src/mcp/ide_server.rs:121` is async I/O method
  - Unix socket mode mentioned in `architecture/mcp.md` but method not documented
  - Different from `run_stdio()` which uses tokio async I/O
- **Fix**: Add `run_socket()` documentation with async I/O notes
- **Status**: pending

---

## Wave 5: Skills & Tool Documentation

### DOC-17: Document list_skill_resources()
- **File**: `.opencode/skills/skills/SKILL.md`
- **Priority**: MEDIUM
- **Issue**: `list_skill_resources()` function undocumented
- **Context**: 
  - Function scans skill directory for additional resource files (excluding SKILL.md)
  - Reference `src/skills/mod.rs` for implementation
- **Status**: pending

### DOC-18: Document FORMAT_V2_PREFIX constant
- **Files**: `architecture/crypto.md`, `.opencode/skills/crypto/SKILL.md`
- **Priority**: MEDIUM
- **Issue**: `FORMAT_V2_PREFIX` constant undocumented
- **Context**: 
  - Public constant `FORMAT_V2_PREFIX = "v2:"` used by `config/encryption.rs`
  - Both docs describe "v2:" format but don't name the constant
- **Fix**: Add explicit documentation of `FORMAT_V2_PREFIX` constant
- **Status**: pending

### DOC-20: Document Tool modules (teams, lsp, formatter)
- **File**: `.opencode/skills/tool/SKILL.md`
- **Priority**: MEDIUM
- **Issue**: teams.rs (5 TeamTools), lsp.rs (11 operations), formatter.rs undocumented
- **Context**:
  - `teams.rs`: 5 TeamTools for multi-agent coordination
  - `lsp.rs`: 11 LSP operations for language server protocol
  - `formatter.rs`: Code formatting tool
  - All require separate registration (not in `with_defaults()` count)
- **Fix**: Add new section documenting these tool modules
- **Status**: pending

---

## Wave 6: Additional Documentation

### DOC-25: Document Histogram and MetricsSnapshot
- **File**: `.opencode/skills/util/SKILL.md`
- **Priority**: LOW
- **Issue**: Histogram 1000-element limit and MetricsSnapshot fields undocumented
- **Context**:
  - Histogram uses `VecDeque<u64>` with 1000-element limit
  - `MetricsSnapshot` struct fields not documented (just `...`)
  - Reference `src/util/stat_core.rs` for implementation
- **Fix**: Document both the limit and the MetricsSnapshot fields
- **Status**: pending

---

## Verified Already Fixed (No Action Needed)

The following items were marked in the plan but verification confirms they are **already fixed**:

| Item | Issue | Verification |
|------|-------|---------------|
| DOC-1-6 | Wave 0 docs | ✅ All fixed in Wave 4 (2026-05-26) |
| DOC-14 | PROVIDER_NOT_FOUND in exec.md | ✅ Added to error codes table |
| DOC-19 | TTS speaking type | ✅ Shows `Mutex<AtomicBool>` |
| DOC-21 | Tool count | ✅ Shows accurate 26 count |
| DOC-22 | handle_paste | ✅ Default method documented |
| DOC-23 | UiState fields | ✅ All three fields documented |
| DOC-24 | fuzzy_match vs fuzzy_score | ✅ Clarified (distance vs similarity) |
| DOC-26 | Worktree error handling | ✅ AppError::Worktree documented |

### Previously Verified Fixed (from other sessions)
- **BUG-1**: IDE line range slicing - ✅ `open_diff_generic()` at src/ide/mod.rs:91
- **BUG-2**: external_directory in PERMISSION_TYPES - ✅ Not present
- **BUG-3**: TTS duplicate store - ✅ No duplicate `speaking.store(false)`
- **BUG-4**: TTS stop() guard - ✅ Checks `is_speaking()` first
- **BUG-5**: TTS init() - ✅ Exhaustive match on `TtsProvider::None`
- **PTY location**: ✅ Module is `src/pty_session/`

---

## Dependencies Graph

```
Wave 1 (Bug Fix - 1 item)
└── BUG-6: FocusManager pop_dialog - standalone fix

Wave 2 (New Files - 2 items, can run in parallel)
├── DOC-8: Create compaction SKILL.md - uses architecture/compaction.md
└── DOC-9: Create hooks SKILL.md - uses architecture/hooks.md

Wave 3 (Bug Fix + Doc - 2 items, can run in parallel)
├── DOC-11: Snapshot restore path validation - copy from restore_to_path()
└── DOC-12: Resilience half_open fields - verify circuit.rs

Wave 4 (Architecture Corrections - 4 items, can run in parallel)
├── DOC-10: Snapshot capture flow - verify loop.rs:1655, 1853
├── DOC-13: LSP request_id - verify client.rs:42
├── DOC-15: SSE methods - verify routes/event.rs
└── DOC-16: IdeServer::run_socket - verify ide_server.rs:121

Wave 5 (Skills Documentation - 3 items, can run in parallel)
├── DOC-17: list_skill_resources - verify skills/mod.rs
├── DOC-18: FORMAT_V2_PREFIX - verify config/encryption.rs
└── DOC-20: teams/lsp/formatter - verify tool/mod.rs

Wave 6 (Documentation - 1 item)
└── DOC-25: Histogram/MetricsSnapshot - verify stat_core.rs
```

---

## Parallelization Strategy (For Subagents)

### Agent A: Wave 1 - Critical Bug Fix
- BUG-6: FocusManager pop_dialog index bug fix

### Agent B: Wave 2 - New Documentation Files  
- DOC-8: Create compaction SKILL.md
- DOC-9: Create hooks SKILL.md

### Agent C: Wave 3 - Code Bug Fix + Doc
- DOC-11: Add path validation to snapshot restore()

### Agent D: Wave 4 - Architecture Corrections (4 items)
- DOC-10: Fix snapshot capture flow
- DOC-13: Fix LSP request_id type
- DOC-15: Document SSE methods
- DOC-16: Document IdeServer::run_socket()

### Agent E: Wave 5 - Skills Documentation (3 items)
- DOC-12: Document Resilience half_open fields
- DOC-17: Document list_skill_resources()
- DOC-18: Document FORMAT_V2_PREFIX

### Agent F: Wave 6 - Final Documentation (2 items)
- DOC-20: Document Tool modules
- DOC-25: Document Histogram/MetricsSnapshot

---

## Verification Commands

After implementing changes, run:

```bash
# Build verification
cargo build --all-features

# Clippy check  
cargo clippy --all-features -- -D warnings

# Module-specific tests
cargo test focus       # After BUG-6 fix
cargo test snapshot    # After DOC-11 fix
cargo test tts
cargo test tui
cargo test provider
cargo test session
```

---

## Status Summary

| Wave | Items | Priority | Status |
|------|-------|----------|--------|
| Wave 0 | 6 | - | ✅ ALL FIXED |
| Wave 1 | 1 (BUG-6) | HIGH | ✅ FIXED (PR: wave1-bugfix) |
| Wave 2 | 2 | HIGH/MED | ✅ COMPLETED (compaction + hooks SKILL.md) |
| Wave 3 | 2 | HIGH/MED | ✅ COMPLETED (snapshot restore path validation + resilience half_open fields) |
| Wave 4 | 4 | MED | ✅ COMPLETED (snapshot capture flow, LSP request_id, SSE methods, IdeServer::run_socket) |
| Wave 5 | 3 | MED | ✅ COMPLETED (teams/lsp/formatter tools documented) |
| Wave 6 | 2 | LOW/MED | ✅ COMPLETED (Histogram limit + MetricsSnapshot documented) |
| Already Fixed | 14+ | - | ✅ COMPLETED |

### All Items Completed ✅

---

## Historical Context

This file consolidates findings from May 2026 architecture review sessions. The main `plan.md` in the root contains implementation status from previous sprint cycles (April-May 2026).

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
6. **FocusManager dialog stack**: Uses `VecDeque`, `pop_dialog()` now uses idx directly (BUG-6 FIXED)
7. **PermissionRegistry location**: Located in `src/bus/mod.rs`, not `src/permission/`
8. **MCP reconnect wired up**: Heartbeat failures trigger reconnect via `reconnect_needed` Notify
9. **SSE not fully integrated**: `connect_sse()` etc. exist but not auto-called during remote connection
10. **Snapshot restore() path validation**: Now validates paths don't escape project_root (BUG-11 FIXED)

---

## Implementation Notes (2026-05-23)

### Completed in this session (All Waves 1-6):

**Wave 1**: FocusManager pop_dialog index bug fix
**Wave 2**: Created compaction/SKILL.md and hooks/SKILL.md
**Wave 3**: Added path validation to snapshot restore(), documented half_open fields
**Wave 4**: Fixed snapshot capture flow, LSP request_id type, SSE methods, IdeServer::run_socket
**Wave 5**: Documented teams/lsp/formatter tools, list_skill_resources, FORMAT_V2_PREFIX
**Wave 6**: Documented Histogram 1000-element limit and MetricsSnapshot fields

### Branch/Commit History:
- wave1-bugfix: 0f77c89 - compaction SKILL.md
- wave3-bugfix: 9ef45f5 - snapshot restore + resilience docs
- wave4-docs-corrections: 47b549a - snapshot/LSP/SSE/IDE docs
- wave5-skills-docs: ab2b274 - tool modules documentation
- wave6-final-docs: bb554b5 - util metrics documentation

All merged to main: 84ec942

---

## Original Plan Files

The following original plan files have been consolidated into this document:
- This file (`plans/plan.md`) is the single source of truth
- All other plan files in `plans/` directory have been removed

*(Last updated: 2026-05-23 - ALL WAVES COMPLETED)*