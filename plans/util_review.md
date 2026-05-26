# Util Module Architecture Review

**Reviewed**: 2026-05-26
**Source**: `src/util/` vs `architecture/util.md`

---

## Module Organization

| File | Status | Notes |
|------|--------|-------|
| `clipboard.rs` | ✅ Verified | Feature-gated with `arboard` |
| `fuzzy.rs` | ✅ Verified | Uses `strsim` for Levenshtein |
| `metrics.rs` | ⚠️ Name mismatch | Doc says `stat_core.rs` but actual file is `metrics.rs` |
| `truncate.rs` | ✅ Verified | Lines and bytes truncation |

**Discrepancy**: The architecture document references `stat_core.rs` (line 54-92), but the actual file is named `metrics.rs`. The file contains metrics infrastructure, confirming the "misleading name" note is accurate—but the filename in the doc is wrong, not just misleading.

---

## Line Number Verification

| Section | Doc Line | Actual Line | Status |
|---------|----------|-------------|--------|
| `pub mod metrics` declaration | N/A | 3 (mod.rs) | ✅ |
| `Metrics::new()` | 62 | 19 | ✅ |
| `Metrics::counter()` | 63 | 27 | ✅ |
| `Metrics::gauge()` | 64 | 36 | ✅ |
| `Metrics::histogram()` | 65 | 45 | ✅ |
| `Metrics::snapshot()` | 66 | 54 | ✅ |
| `Counter::inc()` | 71 | 87 | ✅ |
| `Counter::add()` | 72 | 91 | ✅ |
| `Gauge::set()` | 77 | 99 | ✅ |
| `Gauge::inc()` | 78 | 103 | ✅ |
| `Gauge::dec()` | 79 | 107 | ✅ |
| `Histogram::record()` | 84 | 119 | ✅ |
| `metrics()` function | 88 | 137 | ✅ |
| `MetricsSnapshot` derive | 87 | 128 | ✅ |

All line numbers are correct, though they reference a non-existent `stat_core.rs`.

---

## Function Signature Verification

### clipboard.rs
| Doc Signature | Actual | Status |
|---------------|--------|--------|
| `pub fn copy_to_clipboard(text: &str) -> Result<(), AppError>` | ✅ Exact match | ✅ Line 4 |
| `pub fn read_from_clipboard() -> Option<String>` | ✅ Exact match | ✅ Line 19 |

### fuzzy.rs
| Doc Signature | Actual | Status |
|---------------|--------|--------|
| `pub fn fuzzy_match(query: &str, candidates: &[String]) -> Vec<(String, usize)>` | ✅ Exact match | ✅ Line 3 |
| `pub fn fuzzy_score(query: &str, target: &str) -> usize` | ✅ Exact match | ✅ Line 12 |

### truncate.rs
| Doc Signature | Actual | Status |
|---------------|--------|--------|
| `pub fn truncate_lines(text: &str, max_lines: usize) -> String` | ✅ Exact match | ✅ Line 1 |
| `pub fn truncate_bytes(text: &str, max_bytes: usize) -> String` | ✅ Exact match | ✅ Line 16 |

---

## Field Count Verification

### Metrics struct (metrics.rs:12-16)
Doc claims: `counters`, `gauges`, `histograms` (3 fields)
Actual: ✅ 3 fields confirmed

### Counter struct (metrics.rs:84)
Doc claims: `Arc<AtomicU64>`
Actual: ✅ Correct

### Gauge struct (metrics.rs:96)
Doc claims: `Arc<AtomicU64>`
Actual: ✅ Correct

### Histogram struct (metrics.rs:116)
Doc claims: `Arc<Mutex<VecDeque<u64>>>`
Actual: ✅ Correct

### MetricsSnapshot (metrics.rs:129-133)
Doc claims: `counters`, `gauges`, `histograms` (3 public fields)
Actual: ✅ 3 public fields confirmed

---

## Behavior Verification

### fuzzy_match (fuzzy.rs:3-10)
- Doc: "Returns candidates sorted by Levenshtein distance (lower is better)"
- Actual: ✅ Confirmed - sorts by score ascending (lower distance = better match)

### fuzzy_score (fuzzy.rs:12-42)
- Doc: "Returns weighted score for single target (case-insensitive, bonuses for start-of-string and consecutive matches)"
- Actual: ✅ Confirmed - case-insensitive via `eq_ignore_ascii_case`, bonuses for:
  - Character at index 0 (line 25)
  - Consecutive matches (line 26)
- Returns 0 if query not fully matched (line 38)

### truncate_lines (truncate.rs:1-14)
- Doc: "Keeps `max_lines/2` from start and end, shows '[X lines truncated]' in middle"
- Actual: ✅ Confirmed - uses `half = max_lines / 2` (line 6), formats as `"... [N lines truncated] ..."` (line 9)

### truncate_bytes (truncate.rs:16-27)
- Doc: "Safely truncates at UTF-8 character boundary, appends '... [truncated]'"
- Actual: ✅ Confirmed - uses `char_indices()` to find safe boundary (line 20-25), output ends with `... [truncated]` (line 26)

### Histogram::record (metrics.rs:119-126)
- Doc: No limit behavior specified
- Actual: ⚠️ **Unbounded memory growth** - `vec.push_back(v)` with only `pop_front()` when `len() > 1000`. No limit on unique histogram names. If many distinct histogram names are used, memory grows indefinitely.

---

## Dependencies Verification

| Dependency | Doc | Actual | Status |
|------------|-----|--------|--------|
| `arboard` | Required for clipboard | ✅ Used in clipboard.rs | ✅ |
| `strsim` | Used for Levenshtein | ✅ Used in fuzzy.rs | ✅ |

---

## Issues Found

### 1. Wrong Filename in Documentation
**Severity**: Low
**Location**: architecture/util.md:54
**Description**: Document references `stat_core.rs` but actual file is `metrics.rs`. This is a documentation bug, not a code bug.
**Recommendation**: Update line 54 from `### stat_core.rs` to `### metrics.rs`

### 2. Histogram Memory Growth (Known Issue)
**Severity**: Medium
**Location**: metrics.rs:122-124
**Description**: `Histogram::record()` only pops from front when len > 1000, but imposes no limit on unique histogram names. If the application creates many distinct histogram names, memory grows unbounded.
**Note**: This is already documented in AGENTS.md as a known issue.

### 3. Documentation Accuracy
**Overall**: The architecture document is generally accurate. All function signatures, line numbers (adjusted for filename), and behavior descriptions match the source code. The only substantive error is the filename `stat_core.rs` instead of `metrics.rs`.

---

## Summary

| Category | Status |
|----------|--------|
| Module organization | ⚠️ 1 error (wrong filename) |
| Function signatures | ✅ All correct |
| Line numbers | ✅ All correct |
| Field counts | ✅ All correct |
| Behavior descriptions | ✅ All correct |
| Dependencies | ✅ All correct |

**Conclusion**: The architecture document is largely accurate but contains one filename error (`stat_core.rs` should be `metrics.rs`). All other claims verified against source code.