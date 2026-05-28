# Util Module

The `util` module provides common utility functions.

## Overview

**Location**: `src/util/`

**Key Responsibilities**:
- Clipboard operations (feature-gated)
- Fuzzy string matching and scoring
- Text truncation (by lines or bytes)
- Metrics collection (counters, gauges, histograms)

## Components

### clipboard.rs

Clipboard operations using the `arboard` crate. Requires `arboard` feature flag.

```rust
pub fn copy_to_clipboard(text: &str) -> Result<(), AppError>;
pub fn read_from_clipboard() -> Option<String>;
```

**Feature Gate**: `arboard` must be enabled in Cargo.toml for clipboard support.

### fuzzy.rs

Fuzzy string matching utilities using `strsim` crate for Levenshtein distance.

```rust
pub fn fuzzy_match(query: &str, candidates: &[String]) -> Vec<(String, usize)>;
pub fn fuzzy_score(query: &str, target: &str) -> usize;
```

- `fuzzy_match`: Returns candidates sorted by Levenshtein distance (lower is better)
- `fuzzy_score`: Returns weighted score for single target (case-insensitive, bonuses for start-of-string and consecutive matches)

**Dependencies**: `strsim`

### truncate.rs

Text truncation utilities for handling long content.

```rust
pub fn truncate_lines(text: &str, max_lines: usize) -> String;
pub fn truncate_bytes(text: &str, max_bytes: usize) -> String;
```

- `truncate_lines`: Keeps `max_lines/2` from start and end, shows "[X lines truncated]" in middle
- `truncate_bytes`: Safely truncates at UTF-8 character boundary, appends "... [truncated]"

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

**Note**: `metrics.rs` contains metrics infrastructure (counters, gauges, histograms), not file statistics as the misleading filename might suggest.

**Histogram bound**: The `Histogram` keeps a bounded buffer of at most 1000 entries (uses `VecDeque` with `pop_front()` when length exceeds 1000). This prevents unbounded memory growth.

### pricing.rs

LLM API cost calculation based on token usage.

```rust
pub struct ModelPricing {
    pub input_per_m: f64,    // Price per million input tokens (USD)
    pub output_per_m: f64,   // Price per million output tokens (USD)
    pub cached_discount: f64, // Discount factor for cached tokens (0.0=no discount, 1.0=free)
}

pub struct PricingService {
    rates: HashMap<String, ModelPricing>,
}

impl PricingService {
    pub fn new() -> Self;
    pub fn calculate_cost(&self, provider: &str, model: &str, input_tokens: i64, output_tokens: i64, cached_tokens: i64) -> f64;
}
```

- `calculate_cost()` returns cost in USD (0.0 if model not found in pricing table)
- Pricing lookup is by `"{provider}/{model}"` key (e.g., `"openai/gpt-4o"`)
- Covers OpenAI, Anthropic, Google, and MiniMax providers
- Cached tokens receive a discount based on `cached_discount` factor

### interner.rs

Thread-safe string interning for reducing memory allocation of repeated strings.

```rust
pub struct StringInterner {
    map: DashMap<Arc<str>, Arc<str>>,
}

impl StringInterner {
    pub fn new() -> Self;
    pub fn intern(&self, s: &str) -> Arc<str>;
    pub fn intern_string(&self, s: String) -> Arc<str>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

pub fn tool_interner() -> &'static StringInterner;
```

- `tool_interner()` returns a global `LazyLock<StringInterner>` used for interning tool names and identifiers
- `intern()` deduplicates strings via `DashMap` (concurrent hash map), returning `Arc<str>` references

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
for (name, score) in &results {
    println!("{name}: {score}");
}

let score = fuzzy_score("hello", "hello"); // case-insensitive scoring

// Truncation
let truncated = truncate_lines("line1\nline2\n...", 10);
let truncated = truncate_bytes("very long text...", 10);
```

## See Also

- [tool.md](tool.md) - Tools using utilities
- [tui.md](tui.md) - TUI uses fuzzy scoring for command matching

(Metadata: reviewed 2026-05-26)