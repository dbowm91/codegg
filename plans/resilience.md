# Resilience Architecture Review

## Architecture Document
- Path: architecture/resilience.md

## Source Code Location
- src/resilience/

## Verification Summary
<Pass>

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| `CircuitBreakerInner` struct with `TokioRwLock` fields | Pass | Struct matches exactly at circuit.rs:30-41 |
| `CircuitState` enum (Closed, Open, HalfOpen) | Pass | circuit.rs:8-13 |
| `is_available()` uses write lock from start (TOCTOU fix) | Pass | circuit.rs:79-99 |
| `is_available()` transitions Open to HalfOpen after timeout | Pass | circuit.rs:84-95 |
| `call()` method wraps operations with circuit breaker | Pass | circuit.rs:102-137 |
| `record_success()` resets failure_count in Closed, transitions on success in HalfOpen | Pass | circuit.rs:139-158 |
| `record_failure()` increments failure_count, transitions on threshold in Closed, transitions on failure in HalfOpen | Pass | circuit.rs:160-186 |
| FallbackProvider creates circuit breakers with defaults (failure_threshold=3, timeout_secs=60, success_threshold=2) | Pass | fallback.rs:21-24 |
| FallbackProvider checks `is_available()` before calling | Pass | fallback.rs:56-66 |
| FallbackProvider records success/failure after calls | Pass | fallback.rs:70-88 |
| Exponential backoff: `2^i` seconds, capped at 30s | Pass | fallback.rs:107 - formula is `(2u64.pow(i as u32)).min(30)` |
| CircuitError::Open converts to ProviderError::CircuitOpen | Pass | error.rs:205-212 |
| State transition diagram | Partial | Missing HalfOpen→Open timeout transition |

## Issues Found

### Missing Documentation

1. **`half_open_start_time` field**: The architecture doc shows `is_available()` transitioning Open to HalfOpen, but doesn't mention that it also sets `half_open_start_time` (circuit.rs:88). This field is critical for the half-open timeout mechanism.

2. **`max_half_open_duration` (30s)**: The architecture doc doesn't document that once in HalfOpen state, if a call doesn't succeed within 30 seconds, the circuit transitions back to Open (circuit.rs:114-127). This is a significant behavior.

3. **`name()` and `state()` public methods**: The `CircuitBreaker` struct has `name()` (circuit.rs:71-73) and `state()` (circuit.rs:75-77) public methods that are not documented in the architecture.

4. **`#[instrument]` attribute**: The `call()` method has `#[instrument(skip(self, op), fields(breaker_name = %self.inner.name))]` which adds tracing instrumentation - not documented.

5. **`half_open_start_time` in inner struct**: Architecture shows 6 fields in `CircuitBreakerInner` but actual has 8 fields (circuit.rs:30-41). The missing fields are `half_open_start_time: TokioRwLock<Option<Instant>>` and `max_half_open_duration: Duration`.

6. **CircuitError Display/Debug implementations**: The `CircuitError` enum has `Display` and `Error` trait implementations (circuit.rs:20-28) that produce "circuit breaker open for {name}" - not documented.

7. **HalfOpen→Open timeout behavior in state diagram**: The state transition diagram (lines 44-64) shows basic transitions but doesn't show that HalfOpen can transition back to Open if the half-open request times out after 30s.

### Improvement Opportunities

1. The state diagram should be updated to show the half-open timeout (30s) that triggers Open→HalfOpen→Open cycling.

2. The architecture should document that `is_available()` does TWO things when recovery timeout elapses:
   - Sets state to HalfOpen
   - Records `half_open_start_time = Instant::now()`

3. The `call()` method has a complex half-open timeout check (lines 114-127) that should be explained in the architecture.

4. Consider documenting the test scenarios in `mod tests` (circuit.rs:189-261) as they demonstrate the expected behavior including the half-open timeout.

## Recommendations

1. Update architecture/resilience.md to include the `half_open_start_time` and `max_half_open_duration` fields in the `CircuitBreakerInner` struct.

2. Add the half-open timeout behavior to the state transition diagram.

3. Document the `name()` and `state()` public methods.

4. The skill document (.opencode/skills/resilience/SKILL.md) appears to be more accurate than the architecture doc in some areas - consider syncing them, particularly the `is_available()` code example which matches the implementation.

5. Consider adding a section explaining the half-open timeout mechanism: "When transitioning to HalfOpen, a 30-second timer starts. If the test request doesn't succeed within this window, the circuit transitions back to Open."

(End of file - total 107 lines)
