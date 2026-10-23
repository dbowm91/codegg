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
| Canonical implementation | `crates/codegg-providers/src/circuit.rs` (342 lines) | `CircuitBreaker`, `CircuitState`, `CircuitError` |
| Core re-export | `crates/codegg-core/src/resilience.rs:6` | `pub use codegg_providers::circuit::{CircuitBreaker, CircuitError, CircuitState};` |
| Root re-export | `src/lib.rs:22` | `resilience` in the grouped `pub use codegg_core::{…}` block |

The `resilience` module is **purely a re-export**. All logic lives in
`codegg-providers`. There is no retry logic here — `CircuitBreaker`
only decides whether to admit or reject a call. Retry/backoff is the
caller's responsibility.

`codegg-core`'s `resilience` module exists so the core crate's public
surface stays complete after the provider crate was extracted; it has no
consumer of its own in-tree — the only references to it are the two
re-export sites above. The live consumer is `codegg-providers`' own
`fallback.rs`.

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

### is_available (circuit.rs:90, `#[deprecated]` at :89)

> **Deprecated**: `is_available()` is kept for backward compatibility
> only. Prefer `call()` which atomically owns admission and the
> half-open probe in a single write-lock critical section, preventing
> the TOCTOU race that `is_available()` exposes.

Uses a **write lock** from the start to avoid TOCTOU races. When the
state is Open and the timeout has elapsed, atomically transitions
to HalfOpen and returns `true`. `is_available()` does not take the
half-open probe, so it does not consume probe ownership — only
`call()` and `try_admit()` (`circuit.rs:139`) do.

### call (circuit.rs:114)

```rust
pub async fn call<F, R, E>(&self, op: F) -> Result<R, E>
where
    F: Future<Output = Result<R, E>>,
    E: From<CircuitError>,
```

Delegates admission to `try_admit()` (`circuit.rs:139`), which runs the
same state machine as `call` — including the HalfOpen single-probe CAS
claim — then awaits the operation and records exactly one outcome:
`record_success()` on `Ok`, `record_failure()` on `Err`. Admission failure
short-circuits with `E::from(CircuitError)`.

`try_admit()` is the admission-only variant for stream owners: it claims
the probe but leaves health accounting to the caller, so a successful
stream acquisition is not counted as health success before the terminal
stream outcome is known.

### record_success (circuit.rs:215)

- **Closed**: Resets `failure_count` to 0.
- **HalfOpen**: Increments `success_count`; transitions to Closed
  when threshold reached. Resets all counters and clears
  `last_failure_time`. Releases `half_open_probe`.
- **Open**: No action.

### record_failure (circuit.rs:240)

- **Closed**: Increments `failure_count`; transitions to Open when
  threshold exceeded.
- **HalfOpen**: Transitions to Open immediately. Resets
  `success_count`. Releases `half_open_probe`.
- **Open**: No action.

Always sets `last_failure_time`.

## Key Types & APIs

### CircuitBreaker (circuit.rs:53)

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

### CircuitBreakerInner (circuit.rs:38)

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

### CircuitState (circuit.rs:17)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}
```

### CircuitError (circuit.rs:24)

```rust
#[derive(Debug)]
pub enum CircuitError {
    Open(String),
}
```

Implements `Display` ("circuit breaker open for {name}") and
`std::error::Error`. It converts into `ProviderError::CircuitOpen`
(`crates/codegg-providers/src/error.rs`), so a rejected call surfaces
through the provider error taxonomy rather than as an opaque breaker
error.

## FallbackProvider Integration — library-only (C003)

> `FallbackProvider` (`crates/codegg-providers/src/fallback.rs`) is a
> library/test compatibility primitive only. It is never constructed on
> any production provider registry, session, or turn path. Production
> provider-turn retry/failover ownership lives solely in
> `src/agent/provider_turn.rs`. A static guard
> (`scripts/check_provider_resilience_ownership.py`) enforces this.

`FallbackProvider` creates one `CircuitBreaker` per explicitly supplied
provider when composed directly in library/test code
(`fallback.rs:38`):

```rust
CircuitBreaker::new(p.name(), 3, 60, 2)
```

- `failure_threshold=3`, `timeout_secs=60`, `success_threshold=2`
- Admits each provider via atomic `try_admit()`; terminal stream outcomes
  are charged exactly once to the same slot breaker (`wrap_stream_with_health`)
  so mid-stream failures stay visible to health. Recording acquisition
  success eagerly would reset the Closed failure count before the terminal
  stream failure is known.
- Failover happens when **either** the configured status list **or** the
  canonical `ProviderError::is_retryable()` taxonomy marks the acquisition
  failure retryable (`fallback.rs:135`). The taxonomy covers the
  transport/timeout/rate-limit cases that carry no numeric status; the
  status list preserves explicit operator configuration. A non-retryable
  failure stops the chain immediately.
- Exponential backoff between providers: `2^i` seconds (i=0→1s,
  i=1→2s, i=2→4s…), capped at 30s (`fallback.rs:149`)
- Default retryable status codes: 429, 500, 502, 503, 504
  (`fallback.rs:222`, `default_status_codes()`)

## Invariants & Gotchas

- **Single-probe guarantee**: The `half_open_probe` `AtomicBool` with
  CAS ensures exactly one concurrent probe in HalfOpen state. Second
  callers get `CircuitError::Open`.
- **Admission and probe claim are one critical section**: `try_admit()`
  decides admission *and* claims the half-open probe inside a single
  write-lock block. Splitting them would let a concurrent failure flip
  HalfOpen → Open between the check and the claim, silently discarding
  the probe.
- **Timeout seeding**: When HalfOpen→Open is forced by
  `max_half_open_duration`, `last_failure_time` is seeded to `now`
  so the breaker doesn't get stuck Open forever.
- **No retry logic**: `CircuitBreaker` only gates admission. Callers
  must implement their own retry/backoff.
- **Clone is cheap**: `CircuitBreaker` wraps `Arc<CircuitBreakerInner>`.
  FallbackProvider clones per-call.
- **One health owner**: a stream's health outcome is charged to the same
  breaker that admitted it — never to a second owner.

## Testing

```bash
cargo test -p codegg-providers --lib circuit     # breaker unit tests
cargo test -p codegg-providers --lib fallback    # integration behaviour
```

Two unit tests in `circuit.rs` (`mod tests`, `:271`):
`half_open_allows_only_one_probe` and `half_open_timeout_recovers_via_open`.
The latter asserts the Open→HalfOpen timeout still admits a new probe after
a forced HalfOpen timeout, and that `last_failure_time` stays seeded.

`fallback.rs` (`mod tests`, `:226`) covers the integration contract:
`test_fallback_non_retryable_error_stops_immediately`,
`transient_taxonomy_fails_over_despite_narrow_status_list`,
`terminal_stream_failure_is_charged_to_circuit`, and
`test_default_status_codes`.

The ownership boundary is enforced statically, not by a test:

```bash
python3 scripts/check_provider_resilience_ownership.py
```

## Related Docs

- [provider.md](provider.md) — Provider architecture and FallbackProvider
- [retry.md](retry.md) — the bounded retry chain that owns production
  provider-turn retry (C003)
- [error.md](error.md) — the `ProviderError` taxonomy the breaker converts into
- `crates/codegg-providers/src/circuit.rs` — Canonical implementation
- `crates/codegg-providers/src/fallback.rs` — Consumer integration

## Source verification

Verified 2026-10-23 against `crates/codegg-providers/src/circuit.rs`,
`crates/codegg-providers/src/fallback.rs`,
`crates/codegg-providers/src/error.rs`,
`crates/codegg-core/src/resilience.rs`, `crates/codegg-core/src/lib.rs`,
`src/lib.rs`, and `scripts/check_provider_resilience_ownership.py`.

Corrections made in this pass:
- **Corrected the root re-export ref.** The doc cited `src/lib.rs:11` for
  `pub use codegg_core::resilience;`. There is no such standalone
  statement: `resilience` is one name inside the grouped
  `pub use codegg_core::{…}` block at `src/lib.rs:20-24`, listed on
  **`src/lib.rs:22`**. (`src/lib.rs:11` is `pub use codegg_config as
  config;`.)
- **Corrected `default_status_codes()`'s ref** from `fallback.rs:223` to
  `fallback.rs:222` — `:223` is the `vec![]` body, one line below the
  function.
- **Added the failover predicate**, which the doc omitted entirely:
  failover triggers on the configured status list **or** the canonical
  `ProviderError::is_retryable()` taxonomy, not on the status list alone.
- **Added the stream-health charge** (`wrap_stream_with_health`) and the
  reason acquisition success is not recorded eagerly.
- **Added `CircuitError`'s conversion** into
  `ProviderError::CircuitOpen`.
- **Added `CircuitState`'s derives**, and the note that
  `codegg-core`'s `resilience` module has no in-tree consumer beyond the
  two re-export sites.
- **Added the `fallback.rs` test list** (4 tests) and the ownership guard
  command; the doc previously listed only the two `circuit.rs` tests.
- **Fixed the `Related Docs` links**, which mixed bare paths with a
  `resilience/`-style directory reference.

Confirmed correct as written: the file length `342`; `CircuitState` `:17`,
`CircuitError` `:24`, `CircuitBreakerInner` `:38`, `CircuitBreaker` `:53`,
`is_available` `:90` (`#[deprecated]` at `:89`), `call` `:114`,
`try_admit` `:139`, `record_success` `:215`, `record_failure` `:240`,
`mod tests` `:271`; the `resilience.rs:6` re-export; the 11-field
`CircuitBreakerInner` listing; `max_half_open_duration` = 30s; the
`FallbackProvider` wiring `CircuitBreaker::new(p.name(), 3, 60, 2)`
(`fallback.rs:38`); the `2^i` capped-at-30s backoff (`fallback.rs:149`); and
the existence of both `scripts/check_provider_resilience_ownership.py` and
`src/agent/provider_turn.rs`.
