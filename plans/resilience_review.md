# Resilience Module Architecture Review

**Review Date**: 2026-05-26
**Reviewer**: Architecture Review
**Source**: `architecture/resilience.md` vs `src/resilience/`

---

## Summary

The resilience.md documentation is **mostly accurate** but has one error in the state transition diagram and is missing a detail in the `call()` method implementation.

---

## 1. Module Location

| Claim | Actual | Status |
|-------|--------|--------|
| Location: `src/resilience/` | `src/resilience/mod.rs`, `src/resilience/circuit.rs` | VERIFIED |

---

## 2. CircuitBreakerInner Struct (lines 17-28)

**Claim**: 9 fields listed

**Actual** (`circuit.rs:30-41`):
```rust
struct CircuitBreakerInner {
    name: String,
    state: TokioRwLock<CircuitState>,       // Verified: TokioRwLock
    failure_count: TokioRwLock<usize>,
    success_count: TokioRwLock<usize>,
    last_failure_time: TokioRwLock<Option<Instant>>,
    half_open_start_time: TokioRwLock<Option<Instant>>,
    failure_threshold: usize,
    timeout_secs: u64,
    success_threshold: usize,
    max_half_open_duration: Duration,        // Verified: 30 seconds default
}
```

| Field | Documentation | Actual | Status |
|-------|---------------|--------|--------|
| name | String | String | VERIFIED |
| state | TokioRwLock | TokioRwLock | VERIFIED |
| failure_count | TokioRwLock | TokioRwLock | VERIFIED |
| success_count | TokioRwLock | TokioRwLock | VERIFIED |
| last_failure_time | TokioRwLock<Option<Instant>> | TokioRwLock<Option<Instant>> | VERIFIED |
| half_open_start_time | TokioRwLock<Option<Instant>> | TokioRwLock<Option<Instant>> | VERIFIED |
| failure_threshold | usize | usize | VERIFIED |
| timeout_secs | u64 | u64 | VERIFIED |
| success_threshold | usize | usize | VERIFIED |
| max_half_open_duration | Duration (30s default) | Duration::from_secs(30) at line 66 | VERIFIED |

**Field Count**: 10 fields in actual code (not 9 as might be implied by documentation structure)

---

## 3. CircuitState Enum (lines 35-42)

**Claim**:
```rust
pub enum CircuitState {
    Closed,    // Normal operation
    Open,      // Failing, reject requests
    HalfOpen,  // Testing if recovered
}
```

**Actual** (`circuit.rs:8-13`):
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}
```

**Status**: VERIFIED - variant names and order match exactly.

---

## 4. State Transition Diagram (lines 44-74)

**Diagram shows**: Open → HalfOpen via "recovery_timeout elapsed"

**Actual Code** (`circuit.rs:79-98`):
```rust
pub async fn is_available(&self) -> bool {
    let mut state = self.inner.state.write().await;
    match *state {
        CircuitState::Closed | CircuitState::HalfOpen => true,
        CircuitState::Open => {
            if let Some(last_failure) = *self.inner.last_failure_time.read().await {
                let timeout = Duration::from_secs(self.inner.timeout_secs);
                if last_failure.elapsed() >= timeout {
                    *state = CircuitState::HalfOpen;
                    ...
                }
            }
            false
        }
    }
}
```

**Issue Found**: The transition from Open → HalfOpen is triggered by `last_failure.elapsed() >= timeout_secs`, not a "recovery_timeout". The diagram's labeling is misleading - it should reference `timeout_secs` (the recovery duration since last failure), not "recovery_timeout".

**Status**: INCORRECT - The transition trigger is last_failure time elapsed, not a separate recovery_timeout.

---

## 5. is_available() Implementation (lines 76-102)

**Claim**: Uses write lock from start to avoid TOCTOU

**Actual** (`circuit.rs:79-98`):
```rust
pub async fn is_available(&self) -> bool {
    let mut state = self.inner.state.write().await;  // Verified: write lock
    match *state {
        CircuitState::Closed | CircuitState::HalfOpen => true,
        CircuitState::Open => {
            if let Some(last_failure) = *self.inner.last_failure_time.read().await {
                let timeout = Duration::from_secs(self.inner.timeout_secs);
                if last_failure.elapsed() >= timeout {
                    *state = CircuitState::HalfOpen;
                    *self.inner.half_open_start_time.write().await = Some(Instant::now());
                    tracing::info!(
                        "circuit breaker {} transitioned to HalfOpen",
                        self.inner.name
                    );
                    return true;
                }
            }
            false
        }
    }
}
```

**Status**: VERIFIED - Implementation matches exactly, including the write lock pattern and the HalfOpen transition logic.

---

## 6. call() Method (lines 104-128)

**Documentation shows basic structure** but is **missing** the HalfOpen timeout check.

**Actual Code** (`circuit.rs:102-137`):
```rust
#[instrument(skip(self, op), fields(breaker_name = %self.inner.name))]
pub async fn call<F, R, E>(&self, op: F) -> Result<R, E>
where
    F: core::future::Future<Output = Result<R, E>>,
    E: From<CircuitError>,
{
    if !self.is_available().await {
        let state = *self.inner.state.read().await;
        if state == CircuitState::Open {
            return Err(CircuitError::Open(self.inner.name.clone()).into());
        }
    }

    // MISSING FROM DOCS: HalfOpen timeout check
    if let CircuitState::HalfOpen = *self.inner.state.read().await {
        if let Some(start_time) = *self.inner.half_open_start_time.read().await {
            if start_time.elapsed() >= self.inner.max_half_open_duration {
                *self.inner.state.write().await = CircuitState::Open;
                *self.inner.half_open_start_time.write().await = None;
                *self.inner.last_failure_time.write().await = None;
                tracing::warn!(
                    "circuit breaker {} transitioned to Open after HalfOpen timeout",
                    self.inner.name
                );
                return Err(CircuitError::Open(self.inner.name.clone()).into());
            }
        }
    }

    let result = op.await;

    match &result {
        Ok(_) => self.record_success().await,
        Err(_) => self.record_failure().await,
    }

    result
}
```

**Status**: INCOMPLETE - Documentation shows basic call() pattern but omits the HalfOpen→Open timeout logic that exists in actual code at lines 114-127.

---

## 7. record_success() Behavior (lines 130-136)

**Documentation claims**:
- **Closed**: Resets failure_count to 0
- **HalfOpen**: Increments success_count; transitions to Closed when success_threshold reached
- **Open**: No action

**Actual Code** (`circuit.rs:139-158`):
```rust
pub async fn record_success(&self) {
    let mut state = self.inner.state.write().await;
    match *state {
        CircuitState::Closed => {
            *self.inner.failure_count.write().await = 0;
        }
        CircuitState::HalfOpen => {
            let mut count = self.inner.success_count.write().await;
            *count += 1;
            if *count >= self.inner.success_threshold {
                *state = CircuitState::Closed;
                *self.inner.failure_count.write().await = 0;
                *self.inner.success_count.write().await = 0;
                *self.inner.last_failure_time.write().await = None;
                tracing::info!("circuit breaker {} transitioned to Closed", self.inner.name);
            }
        }
        CircuitState::Open => {}
    }
}
```

**Status**: VERIFIED - All behavior matches exactly, including the additional resets when transitioning Closed (success_count also reset to 0).

---

## 8. record_failure() Behavior (lines 137-140)

**Documentation claims**:
- **Closed**: Increments failure_count; transitions to Open when failure_threshold exceeded
- **HalfOpen**: Transitions to Open immediately
- **Open**: No action

**Actual Code** (`circuit.rs:160-186`):
```rust
pub async fn record_failure(&self) {
    let mut state = self.inner.state.write().await;
    *self.inner.last_failure_time.write().await = Some(Instant::now());
    match *state {
        CircuitState::Closed => {
            let mut count = self.inner.failure_count.write().await;
            *count += 1;
            if *count >= self.inner.failure_threshold {
                *state = CircuitState::Open;
                tracing::warn!(
                    "circuit breaker {} transitioned to Open after {} failures",
                    self.inner.name,
                    self.inner.failure_threshold
                );
            }
        }
        CircuitState::HalfOpen => {
            *state = CircuitState::Open;
            *self.inner.success_count.write().await = 0;
            tracing::warn!(
                "circuit breaker {} transitioned to Open after HalfOpen failure",
                self.inner.name
            );
        }
        CircuitState::Open => {}
    }
}
```

**Status**: VERIFIED - All behavior matches exactly.

---

## 9. FallbackProvider Integration (lines 142-149)

**Documentation claims**:
- Default parameters: `failure_threshold=3`, `timeout_secs=60`, `success_threshold=2`
- Checks `is_available()` before calling provider
- Records success/failure after each call
- Exponential backoff: `2^i` seconds (i=0→1s, i=1→2s, i=2→4s...), capped at 30s
- HalfOpen→Open timeout: 30s default via `max_half_open_duration`

**Actual Code** (`fallback.rs:21-24`):
```rust
let circuit_breakers = providers
    .iter()
    .map(|p| CircuitBreaker::new(p.name(), 3, 60, 2))
    .collect();
```

**Exponential backoff** (`fallback.rs:106-109`):
```rust
let delay_secs = (2u64.pow(i as u32)).min(30);
```

**Status**: VERIFIED - All parameters and behavior match exactly.

---

## 10. ProviderError::CircuitOpen

**Documentation at line 149**: Mentions `CircuitError::Open` conversion via `From` trait

**Actual** (`provider/fallback.rs:63`):
```rust
last_error = Some(ProviderError::CircuitOpen(provider.name().to_string()));
```

The error module has `CircuitError` that converts to `ProviderError::CircuitOpen` as documented in provider.md line 369 and the skill document at line 147.

**Status**: VERIFIED.

---

## Findings Summary

| Item | Line(s) in Doc | Status |
|------|----------------|--------|
| Module location | 7 | VERIFIED |
| CircuitBreakerInner fields | 17-28 | VERIFIED |
| CircuitState enum | 35-42 | VERIFIED |
| State transition diagram | 44-74 | INCORRECT - Uses `last_failure.elapsed()`, not "recovery_timeout" |
| is_available() implementation | 76-102 | VERIFIED |
| call() method | 104-128 | INCOMPLETE - Missing HalfOpen timeout check (actual lines 114-127) |
| record_success() behavior | 130-136 | VERIFIED |
| record_failure() behavior | 137-140 | VERIFIED |
| FallbackProvider defaults | 145 | VERIFIED |
| Exponential backoff formula | 148 | VERIFIED |
| HalfOpen→Open timeout | 149 | VERIFIED |

---

## Corrections Needed

1. **State Transition Diagram (lines 56)**: Change "recovery_timeout elapsed" to "last_failure.elapsed() >= timeout_secs" to accurately describe the trigger.

2. **call() Method Documentation (lines 104-128)**: Add the HalfOpen timeout check that exists in actual code at `circuit.rs:114-127`:

```rust
if let CircuitState::HalfOpen = *self.inner.state.read().await {
    if let Some(start_time) = *self.inner.half_open_start_time.read().await {
        if start_time.elapsed() >= self.inner.max_half_open_duration {
            // Transition to Open and return error
        }
    }
}
```

---

## Source Files Reviewed

| File | Lines | Purpose |
|------|-------|---------|
| `src/resilience/mod.rs` | 1-8 | Module exports |
| `src/resilience/circuit.rs` | 1-261 | CircuitBreaker implementation |
| `src/provider/fallback.rs` | 1-350 | FallbackProvider with circuit breaker integration |

---

## Cross-References Verified

- `architecture/provider.md` line 472 references resilience.md - VERIFIED
- `.opencode/skills/resilience/SKILL.md` - VERIFIED - has additional context on integration