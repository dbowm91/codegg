//! Harness-side eggsact preflight integration.
//!
//! Provides automatic validation before mutating operations (edits, config writes,
//! shell commands) using eggsact's deterministic tool substrate. Findings are
//! severity-classified and can block, warn, or annotate depending on policy.
//!
//! This module is harness-internal only — preflight calls do not appear as
//! model-facing tool calls.

pub mod service;

pub use service::{
    PreflightDecision, PreflightFinding, PreflightLocation, PreflightMode, PreflightPolicy,
    PreflightService, PreflightSeverity,
};
