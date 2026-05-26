# Snapshot Module Architecture Review

**Reviewed against**: `src/snapshot/mod.rs` (450 lines), `src/snapshot/diff.rs` (144 lines)
**Document**: `architecture/snapshot.md` (279 lines)
**Date**: 2026-05-26

---

## Summary

The architecture document is **largely accurate** with one significant discrepancy and several minor issues. The core structures, methods, and integration patterns are correctly documented.

---

## Verified Correct

### Location (Line 7)
- `src/snapshot/` - **Correct**

### Key Types

| Type | Documented | Actual Location | Status |
|------|-----------|-----------------|--------|
| `SnapshotOptions` | Lines 19-27 | `src/snapshot/mod.rs:9-14` | ✅ Correct |
| `FileSnapshot` | Lines 33-40 | `src/snapshot/mod.rs:26-32` | ✅ Correct |
| `Snapshot` | Lines 46-54 | `src/snapshot/mod.rs:34-41` | ✅ Correct |
| `SnapshotView` | Lines 60-68 | `src/snapshot/mod.rs:43-50` | ✅ Correct |
| `SnapshotManager` | Lines 72-93 | `src/snapshot/mod.rs:52-361` | ✅ Correct |

### SnapshotManager Methods (Lines 79-92)
All methods correctly documented with accurate signatures:
- `new()` - `mod.rs:59-65` ✅
- `new_with_options()` - `mod.rs:67-82` ✅
- `capture()` - `mod.rs:84-118` ✅
- `capture_incremental()` - `mod.rs:120-180` ✅
- `get()` - `mod.rs:182-204` ✅
- `list_for_session()` - `mod.rs:206-227` ✅
- `latest()` - `mod.rs:229-251` ✅
- `restore()` - `mod.rs:268-301` ✅
- `restore_to_path()` - `mod.rs:303-342` ✅
- `delete_snapshot()` - `mod.rs:344-351` ✅
- `delete_all_for_session()` - `mod.rs:353-360` ✅

### Default Values (Lines 23-25)
`SnapshotOptions` defaults are correctly documented:
- `max_files`: 5_000 ✅
- `max_file_bytes`: 1_000_000 ✅
- `max_total_bytes`: 20_000_000 ✅

### Integration with AgentLoop (Lines 157-180)
The code examples in the documentation show the structure correctly, but the **actual line numbers differ**:
- `capture_snapshot_if_needed` is at `src/agent/loop.rs:1560` (not 159)
- `capture_incremental_snapshot_if_needed` is at `src/agent/loop.rs:1596` (not 168)

### Database Schema (Lines 184-197)
Schema is correctly documented as being in `src/session/schema.rs` (migration v13).
Actual location: `schema.rs:481-504` (specifically `migrate_v13` function).
Schema itself matches documentation exactly.

### Security - Path Traversal Prevention (Lines 203-213)
Code pattern correctly documented. Actual implementation is at `mod.rs:275-284`.

### Security - Atomic Write Pattern (Lines 217-227)
Code pattern correctly documented. Actual implementation is at `mod.rs:331-335`.

### DiffModule Types (Lines 233-258)
All diff types correctly documented:
- `FileDiff` - `diff.rs:3-7` ✅
- `DiffHunk` - `diff.rs:9-14` ✅
- `DiffLine` - `diff.rs:16-20` ✅
- `DiffKind` - `diff.rs:22-27` ✅
- `diff_files()` - `diff.rs:29-128` ✅
- `format_unified_diff()` - `diff.rs:130-144` ✅

### Configuration (Lines 262-273)
`SnapshotConfig` correctly documented. Location: `config/schema.rs:68-72`.

### File Collection Exclusions (Line 121)
Correctly lists: `.git`, `node_modules`, `target`, `.codegg`
Actual code: `mod.rs:385`

---

## Discrepancies

### 1. Hash Algorithm Inconsistency - Minor
**Document states (Line 37)**: No specific hash algorithm mentioned.

**Actual code**: `collect_files_sync()` uses **MD5** for non-empty files (`mod.rs:431`), but `capture_incremental()` uses **SHA256** (`mod.rs:143`).

This is noted as a known issue in `AGENTS.md` under "Snapshot hash inconsistency". The documentation does not mention this inconsistency.

### 2. AgentLoop Integration Line Numbers
Integration method locations in `AgentLoop` are **off by ~1400 lines**:
- Document `capture_snapshot_if_needed`: Line 159 → Actual: `loop.rs:1560`
- Document `capture_incremental_snapshot_if_needed`: Line 168 → Actual: `loop.rs:1596`

The code shown is structurally correct but uses wrong line references.

### 3. SnapshotManager::restore() - Missing Directory Creation in `restore_to_path()`
Document states (Line 151): `restore_to_path()` [has] "atomic write pattern (temp file + rename)".

Actual behavior (`mod.rs:325-335`):
- Creates parent directories if needed (`mod.rs:326-329`)
- Then writes to temp file and renames (`mod.rs:331-335`)

The documentation omits the directory creation step that precedes the atomic write, but this is present in the actual implementation.

### 4. Missing Skill Guide
Document references (Line 279): `.opencode/skills/snapshot/SKILL.md`

This file does not exist. The skill guide was referenced but never created.

---

## Incomplete/Never-Integrated Features

### Automatic Rollback (Lines 115-118, 147-153)
Document correctly notes: "automatic rollback on tool failure is not implemented". This is accurate.

---

## Module Organization

| File | Description |
|------|-------------|
| `src/snapshot/mod.rs` | Main module - SnapshotManager, types, file collection |
| `src/snapshot/diff.rs` | Diff computation types and functions |

Both files exist and are correctly organized. The `diff.rs` submodule is properly declared at `mod.rs:1`.

---

## Summary of Findings

| Aspect | Status |
|--------|--------|
| Struct definitions | ✅ Accurate |
| Field counts | ✅ Accurate |
| Method signatures | ✅ Accurate |
| Default values | ✅ Accurate |
| Security patterns | ✅ Accurate |
| Database schema | ✅ Accurate |
| Module organization | ✅ Accurate |
| Integration line numbers | ❌ Off by ~1400 lines |
| Hash algorithm consistency | ⚠️ Not documented (MD5 vs SHA256) |
| Missing skill guide reference | ❌ File doesn't exist |

---

## Recommendations

1. **Update line numbers** in Integration with AgentLoop section (current lines 157-180)
2. **Document the inconsistency** between MD5 (full capture) and SHA256 (incremental capture) hashing
3. **Remove or create** the skill guide reference at line 279
