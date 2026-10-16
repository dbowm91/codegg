# M008 Retry Hint Cross-Language Fixture

This frozen task fixture is used by the isolated S0–S3 live comparison. It asks agents to implement a deterministic capped exponential retry delay with an optional server-provided lower bound, matching behavior in Rust and Python.

## Contract

Implement `retry_delay_ms(attempt, base_ms, cap_ms, retry_after_ms=None)` in both packages. `attempt` is one-based; zero is invalid. `base_ms`, `cap_ms`, and a present `retry_after_ms` are nonnegative integers. Compute `base_ms * 2**(attempt - 1)` with saturation at `cap_ms`; the server retry hint is a lower bound, then cap the result at `cap_ms`. Thus the result is `min(max(min(base * 2**(attempt - 1), cap), retry_after_or_zero), cap)`. `cap_ms < base_ms` is valid and returns no value above the cap. `cap_ms == 0` always returns zero for valid input. The behavior is deterministic and uses no randomness or wall clock.

Rust returns `Result<u64, RetryDelayError>` and rejects attempt zero. Python returns `int` and raises `ValueError` for invalid input. Python must reject booleans despite `bool` being an `int` subclass.

Do not modify tests. Keep changes to the corresponding package's source files. Acceptance commands:

```sh
cargo test --manifest-path rust/Cargo.toml --offline
PYTHONPATH=python python3 -m unittest discover -s python/tests -v
```
