# Util Module Architecture Review Findings

## Verified Claims

- **clipboard.rs** (lines 3-28): Feature-gated with `arboard` crate, `copy_to_clipboard` and `read_from_clipboard` functions present
- **fuzzy.rs fuzzy_match** (lines 3-10): Returns `Vec<(String, usize)>` sorted by Levenshtein distance
- **fuzzy.rs fuzzy_score** (lines 12-42): Returns weighted score with bonuses for start-of-string and consecutive matches
- **truncate.rs truncate_lines** (lines 1-14): Keeps half from start/end, shows line count in middle
- **truncate.rs truncate_bytes** (lines 16-27): Safely truncates at UTF-8 boundary
- **stat_core.rs Metrics** (lines 12-76): `counter`, `gauge`, `histogram`, `snapshot` methods with Counter/Gauge/Histogram impls
- **stat_core.rs MetricSnapshot** (lines 129-133): Snapshot struct with counters/gauges/histograms HashMaps
- **Line 92 "misleading filename" note**: Confirmed - `stat_core.rs` contains metrics, not file stats

## Stale Information

- **Line 33 fuzzy_match signature**: Shows `fuzzy_match(query: &str, candidates: &[String]) -> Vec<(String, usize)>` but example at line 109 uses `fuzzy_match("hel", &candidates)` without pattern to get score - misleading example

## Bugs Found

None.

## Improvements Suggested

1. **Line 109 example error**: `fuzzy_match("hel", &candidates)` returns `Vec<(String, usize)>` not `Vec<(String, usize)>` with second element as score - the example shows it being used like `fuzzy_score`. The example should show iterating results.

2. **Line 70-71 Counter/Gauge note**: Using `Arc<AtomicU64>` internally, not standard atomic types - appropriate for the use case but differs from what naive reader might expect.

## Cross-Module Issues

- **tool module uses util::validate_path** for path validation before file operations
- **tui uses fuzzy scoring** for command matching
