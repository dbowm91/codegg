# Util Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| Location: `src/util/` | VERIFIED | Contains clipboard.rs, fuzzy.rs, truncate.rs, stat_core.rs, mod.rs |
| Clipboard: `copy_to_clipboard(text: &str) -> Result<(), AppError>` | VERIFIED | clipboard.rs:4-9 |
| Clipboard: `read_from_clipboard() -> Option<String>` | VERIFIED | clipboard.rs:19-23 |
| Clipboard feature-gated with `arboard` | VERIFIED | `#[cfg(feature = "arboard")]` on lines 3, 11, 18, 25 |
| `fuzzy_match(query: &str, candidates: &[String]) -> Vec<(String, usize)>` | VERIFIED | fuzzy.rs:3-9 |
| `fuzzy_score(query: &str, target: &str) -> usize` | VERIFIED | fuzzy.rs:12-42 |
| `fuzzy_match` uses Levenshtein distance | VERIFIED | fuzzy.rs:6 calls `levenshtein(query, c)` |
| `fuzzy_match` returns sorted by distance (lower is better) | VERIFIED | fuzzy.rs:8 sorts ascending by score |
| `fuzzy_score` is case-insensitive | VERIFIED | fuzzy.rs:23 uses `eq_ignore_ascii_case` |
| `fuzzy_score` gives bonus for start-of-string | VERIFIED | fuzzy.rs:25 checks `*i == 0` |
| `fuzzy_score` gives bonus for consecutive matches | VERIFIED | fuzzy.rs:25 uses `prev_matched` |
| Dependencies: `strsim` crate | VERIFIED | fuzzy.rs:1 imports `strsim::levenshtein` |
| `truncate_lines(text: &str, max_lines: usize) -> String` | VERIFIED | truncate.rs:1-14 |
| `truncate_bytes(text: &str, max_bytes: usize) -> String` | VERIFIED | truncate.rs:16-27 |
| `truncate_lines` keeps `max_lines/2` from start and end | VERIFIED | truncate.rs:6, 12 |
| `truncate_lines` shows `"[X lines truncated]"` in middle | VERIFIED | truncate.rs:9 |
| `truncate_bytes` safely truncates at UTF-8 boundary | VERIFIED | truncate.rs:20-25 uses `char_indices()` |
| `truncate_bytes` appends `"... [truncated]"` | VERIFIED | truncate.rs:26 (note: spacing differs slightly from doc) |
| stat_core.rs contains metrics infrastructure | VERIFIED | stat_core.rs:1-157 defines Counter, Gauge, Histogram, Metrics |
| `stat_core.rs` name is misleading | VERIFIED | Contains metrics, not file statistics |
| `Metrics::new()`, `counter()`, `gauge()`, `histogram()`, `snapshot()` | VERIFIED | stat_core.rs:19-75 |
| `Counter::inc()`, `Counter::add()` | VERIFIED | stat_core.rs:87-93 |
| `Gauge::set()`, `inc()`, `dec()` | VERIFIED | stat_core.rs:99-113 |
| `Histogram::record()` | VERIFIED | stat_core.rs:119-125 |
| `metrics() -> &'static Metrics` | VERIFIED | stat_core.rs:137-139 |

## Bugs Found

### Medium

**Dead code: `stat_core` metrics system is never used**
- The `metrics()` function exists and is public, but `grep` shows zero call sites outside of `stat_core.rs` itself
- `Metrics::snapshot()` is never called anywhere in the codebase
- Recommendation: Either integrate metrics collection into the application or remove the unused module to avoid maintenance burden

### Low

**Minor string mismatch in `truncate_bytes`**
- Arch doc says appends `"... [truncated]"`
- Implementation uses `"... [truncated]"` (note: space after ellipsis)
- Not a functional bug, just documentation inconsistency

## Improvement Suggestions

### Correctness

1. **`truncate_bytes(0)` returns misleading result**: When called with `max_bytes: 0` on non-empty text, returns `"... [truncated]"` which shows truncation message but no actual content. Consider returning empty string or the full behavior specification.

2. **`truncate_bytes(1)` edge case with multi-byte chars**: `truncate_bytes("éclair", 1)` returns `"... [truncated]"` which is correct UTF-8 safety but may surprise users expecting partial character display.

### Performance

3. **`Metrics::snapshot()` locks three separate maps**: Could be optimized with a single critical section or RwLock if snapshot performance is critical.

4. **`fuzzy_match` allocates intermediate Vec**: Creates full `Vec<(String, usize)>` before sorting. For large candidate sets, could consider `Vec::with_capacity()` hint.

### Maintainability

5. **`stat_core` module is unused**: Either integrate metrics throughout the application (for observability) or remove the module entirely. The infrastructure exists but provides no value if not collected.

6. **Missing integration tests**: No integration tests verify end-to-end behavior of utility functions when used together.

7. **fuzzy_score edge case - empty query**: When `query.is_empty()`, returns 0 (line 13-15). This is intentional but could be documented better - empty query returns 0, meaning "no match".

## Priority Actions (Top 5)

1. **[MEDIUM]** Evaluate whether to keep or remove `stat_core` module - it's dead code with maintenance burden
2. **[LOW]** Fix documentation string mismatch in `truncate_bytes`: `"... [truncated]"` vs `"... [truncated]"` (spacing)
3. **[LOW]** Consider documenting `fuzzy_score` empty query behavior explicitly in docstring
4. **[LOW]** Consider adding integration test for `fuzzy_score` scoring consistency
5. **[INFO]** Document that `truncate_bytes(0)` behavior may be surprising to users

## Summary

The `util` module is a small, well-structured collection of utilities with accurate architecture documentation. The implementation is correct for the most part:

- **clipboard.rs**: Clean feature-gated implementation, properly handles disabled feature
- **fuzzy.rs**: Correct Levenshtein-based matching with good test coverage (11 tests)
- **truncate.rs**: Correct UTF-8 boundary handling, good test coverage (13 tests)
- **stat_core.rs**: Complete metrics infrastructure but **completely unused** in the codebase

The main concern is the `stat_core` module which provides observability infrastructure that was apparently never integrated into the application. If metrics are not being collected, this module should be removed to reduce maintenance burden.