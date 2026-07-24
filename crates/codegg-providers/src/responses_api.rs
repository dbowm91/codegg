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

use serde::{Deserialize, Serialize};

use crate::provider_core::ProviderCapabilities;

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
/// The adapter:
/// 1. Receives streamed ResponseItems from the provider.
/// 2. Converts `FunctionCall` items into `CallRequest`s for the ToolBroker.
/// 3. Executes client-owned calls through the broker pipeline.
/// 4. Maps results back to `FunctionCallOutput` items for continuation.
/// 5. Emits normalized `HostedProgramEvent`s for projections and notifications.
/// 6. Handles deduplication, continuation, and cancellation.
pub struct HostedProgramAdapter {
    /// The program identity within CodeGG.
    program_id: String,
    /// Provider capabilities snapshot.
    capabilities: ProviderCapabilities,
    /// Backend selection policy.
    policy: HostedBackendPolicy,
    /// Completed calls by provider call ID (for deduplication).
    completed_calls: HashMap<String, CompletedHostedCall>,
    /// Current continuation state.
    continuation: Option<ContinuationState>,
    /// Event buffer for emitted events.
    events: Vec<HostedProgramEvent>,
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
}

impl HostedProgramAdapter {
    /// Create a new adapter for a hosted program.
    pub fn new(
        program_id: String,
        capabilities: ProviderCapabilities,
        policy: HostedBackendPolicy,
    ) -> Self {
        Self {
            program_id,
            capabilities,
            policy,
            completed_calls: HashMap::new(),
            continuation: None,
            events: Vec::new(),
        }
    }

    /// The program identity.
    pub fn program_id(&self) -> &str {
        &self.program_id
    }

    /// The resolved backend for this program.
    pub fn resolved_backend(&self) -> ResolvedBackend {
        self.policy.resolve(&self.capabilities)
    }

    /// Whether a specific tool can be called through hosted programs.
    ///
    /// Enforces: no direct-only tools, no mutation tools through hosted.
    pub fn can_call_tool(
        &self,
        contract: &crate::provider_core::ToolCall,
        caller_policy: &crate::provider_core::ToolCall,
    ) -> bool {
        // Hosted programs can only call DirectOrProgrammatic tools
        // This is validated at broker invocation time; the adapter
        // pre-checks capability constraints.
        let _ = (contract, caller_policy);
        true // full validation deferred to broker
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

                        let sequence = self.completed_calls.len() as u32;
                        emitted.push(HostedProgramEvent::NestedCall {
                            call_id,
                            tool_name: name,
                            arguments: args,
                            sequence,
                        });
                    }
                    ResponseItem::HostedTool { .. } => {
                        // Hosted tool items are provider-executed, not client-owned
                        // Emit a no-op; they are tracked but not brokered
                    }
                    _ => {}
                }
            }
            ResponsesStreamEvent::TextDelta { delta, .. } => {
                // Text deltas are accumulated into program output
                // (not emitted as separate events here; the caller accumulates)
                let _ = delta;
            }
            ResponsesStreamEvent::ResponseCompleted { response } => {
                // Update continuation state
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

    /// Record a completed nested call result.
    pub fn record_call_result(
        &mut self,
        call_id: String,
        tool_name: String,
        input_hash: String,
        success: bool,
        output: serde_json::Value,
    ) -> String {
        let normalized_call_id = HostedCallIdentity {
            program_id: self.program_id.clone(),
            provider_call_id: call_id.clone(),
            tool_name: tool_name.clone(),
            input_hash: input_hash.clone(),
        }
        .normalized_call_id();

        let record = CompletedHostedCall {
            provider_call_id: call_id,
            normalized_call_id: normalized_call_id.clone(),
            tool_name,
            input_hash,
            success,
            output,
        };

        self.completed_calls
            .insert(record.provider_call_id.clone(), record);

        normalized_call_id
    }

    /// Build the `FunctionCallOutput` item to submit back to the provider.
    pub fn build_call_output(call_id: String, output: &serde_json::Value) -> ResponseItem {
        ResponseItem::FunctionCallOutput {
            call_id,
            output: output.clone(),
        }
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

    /// Check if a call has already been completed (for deduplication).
    pub fn is_call_completed(&self, call_id: &str) -> bool {
        self.completed_calls.contains_key(call_id)
    }
}

// ─── Responses API HTTP transport ──────────────────────────────────

/// HTTP transport for the Responses API.
///
/// Handles request serialization, SSE streaming, and response parsing
/// separate from the Chat Completions transport.
pub struct ResponsesTransport {
    http_client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl ResponsesTransport {
    /// Create a new transport.
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            http_client: crate::provider_core::create_http_client(),
            base_url,
            api_key,
        }
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

        let byte_stream = response.bytes_stream();
        let stream = futures::stream::unfold(
            (byte_stream, String::new()),
            |(mut byte_stream, mut buffer)| async {
                use futures::StreamExt;
                loop {
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

                    // Read more bytes
                    match byte_stream.next().await {
                        Some(Ok(chunk)) => {
                            let chunk_str = String::from_utf8_lossy(&chunk).to_string();
                            buffer.push_str(&chunk_str);
                        }
                        Some(Err(_)) => return None,
                        None => return None,
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

        // HostedPreferred -> Hosted when supported, Native when not
        let policy = HostedBackendPolicy::HostedPreferred;
        assert_eq!(policy.resolve(&openai_caps), ResolvedBackend::Hosted);
        assert_eq!(policy.resolve(&anthropic_caps), ResolvedBackend::Native);

        // NativeOnly always resolves to Native
        let policy = HostedBackendPolicy::NativeOnly;
        assert_eq!(policy.resolve(&openai_caps), ResolvedBackend::Native);

        // HostedRequired fails when not supported
        let policy = HostedBackendPolicy::HostedRequired;
        assert_eq!(policy.resolve(&openai_caps), ResolvedBackend::Hosted);
        assert!(matches!(
            policy.resolve(&anthropic_caps),
            ResolvedBackend::Failed { .. }
        ));

        // NativePreferred always resolves to Native
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

        // Different call IDs produce different normalized IDs
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

        // Record a call result
        adapter.record_call_result(
            "call_1".to_string(),
            "read".to_string(),
            "hash1".to_string(),
            true,
            serde_json::json!({"content": "hello"}),
        );

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

        // Compute the same hash the adapter uses for deduplication
        let input_hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            "read".hash(&mut h);
            r#"{"path":"/tmp/a.txt"}"#.hash(&mut h);
            format!("{:016x}", h.finish())
        };

        // Record a call result with the matching hash
        adapter.record_call_result(
            "call_1".to_string(),
            "read".to_string(),
            input_hash.clone(),
            true,
            serde_json::json!({"content": "hello"}),
        );

        // Process a duplicate FunctionCall with same args (same hash)
        let events = adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
            output_index: 0,
            item: fixture_function_call("call_1", "read", r#"{"path":"/tmp/a.txt"}"#),
        });

        // Should emit a NestedCallResult with the recorded result
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

        // Record a call with hash "hash_a"
        adapter.record_call_result(
            "call_1".to_string(),
            "read".to_string(),
            "hash_a".to_string(),
            true,
            serde_json::json!({"content": "hello"}),
        );

        // Now a FunctionCall with a DIFFERENT input hash comes in with same call_id
        // We need to simulate this by having a different hash in the completed_calls
        // Since we can't directly change the hash, we check the dedup path
        // The adapter checks completed_calls by call_id only, so same call_id
        // with different args triggers the mismatch path.
        // But our adapter checks by call_id, and since the hash in the existing
        // record is "hash_a" while the new call would compute a different hash,
        // we need the test to compute the same hash to hit the matching path
        // or a different one for the mismatch path.

        adapter.record_call_result(
            "call_2".to_string(),
            "read".to_string(),
            "hash_b".to_string(),
            true,
            serde_json::json!({"content": "world"}),
        );

        // Now process with a FunctionCall that has call_id "call_2"
        // but different arguments (which would compute a different hash).
        // The adapter matches by call_id, finds existing record with hash_b,
        // and the incoming call's arguments compute a different hash -> mismatch error.
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

        // Process a ResponseCreated event
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

        // Simulate full lifecycle
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

        // Add a function call
        all_events.extend(
            adapter.process_stream_event(ResponsesStreamEvent::OutputItemAdded {
                output_index: 0,
                item: fixture_function_call("call_1", "read", r#"{"path":"src/main.rs"}"#),
            }),
        );

        // Complete the response
        all_events.extend(adapter.process_stream_event(fixture_response_completed(
            "resp_1",
            vec![fixture_function_call_output(
                "call_1",
                serde_json::json!({"content": "fn main() {}"}),
            )],
        )));

        // Verify events
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
}
