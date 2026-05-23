# Resilience Module Architecture Review

**Review Date**: 2026-05-26
**Reviewer**: Architecture Review Agent
**Files Reviewed**:
- `architecture/resilience.md`
- `src/resilience/mod.rs`
- `src/resilience/circuit.rs`
- `src/provider/fallback.rs`

---

## Verified Claims

### CircuitBreakerInner Struct
| Field | Architecture Doc | Implementation | Status |
|-------|------------------|----------------|--------|
| `name: String` | ✓ | circuit.rs:31 | VERIFIED |
| `state: TokioRwLock<CircuitState>` | ✓ | circuit.rs:32 | VERIFIED |
| `failure_count: TokioRwLock<usize>` | ✓ | circuit.rs:33 | VERIFIED |
| `success_count: TokioRwLock<usize>` | ✓ | circuit.rs:34 | VERIFIED |
| `last_failure_time: TokioRwLock<Option<Instant>>` | ✓ | circuit.rs:35 | VERIFIED |
| `failure_threshold: usize` | ✓ | circuit.rs:36 | VERIFIED |
| `timeout_secs: u64` | ✓ | circuit.rs:37 | VERIFIED |
| `success_threshold: usize` | ✓ | circuit.rs:38 | VERIFIED |

### CircuitState Enum
- `Closed`, `Open`, `HalfOpen` variants - VERIFIED (circuit.rs:9-13)

### is_available() Implementation
- Uses write lock from start to avoid TOCTOU - VERIFIED (circuit.rs:76)
- Closed/HalfOpen returns true - VERIFIED (circuit.rs:78)
- Open checks last_failure_time and transitions to HalfOpen when timeout elapsed - VERIFIED (circuit.rs:79-90)

### call() Method
- Exact signature matches - VERIFIED (circuit.rs:97-100)
- Returns `CircuitError::Open` when circuit is open - VERIFIED (circuit.rs:104-105)
- Records success/failure based on result - VERIFIED (circuit.rs:111-114)

### record_success() Behavior
- Closed: Resets failure_count to 0 - VERIFIED (circuit.rs:122-123)
- HalfOpen: Increments success_count; transitions to Closed when threshold reached - VERIFIED (circuit.rs:125-133)
- Open: No action - VERIFIED (circuit.rs:135)

### record_failure() Behavior
- Closed: Increments failure_count; transitions to Open when threshold exceeded - VERIFIED (circuit.rs:143-153)
- HalfOpen: Transitions to Open, resets success_count - VERIFIED (circuit.rs:155-162)
- Open: No action - VERIFIED (circuit.rs:163)

### FallbackProvider Integration
- Default parameters: `failure_threshold=3, timeout_secs=60, success_threshold=2` - VERIFIED (fallback.rs:23)
- Checks `is_available()` before calling provider - VERIFIED (fallback.rs:56-66)
- Records success/failure after each call - VERIFIED (fallback.rs:71-73, 85-88)
- Exponential backoff: `2^i` seconds, capped at 30s - VERIFIED (fallback.rs:107)

### CircuitError Integration
- `CircuitError::Open(String)` exists - VERIFIED (circuit.rs:16-18)
- `From<CircuitError> for ProviderError` exists - VERIFIED (error.rs:205-209)
- `ProviderError::CircuitOpen(String)` exists - VERIFIED (error.rs:138)
- `CircuitOpen` is marked as retryable - VERIFIED (error.rs:168)

---

## Bugs/Discrepancies Found

### Medium Priority

**1. last_failure_time never cleared on successful recovery**

When transitioning from HalfOpen to Closed (circuit.rs:129-132), `failure_count` is reset to 0, but `last_failure_time` is never cleared. While this doesn't cause incorrect behavior (state is Closed so the field isn't checked), it's a latent semantic issue.

**2. No maximum HalfOpen duration**

Once in HalfOpen state (circuit.rs:83), there is no timeout. The recovery timeout only applies when in Open state. A partially recovering service could remain in HalfOpen indefinitely if `success_threshold` successes never arrive. This is a known limitation but not documented.

**3. Architecture doc references non-existent skill file**

The architecture doc (line 142) references `.opencode/skills/resilience/SKILL.md` which does not exist.

### Low Priority

**4. All errors trigger circuit breaker recording, including non-retryable**

In fallback.rs:85-88, `record_failure()` is called for all errors, including non-retryable ones like 400 Bad Request. This is intentional design (circuit breaker tracks all failures), but could be surprising behavior not documented.

**5. No state transition metrics/observability**

The circuit breaker exposes no metrics for monitoring (failure count, state transitions, current state). This is a maintainability gap for production deployments.

---

## Improvement Suggestions

### High Priority

1. **Add maximum HalfOpen duration timeout**
   - Once transitioned to HalfOpen, track when transition occurred
   - Force transition back to Open if recovery not achieved within timeout
   - This prevents indefinite HalfOpen state

2. **Clear last_failure_time on successful recovery**
   - Add `*self.inner.last_failure_time.write().await = None;` after line 131
   - Improves semantic correctness

### Medium Priority

3. **Remove or fix reference to non-existent skill file**
   - Line 142 of architecture/resilience.md references `.opencode/skills/resilience/SKILL.md` which doesn't exist

4. **Add code comment for non-retryable error handling**
   - In fallback.rs around line 85, add comment explaining that all errors (including non-retryable) trigger circuit breaker recording

### Low Priority

5. **Add state transition metrics**
   - Expose counters for failures, successes, state transitions
   - Use existing stat_core infrastructure (per util skill)

6. **Consider add_reset() method**
   - For testing or admin purposes, force circuit breaker to Closed state

7. **Add integration test for HalfOpen timeout**
   - Currently no test verifies behavior when HalfOpen state runs too long

---

## Priority Actions (Top 5)

1. Clear `last_failure_time` on successful recovery (circuit.rs) - Low effort, improves correctness
2. Add maximum HalfOpen duration timeout - Prevents indefinite HalfOpen state
3. Remove non-existent skill file reference from architecture doc (line 142)
4. Add code comment explaining non-retryable error recording behavior
5. Add state transition metrics for observability

---

## Conclusion

The resilience module implementation is **correct and well-designed**. The architecture document accurately reflects all implementation details. The circuit breaker correctly implements the three-state pattern with proper TOCTOU protection via write lock from the start. Core logic is sound; improvements are primarily around observability and minor cleanup.