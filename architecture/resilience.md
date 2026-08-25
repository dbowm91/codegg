# Resilience Module

Circuit breaker pattern for provider fault tolerance, preventing
cascade failures when upstream LLM services are unavailable.

## Purpose

Provides a reusable `CircuitBreaker` that wraps async fallible
operations and tracks failure/success state to short-circuit calls
to unhealthy backends.

## Where It Lives

| Layer | Path | Role |
|-------|------|------|
| Canonical implementation | `crates/codegg-providers/src/circuit.rs` (282 lines) | `CircuitBreaker`, `CircuitState`, `CircuitError` |
| Core re-export | `crates/codegg-core/src/resilience.rs:6` | `pub use codegg_providers::circuit::{CircuitBreaker, CircuitError, CircuitState};` |
| Root re-export | `src/lib.rs:11` | `pub use codegg_core::resilience;` |

The `resilience` module is **purely a re-export**. All logic lives in
`codegg-providers`. There is no retry logic here — `CircuitBreaker`
only decides whether to admit or reject a call. Retry/backoff is the
caller's responsibility.

## How It Works

### States

```
    ┌─────────┐     failure_threshold exceeded     ┌──────┐
    │ Closed  │─────────────────────────────────────►│ Open │
    │(normal) │◄─────────────────────────────────────│(reject)│
    └─────────┘   success_threshold reached         └──┬───┘
         ▲                                              │
         │              timeout_secs elapsed            │
         │                    ┌─────────────┐           │
         └────────────────────┤  HalfOpen   │◄──────────┘
                              │ (probe one) │   timeout_secs
                              └──────┬──────┘
                                     │ failure
                                     ▼
                               ┌─────────┐
                               │  Open   │──┐
                               └─────────┘  │
                                     ▲      │ max_half_open_duration
                                     └──────┘
```

### State machine (circuit.rs)

- **Closed**: Normal operation. Failures increment `failure_count`.
  On reaching `failure_threshold`, transitions to Open. Successes
  reset `failure_count` to 0.
- **Open**: Rejects all calls with `CircuitError::Open`. After
  `timeout_secs` elapses (since last failure), transitions to
  HalfOpen.
- **HalfOpen**: Admits exactly **one probe** via `half_open_probe`
  (`AtomicBool` CAS). If the probe succeeds and `success_count`
  reaches `success_threshold`, transitions to Closed. If the probe
  fails, transitions to Open immediately. If `max_half_open_duration`
  (30s default) elapses without the probe completing, transitions
  back to Open and seeds `last_failure_time` so the normal
  Open→HalfOpen timeout applies before the next probe.

### is_available (circuit.rs:80)

Uses a **write lock** from the start to avoid TOCTOU races. When the
state is Open and the timeout has elapsed, atomically transitions
to HalfOpen and returns `true`.

### call (circuit.rs:103)

```rust
pub async fn call<F, R, E>(&self, op: F) -> Result<R, E>
where
    F: Future<Output = Result<R, E>>,
    E: From<CircuitError>,
```

Checks availability, then in HalfOpen enforces single-probe via
`half_open_probe` CAS. Executes the operation, records
success/failure.

### record_success (circuit.rs:156)

- **Closed**: Resets `failure_count` to 0.
- **HalfOpen**: Increments `success_count`; transitions to Closed
  when threshold reached. Resets all counters and clears
  `last_failure_time`. Releases `half_open_probe`.
- **Open**: No action.

### record_failure (circuit.rs:181)

- **Closed**: Increments `failure_count`; transitions to Open when
  threshold exceeded.
- **HalfOpen**: Transitions to Open immediately. Resets
  `success_count`. Releases `half_open_probe`.
- **Open**: No action.

Always sets `last_failure_time`.

## Key Types & APIs

### CircuitBreaker (circuit.rs:44)

```rust
#[derive(Clone)]
pub struct CircuitBreaker {
    inner: Arc<CircuitBreakerInner>,
}
```

Constructed via:
```rust
pub fn new(
    name: impl Into<String>,
    failure_threshold: usize,
    timeout_secs: u64,
    success_threshold: usize,
) -> Self
```

### CircuitBreakerInner (circuit.rs:29)

```rust
struct CircuitBreakerInner {
    name: String,
    state: TokioRwLock<CircuitState>,
    failure_count: TokioRwLock<usize>,
    success_count: TokioRwLock<usize>,
    last_failure_time: TokioRwLock<Option<Instant>>,
    half_open_start_time: TokioRwLock<Option<Instant>>,
    half_open_probe: AtomicBool,
    failure_threshold: usize,
    timeout_secs: u64,
    success_threshold: usize,
    max_half_open_duration: Duration,   // 30s
}
```

### CircuitState (circuit.rs:8)

```rust
pub enum CircuitState { Closed, Open, HalfOpen }
```

### CircuitError (circuit.rs:14)

```rust
pub enum CircuitError { Open(String) }
```

Implements `Display`, `Error`.

## FallbackProvider Integration

`FallbackProvider` (`crates/codegg-providers/src/fallback.rs`) creates
one `CircuitBreaker` per provider:

```rust
CircuitBreaker::new(p.name(), 3, 60, 2)
```

- `failure_threshold=3`, `timeout_secs=60`, `success_threshold=2`
- Checks `is_available()` before calling each provider
- Records success/failure after each call
- Exponential backoff between providers: `2^i` seconds (i=0→1s,
  i=1→2s, i=2→4s…), capped at 30s
- Default retryable status codes: 429, 500, 502, 503, 504

## Invariants & Gotchas

- **Single-probe guarantee**: The `half_open_probe` `AtomicBool` with
  CAS ensures exactly one concurrent probe in HalfOpen state. Second
  callers get `CircuitError::Open`.
- **Timeout seeding**: When HalfOpen→Open is forced by
  `max_half_open_duration`, `last_failure_time` is seeded to `now`
  so the breaker doesn't get stuck Open forever.
- **No retry logic**: `CircuitBreaker` only gates admission. Callers
  must implement their own retry/backoff.
- **Clone is cheap**: `CircuitBreaker` wraps `Arc<CircuitBreakerInner>`.
  FallbackProvider clones per-call.

## Testing

```bash
cargo test -p codegg-providers circuit    # unit tests
```

Tests cover: HalfOpen single-probe enforcement, HalfOpen timeout
recovery via Open, and basic state transitions.

## Related Docs

- [provider.md](provider.md) — Provider architecture and FallbackProvider
- `crates/codegg-providers/src/circuit.rs` — Canonical implementation
- `crates/codegg-providers/src/fallback.rs` — Consumer integration
