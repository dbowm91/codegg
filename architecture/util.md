# Util Module

Common utility functions shared across CodeGG.

## Purpose

Provides clipboard access, fuzzy string matching, text truncation,
internal metrics, LLM pricing data, and string interning.

## Where It Lives

- `src/util/mod.rs` — module root, re-exports
- `src/util/clipboard.rs` — clipboard operations
- `src/util/fuzzy.rs` — fuzzy matching/scoring
- `src/util/truncate.rs` — text truncation (lines, bytes, prefix, suffix)
- `src/util/metrics.rs` — counters, gauges, histograms
- `src/util/pricing.rs` — LLM API cost calculation
- `src/util/interner.rs` — thread-safe string interning

## How It Works

### Clipboard (`clipboard.rs`)

Feature-gated behind `arboard` (enabled by default). Uses `arboard` crate
with `default-features = false` — text clipboard API only, no image-data
stack.

When `arboard` is disabled:
- `copy_to_clipboard()` returns `Err(AppError::Clipboard(...))`
- `read_from_clipboard()` returns `None`

### Fuzzy Matching (`fuzzy.rs`)

Two algorithms:

- `fuzzy_match(query, candidates)` — Levenshtein distance via `strsim`.
  Returns candidates sorted by distance (lower = better).
- `fuzzy_score(query, target)` — character-by-character subsequence match
  with bonuses for start-of-string and consecutive matches. Case-insensitive.
  Returns 0 if query chars not all found in order.

### Truncation (`truncate.rs`)

| Function | Behavior |
|----------|----------|
| `truncate_lines(text, max)` | Head/tail truncation, inserts `[N lines truncated]` |
| `truncate_bytes(text, max)` | UTF-8 safe, appends `... [truncated]` |
| `truncate_prefix(text, max)` | Returns `&str` prefix fitting `max` bytes (UTF-8 safe) |
| `truncate_suffix(text, max)` | Returns `&str` suffix fitting `max` bytes (UTF-8 safe) |

`truncate_prefix` and `truncate_suffix` are re-exported from `mod.rs`.

### Metrics (`metrics.rs`)

In-memory observability primitives behind `pub mod inner`:

- `Counter` — atomic `u64` increment/add
- `Gauge` — atomic `u64` set/inc/dec (saturates at 0)
- `Histogram` — bounded `VecDeque<u64>` (max 1000 entries, FIFO eviction)
- `MetricsSnapshot` — point-in-time copy of all counters/gauges/histograms

Global singleton: `inner::metrics()` returns `&'static Metrics` via
`LazyLock`.

### Pricing (`pricing.rs`)

`PricingService` calculates LLM API costs in USD. Pricing lookup is by
`"{provider}/{model}"` key (lowercased), with fuzzy substring fallback.

Providers covered: OpenAI (GPT-4, GPT-3.5, o1, o3), Anthropic (Claude
Opus/Sonnet/Haiku), Google (Gemini), MiniMax.

Formula: `input_cost = non_cached × rate + cached × rate × discount`,
`output_cost = output × rate`.

### String Interning (`interner.rs`)

`StringInterner` wraps a `DashMap<Arc<str>, Arc<str>>` for concurrent
deduplication. `tool_interner()` returns a global `LazyLock<StringInterner>`
used for interning tool names and identifiers in `src/tool/mod.rs:711`.

## Key Types & APIs

| Type | File:Line | Purpose |
|------|-----------|---------|
| `copy_to_clipboard()` | `clipboard.rs:4` | Copy text to system clipboard |
| `read_from_clipboard()` | `clipboard.rs:19` | Read text from system clipboard |
| `fuzzy_match()` | `fuzzy.rs:3` | Levenshtein-based candidate ranking |
| `fuzzy_score()` | `fuzzy.rs:12` | Subsequence score with bonuses |
| `truncate_lines()` | `truncate.rs:1` | Head/tail line truncation |
| `truncate_bytes()` | `truncate.rs:19` | UTF-8 safe byte truncation |
| `truncate_prefix()` | `truncate.rs:34` | UTF-8 safe prefix slice |
| `truncate_suffix()` | `truncate.rs:50` | UTF-8 safe suffix slice |
| `Metrics` | `metrics.rs:12` | Global metrics singleton |
| `Counter` | `metrics.rs:84` | Atomic counter |
| `Gauge` | `metrics.rs:96` | Atomic gauge (saturating dec) |
| `Histogram` | `metrics.rs:116` | Bounded histogram (1000 entries) |
| `MetricsSnapshot` | `metrics.rs:128` | Point-in-time metrics copy |
| `ModelPricing` | `pricing.rs:7` | Per-model token pricing |
| `PricingService` | `pricing.rs:20` | Cost calculation service |
| `StringInterner` | `interner.rs:6` | Concurrent string dedup |
| `tool_interner()` | `interner.rs:41` | Global tool-name interner |

## Configuration Surface

- **`arboard` feature**: enables clipboard support (default on).
  `default-features = false` on the arboard dep — text API only.
- No config-file options for util module.

## Invariants & Gotchas

- **Histogram cap**: `Histogram` keeps at most 1000 entries via
  `pop_front()` (`metrics.rs:122-124`).
- **Gauge saturates at zero**: `dec()` uses `saturating_sub(1)`
  (`metrics.rs:111`).
- **Fuzzy score returns 0 for incomplete matches**: if not all query
  chars found in order, returns 0 (`fuzzy.rs:37-38`).
- **Pricing fallback**: if exact key not found, falls back to substring
  containment match (`pricing.rs:241-250`). Returns `0.0` if no match.
- **`tool_interner()` is process-global**: never cleared, grows monotonically.

## Related Docs

- [tool.md](tool.md) — tools using utilities
- [tui.md](tui.md) — TUI uses fuzzy scoring for command matching
