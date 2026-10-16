"""Deterministic retry delay policy for the M008 fixture."""


def retry_delay_ms(attempt, base_ms, cap_ms, retry_after_ms=None):
    """Return capped exponential delay raised by an optional server hint."""
    raise NotImplementedError("implement the contract from the fixture README")
