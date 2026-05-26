# PTY Session Architecture Review

**Reviewed**: 2026-05-26
**Source**: `architecture/pty_session.md`
**Code**: `src/shell_session/mod.rs`, `src/shell_session/session.rs`

---

## Summary

The documentation has **naming discrepancies** throughout but the underlying structure is correct. All claims about functionality, defaults, and test count are accurate.

---

## Findings

### 1. Module Name (CRITICAL ERROR)

| Aspect | Documentation | Actual Code |
|--------|---------------|-------------|
| Module name | `pty` | `shell_session` |
| Location | `src/pty_session/` | `src/shell_session/` |

**Impact**: The documented module does not exist. All struct names use `Pty` prefix but actual structs use `Shell` prefix. This would cause compile errors if someone followed the documentation.

---

### 2. Struct Names Incorrect

| Documentation | Actual |
|---------------|--------|
| `PtySession` | `ShellSession` |
| `CreatePtySession` | `CreateShellSession` |
| `PtyResize` | `ShellResize` |
| `PtyManager` | `ShellManager` |

Code references at:
- `src/shell_session/mod.rs:6-14` - ShellSession definition
- `src/shell_session/mod.rs:17-23` - CreateShellSession definition
- `src/shell_session/mod.rs:26-29` - ShellResize definition
- `src/shell_session/session.rs:9-11` - ShellManager struct

---

### 3. Field Counts - ALL CORRECT

**ShellSession** (8 fields):
- `id: String` - line 7
- `project_id: String` - line 8
- `cwd: String` - line 9
- `shell: String` - line 10
- `cols: u16` - line 11
- `rows: u16` - line 12
- `created_at: i64` - line 13

**CreateShellSession** (6 fields):
- `project_id: String` - line 18
- `cwd: Option<String>` - line 19
- `shell: Option<String>` - line 20
- `cols: Option<u16>` - line 21
- `rows: Option<u16>` - line 22

**ShellResize** (2 fields):
- `cols: u16` - line 27
- `rows: u16` - line 28

---

### 4. PtyManager Methods - ALL CORRECT (with corrected names)

| Method | Status | Location |
|--------|--------|----------|
| `new()` | Verified | `session.rs:14-18` |
| `default()` | Verified | `session.rs:83-87` |
| `create()` | Verified | `session.rs:20-36` |
| `get()` | Verified | `session.rs:38-40` |
| `update_cwd()` | Verified | `session.rs:42-50` |
| `list()` | Verified | `session.rs:52-60` |
| `resize()` | Verified | `session.rs:62-71` |
| `delete()` | Verified | `session.rs:73-80` |

All signatures match with `ShellManager` (not `PtyManager`).

---

### 5. Default Values - ALL CORRECT

| Default | Claimed | Actual |
|---------|---------|--------|
| Terminal cols | 80 | 80 (`session.rs:29`) |
| Terminal rows | 24 | 24 (`session.rs:30`) |
| Default shell | `bash` | `bash` (`session.rs:28`) |
| Default cwd | `.` | `.` (`session.rs:27`) |
| `created_at` unit | milliseconds (i64) | milliseconds (`session.rs:22`) |

---

### 6. Test Count - CORRECT

The file `src/shell_session/session.rs` contains **11 tests** (lines 89-273):

1. `test_create_session` (line 108)
2. `test_create_session_defaults` (line 124)
3. `test_get_session` (line 143)
4. `test_get_session_not_found` (line 155)
5. `test_update_cwd` (line 163)
6. `test_update_cwd_not_found` (line 174)
7. `test_list_sessions` (line 182)
8. `test_resize` (line 229)
9. `test_resize_not_found` (line 245)
10. `test_delete` (line 255)
11. `test_delete_not_found` (line 267)

---

### 7. Notes Section - ALL CORRECT

| Claim | Status |
|-------|--------|
| Sessions in-memory only | Verified - `Arc<RwLock<HashMap>>` |
| `created_at` uses milliseconds | Verified - `chrono::Utc::now().timestamp_millis()` |
| `cwd` is `String` (not `PathBuf`) | Verified |
| Default 80x24 | Verified |
| Default shell `bash` | Verified |
| Tests in session.rs | Verified |

---

### 8. See Also - Link Valid

`[tool.md](tool.md)` exists at `architecture/tool.md`.

---

## Discrepancy Summary

| Severity | Issue |
|----------|-------|
| **Critical** | Module name `pty` vs `shell_session` |
| **Critical** | All struct/type names use `Pty` prefix, should be `Shell` |
| **Critical** | Location `src/pty_session/` vs `src/shell_session/` |

**Low** | Minor | Documentation mentions "PtyManager" which doesn't exist (actual: ShellManager)

---

## Recommendations

1. Rename `architecture/pty_session.md` to `architecture/shell_session.md`
2. Update all internal references to use `ShellSession`, `CreateShellSession`, `ShellResize`, `ShellManager`
3. Update "See Also" cross-references in other docs that point to this file
4. Keep the skill guide `.opencode/skills/shell_session/SKILL.md` as-is (it's correct)

---

## Verification Commands

```bash
# Verify module structure
ls src/shell_session/

# Verify tests exist
cargo test --lib -- shell_session::tests --nocapture 2>/dev/null | grep -E "test .* ok|test result"
```
