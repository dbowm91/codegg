# PTY Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| Location: `src/pty/` | VERIFIED | Files exist at `src/pty/mod.rs` and `src/pty/session.rs` |
| Module does NOT create actual PTY sessions | VERIFIED | No process spawning code; only manages in-memory metadata |
| PtySession struct with 7 fields (id, project_id, cwd, shell, cols, rows, created_at) | VERIFIED | Exact match in `src/pty/mod.rs:6-14` |
| CreatePtySession struct with 6 fields | VERIFIED | Exact match in `src/pty/mod.rs:16-23` |
| PtyResize struct with 2 fields | VERIFIED | Exact match in `src/pty/mod.rs:25-29` |
| PtyManager has `sessions: Arc<RwLock<HashMap<String, PtySession>>>` | VERIFIED | `src/pty/session.rs:9-11` |
| PtyManager::new() exists | VERIFIED | `src/pty/session.rs:14-18` |
| PtyManager::create() async fn | VERIFIED | `src/pty/session.rs:20-36` |
| PtyManager::get() async fn returns Option | VERIFIED | `src/pty/session.rs:38-40` |
| PtyManager::update_cwd() async fn | VERIFIED | `src/pty/session.rs:42-50` |
| PtyManager::list() async fn | VERIFIED | `src/pty/session.rs:52-60` |
| PtyManager::resize() async fn | VERIFIED | `src/pty/session.rs:62-71` |
| PtyManager::delete() async fn | VERIFIED | `src/pty/session.rs:73-80` |
| Sessions stored in-memory only | VERIFIED | HashMap in memory, no persistence |
| created_at uses milliseconds since epoch | VERIFIED | `chrono::Utc::now().timestamp_millis()` at `session.rs:22` |
| cwd stored as String | VERIFIED | Field is `pub cwd: String` |
| Default terminal size 80x24 | VERIFIED | `cols: input.cols.unwrap_or(80)` and `rows: input.rows.unwrap_or(24)` at lines 29-30 |
| Default shell is bash | VERIFIED | `shell: input.shell.unwrap_or_else(|| "bash".to_string())` at line 28 |
| Unit tests added (11 tests) | VERIFIED | 11 tests in `src/pty/session.rs:107-272` |

## Bugs Found

### Critical
None identified.

### High
None identified.

### Medium

**1. PtyManager not exported from parent module**

The `PtyManager` struct is defined in `src/pty/session.rs` but is not re-exported from `src/pty/mod.rs`. External usage requires importing from `crate::pty::session::PtyManager`, which is inconsistent with other modules that re-export their main types from the module root.

**2. update_cwd clones session after dropping lock**

In `update_cwd()` at `session.rs:42-50`, the code clones `session` after releasing the write lock (via `session.clone()` at line 49 after lock ends at line 43 scope). This is actually safe but the code structure looks suspicious. However, looking more closely, the lock is held for the entire block so this is actually correct - the clone happens before the function returns. This is not a bug.

**3. PtyManager never actually used in codebase**

The entire `pty` module is standalone with no integration into the rest of the codebase. No other module imports or uses `PtyManager`. This raises questions about intended usage and whether this code is dead or planned for future use.

## Improvement Suggestions

### Performance
- Current implementation is efficient with O(1) HashMap operations
- No concerns for in-memory session management

### Correctness
1. **Export PtyManager from mod.rs** for consistent API
2. **Consider adding session count metrics** for monitoring active sessions
3. **Add session age tracking** - `created_at` exists but no utility method to check session age or timeout stale sessions

### Maintainability
1. **Integration needed** - The module should be wired into the application if intended for use (e.g., from tool::terminal or a session manager)
2. **Documentation of intended use case** - The module stores metadata but no other code currently consumes it. Document when/how it should be used
3. **Consider adding session cleanup** - method to remove sessions older than a threshold
4. **Add Default implementation** - PtyManager already has `Default` via `impl Default for PtyManager` at lines 83-87, but this is good

## Priority Actions (top 5 items to fix)

1. **Wire PtyManager into the application** if it's intended for use - currently unused
2. **Re-export PtyManager from `src/pty/mod.rs`** for consistent public API
3. **Add session timeout/cleanup functionality** - add method to remove stale sessions
4. **Add integration test** - verify PtyManager works correctly with actual session lifecycle
5. **Document intended usage pattern** - clarify when terminal sessions should be created/tracked vs when they're just transient