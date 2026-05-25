# Resilience Module Architecture Review

**Review Date**: 2026-05-27
**Module**: `src/resilience/` and `src/provider/fallback.rs`

## Verified Correct Items

1. **CircuitBreakerInner struct** (lines 16-28): All fields match actual implementation at `circuit.rs:30-41`. Uses `TokioRwLock`, not `AtomicU8`.

2. **CircuitState enum** (lines 36-42): Correctly documents `Closed`, `Open`, `HalfOpen` at `circuit.rs:8-13`.

3. **is_available() TOCTOU fix** (lines 68-93): Correctly documented. Implementation at `circuit.rs:79-99` acquires write lock from start (`let mut state = self.inner.state.write().await`), avoiding time-of-check-time-of-use race.

4. **record_success() behavior** (lines 121-126): Correctly documented. Implementation at `circuit.rs:139-158`:
   - Closed: resets failure_count to 0
   - HalfOpen: increments success_count, transitions to Closed when threshold reached

5. **record_failure() behavior** (lines 128-131): Correctly documented. Implementation at `circuit.rs:160-186`:
   - Closed: increments failure_count, transitions to Open when threshold exceeded
   - HalfOpen: transitions to Open immediately

6. **FallbackProvider default parameters** (line 136): Correctly documented as `failure_threshold=3`, `timeout_secs=60`, `success_threshold=2` at `fallback.rs:23`.

7. **Exponential backoff formula** (line 139): Correctly documented as `2^i` seconds, capped at 30s. Implementation at `fallback.rs:107` uses `(2u64.pow(i as u32)).min(30)`.

8. **CircuitBreaker::call() half-open timeout check** (lines 102-127): Architecture doc shows `call()` but implementation at `circuit.rs:101-137` includes critical half-open timeout check (lines 114-127) not shown in diagram. The `max_half_open_duration` (30s) check transitions back to Open if recovery test takes too long.

9. **half_open_start_time field**: Documented at line 23 as field in CircuitBreakerInner. Present at `circuit.rs:36`.

## Incorrect/Stale Items

### 1. HalfOpen timeout check missing from diagram and description

**Location**: Architecture doc lines 44-66 (state transition diagram)

**Issue**: The diagram only shows three transitions but omits the half-open timeout (`max_half_open_duration=30s`) that can transition HalfOpen→Open without a failure.

**Actual behavior** (circuit.rs:114-127):
```
if let CircuitState::HalfOpen = *self.inner.state.read().await {
    if let Some(start_time) = *self.inner.half_open_start_time.read().await {
        if start_time.elapsed() >= self.inner.max_half_open_duration {
            *self.inner.state.write().await = CircuitState::Open;
            // ...
            return Err(CircuitError::Open(self.inner.name.clone()).into());
        }
    }
}
```

**Fix needed**: Add transition "HalfOpen→Open (recovery timeout elapsed)" to diagram at line 66.

### 2. is_available() sets half_open_start_time but doc doesn't mention it

**Location**: Architecture doc line 68-93

**Issue**: The code snippet shows the HalfOpen transition but doesn't capture that `is_available()` also sets `half_open_start_time`:

```rust
// circuit.rs:88
*self.inner.half_open_start_time.write().await = Some(Instant::now());
```

**Fix needed**: Add note at line 86-87 that `half_open_start_time` is set when transitioning to HalfOpen.

### 3. Skill file has incorrect note about is_available() snippet

**Location**: `.opencode/skills/resilience/SKILL.md:34-52`

**Issue**: The skill shows a code snippet that doesn't include the `half_open_start_time` assignment (line 88 in circuit.rs). While the architecture doc snippet is accurate, the skill snippet is slightly outdated.

**Fix needed**: Update SKILL.md to show the complete `is_available()` including the `half_open_start_time` assignment.

## Summary

| Item | Status |
|------|--------|
| CircuitBreakerInner fields | ✅ Correct |
| CircuitState enum | ✅ Correct |
| is_available() TOCTOU fix | ✅ Correct |
| record_success()/record_failure() | ✅ Correct |
| FallbackProvider defaults | ✅ Correct |
| Exponential backoff formula | ✅ Correct |
| HalfOpen timeout check in call() | ✅ Correct (but missing from transition diagram) |
| State transition diagram | ⚠️ Missing HalfOpen→Open via timeout |

**No bugs found** in the resilience module or FallbackProvider. The architecture document is largely accurate with only the transition diagram needing one addition.