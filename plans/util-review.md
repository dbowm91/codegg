# Util Module Review

**Review Date**: 2026-05-26
**Reviewer**: Architecture Review Agent
**Files Reviewed**:
- `architecture/util.md`
- `src/util/mod.rs`
- `src/util/clipboard.rs`
- `src/util/fuzzy.rs`
- `src/util/truncate.rs`
- `src/util/stat_core.rs`

---

## Verified Claims

### clipboard.rs
- `copy_to_clipboard(text: &str) -> Result<(), AppError>` - **MATCHES**
- `read_from_clipboard() -> Option<String>` - **MATCHES**
- Feature gate `arboard` - **MATCHES**
- Stub implementations when feature disabled - **MATCHES**

### fuzzy.rs
- `fuzzy_match(query: &str, candidates: &[String]) -> Vec<(String, usize)>` - **MATCHES**
- `fuzzy_score(query: &str, target: &str) -> usize` - **MATCHES**
- Uses `strsim::levenshtein` for distance - **MATCHES**
- `fuzzy_score` case-insensitive - **MATCHES**
- `fuzzy_score` bonuses for start-of-string and consecutive matches - **MATCHES**

### truncate.rs
- `truncate_lines(text: &str, max_lines: usize) -> String` - **MATCHES**
- `truncate_bytes(text: &str, max_bytes: usize) -> String` - **MATCHES**
- Keeps `max_lines/2` from start and end - **MATCHES** (verified at lines 6, 12)
- Shows "[X lines truncated]" in middle - **MATCHES** (line 9)
- UTF-8 boundary safe - **MATCHES** (tested at line 105-108)
- Appends "... [truncated]" - **MATCHES** (line 26)

### stat_core.rs
- Module name `inner` - **MATCHES**
- `Metrics::new()`, `counter()`, `gauge()`, `histogram()`, `snapshot()` - **MATCHES**
- `Counter::inc()`, `Counter::add()` - **MATCHES**
- `Gauge::set()`, `inc()`, `dec()` - **MATCHES**
- `Histogram::record()` - **MATCHES**
- `metrics()` global function - **MATCHES**
- Filename misleading (contains metrics, not file stats) - **MATCHES** (noted in doc line 54)
- Histogram bounded at 1000 entries - **MATCHES** (line 123)
- `Gauge::dec()` saturates at zero - **MATCHES** (tested at line 146-155)

### mod.rs exports
- All 4 modules exported - **MATCHES**

---

## Bugs/Discrepancies Found

### 1. `fuzzy_score` bonus description is misleading (LOW priority)

**Doc** (line 38): "case-insensitive, bonuses for start-of-string and consecutive matches"

**Actual** (fuzzy.rs:25-27):
```rust
if *i == 0 || prev_matched {
    bonus += 1;
}
```

The bonus is +1 per matched character when:
1. Character is at query position 0 (first char matches), OR
2. Previous character also matched consecutively

The documentation correctly describes the behavior but could be clearer about how bonus accumulates (per-character, not flat bonuses).

### 2. `fuzzy_match` uses Levenshtein distance, not "score" (LOW priority)

**Doc** (line 37): "Returns candidates sorted by Levenshtein distance (lower is better)"

**Actual**: Implementation returns `(String, usize)` where the `usize` is the raw Levenshtein distance. The documentation correctly identifies this, but the function name `fuzzy_match` combined with `fuzzy_score` elsewhere could be confusing since one returns distance and the other returns a weighted score.

---

## Improvement Suggestions

### HIGH Priority

None identified - all core functionality is correctly documented.

### MEDIUM Priority

1. **Add MetricsSnapshot to public exports** (line 54-55 in stat_core.rs)
   - The doc shows `MetricsSnapshot { ... }` but it's not exported via `mod.rs`
   - Currently only accessible internally via `stat_core::inner::MetricsSnapshot`
   - If users need to inspect metrics, add `pub use stat_core::inner::MetricsSnapshot;` to `mod.rs`

### LOW Priority

1. **Clarify `fuzzy_match` vs `fuzzy_score` naming**
   - `fuzzy_match` returns Levenshtein **distance** (lower is better, 0 = exact match)
   - `fuzzy_score` returns a weighted **score** (higher is better)
   - Consider adding doc comments explaining the difference

2. **Add module-level documentation in `stat_core.rs`**
   - The file lacks a doc comment explaining it contains metrics infrastructure
   - Would help future maintainers understand the module purpose

3. **Consider adding `truncate_bytes` edge case test**
   - Test at line 105-108 covers UTF-8 boundary for single-byte chars truncated to 1 byte
   - But doesn't test the "empty result" case where `max_bytes=0`
   - Currently returns "... [truncated]" which may be acceptable but edge case not explicit

4. **Histogram capacity is magic number (1000)**
   - Line 123: `if vec.len() > 1000 { vec.pop_front(); }`
   - Consider making this configurable or documenting why 1000

---

## Summary

| Component | Status | Notes |
|-----------|--------|-------|
| clipboard.rs | ✅ OK | Feature-gated correctly |
| fuzzy.rs | ✅ OK | Implementations match |
| truncate.rs | ✅ OK | UTF-8 safe, logic correct |
| stat_core.rs | ✅ OK | Metrics system complete |

**Overall**: The architecture document is accurate and up-to-date. No bugs or critical discrepancies found. The implementation matches the documented behavior for all components.