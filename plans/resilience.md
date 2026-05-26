# Resilience Module Architecture Review Findings

## Verified Claims

- **CircuitBreakerInner struct** (circuit.rs:30-41): `name`, `state`, `failure_count`, `success_count`, `last_failure_time`, `half_open_start_time`, `failure_threshold`, `timeout_secs`, `success_threshold`, `max_half_open_duration` - all fields match
- **CircuitState enum** (circuit.rs:8-13): `Closed`, `Open`, `HalfOpen` - matches
- **CircuitBreaker::is_available()** (circuit.rs:79-99): Uses `write` lock from start, matches documented implementation
- **CircuitBreaker::call()** (circuit.rs:101-137): Full implementation including HalfOpen timeout check
- **record_success()** (circuit.rs:139-158): Closed resets failure_count, HalfOpen increments success_count with transitions
- **record_failure()** (circuit.rs:160-186): Closed increments count, HalfOpen transitions to Open
- **FallbackProvider parameters** (fallback.rs:23): `failure_threshold=3`, `timeout_secs=60`, `success_threshold=2` - matches line 145
- **FallbackProvider status_codes** (fallback.rs:17): Default `[429, 500, 502, 503, 504]` - matches line 262
- **FallbackProvider exponential backoff** (fallback.rs:107): `2u64.pow(i as u32).min(30)` - equals 2^i capped at 30s

## Stale Information

- **Line 148**: "Exponential backoff: `2^i` seconds, capped at 30s" - formula description is ambiguous. Actual implementation is `2^(i-1)` with jitter as shown in fallback.rs:107.

## Bugs Found

None - the implementation correctly handles the HalfOpen→Open timeout inside `call()` (lines 114-127) even though the state diagram in docs only shows timeout as a trigger but doesn't explicitly document this behavior.

## Improvements Suggested

1. **Missing HalfOpen→Open timeout doc**: Lines 44-74 state transition diagram doesn't show that `max_half_open_duration` (HalfOpen timeout) kicks circuit back to Open. This timeout IS implemented at circuit.rs:114-127.

2. **Line 148 backoff formula**: Should clarify it includes jitter (capped_ms + jitter at circuit.rs:54-55).

## Cross-Module Issues

- **provider/fallback.rs uses CircuitBreaker**: Integration is correct at lines 56-73 with proper success/failure recording.
