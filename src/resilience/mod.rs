//! Resilience module with circuit breaker pattern.
//!
//! Provides fault tolerance mechanisms including circuit breakers to prevent
//! cascade failures when upstream services are unavailable.

pub mod circuit;

pub use circuit::CircuitBreaker;
