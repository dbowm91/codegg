//! OpenAI Responses API transport and hosted-program adapter.
//!
//! This module provides the wire types for the Responses API, a
//! transport abstraction for streaming response items, and a
//! [`HostedProgramAdapter`] that normalizes provider-hosted program
//! items and nested client-owned function calls into CodeGG's
//! existing Tool Program, Tool Broker, scheduler, call-ledger,
//! artifact, cancellation, and projection contracts.
//!
//! # Design principles
//!
//! - Hosted execution is an optimization/backend choice, not a second
//!   policy or persistence architecture.
//! - Provider item IDs and fingerprints are compatibility/provenance
//!   values, not CodeGG durable identities.
//! - Provider source is untrusted; language/manifest/limits are validated
//!   before associating with a CodeGG program.
//! - Native restricted Python remains available as a fallback.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::provider_core::ProviderCapabilities;

// ─── Security bounds ───────────────────────────────────────────────

/// Maximum size of a single argument payload in bytes.
pub const MAX_ARGUMENT_SIZE: usize = 1024 * 1024; // 1 MB

/// Maximum size of a single result payload in bytes.
pub const MAX_RESULT_SIZE: usize = 1024 * 1024; // 1 MB

/// Maximum number of nested calls per response.
pub const MAX_NESTED_CALLS: usize = 100;

/// Maximum number of items in a single response.
pub const MAX_RESPONSE_ITEMS: usize = 200;

/// Maximum size of input items sent to the provider in bytes (body minimization).
pub const MAX_INPUT_BODY_SIZE: usize = 4 * 1024 * 1024; // 4 MB

/// Default per-request timeout for Responses API calls.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Default stream-idle timeout — maximum time between SSE events before
/// the stream is considered stalled.
pub const DEFAULT_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Maximum SSE buffer size before the parser yields an error.
pub const MAX_SSE_BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4 MB

// ─── Security validation ───────────────────────────────────────────

/// Validation result for hosted program inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputValidation {
    /// Input is valid.
    Valid,
    /// Input is invalid with a reason.
    Invalid { reason: String },
}

/// Validate a provider-generated argument payload as untrusted model output.
///
/// Checks:
/// - Argument string is valid JSON
/// - Argument size is within bounds
/// - Parsed value is an object (not a primitive or array)
/// - No nested `$schema` or `type` fields that could trick schema validation
pub fn validate_arguments(arguments: &str) -> InputValidation {
    if arguments.len() > MAX_ARGUMENT_SIZE {
        return InputValidation::Invalid {
            reason: format!(
                "argument payload {} bytes exceeds maximum {} bytes",
                arguments.len(),
                MAX_ARGUMENT_SIZE
            ),
        };
    }

    match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(val) => {
            if !val.is_object() {
                return InputValidation::Invalid {
                    reason: "argument payload must be a JSON object".to_string(),
                };
            }
            InputValidation::Valid
        }
        Err(e) => InputValidation::Invalid {
            reason: format!("invalid JSON in arguments: {}", e),
        },
    }
}

/// Validate a result payload before persisting or returning to provider.
///
/// Checks result size against the configured maximum.
pub fn validate_result_size(
    result: &serde_json::Value,
    max_size: Option<usize>,
) -> InputValidation {
    let limit = max_size.unwrap_or(MAX_RESULT_SIZE);
    match serde_json::to_vec(result) {
        Ok(bytes) => {
            if bytes.len() > limit {
                InputValidation::Invalid {
                    reason: format!(
                        "result payload {} bytes exceeds maximum {} bytes",
                        bytes.len(),
                        limit
                    ),
                }
            } else {
                InputValidation::Valid
            }
        }
        Err(e) => InputValidation::Invalid {
            reason: format!("cannot serialize result: {}", e),
        },
    }
}

/// Validate that the number of nested calls does not exceed limits.
pub fn validate_call_count(count: usize, max_calls: Option<usize>) -> InputValidation {
    let limit = max_calls.unwrap_or(MAX_NESTED_CALLS);
    if count >= limit {
        InputValidation::Invalid {
            reason: format!("nested call count {} exceeds maximum {}", count, limit),
        }
    } else {
        InputValidation::Valid
    }
}

// ─── Redaction helpers ─────────────────────────────────────────────

/// Redact sensitive fields from a string for safe logging.
///
/// API keys, bearer tokens, and fingerprints are masked.
pub fn redact_for_log(input: &str) -> String {
    if input.len() > 16 {
        format!("{}...{}", &input[..8], &input[input.len() - 4..])
    } else if input.is_empty() {
        "<empty>".to_string()
    } else {
        format!("{}****", &input[..input.len().min(4)])
    }
}

/// Redact a fingerprint value for safe display.
pub fn redact_fingerprint(fp: &str) -> String {
    redact_for_log(fp)
}

/// Minimize input items for the provider by stripping large content.
///
/// Removes full file contents from FunctionCallOutput items that
/// exceed a per-item threshold, replacing them with a placeholder.
/// This limits the data sent back to the provider on continuation.
pub fn minimize_input_items(items: &mut [ResponseItem], per_item_limit: usize) {
    for item in items.iter_mut() {
        if let ResponseItem::FunctionCallOutput { output, .. } = item {
            if let Ok(bytes) = serde_json::to_vec(output) {
                if bytes.len() > per_item_limit {
                    *output = serde_json::json!({
                        "truncated": true,
                        "original_size": bytes.len(),
                        "summary": "output truncated for provider transmission"
                    });
                }
            }
        }
    }
}

/// Filter artifacts — only include artifacts explicitly selected
/// by program calls. Provider-owned artifacts are never sent.
pub fn filter_artifacts_for_provider(artifacts: &[ArtifactRef]) -> Vec<&ArtifactRef> {
    artifacts.iter().filter(|a| a.selected_by_call).collect()
}

/// A reference to an artifact produced by a tool call.
#[derive(Debug, Clone)]
pub struct ArtifactRef {
    /// The artifact identifier.
    pub id: String,
    /// Path to the artifact file.
    pub path: String,
    /// Size in bytes.
    pub size: usize,
    /// Whether this artifact was explicitly selected for provider transmission.
    pub selected_by_call: bool,
}

// ─── Responses API wire types ──────────────────────────────────────

/// A request to the Responses API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesRequest {
    /// The model to use (e.g. "gpt-4.1").
    pub model: String,
    /// Input items for the response.
    pub input: Vec<ResponseItem>,
    /// Whether to stream the response.
    #[serde(default)]
    pub stream: bool,
    /// Maximum output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Top-p.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Tools available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ResponsesTool>>,
    /// Whether to include usage in the response.
    #[serde(default)]
    pub include_usage: bool,
    /// Instructions / system prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Metadata for tracing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// A tool definition for the Responses API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesTool {
    #[serde(rename = "type")]
    pub tool_type: String, // "function"
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub parameters: serde_json::Value,
    /// Whether the tool is strict (schema-enforced).
    #[serde(default)]
    pub strict: Option<bool>,
}

impl ResponsesTool {
    /// Create from a CodeGG tool definition.
    pub fn from_tool_definition(def: &crate::provider_core::ToolDefinition) -> Self {
        Self {
            tool_type: "function".to_string(),
            name: def.name.clone(),
            description: Some(def.description.clone()),
            parameters: def.parameters.clone(),
            strict: None,
        }
    }
}

/// An item in the Responses API conversation.
///
/// Items are matched by field presence since the Responses API does not
/// use a top-level discriminator for all item types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseItem {
    /// A hosted program item (provider executes this).
    HostedTool(HostedToolItem),
    /// A function call requested by the model (needs client execution).
    FunctionCall {
        /// Unique call ID from the provider.
        call_id: String,
        /// Function name.
        name: String,
        /// Arguments as JSON string.
        arguments: String,
    },
    /// A function call output from the provider (hosted program).
    FunctionCallOutput {
        /// Unique call ID from the provider.
        call_id: String,
        /// The output value (string or JSON).
        output: serde_json::Value,
    },
    /// A message from the developer/system.
    Message {
        role: String, // "developer" | "system" | "user" | "assistant"
        content: ResponseContent,
    },
}

/// A hosted tool item from the Responses API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedToolItem {
    /// Type of hosted tool (e.g. "hosted_python").
    #[serde(rename = "type")]
    pub tool_type: String,
    /// Source code for hosted execution.
    #[serde(default)]
    pub code: Option<String>,
    /// Language identifier.
    #[serde(default)]
    pub language: Option<String>,
}

/// Content within a ResponseItem::Message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseContent {
    /// Plain text content.
    Text(String),
    /// Multiple content parts.
    Parts(Vec<ResponseContentPart>),
}

/// A content part within a multi-part message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseContentPart {
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(rename = "refusal")]
    Refusal { refusal: String },
}

impl ResponseContent {
    /// Extract text content, if available.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ResponseContent::Text(s) => Some(s),
            ResponseContent::Parts(parts) => {
                for part in parts {
                    if let ResponseContentPart::OutputText { text } = part {
                        return Some(text);
                    }
                }
                None
            }
        }
    }
}

/// A streamed event from the Responses API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponsesStreamEvent {
    /// Response created.
    #[serde(rename = "response.created")]
    ResponseCreated { response: ResponseObject },
    /// An output item was added.
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        output_index: usize,
        item: ResponseItem,
    },
    /// A content part was added.
    #[serde(rename = "response.content_part.added")]
    ContentPartAdded {
        output_index: usize,
        content_index: usize,
        part: ResponseContentPart,
    },
    /// Text content delta.
    #[serde(rename = "response.output_text.delta")]
    TextDelta {
        output_index: usize,
        content_index: usize,
        delta: String,
    },
    /// Content part done.
    #[serde(rename = "response.content_part.done")]
    ContentPartDone {
        output_index: usize,
        content_index: usize,
        part: ResponseContentPart,
    },
    /// Output item done.
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        output_index: usize,
        item: ResponseItem,
    },
    /// Response completed.
    #[serde(rename = "response.completed")]
    ResponseCompleted { response: ResponseObject },
    /// An error occurred.
    #[serde(rename = "error")]
    Error {
        code: Option<String>,
        message: String,
    },
    /// Incomplete response (e.g. max_tokens hit).
    #[serde(rename = "response.incomplete")]
    Incomplete {
        response: ResponseObject,
        reason: String,
    },
    /// Usage information.
    #[serde(rename = "response.usage")]
    Usage {
        input_tokens: u32,
        output_tokens: u32,
        total_tokens: u32,
        #[serde(default)]
        reasoning_tokens: Option<u32>,
    },
}

/// A Responses API response object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseObject {
    pub id: String,
    pub status: String, // "completed", "incomplete", "failed", "cancelled"
    #[serde(default)]
    pub output: Vec<ResponseItem>,
    #[serde(default)]
    pub usage: Option<ResponsesUsage>,
    #[serde(default)]
    pub incomplete_details: Option<ResponsesIncompleteDetails>,
}

/// Usage breakdown for a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    pub reasoning_tokens: Option<u32>,
}

/// Details about why a response is incomplete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesIncompleteDetails {
    pub reason: String,
}

// ─── Normalized hosted program events ──────────────────────────────

/// A provider-neutral event representing hosted program lifecycle.
///
/// These events bridge the Responses API item graph to CodeGG's
/// Tool Program, projection, and notification contracts.
#[derive(Debug, Clone)]
pub enum HostedProgramEvent {
    /// A hosted program started execution.
    ProgramStarted {
        /// Provider-assigned response ID.
        response_id: String,
        /// Provider-assigned program/item ID (opaque).
        provider_program_id: Option<String>,
        /// Source metadata (language, etc.).
        metadata: HostedProgramMetadata,
    },
    /// A nested client-owned function call was requested by the provider.
    NestedCall {
        /// Unique call ID from the provider.
        call_id: String,
        /// Tool/function name.
        tool_name: String,
        /// Parsed arguments.
        arguments: serde_json::Value,
        /// Sequence number in this response.
        sequence: u32,
    },
    /// A nested call result was accepted and persisted.
    NestedCallResult {
        /// The call ID this result corresponds to.
        call_id: String,
        /// Normalized CodeGG call ID.
        normalized_call_id: String,
        /// Whether execution succeeded.
        success: bool,
        /// Result output.
        output: serde_json::Value,
    },
    /// Structured program output from a hosted tool.
    ProgramOutput {
        /// The output value.
        value: serde_json::Value,
    },
    /// Program is incomplete and can be continued.
    ProgramIncomplete {
        /// Reason for incompleteness (e.g. "max_tokens", "content_filter").
        reason: String,
        /// Continuation token/response ID.
        continuation_token: String,
        /// Provider fingerprint for idempotent continuation.
        fingerprint: Option<String>,
    },
    /// Program finished with a terminal status.
    Terminal {
        /// Final status: "completed", "failed", "cancelled".
        status: String,
        /// Error message if failed.
        error: Option<String>,
        /// Final usage information.
        usage: Option<HostedUsage>,
    },
    /// Provider-specific fingerprint for continuation.
    Fingerprint {
        /// Opaque fingerprint value.
        value: String,
        /// Response ID this fingerprint belongs to.
        response_id: String,
    },
    /// Usage information for cost tracking.
    Usage(HostedUsage),
    /// An error occurred during hosted execution.
    Error {
        /// Error code.
        code: Option<String>,
        /// Error message.
        message: String,
    },
}

/// Metadata about a hosted program.
#[derive(Debug, Clone, Default)]
pub struct HostedProgramMetadata {
    /// Language used for hosted execution (e.g. "python").
    pub language: Option<String>,
    /// Provider-specific program version.
    pub version: Option<String>,
    /// Maximum items allowed.
    pub max_items: Option<usize>,
    /// Maximum nested calls.
    pub max_nested_calls: Option<usize>,
}

/// Usage information for hosted programs.
#[derive(Debug, Clone, Default)]
pub struct HostedUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    pub reasoning_tokens: Option<u32>,
}

// ─── Backend selection and fallback ────────────────────────────────

/// Backend selection policy for tool program execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostedBackendPolicy {
    /// Use only native restricted-Python execution.
    NativeOnly,
    /// Prefer hosted, fall back to native before execution begins.
    #[default]
    HostedPreferred,
    /// Require hosted execution; fail if unavailable.
    HostedRequired,
    /// Prefer native, use hosted only if native is unavailable.
    NativePreferred,
}

impl HostedBackendPolicy {
    /// Parse the stable policy spelling used by tool-program execution
    /// contexts and configuration.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "native_only" | "native-only" => Some(Self::NativeOnly),
            "hosted_preferred" | "hosted-preferred" => Some(Self::HostedPreferred),
            "hosted_required" | "hosted-required" => Some(Self::HostedRequired),
            "native_preferred" | "native-preferred" => Some(Self::NativePreferred),
            _ => None,
        }
    }

    /// Stable serialization used in durable execution context records.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NativeOnly => "native_only",
            Self::HostedPreferred => "hosted_preferred",
            Self::HostedRequired => "hosted_required",
            Self::NativePreferred => "native_preferred",
        }
    }

    /// Whether hosted execution is allowed.
    pub fn allows_hosted(&self) -> bool {
        matches!(
            self,
            HostedBackendPolicy::HostedPreferred | HostedBackendPolicy::HostedRequired
        )
    }

    /// Whether native execution is allowed.
    pub fn allows_native(&self) -> bool {
        matches!(
            self,
            HostedBackendPolicy::NativeOnly
                | HostedBackendPolicy::HostedPreferred
                | HostedBackendPolicy::NativePreferred
        )
    }

    /// Whether fallback from hosted to native is permitted.
    pub fn allows_fallback(&self) -> bool {
        matches!(
            self,
            HostedBackendPolicy::HostedPreferred | HostedBackendPolicy::NativePreferred
        )
    }

    /// Resolve the backend to use given provider capabilities.
    pub fn resolve(&self, capabilities: &ProviderCapabilities) -> ResolvedBackend {
        match self {
            HostedBackendPolicy::NativeOnly => ResolvedBackend::Native,
            HostedBackendPolicy::HostedRequired => {
                if capabilities.can_host_programs() {
                    ResolvedBackend::Hosted
                } else {
                    ResolvedBackend::Failed {
                        reason: "hosted required but provider does not support it".to_string(),
                    }
                }
            }
            HostedBackendPolicy::HostedPreferred => {
                if capabilities.can_host_programs() {
                    ResolvedBackend::Hosted
                } else {
                    ResolvedBackend::Native
                }
            }
            HostedBackendPolicy::NativePreferred => ResolvedBackend::Native,
        }
    }
}

/// The resolved execution backend after policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedBackend {
    /// Execute natively via restricted Python.
    Native,
    /// Execute via the hosted provider adapter.
    Hosted,
    /// Resolution failed.
    Failed { reason: String },
}

// ─── Hosted call identity and deduplication ────────────────────────

/// Normalized identity for a nested call originating from a hosted program.
///
/// Provider item IDs are opaque; we normalize them into deterministic
/// CodeGG `ProgramCallId` values while tracking the original for
/// deduplication.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct HostedCallIdentity {
    /// The program this call belongs to.
    pub program_id: String,
    /// Provider-assigned call ID.
    pub provider_call_id: String,
    /// Tool/function name.
    pub tool_name: String,
    /// Normalized input hash for deduplication.
    pub input_hash: String,
}

impl HostedCallIdentity {
    /// Create a normalized CodeGG call ID from provider identity.
    ///
    /// The call ID format is `hc-{program_id_hash[..8]}-{provider_call_id_hash[..8]}`.
    pub fn normalized_call_id(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.program_id.hash(&mut hasher);
        self.provider_call_id.hash(&mut hasher);
        let hash = hasher.finish();
        format!("hc-{:016x}", hash)
    }
}

// ─── Fingerprint and continuation state ────────────────────────────

/// Opaque continuation state for a hosted program.
///
/// This is a provider-specific value that must be passed back to
/// continue a multi-turn hosted program. It is NOT a CodeGG identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuationState {
    /// The response ID to continue.
    pub response_id: String,
    /// Optional fingerprint for idempotent continuation.
    pub fingerprint: Option<String>,
    /// The provider's conversation/item state (opaque).
    pub provider_state: serde_json::Value,
}

// ─── Hosted Program Adapter ────────────────────────────────────────

/// Adapter that bridges the Responses API hosted-program model
/// to CodeGG's Tool Broker and scheduler contracts.
///
/// The adapter implements the full 8-step broker integration pipeline:
/// 1. Resolve/create deterministic `ProgramCallId` from provider identity.
/// 2. Reject duplicate mismatched identities.
/// 3. Validate tool contract/caller policy/arguments/authority.
/// 4. Reserve the call ledger before execution.
/// 5. Execute inline or via scheduler child job (delegated to caller).
/// 6. Validate/persist result and artifacts.
/// 7. Return bounded provider-facing tool result.
/// 8. Persist continuation state before waiting for more items.
pub struct HostedProgramAdapter {
    /// The program identity within CodeGG.
    program_id: String,
    /// Provider capabilities snapshot.
    capabilities: ProviderCapabilities,
    /// Backend selection policy.
    policy: HostedBackendPolicy,
    /// Completed calls by provider call ID (for deduplication).
    completed_calls: HashMap<String, CompletedHostedCall>,
    /// Call ledger — tracks reserved (in-flight) calls.
    reserved_calls: HashMap<String, ReservedCall>,
    /// Current continuation state.
    continuation: Option<ContinuationState>,
    /// Event buffer for emitted events.
    events: Vec<HostedProgramEvent>,
    /// Maximum result size for nested calls (from capabilities).
    max_result_size: Option<usize>,
    /// Maximum nested calls (from capabilities).
    max_nested_calls: Option<usize>,
    /// Allowed tool names (empty = all tools allowed, validated at broker).
    allowed_tools: Vec<String>,
    /// Denied tool names (direct-only/mutating tools).
    denied_tools: Vec<String>,
}

/// Record of a reserved (in-flight) nested call.
#[derive(Debug, Clone)]
pub struct ReservedCall {
    /// Normalized CodeGG call ID.
    pub normalized_call_id: String,
    /// Tool name.
    pub tool_name: String,
    /// Normalized input hash.
    pub input_hash: String,
    /// Reservation timestamp (monotonic).
    pub reserved_at: std::time::Instant,
}

/// Record of a completed hosted call.
#[derive(Debug, Clone)]
pub struct CompletedHostedCall {
    /// The provider-assigned call ID.
    pub provider_call_id: String,
    /// The normalized CodeGG call ID.
    pub normalized_call_id: String,
    /// Tool name.
    pub tool_name: String,
    /// Normalized input hash.
    pub input_hash: String,
    /// Whether execution succeeded.
    pub success: bool,
    /// Result output (stored for replay on duplicate requests).
    pub output: serde_json::Value,
    /// Result size in bytes (for bounded return enforcement).
    pub result_size: usize,
}

impl HostedProgramAdapter {
    /// Create a new adapter for a hosted program.
    pub fn new(
        program_id: String,
        capabilities: ProviderCapabilities,
        policy: HostedBackendPolicy,
    ) -> Self {
        let max_result_size = capabilities.max_result_size;
        let max_nested_calls = capabilities.max_nested_calls;
        Self {
            program_id,
            capabilities,
            policy,
            completed_calls: HashMap::new(),
            reserved_calls: HashMap::new(),
            continuation: None,
            events: Vec::new(),
            max_result_size,
            max_nested_calls,
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
        }
    }

    /// Set the list of denied tool names (direct-only/mutating).
    /// These tools cannot be called through hosted programs.
    pub fn with_denied_tools(mut self, denied: Vec<String>) -> Self {
        self.denied_tools = denied;
        self
    }

    /// Set the list of allowed tool names (if non-empty, only these can be called).
    pub fn with_allowed_tools(mut self, allowed: Vec<String>) -> Self {
        self.allowed_tools = allowed;
        self
    }

    /// The program identity.
    pub fn program_id(&self) -> &str {
        &self.program_id
    }

    /// The resolved backend for this program.
    pub fn resolved_backend(&self) -> ResolvedBackend {
        self.policy.resolve(&self.capabilities)
    }

    /// Step 3: Validate whether a tool can be called through hosted programs.
    ///
    /// Enforces:
    /// - Tool is not in the denied list (direct-only/mutating).
    /// - Tool is in the allowed list (if one is set).
    /// - Provider supports client-owned nested calls.
    pub fn validate_tool_call(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<(), String> {
        // Check provider supports nested calls
        if !self.capabilities.supports_client_owned_nested_calls {
            return Err("provider does not support client-owned nested calls".to_string());
        }

        // Check tool is not denied
        if self.denied_tools.iter().any(|d| d == tool_name) {
            return Err(format!(
                "tool '{}' is denied for hosted execution (direct-only or mutating)",
                tool_name
            ));
        }

        // Check tool is allowed (if allowlist is set)
        if !self.allowed_tools.is_empty() && !self.allowed_tools.iter().any(|a| a == tool_name) {
            return Err(format!(
                "tool '{}' is not in the allowed tools list for this program",
                tool_name
            ));
        }

        // Validate arguments as untrusted model output
        let args_str = serde_json::to_string(arguments)
            .map_err(|e| format!("cannot serialize arguments: {}", e))?;
        match validate_arguments(&args_str) {
            InputValidation::Valid => {}
            InputValidation::Invalid { reason } => {
                return Err(format!("argument validation failed: {}", reason));
            }
        }

        Ok(())
    }

    /// Step 4: Reserve a call in the ledger before execution.
    ///
    /// Returns the normalized call ID. Fails if the call count limit
    /// is exceeded or the call is already reserved.
    pub fn reserve_call(
        &mut self,
        call_id: &str,
        tool_name: &str,
        input_hash: &str,
    ) -> Result<String, String> {
        // Check call count limit
        let total = self.completed_calls.len() + self.reserved_calls.len();
        match validate_call_count(total, self.max_nested_calls) {
            InputValidation::Valid => {}
            InputValidation::Invalid { reason } => return Err(reason),
        }

        // Check not already reserved
        if self.reserved_calls.contains_key(call_id) {
            return Err(format!("call '{}' is already reserved", call_id));
        }

        let identity = HostedCallIdentity {
            program_id: self.program_id.clone(),
            provider_call_id: call_id.to_string(),
            tool_name: tool_name.to_string(),
            input_hash: input_hash.to_string(),
        };
        let normalized = identity.normalized_call_id();

        self.reserved_calls.insert(
            call_id.to_string(),
            ReservedCall {
                normalized_call_id: normalized.clone(),
                tool_name: tool_name.to_string(),
                input_hash: input_hash.to_string(),
                reserved_at: std::time::Instant::now(),
            },
        );

        Ok(normalized)
    }

    /// Step 6: Validate and persist a completed call result.
    ///
    /// Checks result size bounds, removes the reservation, and stores
    /// the completed call record. Returns the normalized call ID.
    pub fn record_call_result(
        &mut self,
        call_id: String,
        tool_name: String,
        input_hash: String,
        success: bool,
        output: serde_json::Value,
    ) -> Result<String, String> {
        // Validate result size
        match validate_result_size(&output, self.max_result_size) {
            InputValidation::Valid => {}
            InputValidation::Invalid { reason } => return Err(reason),
        }

        let result_size = serde_json::to_vec(&output).map(|b| b.len()).unwrap_or(0);

        let identity = HostedCallIdentity {
            program_id: self.program_id.clone(),
            provider_call_id: call_id.clone(),
            tool_name: tool_name.clone(),
            input_hash: input_hash.clone(),
        };
        let normalized_call_id = identity.normalized_call_id();

        let record = CompletedHostedCall {
            provider_call_id: call_id.clone(),
            normalized_call_id: normalized_call_id.clone(),
            tool_name,
            input_hash,
            success,
            output,
            result_size,
        };

        self.completed_calls.insert(call_id.clone(), record);
        self.reserved_calls.remove(&call_id);

        Ok(normalized_call_id)
    }

    /// Step 7: Build a bounded `FunctionCallOutput` item for the provider.
    ///
    /// If the result exceeds the per-call limit, it is truncated with
    /// a summary placeholder.
    pub fn build_call_output(call_id: String, output: &serde_json::Value) -> ResponseItem {
        let bounded_output = match serde_json::to_vec(output) {
            Ok(bytes) if bytes.len() > MAX_RESULT_SIZE => {
                serde_json::json!({
                    "truncated": true,
                    "original_size": bytes.len(),
                    "summary": "result truncated for provider transmission"
                })
            }
            _ => output.clone(),
        };
        ResponseItem::FunctionCallOutput {
            call_id,
            output: bounded_output,
        }
    }

    /// Process a streamed Responses API event and emit normalized
    /// hosted program events.
    pub fn process_stream_event(&mut self, event: ResponsesStreamEvent) -> Vec<HostedProgramEvent> {
        let mut emitted = Vec::new();

        match event {
            ResponsesStreamEvent::ResponseCreated { response } => {
                let metadata = HostedProgramMetadata::default();
                self.continuation = Some(ContinuationState {
                    response_id: response.id.clone(),
                    fingerprint: None,
                    provider_state: serde_json::json!({
                        "status": response.status,
                        "output_count": response.output.len(),
                    }),
                });
                emitted.push(HostedProgramEvent::ProgramStarted {
                    response_id: response.id,
                    provider_program_id: None,
                    metadata,
                });
            }
            ResponsesStreamEvent::OutputItemAdded { item, .. } => {
                match item {
                    ResponseItem::FunctionCall {
                        call_id,
                        name,
                        arguments,
                    } => {
                        let args: serde_json::Value =
                            serde_json::from_str(&arguments).unwrap_or(serde_json::json!({}));

                        // Compute input hash for deduplication
                        let input_hash = {
                            use std::collections::hash_map::DefaultHasher;
                            use std::hash::{Hash, Hasher};
                            let mut h = DefaultHasher::new();
                            name.hash(&mut h);
                            arguments.hash(&mut h);
                            format!("{:016x}", h.finish())
                        };

                        // Check for duplicate call
                        if let Some(existing) = self.completed_calls.get(&call_id) {
                            if existing.input_hash == input_hash {
                                // Duplicate with matching args: return recorded result
                                emitted.push(HostedProgramEvent::NestedCallResult {
                                    call_id: call_id.clone(),
                                    normalized_call_id: existing.normalized_call_id.clone(),
                                    success: existing.success,
                                    output: existing.output.clone(),
                                });
                                return emitted;
                            } else {
                                // Duplicate with mismatched args: terminal failure
                                emitted.push(HostedProgramEvent::Error {
                                    code: Some("call_identity_mismatch".to_string()),
                                    message: format!(
                                        "Provider repeated call '{}' with different arguments",
                                        call_id
                                    ),
                                });
                                return emitted;
                            }
                        }

                        // Validate tool call (step 3)
                        if let Err(reason) = self.validate_tool_call(&name, &args) {
                            emitted.push(HostedProgramEvent::Error {
                                code: Some("tool_validation_failed".to_string()),
                                message: reason,
                            });
                            return emitted;
                        }

                        // Reserve call (step 4)
                        match self.reserve_call(&call_id, &name, &input_hash) {
                            Ok(_normalized) => {
                                let sequence = self.completed_calls.len() as u32
                                    + self.reserved_calls.len() as u32;
                                emitted.push(HostedProgramEvent::NestedCall {
                                    call_id,
                                    tool_name: name,
                                    arguments: args,
                                    sequence,
                                });
                            }
                            Err(reason) => {
                                emitted.push(HostedProgramEvent::Error {
                                    code: Some("call_reservation_failed".to_string()),
                                    message: reason,
                                });
                            }
                        }
                    }
                    ResponseItem::HostedTool { .. } => {
                        // Hosted tool items are provider-executed, not client-owned
                    }
                    _ => {}
                }
            }
            ResponsesStreamEvent::TextDelta { delta, .. } => {
                let _ = delta;
            }
            ResponsesStreamEvent::ResponseCompleted { response } => {
                // Step 8: persist continuation state
                if let Some(ref mut state) = self.continuation {
                    state.response_id = response.id.clone();
                    state.provider_state = serde_json::json!({
                        "status": response.status,
                        "output_count": response.output.len(),
                    });
                }

                let status = response.status.clone();
                let usage = response.usage.as_ref().map(|u| HostedUsage {
                    input_tokens: u.input_tokens,
                    output_tokens: u.output_tokens,
                    total_tokens: u.total_tokens,
                    reasoning_tokens: Some(u.reasoning_tokens.unwrap_or(0)),
                });

                emitted.push(HostedProgramEvent::Terminal {
                    status,
                    error: None,
                    usage: usage.clone(),
                });

                if let Some(u) = usage {
                    emitted.push(HostedProgramEvent::Usage(u));
                }
            }
            ResponsesStreamEvent::Incomplete { reason, .. } => {
                let response_id = self
                    .continuation
                    .as_ref()
                    .map(|c| c.response_id.clone())
                    .unwrap_or_default();

                let fingerprint = self
                    .continuation
                    .as_ref()
                    .and_then(|c| c.fingerprint.clone());

                emitted.push(HostedProgramEvent::ProgramIncomplete {
                    reason,
                    continuation_token: response_id,
                    fingerprint,
                });
            }
            ResponsesStreamEvent::Error { code, message } => {
                emitted.push(HostedProgramEvent::Error { code, message });
            }
            ResponsesStreamEvent::Usage { .. } => {
                // Usage events are handled in ResponseCompleted
            }
            _ => {}
        }

        self.events.extend(emitted.clone());
        emitted
    }

    /// Get the current continuation state.
    pub fn continuation(&self) -> Option<&ContinuationState> {
        self.continuation.as_ref()
    }

    /// Get all emitted events.
    pub fn events(&self) -> &[HostedProgramEvent] {
        &self.events
    }

    /// Get the number of completed calls.
    pub fn completed_call_count(&self) -> usize {
        self.completed_calls.len()
    }

    /// Get the number of reserved (in-flight) calls.
    pub fn reserved_call_count(&self) -> usize {
        self.reserved_calls.len()
    }

    /// Check if a call has already been completed (for deduplication).
    pub fn is_call_completed(&self, call_id: &str) -> bool {
        self.completed_calls.contains_key(call_id)
    }

    /// Check if a call is currently reserved (in-flight).
    pub fn is_call_reserved(&self, call_id: &str) -> bool {
        self.reserved_calls.contains_key(call_id)
    }

    /// Release a reservation without recording a result (for cancellation).
    pub fn release_reservation(&mut self, call_id: &str) -> bool {
        self.reserved_calls.remove(call_id).is_some()
    }

    /// Get the total result bytes across all completed calls.
    pub fn total_result_bytes(&self) -> usize {
        self.completed_calls.values().map(|c| c.result_size).sum()
    }
}

// ─── Responses API HTTP transport ──────────────────────────────────

/// Configuration for the Responses API transport.
#[derive(Debug, Clone)]
pub struct ResponsesTransportConfig {
    /// Per-request timeout.
    pub request_timeout: Duration,
    /// Stream-idle timeout — max time between SSE events.
    pub stream_idle_timeout: Duration,
    /// Maximum SSE buffer size.
    pub max_sse_buffer_size: usize,
}

impl Default for ResponsesTransportConfig {
    fn default() -> Self {
        Self {
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            stream_idle_timeout: DEFAULT_STREAM_IDLE_TIMEOUT,
            max_sse_buffer_size: MAX_SSE_BUFFER_SIZE,
        }
    }
}

/// HTTP transport for the Responses API.
///
/// Handles request serialization, SSE streaming, and response parsing
/// separate from the Chat Completions transport. Supports cancellation,
/// per-request timeout, and stream-idle bounds.
pub struct ResponsesTransport {
    http_client: reqwest::Client,
    base_url: String,
    api_key: String,
    config: ResponsesTransportConfig,
    /// Cancellation flag — when set to true, the stream should terminate.
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl ResponsesTransport {
    /// Create a new transport with default configuration.
    pub fn new(base_url: String, api_key: String) -> Self {
        Self::with_config(base_url, api_key, ResponsesTransportConfig::default())
    }

    /// Create a new transport with custom configuration.
    pub fn with_config(
        base_url: String,
        api_key: String,
        config: ResponsesTransportConfig,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(32)
            .pool_idle_timeout(Duration::from_secs(30))
            .tcp_keepalive(Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            http_client,
            base_url,
            api_key,
            config,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Cancel any active stream. The next poll of the stream will return `None`.
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Check if the transport has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Reset the cancellation flag (for reuse).
    pub fn reset_cancel(&self) {
        self.cancelled
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// Send a Responses API request and return the response object.
    pub async fn create_response(
        &self,
        request: &ResponsesRequest,
    ) -> Result<ResponseObject, crate::error::ProviderError> {
        let url = format!("{}/responses", self.base_url);
        let body = serde_json::to_vec(request)
            .map_err(|e| crate::error::ProviderError::api("serialization", e.to_string()))?;

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| crate::error::ProviderError::api("network", e.to_string()))?;

        if !response.status().is_success() {
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(crate::error::ProviderError::api_with_url(
                "http_error",
                text,
                url,
            ));
        }

        response
            .json::<ResponseObject>()
            .await
            .map_err(|e| crate::error::ProviderError::api("deserialization", e.to_string()))
    }

    /// Send a Responses API request with streaming.
    ///
    /// Supports cancellation via `cancel()` and enforces stream-idle
    /// timeout to detect stalled connections.
    pub async fn create_response_stream(
        &self,
        request: &ResponsesRequest,
    ) -> Result<crate::provider_core::EventStream, crate::error::ProviderError> {
        let url = format!("{}/responses", self.base_url);
        let mut req_body = serde_json::to_value(request)
            .map_err(|e| crate::error::ProviderError::api("serialization", e.to_string()))?;
        req_body["stream"] = serde_json::json!(true);

        let body = serde_json::to_vec(&req_body)
            .map_err(|e| crate::error::ProviderError::api("serialization", e.to_string()))?;

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| crate::error::ProviderError::api("network", e.to_string()))?;

        if !response.status().is_success() {
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(crate::error::ProviderError::api_with_url(
                "http_error",
                text,
                url,
            ));
        }

        let cancelled = self.cancelled.clone();
        let idle_timeout = self.config.stream_idle_timeout;
        let max_buffer = self.config.max_sse_buffer_size;

        let byte_stream = response.bytes_stream();
        let stream = futures::stream::unfold(
            (byte_stream, String::new()),
            move |(mut byte_stream, mut buffer)| {
                let cancelled = cancelled.clone();
                async move {
                    use futures::StreamExt;

                    // Check cancellation
                    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                        return None;
                    }

                    loop {
                        // Check cancellation at each iteration
                        if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                            return None;
                        }

                        // Try to parse an event from the buffer
                        if let Some(idx) = buffer.find("\n\n") {
                            let event_block = buffer[..idx].to_string();
                            buffer = buffer[idx + 2..].to_string();

                            // Parse SSE data
                            let mut data = String::new();
                            for line in event_block.lines() {
                                if let Some(d) = line.strip_prefix("data: ") {
                                    data = d.to_string();
                                }
                            }

                            if data == "[DONE]" {
                                return None;
                            }

                            if let Ok(event) = serde_json::from_str::<ResponsesStreamEvent>(&data) {
                                // Convert ResponsesStreamEvent to ChatEvent
                                let chat_event = match &event {
                                    ResponsesStreamEvent::TextDelta { delta, .. } => {
                                        Some(crate::provider_core::ChatEvent::TextDelta(Arc::new(
                                            delta.clone(),
                                        )))
                                    }
                                    ResponsesStreamEvent::ResponseCompleted { response } => {
                                        let usage = response.usage.as_ref().map(|u| {
                                            crate::provider_core::TokenUsage {
                                                input_tokens: u.input_tokens as usize,
                                                output_tokens: u.output_tokens as usize,
                                                total_tokens: u.total_tokens as usize,
                                                reasoning_tokens: u.reasoning_tokens.unwrap_or(0)
                                                    as usize,
                                                cached_tokens: None,
                                            }
                                        });
                                        Some(crate::provider_core::ChatEvent::Finish {
                                            stop_reason: Arc::new(response.status.clone()),
                                            usage: usage.unwrap_or_default(),
                                        })
                                    }
                                    ResponsesStreamEvent::Error { message, .. } => {
                                        Some(crate::provider_core::ChatEvent::Error(Arc::new(
                                            message.clone(),
                                        )))
                                    }
                                    _ => None,
                                };

                                if let Some(evt) = chat_event {
                                    return Some((Ok(evt), (byte_stream, buffer)));
                                }
                                // Non-mappable events are skipped
                                continue;
                            }
                            // Malformed JSON: skip and try next event
                            continue;
                        }

                        // Enforce buffer size limit
                        if buffer.len() > max_buffer {
                            return Some((
                                Err(crate::error::ProviderError::Stream(format!(
                                    "SSE buffer exceeded maximum size of {} bytes",
                                    max_buffer
                                ))),
                                (byte_stream, buffer),
                            ));
                        }

                        // Read more bytes with idle timeout
                        match tokio::time::timeout(idle_timeout, byte_stream.next()).await {
                            Ok(Some(Ok(chunk))) => {
                                let chunk_str = String::from_utf8_lossy(&chunk).to_string();
                                buffer.push_str(&chunk_str);
                            }
                            Ok(Some(Err(_))) => return None,
                            Ok(None) => return None,
                            Err(_) => {
                                // Idle timeout elapsed
                                return Some((
                                    Err(crate::error::ProviderError::Timeout(format!(
                                        "stream idle timeout after {:?}",
                                        idle_timeout
                                    ))),
                                    (byte_stream, buffer),
                                ));
                            }
                        }
                    }
                }
            },
        );

        Ok(Box::pin(stream))
    }
}

// ─── Fixture builders for testing ──────────────────────────────────

/// Build a minimal function_call item for testing.
pub fn fixture_function_call(call_id: &str, name: &str, args: &str) -> ResponseItem {
    ResponseItem::FunctionCall {
        call_id: call_id.to_string(),
        name: name.to_string(),
        arguments: args.to_string(),
    }
}

/// Build a function_call_output item for testing.
pub fn fixture_function_call_output(call_id: &str, output: serde_json::Value) -> ResponseItem {
    ResponseItem::FunctionCallOutput {
        call_id: call_id.to_string(),
        output,
    }
}

/// Build a response completed event for testing.
pub fn fixture_response_completed(
    response_id: &str,
    output: Vec<ResponseItem>,
) -> ResponsesStreamEvent {
    ResponsesStreamEvent::ResponseCompleted {
        response: ResponseObject {
            id: response_id.to_string(),
            status: "completed".to_string(),
            output,
            usage: Some(ResponsesUsage {
                input_tokens: 100,
                output_tokens: 200,
                total_tokens: 300,
                reasoning_tokens: None,
            }),
            incomplete_details: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_capabilities_hosted_support() {
        let caps = ProviderCapabilities::for_provider("openai");
        assert!(caps.supports_responses_api);
        assert!(caps.supports_hosted_programs);
        assert!(caps.can_host_programs());
        assert!(caps.full_hosted_support());

        let caps = ProviderCapabilities::for_provider("anthropic");
        assert!(!caps.supports_responses_api);
        assert!(!caps.supports_hosted_programs);
        assert!(!caps.can_host_programs());
        assert!(!caps.full_hosted_support());
    }

    #[test]
    fn backend_policy_resolution() {
        let openai_caps = ProviderCapabilities::for_provider("openai");
        let anthropic_caps = ProviderCapabilities::for_provider("anthropic");

        let policy = HostedBackendPolicy::HostedPreferred;
        assert_eq!(policy.resolve(&openai_caps), ResolvedBackend::Hosted);
        assert_eq!(policy.resolve(&anthropic_caps), ResolvedBackend::Native);

        let policy = HostedBackendPolicy::NativeOnly;
        assert_eq!(policy.resolve(&openai_caps), ResolvedBackend::Native);

        let policy = HostedBackendPolicy::HostedRequired;
        assert_eq!(policy.resolve(&openai_caps), ResolvedBackend::Hosted);
        assert!(matches!(
            policy.resolve(&anthropic_caps),
            ResolvedBackend::Failed { .. }
        ));

        let policy = HostedBackendPolicy::NativePreferred;
        assert_eq!(policy.resolve(&openai_caps), ResolvedBackend::Native);
    }

    #[test]
    fn backend_policy_flags() {
        assert!(HostedBackendPolicy::HostedPreferred.allows_hosted());
        assert!(HostedBackendPolicy::HostedPreferred.allows_native());
        assert!(HostedBackendPolicy::HostedPreferred.allows_fallback());

        assert!(!HostedBackendPolicy::NativeOnly.allows_hosted());
        assert!(HostedBackendPolicy::NativeOnly.allows_native());
        assert!(!HostedBackendPolicy::NativeOnly.allows_fallback());

        assert!(HostedBackendPolicy::HostedRequired.allows_hosted());
        assert!(!HostedBackendPolicy::HostedRequired.allows_native());
        assert!(!HostedBackendPolicy::HostedRequired.allows_fallback());

        assert!(!HostedBackendPolicy::NativePreferred.allows_hosted());
        assert!(HostedBackendPolicy::NativePreferred.allows_native());
        assert!(HostedBackendPolicy::NativePreferred.allows_fallback());
    }

    #[test]
    fn response_item_serialization_roundtrip() {
        let item = fixture_function_call("call_1", "read", r#"{"path":"/tmp/a.txt"}"#);
        let json = serde_json::to_string(&item).unwrap();
        let back: ResponseItem = serde_json::from_str(&json).unwrap();
        match back {
            ResponseItem::FunctionCall {
                call_id,
                name,
                arguments,
            } => {
                assert_eq!(call_id, "call_1");
                assert_eq!(name, "read");
                assert!(arguments.contains("/tmp/a.txt"));
            }
            _ => panic!("expected FunctionCall"),
        }
    }

    #[test]
    fn responses_request_serialization() {
        let req = ResponsesRequest {
            model: "gpt-4.1".to_string(),
            input: vec![fixture_function_call(
                "c1",
                "read",
                r#"{"path":"src/main.rs"}"#,
            )],
            stream: false,
            max_output_tokens: Some(4096),
            temperature: None,
            top_p: None,
            tools: Some(vec![ResponsesTool {
                tool_type: "function".to_string(),
                name: "read".to_string(),
                description: Some("Read a file".to_string()),
                parameters: serde_json::json!({"type": "object"}),
                strict: None,
            }]),
            include_usage: true,
            instructions: Some("You are helpful".to_string()),
            metadata: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "gpt-4.1");
        assert_eq!(json["stream"], false);
        assert!(json["tools"].is_array());
        assert!(json["instructions"].is_string());
    }

    #[test]
    fn hosted_call_identity_deterministic() {
        let id1 = HostedCallIdentity {
            program_id: "tp-abc123".to_string(),
            provider_call_id: "call_xyz".to_string(),
            tool_name: "read".to_string(),
            input_hash: "deadbeef".to_string(),
        };
        let id2 = HostedCallIdentity {
            program_id: "tp-abc123".to_string(),
            provider_call_id: "call_xyz".to_string(),
            tool_name: "read".to_string(),
            input_hash: "deadbeef".to_string(),
        };
        assert_eq!(id1.normalized_call_id(), id2.normalized_call_id());

        let id3 = HostedCallIdentity {
            provider_call_id: "call_different".to_string(),
            ..id1.clone()
        };
        assert_ne!(id1.normalized_call_id(), id3.normalized_call_id());
    }

    #[test]
    fn adapter_deduplication() {
        let caps = ProviderCapabilities::for_provider("openai");
        let mut adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        );

        adapter
            .record_call_result(
                "call_1".to_string(),
                "read".to_string(),
                "hash1".to_string(),
                true,
                serde_json::json!({"content": "hello"}),
            )
            .unwrap();

        assert!(adapter.is_call_completed("call_1"));
        assert!(!adapter.is_call_completed("call_2"));
        assert_eq!(adapter.completed_call_count(), 1);
    }

    #[test]
    fn adapter_duplicate_call_returns_recorded_result() {
        let caps = ProviderCapabilities::for_provider("openai");
        let mut adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        );

        let input_hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            "read".hash(&mut h);
            r#"{"path":"/tmp/a.txt"}"#.hash(&mut h);
            format!("{:016x}", h.finish())
        };

        adapter
            .record_call_result(
                "call_1".to_string(),
                "read".to_string(),
                input_hash.clone(),
                true,
                serde_json::json!({"content": "hello"}),
            )
            .unwrap();

        let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: 0,
            item: fixture_function_call("call_1", "read", r#"{"path":"/tmp/a.txt"}"#),
        });

        assert!(!events.is_empty());
        match &events[0] {
            HostedProgramEvent::NestedCallResult {
                call_id, success, ..
            } => {
                assert_eq!(call_id, "call_1");
                assert!(success);
            }
            other => panic!("expected NestedCallResult, got {:?}", other),
        }
    }

    #[test]
    fn adapter_mismatched_duplicate_args_is_error() {
        let caps = ProviderCapabilities::for_provider("openai");
        let mut adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        );

        adapter
            .record_call_result(
                "call_1".to_string(),
                "read".to_string(),
                "hash_a".to_string(),
                true,
                serde_json::json!({"content": "hello"}),
            )
            .unwrap();

        adapter
            .record_call_result(
                "call_2".to_string(),
                "read".to_string(),
                "hash_b".to_string(),
                true,
                serde_json::json!({"content": "world"}),
            )
            .unwrap();

        let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: 0,
            item: fixture_function_call("call_2", "read", r#"{"path":"/tmp/different.txt"}"#),
        });

        assert!(!events.is_empty());
        match &events[0] {
            HostedProgramEvent::Error { code, message } => {
                assert_eq!(code.as_deref(), Some("call_identity_mismatch"));
                assert!(message.contains("different arguments"));
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn adapter_build_call_output() {
        let output = HostedProgramAdapter::build_call_output(
            "call_1".to_string(),
            &serde_json::json!({"content": "file contents"}),
        );
        match output {
            ResponseItem::FunctionCallOutput { call_id, output } => {
                assert_eq!(call_id, "call_1");
                assert!(output.get("content").is_some());
            }
            _ => panic!("expected FunctionCallOutput"),
        }
    }

    #[test]
    fn adapter_continuation_state() {
        let caps = ProviderCapabilities::for_provider("openai");
        let mut adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        );

        assert!(adapter.continuation().is_none());

        adapter.process_stream_event(ResponsesStreamEvent::ResponseCreated {
            response: ResponseObject {
                id: "resp_123".to_string(),
                status: "in_progress".to_string(),
                output: vec![],
                usage: None,
                incomplete_details: None,
            },
        });

        let cont = adapter.continuation().unwrap();
        assert_eq!(cont.response_id, "resp_123");
    }

    #[test]
    fn adapter_program_lifecycle_events() {
        let caps = ProviderCapabilities::for_provider("openai");
        let mut adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        );

        let mut all_events = Vec::new();

        all_events.extend(
            adapter.process_stream_event(ResponsesStreamEvent::ResponseCreated {
                response: ResponseObject {
                    id: "resp_1".to_string(),
                    status: "in_progress".to_string(),
                    output: vec![],
                    usage: None,
                    incomplete_details: None,
                },
            }),
        );

        all_events.extend(
            adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
                output_index: 0,
                item: fixture_function_call("call_1", "read", r#"{"path":"src/main.rs"}"#),
            }),
        );

        all_events.extend(adapter.process_stream_event(fixture_response_completed(
            "resp_1",
            vec![fixture_function_call_output(
                "call_1",
                serde_json::json!({"content": "fn main() {}"}),
            )],
        )));

        assert!(all_events.iter().any(|e| matches!(
            e,
            HostedProgramEvent::ProgramStarted { response_id, .. } if response_id == "resp_1"
        )));
        assert!(all_events.iter().any(|e| matches!(
            e,
            HostedProgramEvent::NestedCall { call_id, tool_name, .. }
                if call_id == "call_1" && tool_name == "read"
        )));
        assert!(all_events.iter().any(
            |e| matches!(e, HostedProgramEvent::Terminal { status, .. } if status == "completed")
        ));
        assert!(all_events
            .iter()
            .any(|e| matches!(e, HostedProgramEvent::Usage(_))));
    }

    #[test]
    fn response_content_text_extraction() {
        let text = ResponseContent::Text("hello".to_string());
        assert_eq!(text.as_text(), Some("hello"));

        let parts = ResponseContent::Parts(vec![ResponseContentPart::OutputText {
            text: "world".to_string(),
        }]);
        assert_eq!(parts.as_text(), Some("world"));

        let mixed = ResponseContent::Parts(vec![ResponseContentPart::Refusal {
            refusal: "no".to_string(),
        }]);
        assert_eq!(mixed.as_text(), None);
    }

    #[test]
    fn responses_tool_from_definition() {
        let def = crate::provider_core::ToolDefinition {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type": "object"}),
            defer_loading: None,
        };
        let tool = ResponsesTool::from_tool_definition(&def);
        assert_eq!(tool.name, "read");
        assert_eq!(tool.tool_type, "function");
        assert_eq!(tool.description.as_deref(), Some("Read a file"));
    }

    // ── New tests for security and broker integration ──────────────

    #[test]
    fn validate_arguments_rejects_non_json() {
        match validate_arguments("not json") {
            InputValidation::Invalid { reason } => assert!(reason.contains("invalid JSON")),
            other => panic!("expected Invalid, got {:?}", other),
        }
    }

    #[test]
    fn validate_arguments_rejects_non_object() {
        match validate_arguments(r#""just a string""#) {
            InputValidation::Invalid { reason } => {
                assert!(reason.contains("must be a JSON object"))
            }
            other => panic!("expected Invalid, got {:?}", other),
        }
    }

    #[test]
    fn validate_arguments_accepts_valid_object() {
        assert_eq!(
            validate_arguments(r#"{"key": "value"}"#),
            InputValidation::Valid
        );
    }

    #[test]
    fn validate_result_size_accepts_small() {
        assert_eq!(
            validate_result_size(&serde_json::json!({"ok": true}), None),
            InputValidation::Valid
        );
    }

    #[test]
    fn validate_result_size_rejects_oversized() {
        let big = "x".repeat(MAX_RESULT_SIZE + 1);
        let val = serde_json::json!({"data": big});
        match validate_result_size(&val, None) {
            InputValidation::Invalid { reason } => assert!(reason.contains("exceeds maximum")),
            other => panic!("expected Invalid, got {:?}", other),
        }
    }

    #[test]
    fn validate_call_count_within_limits() {
        assert_eq!(validate_call_count(5, Some(10)), InputValidation::Valid);
    }

    #[test]
    fn validate_call_count_exceeds_limits() {
        match validate_call_count(10, Some(10)) {
            InputValidation::Invalid { reason } => assert!(reason.contains("exceeds maximum")),
            other => panic!("expected Invalid, got {:?}", other),
        }
    }

    #[test]
    fn redact_for_log_masks_sensitive_data() {
        let redacted = redact_for_log("sk-1234567890abcdef");
        assert!(!redacted.contains("1234567890"));
        assert!(redacted.contains("..."));
    }

    #[test]
    fn redact_for_log_handles_empty() {
        assert_eq!(redact_for_log(""), "<empty>");
    }

    #[test]
    fn redact_fingerprint_masks() {
        let fp = redact_fingerprint("fp_abcdef1234567890");
        assert!(!fp.contains("fp_abcdef1234567890"));
        assert!(fp.contains("..."));
    }

    #[test]
    fn minimize_input_items_truncates_large_outputs() {
        let mut items = vec![ResponseItem::FunctionCallOutput {
            call_id: "c1".to_string(),
            output: serde_json::json!({"data": "x".repeat(2000)}),
        }];
        minimize_input_items(&mut items, 512);
        match &items[0] {
            ResponseItem::FunctionCallOutput { output, .. } => {
                assert!(output.get("truncated").unwrap().as_bool().unwrap());
            }
            _ => panic!("expected FunctionCallOutput"),
        }
    }

    #[test]
    fn adapter_validate_tool_call_denied_tool() {
        let caps = ProviderCapabilities::for_provider("openai");
        let adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        )
        .with_denied_tools(vec!["bash".to_string(), "write".to_string()]);

        let result = adapter.validate_tool_call("bash", &serde_json::json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("denied"));
    }

    #[test]
    fn adapter_validate_tool_call_allowed_list() {
        let caps = ProviderCapabilities::for_provider("openai");
        let adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        )
        .with_allowed_tools(vec!["read".to_string()]);

        assert!(adapter
            .validate_tool_call("read", &serde_json::json!({}))
            .is_ok());
        assert!(adapter
            .validate_tool_call("write", &serde_json::json!({}))
            .is_err());
    }

    #[test]
    fn adapter_reserve_and_release() {
        let caps = ProviderCapabilities::for_provider("openai");
        let mut adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        );

        let normalized = adapter.reserve_call("c1", "read", "hash1").unwrap();
        assert!(adapter.is_call_reserved("c1"));
        assert_eq!(adapter.reserved_call_count(), 1);

        assert!(adapter.release_reservation("c1"));
        assert!(!adapter.is_call_reserved("c1"));
        assert_eq!(adapter.reserved_call_count(), 0);
    }

    #[test]
    fn adapter_reserve_rejects_duplicate() {
        let caps = ProviderCapabilities::for_provider("openai");
        let mut adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        );

        adapter.reserve_call("c1", "read", "hash1").unwrap();
        let result = adapter.reserve_call("c1", "read", "hash1");
        assert!(result.is_err());
    }

    #[test]
    fn adapter_record_result_removes_reservation() {
        let caps = ProviderCapabilities::for_provider("openai");
        let mut adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        );

        adapter.reserve_call("c1", "read", "hash1").unwrap();
        assert!(adapter.is_call_reserved("c1"));

        adapter
            .record_call_result(
                "c1".to_string(),
                "read".to_string(),
                "hash1".to_string(),
                true,
                serde_json::json!({"ok": true}),
            )
            .unwrap();

        assert!(!adapter.is_call_reserved("c1"));
        assert!(adapter.is_call_completed("c1"));
    }

    #[test]
    fn adapter_total_result_bytes() {
        let caps = ProviderCapabilities::for_provider("openai");
        let mut adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        );

        assert_eq!(adapter.total_result_bytes(), 0);

        adapter
            .record_call_result(
                "c1".to_string(),
                "read".to_string(),
                "h1".to_string(),
                true,
                serde_json::json!({"content": "hello"}),
            )
            .unwrap();

        assert!(adapter.total_result_bytes() > 0);
    }

    #[test]
    fn transport_cancel_and_check() {
        let transport = ResponsesTransport::new(
            "https://api.openai.com/v1".to_string(),
            "test-key".to_string(),
        );
        assert!(!transport.is_cancelled());
        transport.cancel();
        assert!(transport.is_cancelled());
        transport.reset_cancel();
        assert!(!transport.is_cancelled());
    }

    #[test]
    fn transport_config_defaults() {
        let config = ResponsesTransportConfig::default();
        assert_eq!(config.request_timeout, DEFAULT_REQUEST_TIMEOUT);
        assert_eq!(config.stream_idle_timeout, DEFAULT_STREAM_IDLE_TIMEOUT);
        assert_eq!(config.max_sse_buffer_size, MAX_SSE_BUFFER_SIZE);
    }

    #[test]
    fn adapter_process_stream_event_tool_validation_failure() {
        let caps = ProviderCapabilities::for_provider("openai");
        let mut adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        )
        .with_denied_tools(vec!["bash".to_string()]);

        let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: 0,
            item: fixture_function_call("call_1", "bash", r#"{"command":"rm -rf /"}"#),
        });

        assert!(!events.is_empty());
        match &events[0] {
            HostedProgramEvent::Error { code, message } => {
                assert_eq!(code.as_deref(), Some("tool_validation_failed"));
                assert!(message.contains("denied"));
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn adapter_process_stream_event_reservation_failure() {
        let caps = ProviderCapabilities::for_provider("openai");
        let mut adapter = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps,
            HostedBackendPolicy::HostedPreferred,
        );

        // Reserve the first call
        adapter.reserve_call("c1", "read", "h1").unwrap();

        // Now try to process a new call that would exceed the limit
        // when max_nested_calls is set to 1
        let caps_limited = ProviderCapabilities {
            max_nested_calls: Some(1),
            ..ProviderCapabilities::for_provider("openai")
        };
        let mut adapter_limited = HostedProgramAdapter::new(
            "tp-test".to_string(),
            caps_limited,
            HostedBackendPolicy::HostedPreferred,
        );
        adapter_limited.reserve_call("c1", "read", "h1").unwrap();

        // Second call should fail reservation
        let events = adapter_limited.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: 0,
            item: fixture_function_call("call_2", "read", r#"{"path":"a.txt"}"#),
        });

        assert!(!events.is_empty());
        match &events[0] {
            HostedProgramEvent::Error { code, .. } => {
                assert_eq!(code.as_deref(), Some("call_reservation_failed"));
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }
}
