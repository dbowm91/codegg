---
name: util
description: Utility functions for clipboard, fuzzy matching, text truncation, metrics collection, string interning, and model pricing lookup
version: 1.1.0
tags:
  - clipboard
  - fuzzy
  - truncate
  - metrics
  - interner
  - pricing
  - utilities
---

# Util Module Guide

This skill covers the utility functions in codegg for common operations.
The authoritative contract lives in `architecture/util.md`; this skill is the
quick orientation map.

## Overview

The `src/util/` module provides:
- Clipboard operations (feature-gated with `arboard`)
- Fuzzy string matching and scoring (using `strsim`)
- Text truncation (by lines or bytes, prefix and suffix)
- Metrics collection (counters, gauges, histograms)
- String interning (`interner.rs`)
- Model cost-per-token pricing tables (`pricing.rs`)

## Components

### clipboard.rs

Clipboard operations using the `arboard` crate, behind the `arboard`
Cargo feature.

```rust
pub fn copy_to_clipboard(text: &str) -> Result<(), AppError>;
pub fn read_from_clipboard() -> Option<String>;
```

**Feature Gate**: `arboard` is a **default** feature (`Cargo.toml:356`,
`default = ["arboard"]`), so it is on unless you build with
`--no-default-features`. Both functions exist either way — the
`#[cfg(not(feature = "arboard"))]` twins return `Err(AppError::Clipboard(...))`
and `None` respectively (`clipboard.rs:13`, `:27`). The dependency is declared
`default-features = false` (`Cargo.toml:197`), so it is the text-only API.

### fuzzy.rs

Fuzzy string matching using Levenshtein distance and weighted scoring.

```rust
pub fn fuzzy_match(query: &str, candidates: &[String]) -> Vec<(String, usize)>;
pub fn fuzzy_score(query: &str, target: &str) -> usize;
```

- `fuzzy_match`: Returns all candidates sorted by Levenshtein distance (lower is better)
- `fuzzy_score`: Returns weighted score for single target (case-insensitive, bonuses for start-of-string and consecutive matches)

**Dependencies**: `strsim` crate for Levenshtein distance

### truncate.rs

Text truncation utilities for handling long content.

```rust
pub fn truncate_lines(text: &str, max_lines: usize) -> String;
pub fn truncate_bytes(text: &str, max_bytes: usize) -> String;
pub fn truncate_prefix(text: &str, max_bytes: usize) -> &str;
pub fn truncate_suffix(text: &str, max_bytes: usize) -> &str;
```

- `truncate_lines`: Keeps `max_lines/2` from start and end, shows "[X lines truncated]" in middle
- `truncate_bytes`: Safely truncates at UTF-8 character boundary, appends "... [truncated]"
- `truncate_prefix` / `truncate_suffix`: Borrowing variants that keep the tail/head respectively, truncated at a UTF-8 char boundary

`mod.rs` re-exports `truncate_bytes`, `truncate_lines`, `truncate_prefix`, and `truncate_suffix`.

### interner.rs

Small string interner for deduplicating repeated strings (e.g., tool names in metrics/tracing paths).

### pricing.rs

Model cost-per-token pricing lookup tables used by usage/cost reporting. Pricing entries are keyed by provider/model identifiers.

### metrics.rs

Internal metrics collection system for observability.

```rust
pub mod inner {
    pub struct Metrics { ... }
    impl Metrics {
        pub fn new() -> Self;
        pub fn counter(&self, name: &str) -> Counter;
        pub fn gauge(&self, name: &str) -> Gauge;
        pub fn histogram(&self, name: &str) -> Histogram;
        pub fn snapshot(&self) -> MetricsSnapshot;
    }

    pub struct Counter(Arc<AtomicU64>);
    impl Counter {
        pub fn inc(&self);
        pub fn add(&self, v: u64);
    }

    pub struct Gauge(Arc<AtomicU64>);
    impl Gauge {
        pub fn set(&self, v: u64);
        pub fn inc(&self);
        pub fn dec(&self);
    }

    pub struct Histogram(Arc<Mutex<VecDeque<u64>>>);
    impl Histogram {
        pub fn record(&self, v: u64);
    }

    pub struct MetricsSnapshot { ... }
    pub fn metrics() -> &'static Metrics;
}
```

## Usage Examples

```rust
use crate::util::clipboard;
use crate::util::fuzzy::{fuzzy_match, fuzzy_score};
use crate::util::truncate::{truncate_lines, truncate_bytes};

// Clipboard
clipboard::copy_to_clipboard("hello")?;
if let Some(text) = clipboard::read_from_clipboard() {
    println!("Pasted: {}", text);
}

// Fuzzy matching
let candidates = vec!["hello".to_string(), "world".to_string()];
let results = fuzzy_match("hel", &candidates); // sorted by score

let score = fuzzy_score("hello", "hello"); // case-insensitive scoring

// Truncation
let truncated = truncate_lines("line1\nline2\n...", 10);
let truncated = truncate_bytes("very long text...", 10);
```

## Integration Points

| Location | Usage |
|----------|-------|
| `src/tui/app/mod.rs` | Uses `fuzzy_score` for subagent-name and item filtering, and `clipboard::copy_to_clipboard` for export/share/copy actions |
| `src/tui/command.rs` | Uses `fuzzy_score` for slash command matching |
| `src/tui/components/completion_overlay.rs` | Uses `fuzzy_score` for completion filtering |
| `src/tui/components/dialogs/share.rs` | Uses `clipboard` for URL copying |

Prefer naming the call site over pinning its line number: `src/tui/app/mod.rs`
is a large, frequently-churned file, so a line reference there drifts fast.

## Testing

Run util tests:
```bash
cargo test --lib -- util
```

Tests include:
- `fuzzy::tests::*` - Fuzzy matching and scoring tests
- `truncate::tests::*` - Line and byte truncation tests
- `metrics::inner::tests::gauge_dec_saturates_at_zero` - Metrics tests

## Dependencies

- `arboard` (optional; the `arboard` feature is a **default** feature) - Clipboard operations
- `strsim` - Levenshtein distance for fuzzy matching
- `parking_lot` - Synchronization for metrics

## See Also

- `architecture/util.md` — authoritative contract, key-type line refs, invariants
- `.opencode/skills/tui/SKILL.md` — the heaviest consumer (`fuzzy_score`, clipboard)

## Source verification

Verified 2026-10-23 against `src/util/{clipboard,fuzzy,truncate,interner,
metrics,pricing,mod}.rs`, `Cargo.toml`, `src/tui/app/mod.rs`,
`src/tui/command.rs`, `src/tui/components/completion_overlay.rs`, and
`src/tui/components/dialogs/share.rs`. Confirmed: `fuzzy_match` /
`fuzzy_score` (`src/util/fuzzy.rs:3,12`); `truncate_lines` / `truncate_bytes` /
`truncate_prefix` / `truncate_suffix` (`src/util/truncate.rs:1,19,32,46`) and
their re-export at `src/util/mod.rs:8`; `gauge_dec_saturates_at_zero`
(`src/util/metrics.rs:150`); the `arboard` default feature (`Cargo.toml:356`)
and its `default-features = false` declaration (`Cargo.toml:197`); the
`#[cfg(not(feature = "arboard"))]` twins (`src/util/clipboard.rs:13,27`);
and the `strsim` / `parking_lot` / `arboard` dependencies.

Corrected: the `arboard` default feature was cited as `Cargo.toml:329-330`
(actually `:356`, a ~27-line drift that pointed at unrelated `rustls` config);
the `default-features = false` declaration was `:196` (actually `:197`); the
clipboard twins were `:12`/`:26` (the `cfg` attributes are at those lines but
the stubs are at `:13`/`:27`); and the three `src/tui/app/mod.rs` line
references (`:3206`, `:9380`, `:8699`) had drifted by more than a thousand
lines — `fuzzy_score` is now called at `:12434` and `:12501`, and
`copy_to_clipboard` at `:9136`, `:11625`, `:11654`, and `:16364`. Those three
refs were replaced with behaviour-level descriptions, because that file
churns too fast for a line number to stay true.

