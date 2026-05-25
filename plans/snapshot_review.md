# Snapshot Module Review (2026-05-25)

## Status: INCOMPLETE

The snapshot module implementation exists but `restore()` is **not integrated into error-handling** as of this review.

---

## Known Issue Verification: restore() Not Integrated

**Previous Review Finding**: "The restore() and restore_to_path() methods exist but are not integrated into the agent loop."

**Current Status**: **CONFIRMED - STILL NOT FIXED**

### Evidence

1. **No calls to `SnapshotManager::restore()` in AgentLoop**
   - Search across entire codebase for `.restore(` only finds one call in `src/core/mod.rs:357` for SessionStore (not snapshot)
   - `src/agent/loop.rs:1559-1624` shows only `capture_snapshot_if_needed()` and `capture_incremental_snapshot_if_needed()`
   - No error handler invokes `SnapshotManager::restore()`

2. **Snapshot capture is wired but rollback is not**
   - Lines 1650-1655 in loop.rs capture snapshot before file-modifying tools
   - Lines 1853 capture incremental snapshot on file changes
   - Neither path has error recovery that calls `restore()`

3. **Architecture document is misleading**
   - Lines 97-119 and 139-156 show "If error → SnapshotManager::restore(snapshot_view)" flow
   - This flow **does not exist in code**

---

## Architecture vs Implementation Comparison

| Documented Item | Arch Line | Implementation | Match |
|-----------------|-----------|----------------|-------|
| `SnapshotOptions` with defaults | 22-27 | mod.rs:8-23 | ✓ |
| `FileSnapshot` struct | 29-40 | mod.rs:25-31 | ✓ |
| `Snapshot` struct with `data: String` | 42-54 | mod.rs:33-40 | ✓ |
| `SnapshotView` with `files: HashMap` | 56-68 | mod.rs:42-49 | ✓ |
| `SnapshotManager::new(pool, project_root)` | 80 | mod.rs:57-64 | ✓ |
| `SnapshotManager::new_with_options(...)` | 81 | mod.rs:66-81 | ✓ |
| `restore()` method | 88 | mod.rs:267-300 | ✓ |
| `restore_to_path()` method | 89 | mod.rs:302-341 | ✓ |
| Path traversal protection | 193-206 | mod.rs:280-284, 319-323 | ✓ |
| Atomic write pattern | 207-219 | mod.rs:330-334 | ✓ |
| Diff types | 221-250 | diff.rs:3-27 | ✓ |
| `diff_files()` function | 248 | diff.rs:29-128 | ✓ |
| `format_unified_diff()` function | 249 | diff.rs:130-143 | ✓ |

All documented types/functions match implementation exactly.

---

## Path Traversal Protection Verification

Both `restore()` and `restore_to_path()` have proper path traversal protection:

```rust
// mod.rs:274-284 (restore)
let canonical_project_root = project_root.canonicalize()?;
for (rel_path, file_snapshot) in files {
    let full_path = project_root.join(&rel_path);
    let canonical_path = full_path.canonicalize()
        .unwrap_or_else(|_| full_path.clone());
    if !canonical_path.starts_with(&canonical_project_root) {
        return Err(format!("path traversal attempt detected"));
    }
    // ... write
}

// mod.rs:312-323 (restore_to_path)
let canonical_target = target.canonicalize()?;
for (rel_path, file_snapshot) in files {
    let full_path = target.join(&rel_path);
    let canonical_path = full_path.canonicalize()
        .unwrap_or_else(|_| full_path.clone());
    if !canonical_path.starts_with(&canonical_target) {
        return Err(format!("path traversal attempt detected"));
    }
    // ... atomic write
}
```

**Status**: Path traversal protection is correctly implemented.

---

## Discrepancies Found

### 1. Architecture Documents Non-Existent Error-Recovery Flow (HIGH)

**Location**: `architecture/snapshot.md:139-156`

The architecture shows this flow:
```
If error → SnapshotManager::restore(snapshot_view)
```

This flow **does not exist**. The agent captures snapshots but never restores them on error.

**Recommendation**: Update architecture to document that `restore()` is available but not yet integrated into the agent error-handling loop.

### 2. Architecture Shows AgentLoop Integration Code (LOW)

**Location**: `architecture/snapshot.md:147-172`

The code examples show:
```rust
impl AgentLoop {
    async fn capture_snapshot_if_needed(&mut self) { ... }
    async fn capture_incremental_snapshot_if_needed(&mut self, label: Option<String>) { ... }
}
```

These methods exist at `loop.rs:1559-1624` but no `restore` call exists after these methods. The architecture correctly shows `capture` methods but incorrectly implies `restore` is called on error.

---

## Bugs Identified

### 1. restore() continues after write failure (LOW)

**File**: `src/snapshot/mod.rs:291-293`

```rust
if let Err(e) = std::fs::write(&full_path, &file_snapshot.content) {
    return Err(format!("failed to write {}: {}", full_path.display(), e));
}
```

Actually, the code **does return early** on write failure. The previous review was incorrect here.

Wait - let me verify more carefully...

Looking at lines 291-293:
```rust
if let Err(e) = std::fs::write(&full_path, &file_snapshot.content) {
    return Err(format!("failed to write {}: {}", full_path.display(), e));
}
```

This IS a fast-fail on write error. The previous review was wrong about this.

### 2. restore_to_path() continues after write failure (LOW - but verified correct fast-fail)

Looking at lines 331-334:
```rust
std::fs::write(&temp_path, &file_snapshot.content)
    .map_err(|e| format!("failed to write {}: {}", temp_path.display(), e))?;
std::fs::rename(&temp_path, &full_path)
    .map_err(|e| format!("failed to rename {}: {}", temp_path.display(), e))?;
```

This also fails fast on error. Both restore functions properly return early on failure.

---

## Summary

| Category | Status |
|----------|--------|
| Type/Function Documentation | COMPLETE - all match |
| Path Traversal Protection | COMPLETE - implemented correctly |
| Error Handling Integration | **INCOMPLETE** - restore() never called |
| Atomic Write Pattern | COMPLETE - implemented in restore_to_path() |
| Architecture Accuracy | INCOMPLETE - documents non-existent error recovery |

---

## Recommendations

1. **Architecture Document**: Remove or mark as "planned" the error-triggered restore flow (lines 139-156). Document that snapshots are captured but restore must be triggered manually or via future integration.

2. **AgentLoop Integration**: If automatic rollback on error is desired, add error-handling that calls `restore()` when tool execution fails. This would require storing the captured snapshot ID for use on error.

3. **Minimal Fix**: Even without full rollback integration, adding a `/restore` command that calls `SnapshotManager::restore()` would make the functionality accessible.

---

## File References

| File | Lines | Note |
|------|-------|------|
| `src/snapshot/mod.rs` | 267-341 | `restore()` and `restore_to_path()` implementations |
| `src/agent/loop.rs` | 1559-1624 | Capture methods only, no restore call |
| `src/agent/loop.rs` | 1650-1655 | Pre-tool capture (no restore on failure) |
| `src/agent/loop.rs` | 1853 | Incremental capture (no restore) |
| `architecture/snapshot.md` | 139-156 | Documents non-existent error recovery |