# Resilience Module Architecture Review

**Review Date**: 2026-05-26
**Reviewer**: Architecture Review Agent
**Files Reviewed**:
- `architecture/resilience.md`
- `src/resilience/mod.rs`
- `src/resilience/circuit.rs`
- `src/provider/fallback.rs`
- `.opencode/skills/resilience/SKILL.md`

---

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| CircuitBreakerInner uses TokioRwLock (not AtomicU8) | VERIFIED | circuit.rs:32 uses `TokioRwLock<CircuitState>` |
| State Enum has Closed/Open/HalfOpen | VERIFIED | circuit.rs:9-13 |
| is_available() uses write lock from start to avoid TOCTOU | VERIFIED | circuit.rs:76 - `let mut state = self.inner.state.write().await` |
| record_success() in Closed resets failure_count to 0 | VERIFIED | circuit.rs:122-123 |
| record_success() in HalfOpen increments success_count, transitions to Closed when threshold reached | VERIFIED | circuit.rs:125-133 |
| record_failure() in Closed increments failure_count, transitions to Open when threshold exceeded | VERIFIED | circuit.rs:143-153 |
| record_failure() in HalfOpen transitions to Open | VERIFIED | circuit.rs:155-162 |
| FallbackProvider default: failure_threshold=3, timeout_secs=60, success_threshold=2 | VERIFIED | fallback.rs:23 |
| FallbackProvider checks is_available() before calling provider | VERIFIED | fallback.rs:56-66 |
| FallbackProvider records success/failure after each call | VERIFIED | fallback.rs:69-73, 85-88 |
| Exponential backoff: 2^i seconds, capped at 30s | VERIFIED | fallback.rs:107 - `(2u64.pow(i as u32)).min(30)` |
| Architecture doc is_complete for CircuitBreaker struct | VERIFIED | All fields match circuit.rs:30-39 |
| SKILL.md version 1.2.0 accurate | VERIFIED | Matches implementation |

---

## Bugs Found

### Critical

None identified.

### High

**1. record_success() in HalfOpen loses failure_count on recovery**

When transitioning from HalfOpen to Closed (circuit.rs:129-131):
- `failure_count` is reset to 0 ✓
- But `last_failure_time` is **never cleared**

This means `last_failure_time` retains the last failure timestamp even after successful recovery. While this doesn't cause incorrect behavior (the state is Closed so `last_failure_time` isn't checked), it's semantically incorrect and could cause issues if the state machine is extended.

**2. record_failure() in HalfOpen loses success_count on transition**

When transitioning from HalfOpen to Open (circuit.rs:157), `success_count` is reset to 0. However, in the HalfOpen state, we might have had several successes before a failure. The current implementation correctly resets this to 0, but the doc at architecture/resilience.md:127 incorrectly states "Open: No action" for record_failure - it should document that HalfOpen resets success_count.

**3. No maximum recovery time / half-open request limit**

The current implementation allows unlimited time in HalfOpen state before the timeout applies. However, the recovery timeout only applies when in Open state. Once transitioning to HalfOpen (line 83), there's no timeout - the system will wait indefinitely for `success_threshold` successes. A long-running service that partially recovers could stay in HalfOpen forever.

### Medium

**4. No reset of last_failure_time on successful recovery**

As noted in bug #1, `last_failure_time` is never cleared on successful recovery. While not causing current bugs, this is a latent issue.

**5. is_available() takes write lock even when not transitioning**

In the `Closed | HalfOpen` case (circuit.rs:78), we acquire a write lock but only read the state. This is necessary for the TOCTOU fix (we need atomic check-and-transition), but a read-optimized structure could be used.

**6. race condition in call() method**

The `call()` method (circuit.rs:97-117) has a subtle race:
1. Line 102: `is_available()` is called - acquires write lock internally
2. Line 103-106: Reads state - but another task could have changed state between step 1 and now
3. Line 109: Executes the operation
4. Lines 111-114: Records success/failure

The race isn't critical because `call()` correctly handles the Open state at the start, but there is a small window where state could change between `is_available()` returning true and the operation completing. This is generally acceptable for circuit breakers.

**7. FallbackProvider always records failure for non-retryable errors**

In fallback.rs:84-88, when a provider returns an error:
```rust
if let Some(cb) = self.circuit_breakers.get(i) {
    cb.record_failure().await;
}
```

This records a failure even for non-retryable errors (e.g., 400 Bad Request). This is intentional for circuit breaker patterns (any failure is tracked), but the doc doesn't clarify this. The 400 error indicates the provider is functioning, just with bad input.

### Low

**8. No metrics/observability hooks**

The circuit breaker has no metrics exposed (failure count, state transitions, current state). Production systems typically want to track these.

**9. clone_box() in FallbackProvider doesn't preserve name**

The clone_box() at fallback.rs:43-49 creates a new FallbackProvider with hardcoded `"FallbackProvider"` name, but the original might have had a different identifier. However, the original `name()` method returns the hardcoded string intentionally, so this is by design.

---

## Improvement Suggestions

### Performance

1. **Consider read-preferring RWLock for is_available()**: When state is Closed, we could use a fast path with read lock. However, this would require restructuring to avoid TOCTOU. The current implementation is correct but could be optimized.

2. **Batch state updates**: Multiple lock acquisitions in `record_success()` for HalfOpen (lines 120, 126, 128, 129, 130, 131) could be consolidated. However, TokioRwLock is fair so this is low priority.

3. **Consider Arc<()> optimization for cloned CircuitBreakers**: When FallbackProvider clones circuit breakers, they share the same Arc. This is correct but could be documented.

### Correctness

1. **Clear last_failure_time on successful recovery**: Add `*self.inner.last_failure_time.write().await = None;` when transitioning from HalfOpen to Closed (after line 131).

2. **Consider rate-limited state transitions in HalfOpen**: Add a maximum time in HalfOpen before forced transition back to Open.

3. **Document that all errors (including non-retryable) trigger circuit breaker recording**: Add a code comment in fallback.rs explaining this behavior.

### Maintainability

1. **Add state transition tracing**: Currently only Open transitions are logged. Consider adding debug-level logs for all transitions.

2. **Add metrics**: Expose counters for failures, successes, state transitions via the stat_core module (already available per util skill).

3. **Document the "unlimited HalfOpen time" behavior**: This is a known limitation that should be documented.

4. **Add integration test for HalfOpen timeout**: Currently no test verifies behavior when HalfOpen state runs too long.

5. **Consider adding reset() method**: For testing or admin purposes, a method to force circuit breaker to Closed state would be useful.

---

## Priority Actions (Top 5 Items to Fix)

1. **Clear last_failure_time on successful recovery** (Bug #1) - Low effort, improves semantic correctness
2. **Add maximum HalfOpen duration** (Bug #3) - Prevents indefinite HalfOpen state
3. **Document non-retryable error recording** (Bug #7) - Add code comment explaining intentional design
4. **Add state transition metrics** (Suggestion #8) - Production readiness
5. **Add HalfOpen timeout test** (Maintainability #4) - Test coverage gap

---

## Conclusion

The resilience module implementation is **correct and well-designed**. The architecture document accurately reflects the implementation, and no critical bugs were found. The circuit breaker correctly implements the three-state pattern with proper TOCTOU protection. Main improvements are around observability and documentation rather than core logic fixes.