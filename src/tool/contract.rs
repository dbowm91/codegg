//! Structured tool contracts and typed invocation context.
//!
//! This module defines the metadata that describes *how* a tool may be
//! called, what effects it has, and what it returns. The `ToolBroker`
//! uses these contracts to enforce caller policy, validate inputs,
//! select execution routes, and validate outputs.
//!
//! # Design principles
//!
//! - **Additive and backward-compatible**: legacy tools that do not
//!   supply a contract receive conservative defaults (`DirectOnly`,
//!   `Mutating`, no cache, no retry, string output).
//! - **Additive serialization**: serde derives are available for
//!   protocol/diagnostic forms but internal trait objects stay
//!   internal.
//! - **No lossy conversions**: typed IDs are used throughout; numeric
//!   IDs are not introduced.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Caller policy ────────────────────────────────────────────────

/// Who is permitted to invoke a tool through the broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallerPolicy {
    /// Only the agent loop (direct model-facing calls). Programs
    /// cannot call this tool.
    DirectOnly,
    /// Both agent-loop direct calls and programmatic (Tool Program)
    /// calls are permitted.
    DirectOrProgrammatic,
    /// Only programmatic calls are permitted (rare; used for
    /// runtime-internal helpers exposed to programs).
    ProgrammaticOnly,
}

impl Default for ToolCallerPolicy {
    fn default() -> Self {
        Self::DirectOnly
    }
}

// ─── Effect classification ────────────────────────────────────────

/// Classifies the side-effect nature of a tool for caching,
/// idempotency, and retry decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffectClass {
    /// Pure read; no observable state change. Safe to cache and
    /// retry.
    ReadOnly,
    /// Reads plus deterministic validation; idempotent and
    /// cacheable.
    ReadValidate,
    /// Mutates local workspace state; idempotent when the same
    /// input is applied repeatedly (e.g. `write` with identical
    /// content).
    SafeRepeat,
    /// Mutates state; re-application with the same input is
    /// idempotent only because it overwrites, but intermediate
    /// states differ.
    IdempotentMutating,
    /// Non-idempotent mutation; must not be retried automatically.
    NonIdempotent,
    /// Executes external processes with side effects; retry
    /// classification depends on the specific command.
    ProcessExec,
}

impl Default for ToolEffectClass {
    fn default() -> Self {
        Self::NonIdempotent
    }
}

impl ToolEffectClass {
    /// Whether calls with this effect class may be cached.
    pub fn is_cacheable(self) -> bool {
        matches!(self, Self::ReadOnly | Self::ReadValidate | Self::SafeRepeat)
    }

    /// Whether calls with this effect class are safe to retry
    /// automatically.
    pub fn is_retry_eligible(self) -> bool {
        matches!(
            self,
            Self::ReadOnly | Self::ReadValidate | Self::SafeRepeat | Self::IdempotentMutating
        )
    }
}

// ─── Idempotency class ────────────────────────────────────────────

/// Whether repeated identical submissions should produce the same
/// durable result or create new work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyClass {
    /// Duplicate submission keys return the same result.
    Idempotent,
    /// Each submission creates new work even with the same key.
    NonIdempotent,
}

impl Default for IdempotencyClass {
    fn default() -> Self {
        Self::NonIdempotent
    }
}

// ─── Retry policy ─────────────────────────────────────────────────

/// Controls broker-side retry behavior for a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolRetryPolicy {
    /// Maximum number of broker-side retries (0 = no retry).
    pub max_retries: u8,
    /// Base delay between retries in milliseconds.
    pub base_delay_ms: u64,
    /// Maximum delay cap in milliseconds.
    pub max_delay_ms: u64,
}

impl Default for ToolRetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 0,
            base_delay_ms: 500,
            max_delay_ms: 5_000,
        }
    }
}

impl ToolRetryPolicy {
    /// No retry.
    pub fn none() -> Self {
        Self::default()
    }

    /// Conservative retry with bounded backoff.
    pub fn transient(max_retries: u8) -> Self {
        Self {
            max_retries,
            base_delay_ms: 500,
            max_delay_ms: 5_000,
        }
    }
}

// ─── Cache policy ─────────────────────────────────────────────────

/// Controls broker-side result caching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolCachePolicy {
    /// Whether caching is enabled for this tool.
    pub enabled: bool,
    /// Time-to-live in seconds (0 = no expiry within session).
    pub ttl_secs: u64,
    /// Maximum cached entries per tool.
    pub max_entries: u32,
}

impl Default for ToolCachePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            ttl_secs: 0,
            max_entries: 0,
        }
    }
}

// ─── Projection policy ────────────────────────────────────────────

/// Controls how tool output is projected into the model transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolProjectionPolicy {
    /// Whether the tool's raw output should be placed in the
    /// transcript or kept behind an artifact handle.
    pub inline_in_transcript: bool,
    /// Maximum inline bytes before the output is moved to an
    /// artifact.
    pub max_inline_bytes: usize,
}

impl Default for ToolProjectionPolicy {
    fn default() -> Self {
        Self {
            inline_in_transcript: true,
            max_inline_bytes: 4096,
        }
    }
}

// ─── Tool contract ────────────────────────────────────────────────

/// Describes the runtime policy and metadata for a single tool.
///
/// Contracts are immutable snapshots; they are frozen at submission
/// time for programmatic callers and resolved once at registry
/// construction for direct callers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContract {
    /// Tool name (must match `Tool::name()`).
    pub name: String,
    /// Caller policy.
    pub caller_policy: ToolCallerPolicy,
    /// Effect classification.
    pub effect_class: ToolEffectClass,
    /// Idempotency classification.
    pub idempotency: IdempotencyClass,
    /// Retry policy.
    pub retry_policy: ToolRetryPolicy,
    /// Cache policy.
    pub cache_policy: ToolCachePolicy,
    /// Projection policy.
    pub projection_policy: ToolProjectionPolicy,
    /// Implementation identifier (e.g. "codegg/read", "eggsearch").
    pub implementation_id: String,
    /// Implementation version string (e.g. crate version).
    pub implementation_version: String,
    /// Input schema as JSON Schema (from `Tool::parameters()`).
    pub input_schema: serde_json::Value,
    /// Optional output schema as JSON Schema. `None` for legacy
    /// tools that only produce string output.
    pub output_schema: Option<serde_json::Value>,
}

impl ToolContract {
    /// Build a conservative default contract for a legacy tool that
    /// does not supply explicit metadata. This is the backward-
    /// compatibility path: the tool is direct-only, non-cacheable,
    /// non-retryable, and treated as non-idempotent.
    pub fn legacy(tool_name: &str, input_schema: serde_json::Value) -> Self {
        Self {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::default(),
            effect_class: ToolEffectClass::default(),
            idempotency: IdempotencyClass::default(),
            retry_policy: ToolRetryPolicy::default(),
            cache_policy: ToolCachePolicy::default(),
            projection_policy: ToolProjectionPolicy::default(),
            implementation_id: format!("codegg/{}", tool_name),
            implementation_version: env!("CARGO_PKG_VERSION").to_string(),
            input_schema,
            output_schema: None,
        }
    }

    /// Validate internal consistency of the contract.
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        if self.name.is_empty() {
            return Err(ContractValidationError::EmptyName);
        }
        if self.implementation_id.is_empty() {
            return Err(ContractValidationError::EmptyImplementationId);
        }
        // Retry-eligible tools must have a non-zero retry policy when
        // retry is actually enabled.
        if self.retry_policy.max_retries > 0 && !self.effect_class.is_retry_eligible() {
            return Err(ContractValidationError::RetryOnNonRetryableEffect);
        }
        // Cacheable tools must have cache enabled.
        if self.effect_class.is_cacheable() && !self.cache_policy.enabled {
            // This is a warning-level inconsistency, not a hard error.
            // Programs may want to cache read-only tools even when the
            // legacy default says no.
        }
        Ok(())
    }
}

/// Errors from contract validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractValidationError {
    EmptyName,
    EmptyImplementationId,
    RetryOnNonRetryableEffect,
}

impl std::fmt::Display for ContractValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => write!(f, "tool contract name must not be empty"),
            Self::EmptyImplementationId => {
                write!(f, "tool contract implementation_id must not be empty")
            }
            Self::RetryOnNonRetryableEffect => {
                write!(f, "retry Policy set on non-retryable effect class")
            }
        }
    }
}

impl std::error::Error for ContractValidationError {}

// ─── Tool caller lineage ──────────────────────────────────────────

/// Identifies who initiated a tool call.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCaller {
    /// The agent loop making a direct model-facing tool call.
    Agent,
    /// A Tool Program executing a nested call.
    Program { program_id: String },
    /// A subagent making a tool call on behalf of a parent agent.
    Subagent { parent_agent_id: String },
    /// An ACP/API caller.
    Api { client_id: String },
    /// Internal diagnostic/test path.
    Internal,
}

// ─── Typed terminal status ────────────────────────────────────────

/// Terminal status of a tool invocation, used by the broker and
/// programs to classify outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTerminalStatus {
    /// Tool completed successfully.
    Success,
    /// Tool returned a structured error (e.g. validation failure,
    /// not-found).
    Error,
    /// Tool was denied by caller policy, permission, or authority.
    Denied,
    /// Tool execution was cancelled.
    Cancelled,
    /// Tool timed out.
    TimedOut,
    /// Tool execution failed due to infrastructure (storage,
    //  network, process).
    InfrastructureError,
}

// ─── Typed tool value ─────────────────────────────────────────────

/// A typed tool result that carries display output, optional
/// structured value, artifacts, provenance, and terminal status.
///
/// This supersedes `StructuredToolResult` for new code paths. The
/// broker returns `ToolValue`; legacy tools are adapted through
/// `ToolValue::from_legacy()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolValue {
    /// Human-readable display output (always present).
    pub display: String,
    /// Optional structured JSON value for programmatic consumers.
    pub value: Option<serde_json::Value>,
    /// Artifact handles for large bodies (file contents, diffs, etc).
    pub artifacts: Vec<ToolArtifactHandle>,
    /// Provenance metadata.
    pub provenance: Option<super::backend::ToolProvenance>,
    /// Terminal status.
    pub terminal_status: ToolTerminalStatus,
    /// Whether the output was truncated.
    pub truncated: bool,
}

impl ToolValue {
    /// Build from a legacy `StructuredToolResult`.
    pub fn from_legacy(result: super::backend::StructuredToolResult) -> Self {
        let terminal_status = if result.success {
            ToolTerminalStatus::Success
        } else {
            ToolTerminalStatus::Error
        };
        Self {
            display: result.output,
            value: None,
            artifacts: Vec::new(),
            provenance: result.provenance,
            terminal_status,
            truncated: false,
        }
    }

    /// Build a successful value with display text.
    pub fn success(display: String) -> Self {
        Self {
            display,
            value: None,
            artifacts: Vec::new(),
            provenance: None,
            terminal_status: ToolTerminalStatus::Success,
            truncated: false,
        }
    }

    /// Build a denied value.
    pub fn denied(reason: String) -> Self {
        Self {
            display: reason,
            value: None,
            artifacts: Vec::new(),
            provenance: None,
            terminal_status: ToolTerminalStatus::Denied,
            truncated: false,
        }
    }

    /// Build a cancelled value.
    pub fn cancelled() -> Self {
        Self {
            display: "Tool execution cancelled".to_string(),
            value: None,
            artifacts: Vec::new(),
            provenance: None,
            terminal_status: ToolTerminalStatus::Cancelled,
            truncated: false,
        }
    }

    /// Build a timed-out value.
    pub fn timed_out() -> Self {
        Self {
            display: "Tool execution timed out".to_string(),
            value: None,
            artifacts: Vec::new(),
            provenance: None,
            terminal_status: ToolTerminalStatus::TimedOut,
            truncated: false,
        }
    }

    /// Build an infrastructure error value.
    pub fn infrastructure_error(reason: String) -> Self {
        Self {
            display: reason,
            value: None,
            artifacts: Vec::new(),
            provenance: None,
            terminal_status: ToolTerminalStatus::InfrastructureError,
            truncated: false,
        }
    }

    /// Convert to legacy string output for backward compatibility.
    pub fn into_legacy_output(self) -> String {
        self.display
    }
}

// ─── Artifact handles ─────────────────────────────────────────────

/// A reference to a large tool output body stored in an artifact
/// service. Programs and direct callers use these handles to expand
/// content on demand rather than embedding unbounded data in
/// transcripts or job payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolArtifactHandle {
    /// Unique artifact identifier.
    pub artifact_id: String,
    /// The tool that produced this artifact.
    pub tool_name: String,
    /// MIME type (e.g. "text/plain", "application/json").
    pub content_type: String,
    /// Byte length of the stored content.
    pub byte_length: u64,
    /// Optional content digest for integrity verification.
    pub digest: Option<String>,
}

// ─── Tool contract catalog ────────────────────────────────────────

/// A snapshot of all tool contracts, built once at registry
/// construction and used by the broker for lookup.
#[derive(Debug, Clone, Default)]
pub struct ToolContractCatalog {
    contracts: HashMap<String, ToolContract>,
}

impl ToolContractCatalog {
    /// Build from an iterator of contracts.
    pub fn new(contracts: impl IntoIterator<Item = ToolContract>) -> Self {
        let map: HashMap<String, ToolContract> =
            contracts.into_iter().map(|c| (c.name.clone(), c)).collect();
        Self { contracts: map }
    }

    /// Look up a contract by tool name.
    pub fn get(&self, name: &str) -> Option<&ToolContract> {
        self.contracts.get(name)
    }

    /// Whether a tool has a registered contract.
    pub fn contains(&self, name: &str) -> bool {
        self.contracts.contains_key(name)
    }

    /// All registered tool names.
    pub fn tool_names(&self) -> impl Iterator<Item = &str> {
        self.contracts.keys().map(|s| s.as_str())
    }

    /// Number of registered contracts.
    pub fn len(&self) -> usize {
        self.contracts.len()
    }

    /// Whether the catalog is empty.
    pub fn is_empty(&self) -> bool {
        self.contracts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_contract_defaults() {
        let c = ToolContract::legacy("read", serde_json::json!({"type": "object"}));
        assert_eq!(c.name, "read");
        assert_eq!(c.caller_policy, ToolCallerPolicy::DirectOnly);
        assert_eq!(c.effect_class, ToolEffectClass::NonIdempotent);
        assert_eq!(c.idempotency, IdempotencyClass::NonIdempotent);
        assert!(!c.cache_policy.enabled);
        assert_eq!(c.retry_policy.max_retries, 0);
        assert!(c.output_schema.is_none());
    }

    #[test]
    fn contract_validate_ok() {
        let c = ToolContract::legacy("grep", serde_json::json!({}));
        assert!(c.validate().is_ok());
    }

    #[test]
    fn contract_validate_rejects_empty_name() {
        let mut c = ToolContract::legacy("x", serde_json::json!({}));
        c.name = String::new();
        assert_eq!(c.validate(), Err(ContractValidationError::EmptyName));
    }

    #[test]
    fn effect_class_cacheability() {
        assert!(ToolEffectClass::ReadOnly.is_cacheable());
        assert!(ToolEffectClass::ReadValidate.is_cacheable());
        assert!(ToolEffectClass::SafeRepeat.is_cacheable());
        assert!(!ToolEffectClass::IdempotentMutating.is_cacheable());
        assert!(!ToolEffectClass::NonIdempotent.is_cacheable());
        assert!(!ToolEffectClass::ProcessExec.is_cacheable());
    }

    #[test]
    fn effect_class_retry_eligibility() {
        assert!(ToolEffectClass::ReadOnly.is_retry_eligible());
        assert!(ToolEffectClass::SafeRepeat.is_retry_eligible());
        assert!(!ToolEffectClass::NonIdempotent.is_retry_eligible());
    }

    #[test]
    fn tool_value_from_legacy_success() {
        let sr = super::super::backend::StructuredToolResult::legacy("test", "ok".into());
        let tv = ToolValue::from_legacy(sr);
        assert_eq!(tv.terminal_status, ToolTerminalStatus::Success);
        assert_eq!(tv.display, "ok");
    }

    #[test]
    fn tool_value_denied() {
        let tv = ToolValue::denied("no permission".into());
        assert_eq!(tv.terminal_status, ToolTerminalStatus::Denied);
    }

    #[test]
    fn catalog_lookup() {
        let c1 = ToolContract::legacy("read", serde_json::json!({}));
        let c2 = ToolContract::legacy("write", serde_json::json!({}));
        let cat = ToolContractCatalog::new(vec![c1, c2]);
        assert!(cat.contains("read"));
        assert!(cat.contains("write"));
        assert!(!cat.contains("bash"));
        assert_eq!(cat.len(), 2);
    }

    #[test]
    fn retry_policy_defaults() {
        let p = ToolRetryPolicy::default();
        assert_eq!(p.max_retries, 0);
    }

    #[test]
    fn retry_policy_transient() {
        let p = ToolRetryPolicy::transient(3);
        assert_eq!(p.max_retries, 3);
        assert!(p.base_delay_ms > 0);
    }

    #[test]
    fn projection_policy_defaults() {
        let p = ToolProjectionPolicy::default();
        assert!(p.inline_in_transcript);
        assert!(p.max_inline_bytes > 0);
    }

    #[test]
    fn caller_policy_serialization_roundtrip() {
        let policies = [
            ToolCallerPolicy::DirectOnly,
            ToolCallerPolicy::DirectOrProgrammatic,
            ToolCallerPolicy::ProgrammaticOnly,
        ];
        for p in policies {
            let json = serde_json::to_string(&p).unwrap();
            let back: ToolCallerPolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(p, back);
        }
    }

    #[test]
    fn contract_serialization_roundtrip() {
        let c = ToolContract::legacy("grep", serde_json::json!({"type": "object"}));
        let json = serde_json::to_string(&c).unwrap();
        let back: ToolContract = serde_json::from_str(&json).unwrap();
        assert_eq!(c.name, back.name);
        assert_eq!(c.caller_policy, back.caller_policy);
    }
}
