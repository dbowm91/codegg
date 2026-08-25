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

Clipboard operations using the `arboard` crate. Requires `arboard` feature flag.

```rust
pub fn copy_to_clipboard(text: &str) -> Result<(), AppError>;
pub fn read_from_clipboard() -> Option<String>;
```

**Feature Gate**: `arboard` must be enabled in Cargo.toml for clipboard support.

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
```## Integration Points

| Location | Usage |
|----------|-------|
| `src/tui/app/mod.rs` | Uses `fuzzy_score` for command filtering |
| `src/tui/command.rs` | Uses `fuzzy_score` for slash command matching |
| `src/tui/components/completion_overlay.rs` | Uses `fuzzy_score` for completion filtering |
| `src/tui/components/dialogs/share.rs` | Uses `clipboard` for URL copying |

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

- `arboard` (optional, requires `arboard` feature) - Clipboard operations
- `strsim` - Levenshtein distance for fuzzy matching
- `parking_lot` - Synchronization for metrics