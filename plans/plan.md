# Implementation Plan - Code Review Consolidation (Phase 2)

**Status**: Active
**Created**: 2026-05-25
**Last Updated**: 2026-05-25

---

## Summary

This plan consolidates remaining items from Phase 1 review (2026-05-24) that need attention. Many items from initial batch reviews were already fixed.

### Items Status

| Category | Count | Status |
|----------|-------|--------|
| Code Bugs | 1 | Pending |
| Documentation Corrections | 6 | Pending |

---

## Wave 1: Code Bug Fix

### 1.1 Command Module Panic on Error
**File**: `src/command/mod.rs:21-24`
**Severity**: High
**Issue**: `find_command_files()` panics with `panic!("expected")` on error instead of gracefully skipping failed commands
**Current Code**:
```rust
pub async fn find_command_files(base: &Path) -> Vec<Command> {
    find_command_files_sync(base).into_iter().map(|r| r.unwrap_or_else(|e| {
        warn!("Failed to load command: {}", e);
        panic!("expected")
    })).collect()
}
```
**Fix**: Change to use `filter_map(|r| r.ok())` pattern to skip failed commands gracefully:
```rust
pub async fn find_command_files(base: &Path) -> Vec<Command> {
    find_command_files_sync(base)
        .into_iter()
        .filter_map(|r| r.ok())
        .collect()
}
```
**Verification**: Sync version handles errors gracefully - async version should match

---

## Wave 2: Documentation Corrections

### 2.1 Overview Architecture - Component/Dialog Counts
**File**: `architecture/overview.md`
**Issue**: Inconsistent component/dialog counts in same document
**Fixes**:
- Line 25: Change "Components (17)" to "Components (14)" OR update line 285 to say "14 reusable widgets" (not 17)
- Line 25: Change "Dialogs (21)" to "Dialogs (20)" OR update line 286 to say "20 modal dialogs" (not 21)
**Note**: PermissionRegistry/QuestionRegistry location (lines 152-153) is CORRECT (src/bus/mod.rs)

### 2.2 MCP Architecture - Heartbeat Task Field
**File**: `architecture/mcp.md`
**Issue**: Line 117 incorrectly shows `heartbeat_task: Arc<AtomicBool>` in `McpConnectionManager` struct
**Actual Fields** (lines 107-119):
```rust
client: RemoteClient,
state: Arc<Mutex<ConnectionState>>,
retry_count: Arc<AtomicU64>,
max_retries: u64,
base_delay: Duration,
max_delay: Duration,
heartbeat_interval: Duration,
shutdown: Arc<Notify>,        // line 117
reconnect_needed: Arc<Notify>, // line 118
```
**Fix**: Remove `heartbeat_task: Arc<AtomicBool>` from documentation

### 2.3 Core Architecture - CoreRequest Variants
**File**: `architecture/core.md:56-60`
**Issue**: "Request Families" section doesn't explicitly list all CoreRequest variants
**Fix**: Add explicit enumeration of variants:
- `Initialize`
- `Subscribe`
- `Resume`
- `TurnCancel`
- `TurnSteer`
- `AgentSelect`
- `ModelSelect`

### 2.4 LSP Architecture - Server Count
**File**: `architecture/lsp.md`
**Issue**: Line 229 says "44 servers" but actual count is **40**
**Fix**: Update to "Supported Languages (40 servers)"

### 2.5 Config Architecture - Line Number References
**File**: `architecture/config.md`
**Issues**:
- Line 221: `decrypt_provider_keys()` at `watcher.rs` is line **163** (not 157-158)
- Lines 223-224: `decrypt_provider_keys()` at `schema.rs:542` is CORRECT
**Fix**: Update line 221 reference to `watcher.rs:163`

### 2.6 Command Architecture - Built-in Command Count
**File**: `architecture/command.md:52, 115`, `.opencode/skills/command/SKILL.md`
**Issue**: Documents 36 built-in commands but actual count is **41**
**Fix**: Update count from 36 to 41 in both files

---

## Completed Items (Verified)

The following were initially flagged but are already correct:

| Item | Status | Notes |
|------|--------|-------|
| Hooks architecture line 191 | ✅ Fixed | Now correctly states AgentEnd hooks don't run on stream errors |
| Snapshot restore integration | ✅ Documented | Architecture correctly notes restore() is available but not integrated |
| TUI TuiMsg::SelectSession | ✅ Fixed | Architecture shows `SelectSession(Box<Session>)` correctly |
| TUI OpenDiffDialog fields | ✅ Fixed | Architecture shows `Box<str>` correctly |
| TUI TuiMsg variants | ✅ Fixed | ExternalEditor, UndoDelete, ConfirmResult now documented |
| TUI Shift+Tab | ✅ Fixed | Architecture shows "Toggle permission mode" correctly |
| TUI InfoDialog | ✅ Fixed | Architecture notes single InfoDialog with InfoType enum |
| ServerRuntimeError | ✅ Fixed | Architecture shows all 5 variants correctly |
| TTS configuration | ✅ Fixed | Architecture notes no [tts].enabled config exists |
| TTS stop()/is_speaking() | ✅ Fixed | Methods documented |
| PermissionRegistry location | ✅ Fixed | Correctly in src/bus/mod.rs |
| Plugin WASM path | ✅ Fixed | Uses plugins_dir() correctly |
| Error skill Auth variant | ✅ Fixed | ProviderError::Auth(_) in is_retryable() |
| Permission skill docs mode | ✅ Fixed | Shows default: "ask" correctly |
| Command skill line references | ✅ Verified | Template execution and frontmatter parsing lines correct |

---

## Verification Commands

```bash
cargo check  # Should pass
cargo test   # All tests should pass
```

---

## Implementation Guidance

### Parallelization

**Wave 1 (Code Bug)** should be done first as it affects runtime behavior:
- Agent A: Fix command/mod.rs panic bug

**Wave 2 (Documentation)** items are independent and can be done in parallel:
- Agent B: Overview architecture counts
- Agent C: MCP architecture heartbeat field
- Agent D: Core architecture CoreRequest variants
- Agent E: LSP architecture server count
- Agent F: Config architecture line numbers
- Agent G: Command architecture count

### Dependencies

- Wave 1 is prerequisite for nothing (can be done anytime)
- Wave 2 documentation fixes have no dependencies on each other

---

*Plan created by consolidating 33 review files across all modules (2026-05-25)*
