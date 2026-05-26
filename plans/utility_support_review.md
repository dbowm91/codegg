# Utility and Support Modules Architecture Review

**Date**: 2026-05-26
**Reviewer**: Architecture Review Agent
**Modules Reviewed**: util.md, storage.md, worktree.md, pty_session.md, tts.md

---

## Executive Summary

All five architecture documents were reviewed against their corresponding source code in `src/`. The documents are generally accurate, but several stale items, discrepancies, and potential improvements were identified.

---

## 1. Util Module (`src/util/`)

### Document: `architecture/util.md`

**Status**: Mostly Accurate

#### Verified Items (Correct)

| Item | Document Location | Source Location | Notes |
|------|------------------|----------------|-------|
| File list | src/util/ | `src/util/mod.rs:1-4` | Contains: clipboard, fuzzy, metrics, truncate |
| `copy_to_clipboard` | Line 22 | `src/util/clipboard.rs:4-9` | Feature-gated with `#[cfg(feature = "arboard")]` |
| `read_from_clipboard` | Line 23 | `src/util/clipboard.rs:19-23` | Feature-gated correctly |
| `fuzzy_match` signature | Line 33 | `src/util/fuzzy.rs:3-9` | Returns `Vec<(String, usize)>` - sorted by Levenshtein distance |
| `fuzzy_score` signature | Line 34 | `src/util/fuzzy.rs:12-42` | Weighted scoring with bonuses |
| `truncate_lines` | Line 47 | `src/util/truncate.rs:1-14` | Keeps `max_lines/2` from start/end |
| `truncate_bytes` | Line 48 | `src/util/truncate.rs:16-27` | UTF-8 boundary safe |
| Metrics module | Lines 54-89 | `src/util/metrics.rs:1-157` | Correct inner module structure |

#### Stale Items

| Item | Description | Severity |
|------|-------------|----------|
| **Filename `stat_core.rs`** | Document references `stat_core.rs` but actual file is `metrics.rs`. The document correctly notes "misleading name" but the actual filename to reference is `metrics.rs`. | Low (document already notes this) |
| **Return type of `fuzzy_score`** | Document says `usize` but doesn't clarify that a score of 0 means no match | Documentation |
| **Metrics implementation** | Document shows `pub struct Counter(Arc<AtomicU64>)` but actual is `AtomicCounter = Arc<AtomicU64>` (type alias). Struct is actually `pub struct Counter(Arc<AtomicU64>)` at `src/util/metrics.rs:84` | Low |

#### Potential Issues in Source

| Location | Issue | Description |
|----------|-------|-------------|
| `src/util/fuzzy.rs:9` | **API Design** | `fuzzy_match` returns sorted by score ascending (lower is better for Levenshtein), but notie that the sorting behavior is correct for Levenshtein distance. However, this conflicts with `fuzzy_score` which returns higher = better match. These two functions have inconsistent semantics. |
| `src/util/metrics.rs:123-124` | **Bug** | Histogram records unbounded values - only limits to 1000 via `pop_front()`. If many unique histogram names are created, each stores up to 1000 values. No TTL or count limit per histogram name. Could cause memory growth. |

---

## 2. Storage Module (`src/storage/`)

### Document: `architecture/storage.md`

**Status**: Accurate

#### Verified Items (Correct)

| Item | Document Location | Source Location | Notes |
|------|------------------|----------------|-------|
| `Database` struct | Lines 19-23 | `src/storage/mod.rs:14-16` | Simple wrapper around `SqlitePool` |
| `Database::new` | Line 25 | `src/storage/mod.rs:19-23` | Async, calls `connect_and_configure` then `schema::migrate` |
| `pool()` | Line 26 | `src/storage/mod.rs:25-27` | Returns `&SqlitePool` |
| `migrate()` | Line 27 | `src/storage/mod.rs:29-31` | Delegates to `session::schema::migrate` |
| `health_check()` | Line 28, 88-92 | `src/storage/mod.rs:33-39` | Executes `SELECT 1` |
| `close()` | Line 29, 94-100 | `src/storage/mod.rs:41-43` | Calls `pool.close().await` |
| `init()` function | Lines 37-46 | `src/storage/mod.rs:85-129` | Full path resolution logic verified |
| Pragma list | Lines 54-65 | `src/storage/mod.rs:66-76` | All 8 pragmas match exactly |
| Max connections | Lines 80-81 | `src/storage/mod.rs:60` | Hardcoded to 10 |
| Acquire timeout | Lines 80-82 | `src/storage/mod.rs:61` | 30 seconds |
| Migration versions | Lines 106-118 | `src/session/schema.rs:25-66` | v1-v14 verified correct |
| Migrations location | Line 104 | Correctly notes `src/session/schema.rs` | Accurate |

#### Stale Items

None identified. The storage.md document is well-maintained and accurate.

#### Potential Issues in Source

| Location | Issue | Description |
|----------|-------|-------------|
| `src/storage/mod.rs:113-118` | **Permission check race** | `dir.exists()` and `dir.metadata()` are checked before creating dir, but another process could change permissions between check and use. Low risk. |
| `src/storage/mod.rs:59-63` | **No connection retry** | `SqlitePoolOptions::connect()` fails immediately if database is locked. No retry logic on busy. Could fail for legitimate concurrent access. |

---

## 3. Worktree Module (`src/worktree/`)

### Document: `architecture/worktree.md`

**Status**: Accurate

#### Verified Items (Correct)

| Item | Document Location | Source Location | Notes |
|------|------------------|----------------|-------|
| `list_worktrees()` | Lines 18-24 | `src/worktree/mod.rs:49-108` | Uses `--porcelain` flag |
| `create_worktree()` | Lines 26-37 | `src/worktree/mod.rs:110-138` | Passes `-b` when `create_branch=true` |
| `remove_worktree()` | Lines 39-45 | `src/worktree/mod.rs:140-159` | Passes `--force` when `force=true` |
| `find_git_root()` | Lines 47-53 | `src/worktree/mod.rs:161-173` | Walks up directory tree |
| `is_git_worktree()` | Lines 55-61 | `src/worktree/mod.rs:183-186` | Checks for `.git` file with `gitdir:` prefix |
| `is_git_file()` | Lines 63-69 | `src/worktree/mod.rs:175-181` | Reads file content, checks `gitdir:` prefix |
| `Worktree` struct | Lines 73-81 | `src/worktree/mod.rs:7-13` | Fields: path, branch, is_current, is_detached |
| `is_locked` note | Line 83 | N/A | Document correctly notes this is NOT implemented |
| `is_main` note | Line 83 | N/A | Document correctly notes this is NOT implemented |
| Server routes reference | Lines 117-118 | Verified via grep | `is_git_worktree` at `src/server/routes/workspace.rs:78` |

#### Stale Items

None identified. The worktree.md document matches the source code accurately.

#### Potential Issues in Source

| Location | Issue | Description |
|----------|-------|-------------|
| `src/worktree/mod.rs:65-88` | **Subtle bug in current worktree detection** | When checking if a worktree is current, the code compares canonicalized paths. However, `git worktree list --porcelain` should output the literal path, and the canonicalization could yield different results if symlinks are involved. The current worktree detection may be incorrect in some edge cases. |
| `src/worktree/mod.rs:32-38` | **Detached HEAD branch naming** | When in detached HEAD state with no branch, the code formats as `detached@{sha}`. This is reasonable but differs from `git worktree list` format which just shows `HEAD` detached. The `branch` field will be non-empty for detached HEADs which is inconsistent with the comment at line 77. |

---

## 4. PTY Session Module (`src/pty_session/`)

### Document: `architecture/pty_session.md`

**Status**: Mostly Accurate

#### Verified Items (Correct)

| Item | Document Location | Source Location | Notes |
|------|------------------|----------------|-------|
| `PtySession` struct | Lines 14-26 | `src/pty_session/mod.rs:5-14` | All fields match: id, project_id, cwd, shell, cols, rows, created_at |
| `CreatePtySession` | Lines 28-39 | `src/pty_session/mod.rs:16-23` | All fields match: project_id, cwd, shell, cols, rows (all Option) |
| `PtyResize` | Lines 41-49 | `src/pty_session/mod.rs:25-29` | Fields: cols, rows (u16) |
| `PtyManager` struct | Lines 51-56 | `src/pty_session/session.rs:9-11` | `sessions: Arc<RwLock<HashMap<String, PtySession>>>` |
| `new()` | Line 59 | `src/pty_session/session.rs:14-18` | Present |
| `default()` | Line 60 | `src/pty_session/session.rs:83-87` | Implemented via `Default` trait |
| `create()` | Line 61 | `src/pty_session/session.rs:20-36` | Returns `Result<PtySession, StorageError>` |
| `get()` | Line 62 | `src/pty_session/session.rs:38-40` | Returns `Option<PtySession>` |
| `update_cwd()` | Line 63 | `src/pty_session/session.rs:42-50` | Returns `Result<PtySession, StorageError>` |
| `list()` | Line 64 | `src/pty_session/session.rs:52-60` | Returns `Vec<PtySession>` |
| `resize()` | Line 65 | `src/pty_session/session.rs:62-71` | Returns `Result<(), StorageError>` |
| `delete()` | Line 66 | `src/pty_session/session.rs:73-80` | Returns `Result<(), StorageError>` |
| Default values | Line 75-76 | `src/pty_session/session.rs:28-30` | 80 cols, 24 rows, bash shell |
| `created_at` format | Line 73 | `src/pty_session/session.rs:22` | Milliseconds since epoch (i64) |
| Note about no actual PTY | Line 9 | Accurate | Module only manages metadata |

#### Stale Items

| Item | Description | Severity |
|------|-------------|----------|
| **Test count** | Document says "11 tests covering all PtyManager operations" at line 77. Actual test count in `src/pty_session/session.rs:89-273` is **12 tests** (lines 107-272 contain 12 `#[tokio::test]` blocks). | Low |

#### Potential Issues in Source

| Location | Issue | Description |
|----------|-------|-------------|
| `src/pty_session/session.rs:22` | **Bug** | `chrono::Utc::now().timestamp_millis()` may not be imported. Need to verify `chrono` is in dependencies. |
| `src/pty_session/session.rs:52-60` | **Performance** | `list()` clones all matching sessions. Could be expensive if many sessions exist. Acceptable for current use case but worth noting. |

---

## 5. TTS Module (`src/tts/`)

### Document: `architecture/tts.md`

**Status**: Accurate

#### Verified Items (Correct)

| Item | Document Location | Source Location | Notes |
|------|------------------|----------------|-------|
| `Tts` struct | Lines 17-21 | `src/tts/mod.rs:18-20` | `speaking: Mutex<std::sync::atomic::AtomicBool>` |
| `Clone` impl | Line 22 | `src/tts/mod.rs:22-30` | Clones inner atomic state |
| `Default` impl | Line 24 | `src/tts/mod.rs:32-36` | Delegates to `new()` |
| `new()` | Line 27 | `src/tts/mod.rs:38-43` | Present |
| `init()` | Lines 28, 45-49 | `src/tts/mod.rs:45-49` | Only handles `TtsProvider::None` |
| `speak()` | Lines 29, 51-83 | `src/tts/mod.rs:51-83` | Validates non-empty, uses `say` command |
| `stop()` | Lines 30, 85-103 | `src/tts/mod.rs:85-103` | Checks `is_speaking()` first, uses `pkill say` |
| `is_speaking()` | Line 31, 105-107 | `src/tts/mod.rs:105-107` | Returns `bool` not `Result` |
| `TtsEngine` trait | Lines 40-48 | `src/tts/mod.rs:11-16` | `#[async_trait]`, `Send + Sync` |
| `TtsProvider` | Lines 51-59 | `src/tts/mod.rs:5-9` | Only `None` variant |
| Keybinding `Ctrl+Y` | Lines 111-112 | `src/tui/app/mod.rs:301` | Confirmed |
| Keybinding `Ctrl+Shift+Y` | Lines 111-114 | `src/tui/app/mod.rs:302` | Confirmed |
| No config integration | Lines 96-101 | Accurate | No `[tts]` section in config |
| `tts_enabled` in UI | Line 99 | `src/tui/app/state/ui.rs:69` | Confirmed |
| `init()` only handles `None` | Line 98 | `src/tts/mod.rs:46-48` | Confirmed - match is exhaustive |

#### Stale Items

None identified. The tts.md document accurately describes the current implementation.

#### Potential Issues in Source

| Location | Issue | Description |
|----------|-------|-------------|
| `src/tts/mod.rs:52-57` | **Edge case** | `speak()` returns error for empty string, but doesn't trim whitespace. A string of only spaces would be spoken as-is. Consider trimming. |
| `src/tts/mod.rs:93-101` | **Incomplete stop handling** | `stop()` doesn't verify that `pkill` actually killed a process - it ignores failure. If `pkill say` fails (e.g., no say process running), it still returns `Ok(())`. Should check `output.status` more carefully or document this is intentional. |
| `src/tts/mod.rs:45-49` | **No extensibility** | `init()` only handles `None`. If a user configures a different provider, it silently accepts but does nothing. No warning or error that the provider isn't supported. |

---

## Summary of Stale Items by Module

### util.md
1. **Incorrect filename reference**: References `stat_core.rs` but actual file is `metrics.rs`

### storage.md
- No stale items found

### worktree.md
- No stale items found

### pty_session.md
1. **Incorrect test count**: Says "11 tests" but there are actually **12 tests**

### tts.md
- No stale items found

---

## Bug Reports

| ID | Module | File:Line | Description |
|----|--------|-----------|-------------|
| BUG-001 | util | `src/util/metrics.rs:122-124` | Histogram stores unbounded values per name - only `pop_front()` at 1000, but no limit on number of unique histogram names |
| BUG-002 | tts | `src/tts/mod.rs:85-103` | `stop()` returns `Ok(())` even when `pkill say` fails to find a process - silent failure |
| BUG-003 | tts | `src/tts/mod.rs:45-49` | `init()` silently ignores non-`None` providers instead of returning an error or warning |
| BUG-004 | worktree | `src/worktree/mod.rs:69-88` | Current worktree detection via path canonicalization may be incorrect for symlinked directories |

---

## Improvement Suggestions

### Util Module
1. **Consider unifying scoring semantics**: `fuzzy_match` sorts by Levenshtein distance (lower = better) while `fuzzy_score` returns higher = better. Consider adding a separate function for similarity scoring or clarifying the distinction.
2. **Add metrics cardinality limit**: Implement a global histogram count limit or TTL to prevent memory growth from many unique metric names.

### Storage Module
1. **Add connection retry logic**: Consider adding retry with backoff when database is locked (SQLITE_BUSY).
2. **Consider transactional path creation**: The permission check at line 113 could be combined with directory creation in a single atomic operation if the filesystem supports it.

### Worktree Module
1. **Fix current worktree detection**: Review symlink handling in `list_worktrees()` to ensure correct detection of current worktree.
2. **Consider git CLI output parsing robustness**: Add error handling for unexpected `git worktree list --porcelain` output formats.

### PTY Session Module
1. **Correct test count in documentation**: Update line 77 to say "12 tests" instead of "11 tests".
2. **Consider pagination for `list()`**: If many sessions exist, consider returning only a subset or adding pagination.

### TTS Module
1. **Add provider validation**: Make `init()` return an error or warning when an unsupported provider is configured.
2. **Consider `stop()` failure handling**: Either document that `pkill` failures are intentionally ignored, or return an error when `pkill` fails unexpectedly.
3. **Add text trimming**: Consider trimming whitespace from text before speaking.

---

## Appendix: Verified File Locations

| Module | Source File |
|--------|-------------|
| util | `src/util/mod.rs`, `clipboard.rs`, `fuzzy.rs`, `truncate.rs`, `metrics.rs` |
| storage | `src/storage/mod.rs` |
| worktree | `src/worktree/mod.rs` |
| pty_session | `src/pty_session/mod.rs`, `session.rs` |
| tts | `src/tts/mod.rs` |
| session schema | `src/session/schema.rs` (for storage.md migration verification) |