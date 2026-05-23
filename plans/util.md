# Util Architecture Review

## Architecture Document
- Path: architecture/util.md

## Source Code Location
- src/util/

## Verification Summary
Pass

## Verified Claims (table format)
| Claim | Status | Notes |
|-------|--------|-------|
| clipboard.rs provides copy_to_clipboard and read_from_clipboard | Pass | Feature-gated with `arboard` cfg, matches exactly |
| clipboard requires `arboard` feature flag | Pass | Correctly gated with `#[cfg(feature = "arboard")]` |
| copy_to_clipboard returns Result<(), AppError> | Pass | Signature matches exactly |
| read_from_clipboard returns Option<String> | Pass | Signature matches exactly |
| fuzzy.rs uses strsim crate for Levenshtein distance | Pass | imports `strsim::levenshtein` |
| fuzzy_match(query, candidates) -> Vec<(String, usize)> | Pass | Returns vec of (candidate, distance) sorted by distance |
| fuzzy_score returns weighted score for single target | Pass | Case-insensitive, bonuses for start and consecutive matches |
| truncate_lines keeps max_lines/2 from start and end | Pass | Shows "[X lines truncated]" in middle |
| truncate_bytes safely truncates at UTF-8 boundary | Pass | Uses char_indices to find safe cut point, appends "... [truncated]" |
| stat_core.rs contains metrics infrastructure | Pass | Not file stats - contains Counter, Gauge, Histogram, MetricsSnapshot |
| Metrics::counter/gauge/histogram methods | Pass | All present with correct signatures |
| Counter has inc() and add() methods | Pass | Present and working |
| Gauge has set(), inc(), dec() methods | Pass | All present; dec() saturates at zero |
| Histogram has record() method | Pass | Present with 1000-element limit |
| metrics() singleton function exists | Pass | Returns &'static Metrics via LazyLock |

## Issues Found
### Bugs
- **None identified** - All implementations are correct

### Inconsistencies
- **Minor**: `fuzzy_match` return value description uses "score" but it's actually a **distance** (lower = better). The doc doesn't clarify this distinction from `fuzzy_score` which returns higher = better. This could confuse readers.

### Missing Documentation
- **Histogram limit**: The 1000-element cap on Histogram values is not documented. When recording more than 1000 values, older values are dropped (FIFO).
- **MetricsSnapshot fields**: The three public fields (`counters`, `gauges`, `histograms`) are not documented in the arch doc.
- **Gauge::dec() saturation behavior**: Not documented that dec() saturates at zero rather than wrapping.

### Improvement Opportunities
1. **fuzzy_match vs fuzzy_score naming clarification**: Add a note that fuzzy_match returns distance (lower is better) while fuzzy_score returns similarity (higher is better).
2. **Histogram capacity**: Document the 1000-element limit in the architecture.
3. **Metrics types export**: The `inner` module is public but its types (Counter, Gauge, Histogram, MetricsSnapshot) could be more clearly documented as part of the public API.

## Recommendations
1. Add a clarifying note to `fuzzy_match` description explaining the distance metric vs similarity score distinction.
2. Document the Histogram 1000-element limit.
3. Consider renaming `stat_core.rs` to something like `metrics.rs` to better reflect its purpose, though this is low priority since existing references would need updating.
