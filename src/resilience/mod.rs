//! Resilience module with circuit breaker pattern.
//!
//! Provides fault tolerance mechanisms including circuit breakers to prevent
//! cascade failures when upstream services are unavailable.

pub use codegg_providers::circuit::{CircuitBreaker, CircuitError, CircuitState};
