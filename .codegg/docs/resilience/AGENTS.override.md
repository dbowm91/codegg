# Resilience Module Override

This file contains resilience-specific guidance and overrides root AGENTS.md.

## CircuitBreaker API (Updated 2026-05-02)

The `CircuitBreaker` in `src/resilience/circuit.rs` has the following public API:

### Public Methods
- `new(name, failure_threshold, timeout_secs, success_threshold)` - Create a new circuit breaker
- `name()` - Get the circuit breaker name
- `state()` - Get current state (Closed, Open, HalfOpen)
- `is_available()` - Check if circuit allows calls (considers Open state timeout)
- `call(op)` - Execute an operation through the circuit breaker
- `record_success()` - Record a successful call (public as of 2026-05-02)
- `record_failure()` - Record a failed call (public as of 2026-05-02)

### Integration with FallbackProvider
- `FallbackProvider` now creates a `CircuitBreaker` for each provider
- Before calling a provider, checks if its circuit is available
- Records successes/failures to update circuit state
- Prevents cascading failures when providers are down