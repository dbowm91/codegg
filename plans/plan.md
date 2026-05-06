# Code Review Consolidation Plan

**Status**: Draft - Needs Implementation
**Last Updated**: 2026-05-06
**Goal**: Address all HIGH severity issues identified across 28 module reviews in a coordinated, parallelizable implementation approach.

---

## Overview

This plan consolidates findings from 28 module reviews into a coordinated implementation strategy. Issues are organized into "waves" representing groups of work that can be performed in parallel by separate sub-agents.

### Verification Status of Current Codebase

The following items have been **verified as correct** (not bugs):

| Item | Location | Status |
|------|----------|--------|
| `process_request()` in `worker.rs:278-303` | agent/worker.rs | VERIFIED - correctly publishes SubagentStarted/SubagentCompleted events and returns SubAgentResult::success() |
| WebSocket rate limiter fallback | provider/mod.rs | VERIFIED - If REDIS_URL is set → use Redis; otherwise → use in-memory |
| `SubAgentPool` bounded concurrency (5) | agent/worker.rs | VERIFIED - Config `subagent.max_concurrent` defaults to 5 |
| Tool definition caching | agent/loop.rs | VERIFIED - `tool_def_cache` with version key |

---

## Wave Architecture

Issues are organized into waves based on:
1. **Independence**: Can be fixed without waiting for other fixes
2. **Risk**: Lower-risk fixes first, then high-risk refactors
3. **Dependencies**: Foundational issues before dependent issues
4. **Parallelization**: Items that can run concurrently in separate subagents

### Wave 1: Critical Memory & Race Conditions (Foundation)
*Execute these first - they affect system stability*

### Wave 2: Security & Data Integrity
*Execute in parallel after Wave 1*

### Wave 3: API Correctness & Reliability
*Execute in parallel with Wave 2*

### Wave 4: Code Quality & Documentation
*Execute after Waves 1-3*

---

## Wave 1: Critical Memory & Race Conditions

### 1.1 Memory Module Doesn't Persist - CRITICAL
**Module**: memory/
**Severity**: HIGH
**File**: `src/memory/mod.rs:92-117`

**Problem**: `add()` and `delete()` methods only modify in-memory HashMap. The `save()` method exists but is never called by either operation. Data is lost on restart.

**Implementation**:
1. Modify `add()` to call `self.save()` after insertion
2. Modify `delete()` to call `self.save()` after removal
3. Alternatively, implement auto-save on a interval or shutdown hook

**Files to Modify**:
- `src/memory/mod.rs`

**Verification**: Restart application and verify memories persist across sessions.

---

### 1.2 Bus Module Dead Letter Channels - CRITICAL
**Module**: bus/
**Severity**: HIGH
**File**: `src/bus/mod.rs:21-89`

**Problem**: When a sender (permission or question response) is dropped without answering, the entry remains in `DashMap` forever. No cleanup mechanism. Additionally, `respond()` silently ignores send failures with `let _ = tx.send(choice)`.

**Implementation**:
1. Add TTL-based cleanup or use channel with drop notification
2. Log when send fails instead of silently ignoring

**Files to Modify**:
- `src/bus/mod.rs`

**Alternative Approach**: Consider replacing `DashMap` with a watch channel that notifies on drop.

**Verification**: Create test that verifies cleanup occurs when receiver dropped.

---

### 1.3 PlanRegistry `wait_for_response()` Bug - HIGH
**Module**: agent/
**Severity**: HIGH
**File**: `src/agent/plan_registry.rs:75-98`

**Problem**: Send-then-discard pattern:
```rust
let _ = tx.send(PlanResponse::Cancelled).await;  // Send first
let _ = response_tx;  // Then discard receiver
// Then tries to receive on dropped channel
```

**Implementation**: Either:
- Remove `PlanRegistry` entirely if unused (no callers found in codebase)
- Fix the send/receive pattern to properly await a response

**Files to Modify**:
- `src/agent/plan_registry.rs`

**Verification**: If keeping the code, verify `wait_for_response()` actually waits for a response.

---

### 1.4 Session `share_session` Race Condition - HIGH
**Module**: session/
**Severity**: HIGH
**File**: `src/session/store.rs:1290-1313`

**Problem**: UPSERT into `session_share` and `set_share_url` are not atomic. If `set_share_url` fails, the session_share record exists with a different URL than what the session record shows.

**Implementation**:
1. Wrap both operations in a single database transaction
2. Use `sqlx::query` with explicit transaction

**Files to Modify**:
- `src/session/store.rs`

**Verification**: Write test that verifies atomicity - if second operation fails, first should rollback.

---

### 1.5 Resilience Circuit Breaker TOCTOU Race - HIGH
**Module**: resilience/
**Severity**: HIGH
**File**: `src/resilience/circuit.rs:67-84`

**Problem**: `is_available()` has time-of-check-time-of-use race:
1. Read lock acquired, checks state is Open and timeout elapsed
2. Read lock released
3. Another task can transition state
4. Write lock acquired, modifies state

**Implementation**: Use atomic compare-and-swap pattern:
```rust
pub async fn is_available(&self) -> bool {
    let mut state = self.state.write().await;
    match *state {
        CircuitState::Closed => true,
        CircuitState::HalfOpen => true,
        CircuitState::Open => {
            if let Some(last_failure) = *self.last_failure_time.read().await {
                if last_failure.elapsed() >= Duration::from_secs(self.timeout_secs) {
                    *state = CircuitState::HalfOpen;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
    }
}
```

**Files to Modify**:
- `src/resilience/circuit.rs`

**Verification**: Run concurrent access tests with multiple tasks calling `is_available()`.

---

### 1.6 Config Watcher Race Condition - HIGH
**Module**: config/
**Severity**: HIGH
**File**: `src/config/watcher.rs:55-76`

**Problem**: Closure captures `tx` before stored in `self.watcher`. The watcher callback sends on `tx` but `recv()` is async and could be awaiting while callback fires.

**Implementation**:
1. Use `Arc<Mutex<...>>` for shared state between watcher callback and recv
2. Or use `notify::RecommendedWatcher` with proper synchronization

**Files to Modify**:
- `src/config/watcher.rs`

**Verification**: Test concurrent start/recv operations.

---

## Wave 2: Security & Data Integrity

### 2.1 Auth Middleware Broken - CRITICAL
**Module**: server/
**Severity**: HIGH
**File**: `src/server/middleware/auth.rs`

**Problem**: Wrong signature and undefined variables in auth middleware.

**Implementation**: Fix the middleware signature and ensure all variables are properly defined before use.

**Files to Modify**:
- `src/server/middleware/auth.rs`

---

### 2.2 IDE Temp File Race Condition - HIGH
**Module**: ide/
**Severity**: HIGH
**File**: `src/ide/ide.rs`

**Problem**: Using predictable temp file names (`codegg_original`, `codegg_modified`) without `mkstemp` or `tempfile` crate. Concurrent calls can overwrite each other's temp files.

**Implementation**:
```rust
// Use tempfile crate for automatic unique naming
let original_temp = tempfile::NamedTempFile::new()?;
let modified_temp = tempfile::NamedTempFile::new()?;
```

**Files to Modify**:
- `src/ide/ide.rs`

---

### 2.3 Snapshot No Persistence - HIGH
**Module**: snapshot/
**Severity**: HIGH
**File**: `src/snapshot/mod.rs`

**Problem**: In-memory only, lost on restart. Needs SQLite persistence.

**Implementation**:
1. Add snapshot table to session database
2. Serialize snapshot state and store on save
3. Load snapshots from database on startup

**Files to Modify**:
- `src/snapshot/mod.rs`
- `src/storage/mod.rs` or `src/session/schema.rs`

---

### 2.4 Plugin Cache Invalidation Bug - HIGH
**Module**: plugin/
**Severity**: HIGH
**File**: `src/plugin/loader.rs:156-162`

**Problem**: Uses `.elapsed()` instead of actual modification timestamp:
```rust
let mtime = std::fs::metadata(path)
    .ok()?
    .modified()
    .ok()?
    .elapsed()  // WRONG: returns Duration since mod, not timestamp
    .ok()?
    .as_secs();
```

**Implementation**:
```rust
let mtime = std::fs::metadata(path)
    .ok()?
    .modified()
    .ok()?
    .duration_since(std::time::UNIX_EPOCH)
    .ok()?
    .as_secs();
```

**Files to Modify**:
- `src/plugin/loader.rs`

---

### 2.5 Tool Symlink Bypass - HIGH
**Module**: tool/
**Severity**: HIGH
**File**: `src/tool/read.rs`, `src/tool/write.rs`, `src/tool/edit.rs`

**Problem**: `canonicalize_path()` doesn't check intermediate symlinks. Use `check_path_for_symlinks()` before canonicalization.

**Implementation**:
1. Add `check_path_for_symlinks()` call before `canonicalize_path()`
2. Reject paths with symlink components

**Files to Modify**:
- `src/tool/read.rs`
- `src/tool/write.rs`
- `src/tool/edit.rs`

---

## Wave 3: API Correctness & Reliability

### 3.1 MCP ConnectionManager Clone Unsound - HIGH
**Module**: mcp/
**Severity**: HIGH
**File**: `src/mcp/remote.rs:179-193`

**Problem**: Clone impl clones internal Arc pointers but shares mutable state. Multiple clones can modify same connection simultaneously, violating Rust's aliasing rules.

**Implementation**: Either:
- Remove Clone implementation entirely
- Use `Arc<Mutex<...>>` internally to make clone safe
- Implement proper copy-on-write semantics

**Files to Modify**:
- `src/mcp/remote.rs`

---

### 3.2 LSP Request ID Race - HIGH
**Module**: lsp/
**Severity**: HIGH
**File**: `src/lsp/client.rs:451-457`

**Problem**: Wrap-around issue with `request_id.fetch_add()`. When counter wraps, could cause ID collisions.

**Implementation**:
1. Use stronger atomic operations
2. Consider using a UUID per request instead of sequential integers
3. Add overflow handling

**Files to Modify**:
- `src/lsp/client.rs`

---

### 3.3 `process_request()` Dead Code Confusion - HIGH
**Module**: agent/
**Severity**: HIGH
**File**: `src/agent/worker.rs:278-303`

**Problem**: Public method that appears to handle subagent requests but only publishes events and returns formatted string. Misleading API if external callers expect actual execution.

**Implementation**: Either:
- Remove the method entirely (recommended if unused)
- Document clearly that it only publishes events
- Have it actually queue and execute via worker pool

**Files to Modify**:
- `src/agent/worker.rs`

---

### 3.4 IdeServer Blocking I/O - HIGH
**Module**: mcp/
**Severity**: HIGH
**File**: `src/mcp/ide_server.rs:79-113`

**Problem**: `stdin.read_line()` and `stdout.write_all()` are synchronous operations in async context.

**Implementation**:
```rust
use tokio::io::{stdin, stdout, AsyncReadExt, AsyncWriteExt};

async fn run_stdio(&self) -> Result<(), AppError> {
    let mut stdin = stdin();
    let mut stdout = stdout();
    // ... use async read/write
}
```

**Files to Modify**:
- `src/mcp/ide_server.rs`

---

### 3.5 Client Orphaned Input Channel - HIGH
**Module**: client/
**Severity**: HIGH
**File**: `src/client/attach.rs:98, 105-115`

**Problem**: `input_rx` created but never populated. `input_handler` task waits forever on channel that never receives data.

**Implementation**: Either:
- Remove the orphaned `input_handler` and `input_rx` if input handling not needed
- Wire up proper input source

**Files to Modify**:
- `src/client/attach.rs`

---

### 3.6 OAuth Replay Protection Race - HIGH
**Module**: mcp/
**Severity**: HIGH
**File**: `src/mcp/auth.rs:318-332`

**Problem**: Marks code as used BEFORE verifying exchange succeeds. If exchange fails after marking, code is permanently unusable.

**Implementation**: Reorder operations - mark code used only AFTER successful token exchange.

**Files to Modify**:
- `src/mcp/auth.rs`

---

### 3.7 Storage Race Condition - HIGH
**Module**: storage/
**Severity**: HIGH
**File**: `src/storage/mod.rs`

**Problem**: `std::fs::File::create` vs SQLite atomic creation race.

**Implementation**: Use SQLite's atomic file creation or ensure proper file locking.

**Files to Modify**:
- `src/storage/mod.rs`

---

## Wave 4: Code Quality & Documentation

### 4.1 Commands.rs Duplicate Code
**Module**: command/
**Severity**: HIGH
**File**: `src/tui/app/commands.rs`

**Problem**: `handle_slash_command` appears twice (lines 62-288 and 323-536). Same with `on_paste`/`on_resize`.

**Implementation**:
1. Deduplicate into single implementations
2. Remove dead `execute_command` function (lines 538-727) if unused

**Files to Modify**:
- `src/tui/app/commands.rs`

---

### 4.2 Hook Errors Silently Swallowed
**Module**: hooks/
**Severity**: HIGH
**File**: `src/agent/loop.rs:1326-1329, 1366-1370`

**Problem**: `let _ = hr.run_hooks(...)` - failures not logged or reported.

**Implementation**:
```rust
if let Some(ref hr) = hook_registry {
    if let Err(e) = hr.run_hooks(HookEvent::PreToolExecute, &pre_ctx).await {
        tracing::warn!("Pre-tool hook failed: {}", e);
    }
}
```

**Files to Modify**:
- `src/agent/loop.rs`

---

### 4.3 Provider debug_log! Macro Performance Bug
**Module**: provider/
**Severity**: HIGH
**File**: `src/provider/mod.rs:7-17`

**Problem**: Opens file handle on every invocation - severe performance degradation.

**Implementation**: Replace with `tracing::debug!` or implement buffered writer:
```rust
use std::sync::OnceLock;
static WRITER: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
// Or simply use tracing crate
```

**Files to Modify**:
- `src/provider/mod.rs`

---

### 4.4 Missing Exponential Backoff
**Module**: provider/
**Severity**: HIGH
**File**: `src/provider/fallback.rs`

**Problem**: No delay between provider retries. Hammer rate-limited providers immediately.

**Implementation**:
```rust
let delay = std::time::Duration::from_secs(30).min(base_delay * 2^i);
tokio::time::sleep(delay).await;
```

**Files to Modify**:
- `src/provider/fallback.rs`

---

### 4.5 Plugin dispatch_to_plugin No-Op
**Module**: plugin/
**Severity**: HIGH
**File**: `src/plugin/event_bus.rs:63-69`

**Problem**: Function only logs, doesn't actually dispatch to plugins.

**Implementation**: Either implement actual dispatch or remove dead code.

**Files to Modify**:
- `src/plugin/event_bus.rs`

---

### 4.6 Missing Hook Events
**Module**: hooks/
**Severity**: HIGH
**File**: `src/hooks/mod.rs:19-22`

**Problem**: `SessionStart`, `SessionEnd`, `AgentStart`, `AgentEnd` defined but never triggered.

**Implementation**: Emit events in `agent/loop.rs`:
- `SessionStart` at beginning of `AgentLoop::run()`
- `SessionEnd` on loop termination
- `AgentStart`/`AgentEnd` around agent execution

**Files to Modify**:
- `src/hooks/mod.rs`
- `src/agent/loop.rs`

---

### 4.7 Permission DoomLoop O(n) Issue
**Module**: permission/
**Severity**: HIGH
**File**: `src/permission/mod.rs`

**Problem**: DoomLoopDetector uses `VecDeque` with O(n) iteration but docs claim O(1).

**Implementation**: Use proper O(1) data structure (HashSet or HashMap with timestamp) for membership tracking.

**Files to Modify**:
- `src/permission/mod.rs`

---

### 4.8 PTY Module Misleading Name
**Module**: pty/
**Severity**: MEDIUM
**File**: `src/pty/mod.rs`

**Problem**: Module name suggests PTY support but only manages session metadata.

**Implementation**: Either:
- Rename to `pty-session` or `session-metadata`
- Or implement actual PTY support

**Files to Modify**:
- `src/pty/mod.rs` (rename)
- Update all references in `lib.rs` and `main.rs`

---

## Parallelization Strategy

### Subagent Group A (Wave 1 - Memory/Race)
- `memory/mod.rs` - Add persistence
- `bus/mod.rs` - Dead letter channels
- `agent/plan_registry.rs` - Fix or remove
- `session/store.rs` - Atomic share_session
- `resilience/circuit.rs` - Fix TOCTOU
- `config/watcher.rs` - Fix race

### Subagent Group B (Wave 2 - Security)
- `server/middleware/auth.rs` - Fix auth middleware
- `ide/ide.rs` - Fix temp file race
- `snapshot/mod.rs` - Add persistence
- `plugin/loader.rs` - Fix cache invalidation
- `tool/read.rs, write.rs, edit.rs` - Fix symlink bypass

### Subagent Group C (Wave 3 - API Correctness)
- `mcp/remote.rs` - Fix Clone unsoundness
- `lsp/client.rs` - Fix request ID wrap-around
- `agent/worker.rs` - Fix process_request
- `mcp/ide_server.rs` - Fix blocking I/O
- `client/attach.rs` - Fix orphaned input channel
- `mcp/auth.rs` - Fix OAuth race
- `storage/mod.rs` - Fix race condition

### Subagent Group D (Wave 4 - Code Quality)
- `command/commands.rs` - Remove duplication
- `agent/loop.rs` - Handle hook errors
- `provider/mod.rs` - Replace debug_log macro
- `provider/fallback.rs` - Add exponential backoff
- `plugin/event_bus.rs` - Implement or remove dispatch
- `hooks/mod.rs` + `agent/loop.rs` - Emit missing events
- `permission/mod.rs` - Fix DoomLoop O(1)
- `pty/mod.rs` - Rename or implement

---

## Implementation Notes

### Database Migrations
If adding new persistence (snapshot, memory), create a new migration in `src/session/schema.rs`:
```rust
async fn migrate_v13(tx: &mut Transaction<'_>) -> Result<(), DbError> {
    // Add snapshot table
    sqlx::query("CREATE TABLE IF NOT EXISTS snapshot (...)").execute(&mut *tx).await?;
    Ok(())
}
```

### Testing Strategy
1. Unit tests for each fix in the relevant module
2. Integration tests for cross-module issues (e.g., session race condition)
3. Concurrency tests for race condition fixes
4. Manual verification for UI changes

### Verification Commands
```bash
# Run all tests
cargo test --workspace

# Run specific module tests
cargo test -p codegg-memory
cargo test -p codegg-session
cargo test -p codegg-bus

# Run with concurrency test
cargo test -p codegg-resilience -- --test-threads=10
```

---

## Commit History Reference

Original review plans document detailed verification of each module against ARCHITECTURE.md claims. Refer to individual review files in `plans/` directory for:
- Documentation verification tables
- Full code snippets of issues
- Medium/Low severity recommendations
- File-by-file analysis

---

## Items NOT To Implement (Verified Correct)

These items from reviews are actually correctly implemented:

1. **WebSocket rate limiter fallback**: If `REDIS_URL` is set → use Redis; otherwise → use in-memory
2. **`process_request()` implementation**: Correctly publishes events and returns success
3. **`SubAgentPool` bounded concurrency**: Properly uses semaphore with default of 5
4. **Tool definition caching**: Properly versioned cache key

(End of file)