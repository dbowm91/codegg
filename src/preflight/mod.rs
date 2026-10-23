//! Harness-side eggsact preflight integration.
//!
//! This module is harness-internal only — preflight calls do not appear as
//! model-facing tool calls.

pub mod service;

pub use service::{
    PreflightDecision, PreflightFinding, PreflightLocation, PreflightMode, PreflightPolicy,
    PreflightService, PreflightSeverity,
};
