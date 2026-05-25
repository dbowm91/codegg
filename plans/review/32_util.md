# Review: architecture/util.md vs src/util/

**Reviewed**: 2026-05-25

## Verification Summary

All source files verified: `clipboard.rs`, `fuzzy.rs`, `truncate.rs`, `stat_core.rs`, `mod.rs`

---

## Verified Correct Items

| Item | Location | Status |
|------|----------|--------|
| `copy_to_clipboard(text: &str) -> Result<(), AppError>` | clipboard.rs:4,12 | ✅ |
| `read_from_clipboard() -> Option<String>` | clipboard.rs:19,26 | ✅ |
| `arboard` feature gate | clipboard.rs:3,11,18,25 | ✅ |
| `fuzzy_match(query, candidates) -> Vec<(String, usize)>` | fuzzy.rs:3 | ✅ |
| `fuzzy_score(query, target) -> usize` | fuzzy.rs:12 | ✅ |
| Levenshtein distance sorting (lower=better) | fuzzy.rs:6-8 | ✅ |
| Case-insensitive fuzzy_score with bonuses | fuzzy.rs:23-28 | ✅ |
| `strsim` dependency | fuzzy.rs:1 | ✅ |
| `truncate_bytes(text, max_bytes)` signature | truncate.rs:16 | ✅ |
| UTF-8 boundary safe truncation | truncate.rs:20-25 | ✅ |
| `"... [truncated]"` suffix | truncate.rs:26 | ✅ |
| Metrics struct fields | stat_core.rs:12-16 | ✅ |
| Counter/Gauge/Histogram/MetricsSnapshot | stat_core.rs:84,96,116,128-133 | ✅ |
| `metrics()` singleton function | stat_core.rs:137-139 | ✅ |
| `stat_core.rs` misleading filename note | architecture doc line 54,92 | ✅ |
| Module exports in mod.rs | mod.rs:1-4 | ✅ |

---

## Incorrect/Stale Items

### 1. `truncate_lines` description (line 51)

**Current doc**:
```
- truncate_lines: Keeps `max_lines/2` from start and end, shows "[X lines truncated]" in middle
```

**Issue**: Ambiguous wording. The implementation keeps `max_lines/2` lines from BOTH start AND end (total `max_lines` lines), not `max_lines/2` total.

**Actual behavior** (truncate.rs:1-14):
- `half = max_lines / 2`
- `lines[..half]` = first half from start
- `lines[lines.len() - half..]` = last half from end
- Total: `max_lines` lines displayed

**Recommended fix**: Change to:
```
- truncate_lines: Keeps `max_lines/2` lines from each end (total max_lines), shows "[X lines truncated]" in middle
```

---

## No Bugs Found

No bugs found in `src/util/` code. All implementations are correct and match their specifications.

---

## Line Numbers Requiring Updates

| Line | Change |
|------|--------|
| 51 | Clarify `truncate_lines` behavior: "Keeps `max_lines/2` lines from each end (total max_lines)" |

---

## Minor Documentation Note

The architecture doc (line 93) correctly notes `stat_core.rs` is a misleading filename. This is accurate and should be preserved.
