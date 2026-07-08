//! Eggsact in-process deterministic utility adapter.
//!
//! This module provides a thin adapter layer that maps eggsact's
//! agent API into Codegg's `Tool` and `StructuredToolResult` contracts.
//! Eggsact is consumed as a direct Rust dependency (not MCP) for
//! low-latency, deterministic local preflight tools.

pub mod adapter;

pub use adapter::EggsactRuntime;
