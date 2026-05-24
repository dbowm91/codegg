# Util Module Review

**Date**: 2026-05-24  
**Reviewer**: Code Review  
**Module**: `src/util/` and `architecture/util.md`

---

## Summary

The `util` module provides common utility functions for clipboard operations, fuzzy string matching, text truncation, and metrics collection. Both the architecture document and skill documentation are **accurate and well-maintained**. All 24 unit tests pass.

---

## Verified Components

### 1. `clipboard.rs`
- **Status**: Verified accurate
- **Functions**: `copy_to_clipboard()`, `read_from_clipboard()`
- **Feature gate**: Properly uses `#[cfg(feature = "arboard")]`
- **Error handling**: Returns `AppError::Clipboard` on failure

### 2. `fuzzy.rs`
- **Status**: Verified accurate
- **Functions**: `fuzzy_match()`, `fuzzy_score()`
- **Algorithm**: Levenshtein distance via `strsim` crate
- **Scoring**: Case-insensitive with bonuses for start-of-string and consecutive matches

### 3. `truncate.rs`
- **Status**: Verified accurate
- **Functions**: `truncate_lines()`, `truncate_bytes()`
- **Line truncation**: Keeps `max_lines/2` from start and end with "[X lines truncated]" marker
- **Byte truncation**: Safely handles UTF-8 boundaries with "... [truncated]" suffix

### 4. `stat_core.rs` (misleading name acknowledged)
- **Status**: Verified accurate
- **Structs**: `Metrics`, `Counter`, `Gauge`, `Histogram`, `MetricsSnapshot`
- **Histogram**: Uses `VecDeque<u64>` with 1000-element limit, auto-evicts oldest
- **Gauge**: Uses saturating arithmetic on `dec()` (verified by test `gauge_dec_saturates_at_zero`)

---

## Discrepancies Found

**None**. The architecture document and skill are fully accurate against the actual implementation.

---

## Bugs or Issues

**No bugs found**. All implementations are correct:

1. **Clipboard feature-gating works correctly** (`src/util/clipboard.rs:3-28`)
   - When `arboard` feature is disabled, `copy_to_clipboard` returns an appropriate error and `read_from_clipboard` returns `None`

2. **Fuzzy scoring is correct** (`src/util/fuzzy.rs:12-42`)
   - Empty query returns 0
   - Partial matches work correctly
   - Case-insensitive matching
   - Bonuses for start-of-string and consecutive matches

3. **Truncation handles edge cases** (`src/util/truncate.rs`)
   - Empty strings handled
   - UTF-8 character boundaries respected in `truncate_bytes`
   - Single line/byte cases work

4. **Metrics histogram limit** (`src/util/stat_core.rs:119-125`)
   - 1000-element limit enforced with `pop_front()`

---

## Integration Points Verified

| Location | Usage | Line |
|----------|-------|------|
| `src/tui/app/mod.rs:44` | `fuzzy_score` for command filtering | 44 |
| `src/tui/command.rs:2` | `fuzzy_score` for slash command matching | 2 |
| `src/tui/components/completion_overlay.rs:10` | `fuzzy_score` for completion filtering | 10 |
| `src/tui/components/dialogs/share.rs:12` | `clipboard` for URL copying | 12 |
| `src/tui/mod.rs:454` | `clipboard` for session export | 454 |

---

## Test Results

```
running 24 tests
test util::fuzzy::tests::test_fuzzy_score_empty_query ... ok
test util::truncate::tests::test_truncate_bytes_empty ... ok
test util::truncate::tests::test_truncate_bytes_exact ... ok
test util::truncate::tests::test_truncate_bytes_no_truncation ... ok
test util::fuzzy::tests::test_fuzzy_score_no_match ... ok
test util::fuzzy::tests::test_fuzzy_score_partial ... ok
test util::fuzzy::tests::test_fuzzy_score_missing_char ... ok
test util::fuzzy::tests::test_fuzzy_score_bonus_for_start ... ok
test util::fuzzy::tests::test_fuzzy_score_case_insensitive ... ok
test util::fuzzy::tests::test_fuzzy_score_consecutive_bonus ... ok
test util::fuzzy::tests::test_fuzzy_score_exact ... ok
test util::truncate::tests::test_truncate_bytes_utf8_boundary_safe ... ok
test util::truncate::tests::test_truncate_bytes_truncates ... ok
test util::truncate::tests::test_truncate_lines_empty ... ok
test util::truncate::tests::test_truncate_lines_no_truncation ... ok
test util::truncate::tests::test_truncate_lines_single ... ok
test util::fuzzy::tests::test_fuzzy_match_empty_query ... ok
test util::fuzzy::tests::test_fuzzy_match_exact ... ok
test util::fuzzy::tests::test_fuzzy_match_sorted_by_score ... ok
test util::truncate::tests::test_truncate_lines_odd_max ... ok
test util::truncate::tests::test_truncate_lines_truncates ... ok
test util::truncate::tests::test_truncate_lines_keeps_head_and_tail ... ok
test util::truncate::tests::test_truncate_lines_even_max ... ok
test util::stat_core::inner::tests::gauge_dec_saturates_at_zero ... ok

test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured
```

---

## Recommendations

### Documentation
- **No changes needed** to `architecture/util.md` or `.opencode/skills/util/SKILL.md`

### Code
- **No changes needed** to implementation

### Minor Note
The `stat_core.rs` filename is acknowledged as misleading in both docs. The metrics infrastructure is correctly implemented but the filename suggests file statistics rather than observability metrics. This is a cosmetic issue only and is already documented.

---

## Conclusion

The `util` module is well-implemented, thoroughly documented, and has comprehensive test coverage. No bugs or documentation issues were found. The architecture document accurately reflects the implementation, and all integration points verified correctly.
