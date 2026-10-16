//! Deterministic retry delay policy shared semantically with the Python fixture.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDelayError {
    AttemptZero,
}

/// Return the capped exponential delay, raised to a server retry hint if present.
pub fn retry_delay_ms(
    _attempt: u32,
    _base_ms: u64,
    _cap_ms: u64,
    _retry_after_ms: Option<u64>,
) -> Result<u64, RetryDelayError> {
    todo!("implement the contract from the fixture README")
}
