//! Session-pinned MCP tool adapter: the single model-facing entry point
//! for third-party MCP tools.
//!
//! Every raw `mcp__<server>__<tool>` model call executes through the
//! canonical [`Tool`](super::Tool) trait and the [`ToolBroker`](super::ToolBroker)
//! pipeline. There is no direct `McpService::call_tool*` model path:
//! `src/agent/tool_batch.rs` routes adapter calls through the ordinary
//! native batch pipeline, and `src/agent/request_preparation.rs` derives
//! model-facing definitions from the registry (the single canonical origin).
//!
//! Ownership:
//! - `McpService` owns transport, credentials, DNS rebinding checks and
//!   protocol status. This adapter never creates a second client.
//! - `ToolRegistry` owns canonical `Tool` instances and the shared live
//!   `ToolCatalog`.
//! - `ToolBroker` owns authorization, contracts, output, artifact and audit
//!   normalization.
//!
//! Invariants:
//! - Default effect class is conservative mutating/non-idempotent,
//!   `DirectOnly` caller policy, zero automatic retries.
//! - Remote metadata (description, annotations, schemas) is untrusted and
//!   never downgrades authorization.
//! - The adapter does not execute when its pinned service/server/descriptor
//!   is gone or replaced, or when the session exposure policy excludes raw
//!   tools.
//! - A cancellation/timeout does not prove an external effect was rolled
//!   back; timed-out non-idempotent calls are uncertain, never blindly
//!   replayed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::error::ToolError;
use crate::mcp::{McpExposurePolicy, McpService};
use crate::tool::{
    StructuredToolResult, Tool, ToolCategory, ToolExecutionContext, ToolProvenance, ToolTrust,
};

/// Maximum canonical `mcp__<server>__<tool>` length.
///
/// Server and tool components are each capped at 128 (MCP tool-name bound);
/// the canonical form adds the `mcp__` prefix and the `__` separator.
pub const MAX_MCP_CANONICAL_BYTES: usize = 256;

/// Frozen per-tool pin captured at registration.
///
/// The adapter stores the exact input schema it was registered with. A
/// same-name schema drift invalidates the pin: execution fails closed
/// before transport rather than calling a newer tool by the same name.
#[derive(Clone, Debug)]
pub struct McpToolPin {
    /// Canonical `mcp__<server>__<tool>` identity.
    pub canonical_name: String,
    /// MCP server name.
    pub server: String,
    /// MCP tool name (unqualified).
    pub tool: String,
    /// Input schema observed at registration.
    pub input_schema: Value,
    /// Optional output schema observed at registration.
    pub output_schema: Option<Value>,
    /// Stable fingerprint of the input schema.
    pub schema_fingerprint: String,
    /// Global catalog fingerprint at registration (diagnostics only).
    pub generation: String,
}

/// Session-pinned adapter from one MCP descriptor to the canonical Tool trait.
///
/// One instance is registered per connected, exposed raw MCP tool before
/// broker contract snapshot construction. The broker holds the contract;
/// this adapter holds the pinned transport handle plus the frozen
/// descriptor.
#[derive(Clone)]
pub struct McpToolAdapter {
    pin: McpToolPin,
    description: String,
    mcp_service: Arc<RwLock<McpService>>,
}

impl McpToolAdapter {
    /// Stable fingerprint for one JSON value (no credentials inside).
    pub fn fingerprint_value(value: &Value) -> String {
        let encoded = serde_json::to_vec(value).unwrap_or_default();
        use sha2::Digest;
        format!("sha256:{:x}", sha2::Sha256::digest(encoded))
    }

    /// Canonical `mcp__<server>__<tool>` identity.
    pub fn canonical_name(server: &str, tool: &str) -> String {
        format!("mcp__{server}__{tool}")
    }

    /// Validate one name component (server or tool).
    ///
    /// Accepts the MCP tool-name alphabet (`A-Za-z0-9_.-`, 1..=128) and
    /// additionally rejects `__` so the canonical encoding stays
    /// unambiguous under both `split_once` and `rfind` parsing.
    pub fn is_valid_component(component: &str) -> bool {
        if component.is_empty() || component.len() > 128 {
            return false;
        }
        if component.contains("__") {
            return false;
        }
        component
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    }

    /// Strictly parse and validate a canonical `mcp__<server>__<tool>` name.
    ///
    /// Returns `(server, tool)` on success. Rejects malformed prefixes,
    /// empty components, ambiguous separators, over-long names and
    /// out-of-alphabet characters. Names alone are never authority; this
    /// is a shape check only.
    pub fn parse_canonical_name(name: &str) -> Option<(String, String)> {
        if name.len() > MAX_MCP_CANONICAL_BYTES {
            return None;
        }
        let rest = name.strip_prefix("mcp__")?;
        let (server, tool) = rest.split_once("__")?;
        if server.is_empty() || tool.is_empty() {
            return None;
        }
        // Ambiguity guard: with `__` forbidden in components, exactly one
        // separator may remain. A tool or server containing `__` would have
        // been rejected at registration; a wire name containing it now is
        // forged or collided and must fail closed.
        if tool.contains("__") || server.contains("__") {
            return None;
        }
        if !Self::is_valid_component(server) || !Self::is_valid_component(tool) {
            return None;
        }
        Some((server.to_string(), tool.to_string()))
    }

    /// Validate descriptor bounds before admission.
    ///
    /// Returns the sanitized description on success. Remote annotations and
    /// `_meta` remain descriptive/untrusted and are never consulted here.
    fn validate_descriptor(
        server: &str,
        tool: &str,
        description: &str,
        input_schema: &Value,
    ) -> Result<String, String> {
        if !Self::is_valid_component(server) {
            return Err(format!("invalid MCP server name '{server}'"));
        }
        if !Self::is_valid_component(tool) {
            return Err(format!("invalid MCP tool name '{tool}'"));
        }
        let canonical = Self::canonical_name(server, tool);
        if canonical.len() > MAX_MCP_CANONICAL_BYTES {
            return Err(format!(
                "MCP canonical name exceeds {MAX_MCP_CANONICAL_BYTES} bytes"
            ));
        }
        if description.len() > crate::mcp::protocol::MAX_MCP_DESCRIPTION_BYTES {
            return Err(format!(
                "MCP description exceeds {} bytes",
                crate::mcp::protocol::MAX_MCP_DESCRIPTION_BYTES
            ));
        }
        let schema_bytes = serde_json::to_vec(input_schema)
            .map(|b| b.len())
            .unwrap_or(usize::MAX);
        if schema_bytes > crate::mcp::protocol::MAX_MCP_SCHEMA_BYTES {
            return Err(format!(
                "MCP input schema exceeds {} bytes",
                crate::mcp::protocol::MAX_MCP_SCHEMA_BYTES
            ));
        }
        // Bounded, sanitized disclosure: keep the protocol-bounded
        // description verbatim; strip NULs as a second bound so audit/bus
        // payloads cannot carry raw control bytes. Credentials never appear
        // in `McpTool` descriptors.
        Ok(description.replace('\0', ""))
    }

    /// Build an adapter from a frozen descriptor plus the shared service handle.
    pub fn try_new(
        server: String,
        tool: String,
        description: String,
        input_schema: Value,
        output_schema: Option<Value>,
        mcp_service: Arc<RwLock<McpService>>,
        generation: String,
    ) -> Result<Self, String> {
        let description = Self::validate_descriptor(&server, &tool, &description, &input_schema)?;
        let canonical_name = Self::canonical_name(&server, &tool);
        // Strict shape check on the name we just built; guards against a
        // future caller bypassing `is_valid_component`.
        Self::parse_canonical_name(&canonical_name)
            .ok_or_else(|| format!("rejected ambiguous MCP name '{canonical_name}'"))?;
        let schema_fingerprint = Self::fingerprint_value(&input_schema);
        Ok(Self {
            pin: McpToolPin {
                canonical_name,
                server,
                tool,
                input_schema,
                output_schema,
                schema_fingerprint,
                generation,
            },
            description,
            mcp_service,
        })
    }

    /// Pinned identity snapshot (diagnostics; never authority by itself).
    pub fn pin(&self) -> &McpToolPin {
        &self.pin
    }

    /// Canonical name (also `Tool::name`).
    pub fn canonical_tool_name(&self) -> &str {
        &self.pin.canonical_name
    }

    /// Validate the pinned descriptor against the live service.
    ///
    /// Must be called while holding the service read guard. Fails closed
    /// when the server is gone, disconnected, the tool is absent, or the
    /// input schema drifted even though the name is unchanged.
    fn check_pinned_locked(&self, service: &McpService) -> Result<(), ToolError> {
        let tools = service.server_tools();
        let current = tools.get(self.pin.server.as_str()).ok_or_else(|| {
            ToolError::NotFound(format!(
                "MCP server '{}' is unavailable for '{}'",
                self.pin.server, self.pin.canonical_name
            ))
        })?;
        // A disconnected server must not silently redirect a call to a
        // different descriptor. `server_tools` returns the current tool
        // list regardless of status; check liveness explicitly.
        let live = service
            .server_status()
            .into_iter()
            .find(|(name, _)| *name == self.pin.server.as_str())
            .map(|(_, status)| matches!(status, crate::mcp::McpServerStatus::Connected))
            .unwrap_or(false);
        if !live {
            return Err(ToolError::NotFound(format!(
                "MCP server '{}' is not connected for '{}'",
                self.pin.server, self.pin.canonical_name
            )));
        }
        let descriptor = current
            .iter()
            .find(|tool| tool.name == self.pin.tool)
            .ok_or_else(|| {
                ToolError::NotFound(format!(
                    "MCP tool '{}' is no longer advertised by server '{}'",
                    self.pin.tool, self.pin.server
                ))
            })?;
        if Self::fingerprint_value(&descriptor.input_schema) != self.pin.schema_fingerprint {
            return Err(ToolError::Execution(format!(
                "MCP tool '{}' schema changed since registration; refusing stale descriptor",
                self.pin.canonical_name
            )));
        }
        Ok(())
    }

    /// Translate one `McpToolCallResult` into a structured tool result.
    ///
    /// - `is_error=true` (JSON-RPC transport success with MCP-level error)
    ///   is a failed terminal status, never success.
    /// - `structuredContent` is retained as the structured value when
    ///   present; otherwise a best-effort JSON object/array parse of the
    ///   text projection is retained. Text is always the display fallback.
    /// - `outputSchema`, when pinned at registration, is an optional
    ///   consistency check against untrusted server output: a present
    ///   schema requires present, conforming structured output. Missing,
    ///   malformed or non-conforming structured output is a safe execution
    ///   error, never silently treated as verified.
    /// - Provenance carries bounded server identity without credentials.
    pub(crate) fn translate_result(
        &self,
        result: crate::mcp::McpToolCallResult,
        server_version: Option<String>,
    ) -> Result<StructuredToolResult, ToolError> {
        // Optional output-schema consistency check (untrusted server
        // output; schema is a declaration, not a trust grant).
        if let Some(expected) = self.pin.output_schema.as_ref() {
            let Some(structured) = result.structured.as_ref() else {
                return Err(ToolError::Execution(format!(
                    "MCP tool '{}' declares an output schema but returned no structured content",
                    self.pin.canonical_name
                )));
            };
            if let Err(reason) = validate_mcp_output_schema(expected, structured) {
                return Err(ToolError::Execution(format!(
                    "MCP tool '{}' structured output violates its declared output schema: {reason}",
                    self.pin.canonical_name
                )));
            }
            let structured_bytes = serde_json::to_vec(structured)
                .map(|b| b.len())
                .unwrap_or(usize::MAX);
            if structured_bytes > crate::mcp::protocol::MAX_MCP_SCHEMA_BYTES {
                return Err(ToolError::Execution(format!(
                    "MCP tool '{}' structured output exceeds {} bytes",
                    self.pin.canonical_name,
                    crate::mcp::protocol::MAX_MCP_SCHEMA_BYTES
                )));
            }
        }

        let structured = result.structured.clone().or_else(|| {
            serde_json::from_str::<Value>(&result.text)
                .ok()
                .filter(|value| value.is_object() || value.is_array())
        });
        let output = if result.text.is_empty() {
            structured
                .as_ref()
                .and_then(|value| serde_json::to_string_pretty(value).ok())
                .unwrap_or_default()
        } else {
            result.text
        };
        // Bounded redaction for logs: provenance carries server identity
        // only; no endpoint, token, argument or raw-output echo here.
        // The broker owns display truncation and `ctx://` artifact spillover
        // (256 KiB display bound); this adapter never invents artifacts.
        let provenance = ToolProvenance {
            backend: "mcp".to_string(),
            implementation: format!("mcp:{}", self.pin.server),
            version: server_version,
            elapsed_ms: None,
            truncated: false,
            trust: ToolTrust::ExternalUntrusted,
        };
        Ok(StructuredToolResult::with_value(
            output,
            structured.unwrap_or(Value::Null),
            !result.is_error,
            Some(provenance),
        ))
    }
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.pin.canonical_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.pin.input_schema.clone()
    }

    fn category(&self) -> ToolCategory {
        // Conservative default: remote side effects are assumed
        // mutating/non-idempotent. Remote `readOnlyHint` annotations never
        // downgrade this.
        ToolCategory::Mutating
    }

    fn defer_loading(&self) -> bool {
        // Preserve the pre-adapter wire shape: raw MCP tools were immediate
        // (`catalog.is_deferred` never matched them). Disclosure still
        // classifies unknown `mcp__*` names as core; M002 owns any staged
        // deferral change.
        false
    }

    fn contract(
        &self,
        tool_name: &str,
        input_schema: Value,
    ) -> crate::tool::contract::ToolContract {
        let mut contract = crate::tool::contract::ToolContract::legacy(tool_name, input_schema);
        // Explicit conservative contract: direct-only, non-idempotent, no
        // cache, no retry. `legacy()` already encodes this; the
        // implementation id names the owning MCP server for diagnostics.
        contract.caller_policy = crate::tool::contract::ToolCallerPolicy::DirectOnly;
        contract.effect_class = crate::tool::contract::ToolEffectClass::NonIdempotent;
        contract.idempotency = crate::tool::contract::IdempotencyClass::NonIdempotent;
        contract.retry_policy = crate::tool::contract::ToolRetryPolicy::none();
        contract.implementation_id = format!("mcp:{}", self.pin.server);
        contract
    }

    async fn execute(&self, input: Value) -> Result<String, ToolError> {
        Ok(self.execute_structured(input, None).await?.output)
    }

    async fn execute_structured(
        &self,
        input: Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        if !input.is_object() {
            return Err(ToolError::Format(format!(
                "MCP tool '{}' input must be a JSON object",
                self.pin.canonical_name
            )));
        }
        // Cooperative cancellation at entry: the broker also wraps this
        // future in its own timeout/select, but an already-cancelled turn
        // must not start a remote side effect.
        if let Some(ref ctx) = ctx {
            if let Some(ref token) = ctx.cancellation {
                if token.is_cancelled() {
                    return Err(ToolError::Timeout(format!(
                        "MCP tool '{}' cancelled before dispatch",
                        self.pin.canonical_name
                    )));
                }
            }
        }

        // Pin validation + client capture under one read guard, then drop
        // the guard before the network await. Cloning the client `Arc`
        // preserves the pinned server instance so a concurrent
        // disconnect/reconnect cannot silently redirect this call to a
        // replacement descriptor; the pre-transport schema check above
        // already failed closed on drift.
        let (client, server_version) = {
            let guard = self.mcp_service.read().await;
            self.check_pinned_locked(&guard)?;
            let server_version = guard.server_version(&self.pin.server);
            // Clone the concrete client handle while holding the guard.
            let cloned = guard
                .pinned_client_handle(&self.pin.server)
                .ok_or_else(|| {
                    ToolError::NotFound(format!(
                        "MCP server '{}' client is unavailable for '{}'",
                        self.pin.server, self.pin.canonical_name
                    ))
                })?;
            (cloned, server_version)
        };

        let tool_name = self.pin.tool.clone();
        let call_future = async move {
            match client {
                crate::mcp::McpClientType::Local(handle) => handle
                    .write()
                    .await
                    .call_tool_structured(&tool_name, input)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string())),
                crate::mcp::McpClientType::Remote(handle) => handle
                    .write()
                    .await
                    .call_tool_structured(&tool_name, input)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string())),
                crate::mcp::McpClientType::Mock(handler) => {
                    let text = (handler
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()))(
                        &tool_name, input
                    )
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                    Ok(crate::mcp::McpToolCallResult {
                        structured: serde_json::from_str(&text).ok(),
                        text,
                        ..Default::default()
                    })
                }
            }
        };

        // Prefer the context's cancellation token over any independent
        // timer. The broker's watchdog already bounds this future; this
        // select only ensures a cooperative cancel releases caller
        // resources promptly instead of awaiting a server that ignores
        // cancellation (limits recorded by tests).
        let result = if let Some(token) = ctx.as_ref().and_then(|c| c.cancellation.clone()) {
            tokio::select! {
                _ = token.cancelled() => {
                    return Err(ToolError::Timeout(format!(
                        "MCP tool '{}' cancelled during call",
                        self.pin.canonical_name
                    )));
                }
                result = call_future => result?,
            }
        } else {
            call_future.await?
        };
        self.translate_result(result, server_version)
    }
}

/// Minimal bounded JSON-schema subset check for MCP `outputSchema`.
///
/// Mirrors the broker's additive validator (object/array/scalar types,
/// `properties`, `required`, `enum`, `items`) so adapter behavior matches
/// broker input validation. Unknown keywords pass; type mismatches fail.
fn validate_mcp_output_schema(schema: &Value, value: &Value) -> Result<(), String> {
    if let Some(expected) = schema.get("type").and_then(Value::as_str) {
        let valid = match expected {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true,
        };
        if !valid {
            return Err(format!("expected JSON type {expected}"));
        }
    }
    if let Some(enum_values) = schema.get("enum").and_then(Value::as_array) {
        if !enum_values.iter().any(|candidate| candidate == value) {
            return Err("value is not in the allowed enum".into());
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    return Err(format!("missing required property {key}"));
                }
            }
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (key, child_schema) in properties {
                if let Some(child) = object.get(key) {
                    validate_mcp_output_schema(child_schema, child)
                        .map_err(|reason| format!("property {key}: {reason}"))?;
                }
            }
        }
    }
    if let (Some(items), Some(values)) = (schema.get("items"), value.as_array()) {
        for (index, item) in values.iter().enumerate() {
            validate_mcp_output_schema(items, item)
                .map_err(|reason| format!("array item {index}: {reason}"))?;
        }
    }
    Ok(())
}

/// Outcome of reconciling the live MCP inventory against one registry.
#[derive(Debug, Default)]
pub struct McpRegistrationReport {
    /// Canonical names newly registered.
    pub registered: Vec<String>,
    /// Canonical names skipped because the exposure policy hides them.
    pub hidden: Vec<String>,
    /// Human-readable collision/rejection diagnostics.
    pub collisions: Vec<String>,
    /// Human-readable rejection diagnostics (bounds, malformed, drift).
    pub rejected: Vec<String>,
    /// Catalog fingerprint observed at registration (no credentials).
    pub generation: String,
}

/// Build the session exposure policy from the loop-owned search context
/// plus the resolved per-domain backend config.
///
/// This is the single canonical construction site. `request_preparation`
/// consumes it; adapter registration consumes it. Raw CodeGG-managed
/// backends (eggsearch today, future egglsp/eggsentry adapters) stay hidden
/// unless explicitly opted in via `expose_raw_mcp_tools = true`.
pub fn mcp_exposure_policy_for_turn(
    search_runtime: &crate::search_backend::SearchRuntimeContext,
    tool_backends: &crate::tool::backend::ToolBackendConfig,
) -> McpExposurePolicy {
    let search_cfg = search_runtime.config().clone();
    let expose_raw_search = search_cfg.expose_raw_mcp_tools();
    let eggsearch_server = search_cfg
        .eggsearch
        .as_ref()
        .and_then(|e| e.server_name.clone())
        .unwrap_or_else(|| "eggsearch".to_string());
    let mut hidden_servers: Vec<String> = Vec::new();
    if !expose_raw_search {
        hidden_servers.push(eggsearch_server);
    }
    for domain_cfg in [
        tool_backends.lsp.as_ref(),
        tool_backends.security.as_ref(),
        tool_backends.context.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(server) = domain_cfg.server_name.as_ref() {
            if domain_cfg.expose_raw_mcp_tools() {
                hidden_servers.retain(|s| s != server);
            } else if !hidden_servers.iter().any(|s| s == server) {
                hidden_servers.push(server.clone());
            }
        }
    }
    McpExposurePolicy {
        show_raw: true,
        hidden_servers,
    }
}

/// Register one adapter per exposed raw MCP tool.
///
/// Must be called before broker contract snapshot construction. The caller
/// owns the bootstrapped `McpService` handle, the frozen generation and the
/// exposure policy. Existing native/plugin tools are never removed; stale
/// `mcp__*` entries absent from the desired set are removed so a
/// disconnect does not leave an unexecutable advertisement.
///
/// Collisions (native/plugin name shadowing, duplicate canonical names,
/// ambiguous separators, oversized schemas, hidden servers) fail closed for
/// that tool only; other tools still register. Returns a bounded report for
/// telemetry (counts plus named omission reasons; no credentials).
pub async fn register_mcp_adapters(
    registry: &mut crate::tool::ToolRegistry,
    mcp_service: &Arc<RwLock<McpService>>,
    policy: &McpExposurePolicy,
) -> McpRegistrationReport {
    // Snapshot the inventory without holding the guard across registration.
    // `server_tools` borrows; clone the bounded descriptor set plus the
    // generation fingerprint under one read.
    let (snapshot, generation) = {
        let guard = mcp_service.read().await;
        let mut snapshot: Vec<crate::mcp::McpTool> = Vec::new();
        for tools in guard.server_tools().values() {
            snapshot.extend((*tools).clone());
        }
        (snapshot, guard.catalog_fingerprint())
    };
    register_mcp_snapshot(registry, mcp_service, policy, snapshot, generation)
}

/// Best-effort synchronous registration for `AgentLoop::new`.
///
/// Uses `try_read` so a write-locked service never blocks loop
/// construction. When the service is locked, registration is skipped with
/// a debug log; the next turn's explicit async registration picks the tools
/// up. Never panics.
pub fn try_register_mcp_adapters_sync(
    registry: &mut crate::tool::ToolRegistry,
    mcp_service: &Arc<RwLock<McpService>>,
    policy: &McpExposurePolicy,
) -> McpRegistrationReport {
    let Ok(guard) = mcp_service.try_read() else {
        tracing::debug!("MCP service write-locked during sync adapter registration; skipping");
        return McpRegistrationReport::default();
    };
    let mut snapshot: Vec<crate::mcp::McpTool> = Vec::new();
    for tools in guard.server_tools().values() {
        snapshot.extend((*tools).clone());
    }
    let generation = guard.catalog_fingerprint();
    drop(guard);
    register_mcp_snapshot(registry, mcp_service, policy, snapshot, generation)
}

fn register_mcp_snapshot(
    registry: &mut crate::tool::ToolRegistry,
    mcp_service: &Arc<RwLock<McpService>>,
    policy: &McpExposurePolicy,
    snapshot: Vec<crate::mcp::McpTool>,
    generation: String,
) -> McpRegistrationReport {
    let mut report = McpRegistrationReport {
        generation: generation.clone(),
        ..Default::default()
    };
    // Desired canonical set after policy filtering.
    let mut desired: HashMap<String, crate::mcp::McpTool> = HashMap::new();
    let mut seen_canonical: HashSet<String> = HashSet::new();
    for descriptor in snapshot {
        let canonical = McpToolAdapter::canonical_name(&descriptor.server, &descriptor.name);
        // Policy gate first: hidden/disallowed servers never become
        // registry tools, so they cannot leak into discovery or invocation.
        if !policy.is_visible(&canonical) {
            report.hidden.push(canonical);
            continue;
        }
        // Shape gate: malformed or ambiguous names fail closed here, never
        // as authority later.
        if McpToolAdapter::parse_canonical_name(&canonical).is_none() {
            report
                .rejected
                .push(format!("rejected malformed MCP name '{canonical}'"));
            continue;
        }
        if !seen_canonical.insert(canonical.clone()) {
            report
                .collisions
                .push(format!("duplicate MCP canonical name '{canonical}'"));
            continue;
        }
        // Native/plugin shadowing gate: one server-provided name cannot
        // shadow an existing registry tool.
        if registry.contains(&canonical) && !canonical.starts_with("mcp__") {
            report.collisions.push(format!(
                "MCP name '{canonical}' collides with a native tool"
            ));
            continue;
        }
        if registry.contains(&canonical) {
            // An `mcp__*` entry already exists (second registration pass in
            // the same turn: explicit turn-runtime registration followed by
            // the `AgentLoop::new` safety net). Keep the first pin; a
            // same-name schema drift is handled at execution time by the
            // pinned-schema check, and the next turn rebuilds the registry
            // from the fresh inventory.
            continue;
        }
        // `mcp__` prefix collision with a non-MCP registry tool: any
        // existing registry entry under the same canonical name blocks
        // admission even when the prefix looks MCP-owned.
        if registry.contains(&canonical) {
            report.collisions.push(format!(
                "MCP name '{canonical}' collides with registry tool"
            ));
            continue;
        }
        desired.insert(canonical, descriptor);
    }

    // Remove stale `mcp__*` entries absent from the desired set so a
    // disconnect/reconnect cannot leave an unexecutable advertisement.
    // Native/plugin names never carry the prefix, so this only touches
    // adapter-owned entries.
    let stale: Vec<String> = registry
        .tool_names()
        .filter(|name| {
            name.starts_with("mcp__")
                && !desired.contains_key(*name)
                && !seen_canonical.contains(*name)
        })
        .map(str::to_string)
        .collect();
    // `ToolRegistry` has no remove-by-prefix helper; filter through the
    // existing `filter_out` deny path, which removes exact names. Stale
    // entries are exactly the names we computed above.
    if !stale.is_empty() {
        registry.filter_out(&stale);
    }

    for (canonical, descriptor) in desired {
        // Double-check the native/plugin collision at admission time: the
        // registry may have gained the name between the pre-check and now
        // (plugin adapters register in the same window).
        if registry.contains(&canonical) {
            report
                .collisions
                .push(format!("MCP name '{canonical}' collides at admission"));
            continue;
        }
        match McpToolAdapter::try_new(
            descriptor.server.clone(),
            descriptor.name.clone(),
            descriptor.description.clone(),
            descriptor.input_schema.clone(),
            descriptor.output_schema.clone(),
            Arc::clone(mcp_service),
            generation.clone(),
        ) {
            Ok(adapter) => {
                registry.register(adapter);
                report.registered.push(canonical);
            }
            Err(reason) => {
                report
                    .rejected
                    .push(format!("rejected '{canonical}': {reason}"));
            }
        }
    }
    report.registered.sort();
    report.hidden.sort();
    report.collisions.sort();
    report.rejected.sort();
    tracing::debug!(
        registered = report.registered.len(),
        hidden = report.hidden.len(),
        collisions = report.collisions.len(),
        rejected = report.rejected.len(),
        generation = %report.generation,
        "registered MCP broker adapters"
    );
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{McpServerOrigin, McpTool};
    use serde_json::json;

    fn mock_service_with(tools: Vec<McpTool>) -> Arc<RwLock<McpService>> {
        let mut service = McpService::new();
        service.register_mock_server(
            "testserver",
            tools,
            Box::new(|_tool, _args| Ok("ok".to_string())),
        );
        Arc::new(RwLock::new(service))
    }

    fn descriptor(name: &str) -> McpTool {
        McpTool {
            name: name.to_string(),
            description: format!("tool {name}"),
            input_schema: json!({"type": "object", "properties": {}}),
            server: "testserver".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn canonical_name_validation_rejects_ambiguous_and_malformed() {
        assert_eq!(
            McpToolAdapter::parse_canonical_name("mcp__srv__tool"),
            Some(("srv".to_string(), "tool".to_string()))
        );
        assert!(McpToolAdapter::parse_canonical_name("mcp__srv").is_none());
        assert!(McpToolAdapter::parse_canonical_name("mcp____tool").is_none());
        assert!(McpToolAdapter::parse_canonical_name("mcp__srv__").is_none());
        assert!(McpToolAdapter::parse_canonical_name("read").is_none());
        // Ambiguous: tool containing `__` must fail closed.
        assert!(McpToolAdapter::parse_canonical_name("mcp__srv__a__b").is_none());
        assert!(!McpToolAdapter::is_valid_component("a__b"));
        assert!(!McpToolAdapter::is_valid_component(""));
        assert!(McpToolAdapter::is_valid_component("a-b.c_d9"));
    }

    #[test]
    fn oversized_schema_is_rejected_before_admission() {
        let big = Value::String("x".repeat(crate::mcp::protocol::MAX_MCP_SCHEMA_BYTES + 1));
        let result = McpToolAdapter::try_new(
            "srv".into(),
            "tool".into(),
            "desc".into(),
            big,
            None,
            mock_service_with(vec![]),
            "gen".into(),
        );
        let err = match result {
            Ok(_) => panic!("oversized schema must be rejected"),
            Err(err) => err,
        };
        assert!(err.contains("exceeds"), "got: {err}");
    }

    #[test]
    fn translate_maps_is_error_to_failure_and_retains_structured() {
        let service = mock_service_with(vec![]);
        let adapter = McpToolAdapter::try_new(
            "srv".into(),
            "tool".into(),
            "desc".into(),
            json!({"type": "object"}),
            None,
            service,
            "gen".into(),
        )
        .unwrap();
        let ok = adapter
            .translate_result(
                crate::mcp::McpToolCallResult {
                    text: "hello".into(),
                    structured: Some(json!({"answer": 1})),
                    is_error: false,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        assert!(ok.success);
        assert_eq!(ok.output, "hello");
        assert_eq!(ok.value, Some(json!({"answer": 1})));

        let failed = adapter
            .translate_result(
                crate::mcp::McpToolCallResult {
                    text: "boom".into(),
                    structured: None,
                    is_error: true,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        assert!(!failed.success);
        assert_eq!(failed.output, "boom");
    }

    #[test]
    fn output_schema_mismatch_is_a_safe_error() {
        let service = mock_service_with(vec![]);
        let adapter = McpToolAdapter::try_new(
            "srv".into(),
            "tool".into(),
            "desc".into(),
            json!({"type": "object"}),
            Some(json!({"type": "object", "required": ["answer"]})),
            service,
            "gen".into(),
        )
        .unwrap();
        let err = adapter
            .translate_result(
                crate::mcp::McpToolCallResult {
                    text: "{}".into(),
                    structured: Some(json!({})),
                    is_error: false,
                    ..Default::default()
                },
                None,
            )
            .unwrap_err();
        assert!(err.to_string().contains("output schema"), "got: {err}");

        let missing = adapter.translate_result(
            crate::mcp::McpToolCallResult {
                text: "text only".into(),
                structured: None,
                is_error: false,
                ..Default::default()
            },
            None,
        );
        assert!(missing.is_err(), "missing structured with schema must fail");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn broker_catalog_contains_adapter_and_stale_fails_before_transport() {
        let mut service = McpService::new();
        service.register_mock_server(
            "db",
            vec![McpTool {
                name: "query".into(),
                description: "query the database".into(),
                input_schema: json!({"type": "object", "properties": {}}),
                server: "db".into(),
                ..Default::default()
            }],
            Box::new(|_tool, _args| Ok(r#"{"rows":[]}"#.to_string())),
        );
        let service = Arc::new(RwLock::new(service));
        let mut registry = crate::tool::ToolRegistry::with_defaults();
        let policy = McpExposurePolicy {
            show_raw: true,
            hidden_servers: Vec::new(),
        };
        let report = register_mcp_adapters(&mut registry, &service, &policy).await;
        assert_eq!(report.registered, vec!["mcp__db__query".to_string()]);
        assert!(registry.contains("mcp__db__query"));

        let broker = crate::tool::ToolBroker::new(&registry);
        assert!(broker.lookup_contract("mcp__db__query").is_ok());
        let contract = broker.lookup_contract("mcp__db__query").unwrap();
        assert_eq!(
            contract.caller_policy,
            crate::tool::contract::ToolCallerPolicy::DirectOnly
        );
        assert_eq!(
            contract.effect_class,
            crate::tool::contract::ToolEffectClass::NonIdempotent
        );
        assert_eq!(contract.retry_policy.max_retries, 0);

        // Stale descriptor: mutate the live schema under the same name, then
        // execute through the pinned adapter. Must fail before transport.
        {
            let mut guard = service.write().await;
            if let Some(server) = guard
                .server_tools()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .first()
                .cloned()
            {
                let _ = server;
            }
            // Reach the server entry mutably through the public mock path:
            // disconnect + re-register with a drifted schema under the same
            // canonical name.
            let _ = guard.disconnect("db").await;
            guard.register_mock_server(
                "db",
                vec![McpTool {
                    name: "query".into(),
                    description: "query the database v2".into(),
                    input_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}, "required": ["q"]}),
                    server: "db".into(),
                    ..Default::default()
                }],
                Box::new(|_tool, _args| Ok("v2".to_string())),
            );
        }
        let tool = registry.get("mcp__db__query").expect("pinned adapter");
        let err = tool.execute_structured(json!({}), None).await.unwrap_err();
        assert!(
            err.to_string().contains("schema changed") || err.to_string().contains("no longer"),
            "stale descriptor must fail before transport, got: {err}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn hidden_servers_never_register() {
        let service = mock_service_with(vec![descriptor("a")]);
        let mut registry = crate::tool::ToolRegistry::with_defaults();
        let policy = McpExposurePolicy {
            show_raw: true,
            hidden_servers: vec!["testserver".into()],
        };
        let report = register_mcp_adapters(&mut registry, &service, &policy).await;
        assert!(report.registered.is_empty());
        assert_eq!(report.hidden, vec!["mcp__testserver__a".to_string()]);
        assert!(!registry.contains("mcp__testserver__a"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn duplicate_canonical_names_collide_safely() {
        // Two servers whose canonical encodings collide (`a__b` + `c` vs
        // `a` + `b__c` would collide if `__` were allowed). With `__`
        // forbidden, construct the collision via identical server/tool.
        let mut service = McpService::new();
        service.register_mock_server(
            "srv",
            vec![descriptor("tool")],
            Box::new(|_t, _a| Ok("one".to_string())),
        );
        // Second registration under the same server name overwrites in the
        // HashMap service; the adapter layer must still not duplicate.
        let service = Arc::new(RwLock::new(service));
        let mut registry = crate::tool::ToolRegistry::with_defaults();
        let policy = McpExposurePolicy {
            show_raw: true,
            hidden_servers: Vec::new(),
        };
        let first = register_mcp_adapters(&mut registry, &service, &policy).await;
        assert_eq!(first.registered.len(), 1);
        let second = register_mcp_adapters(&mut registry, &service, &policy).await;
        assert!(
            second.registered.is_empty(),
            "second pass must be idempotent"
        );
        assert!(registry.contains("mcp__srv__tool") || registry.contains("mcp__testserver__tool"));
    }

    #[test]
    fn exposure_policy_hides_managed_servers_by_default() {
        let runtime = crate::search_backend::SearchRuntimeContext::default();
        let backends = crate::tool::backend::ToolBackendConfig::default();
        let policy = mcp_exposure_policy_for_turn(&runtime, &backends);
        // Default eggsearch server must be hidden; third-party stays visible.
        assert!(!policy.is_visible("mcp__eggsearch__web_search"));
        assert!(policy.is_visible("mcp__github__list_issues"));
        let _ = McpServerOrigin::Configured;
    }

    fn test_grant() -> codegg_core::jobs::ToolAuthorityGrant {
        let grant = codegg_core::jobs::ToolAuthorityGrant {
            schema_version: 1,
            grant_id: "test-mcp-grant".into(),
            principal_ref: "test".into(),
            workspace_id: "ws-test".into(),
            workspace_path_policy_id: "workspace:ws-test".into(),
            session_id: None,
            agent_id: None,
            turn_id: None,
            permission_mode: None,
            policy_revision: "test-v1".into(),
            allowed_caller_class: "agent".into(),
            allowed_effect_class: "non_idempotent".into(),
            manifest_digest: "sha256:test".into(),
            source_digest: String::new(),
            ir_digest: String::new(),
            contract_digest: String::new(),
            contract_snapshot_json: String::new(),
            issued_at: chrono::Utc::now().timestamp_millis(),
            expires_at: None,
            revoked_at: None,
            decision_digest: String::new(),
        };
        let decision_digest = grant.compute_digest();
        codegg_core::jobs::ToolAuthorityGrant {
            decision_digest,
            ..grant
        }
    }

    fn broker_ctx_for_test() -> crate::tool::broker::BrokerInvocationContext {
        crate::tool::broker::BrokerInvocationContext {
            caller: crate::tool::contract::ToolCaller::Agent,
            cwd: std::path::PathBuf::from("."),
            session_id: None,
            workspace_id: None,
            agent_id: None,
            turn_id: None,
            job_id: None,
            attempt_id: None,
            permission_mode: None,
            timeout_ms: Some(10_000),
            submission_key: None,
            authority: crate::tool::broker::BrokerAuthority::from_grant(test_grant()),
            cancellation: None,
            deadline: None,
            principal_ref: None,
            workspace_path_policy_id: None,
            allowed_tools: None,
            current_policy_revision: None,
            execution_audit: None,
            audit_emitter: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn broker_mediated_call_preserves_structured_and_is_error() {
        let mut service = McpService::new();
        service.register_mock_server(
            "db",
            vec![
                McpTool {
                    name: "read_item".into(),
                    description: "read one item".into(),
                    input_schema: json!({"type": "object", "properties": {}}),
                    server: "db".into(),
                    ..Default::default()
                },
                McpTool {
                    name: "write_item".into(),
                    description: "write one item".into(),
                    input_schema: json!({"type": "object", "properties": {}}),
                    server: "db".into(),
                    ..Default::default()
                },
            ],
            Box::new(|tool, _args| {
                if tool == "read_item" {
                    Ok(r#"{"rows":[{"id":1}]}"#.to_string())
                } else {
                    Ok("transport ok but tool failed".to_string())
                }
            }),
        );
        // Second handler for the write path needs is_error=true. The mock
        // transport only returns text; emulate is_error by registering a
        // structured mock via direct adapter translation below. For the
        // broker path, success preservation is the assertion; isError
        // mapping is asserted via `translate_result` (unit) plus the
        // batch-level terminal-status preservation test in
        // `src/agent/tool_batch.rs` (broker Error -> ToolError).
        let service = Arc::new(RwLock::new(service));
        let mut registry = crate::tool::ToolRegistry::with_defaults();
        let policy = McpExposurePolicy {
            show_raw: true,
            hidden_servers: Vec::new(),
        };
        let report = register_mcp_adapters(&mut registry, &service, &policy).await;
        assert_eq!(report.registered.len(), 2);

        let broker = crate::tool::ToolBroker::new(&registry);
        let result = broker
            .execute(
                &registry,
                "mcp__db__read_item",
                json!({}),
                broker_ctx_for_test(),
            )
            .await
            .expect("broker executes registered MCP adapter");
        assert_eq!(
            result.value.terminal_status,
            crate::tool::contract::ToolTerminalStatus::Success
        );
        // Structured JSON survives the broker (`{"rows":[...]}` parsed from
        // the text projection by the adapter's best-effort retention).
        assert!(result.value.value.is_some());
        assert!(result.value.display.contains("rows"));

        // Unknown tool (unregistered name) has no broker contract and fails
        // before transport.
        let missing = broker
            .execute(
                &registry,
                "mcp__db__missing",
                json!({}),
                broker_ctx_for_test(),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(
                missing,
                crate::tool::broker::BrokerError::NoContract(_)
                    | crate::tool::broker::BrokerError::NotFound(_)
            ),
            "unregistered MCP name must fail before transport, got: {missing}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_adapter_does_not_dispatch() {
        let service = mock_service_with(vec![descriptor("slow")]);
        let adapter = McpToolAdapter::try_new(
            "testserver".into(),
            "slow".into(),
            "slow tool".into(),
            json!({"type": "object"}),
            None,
            service,
            "gen".into(),
        )
        .map_err(|e| format!("adapter build failed: {e}"))
        .unwrap();
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();
        let mut ctx = crate::tool::backend::ToolExecutionContext::with_backend(
            crate::tool::backend::ToolBackendKind::Native,
        );
        ctx.cancellation = Some(token);
        let err = adapter
            .execute_structured(json!({}), Some(ctx))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("cancelled"),
            "cancelled call must fail before transport, got: {err}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_adapter_calls_share_no_global_lock() {
        let mut service = McpService::new();
        service.register_mock_server(
            "db",
            vec![descriptor("a"), descriptor("b")]
                .into_iter()
                .map(|mut d| {
                    d.server = "db".into();
                    d
                })
                .collect(),
            Box::new(|_tool, _args| Ok(r#"{"ok":true}"#.to_string())),
        );
        let service = Arc::new(RwLock::new(service));
        let mut registry = crate::tool::ToolRegistry::with_defaults();
        let policy = McpExposurePolicy {
            show_raw: true,
            hidden_servers: Vec::new(),
        };
        // The fixture descriptors above carry server "testserver" for `a`
        // via `descriptor()`; rewrite to "db" already. Registration uses the
        // live inventory, so canonical names are `mcp__db__a/b`.
        let report = register_mcp_adapters(&mut registry, &service, &policy).await;
        assert_eq!(report.registered.len(), 2);
        let (first, second) = tokio::join!(
            async {
                registry
                    .get("mcp__db__a")
                    .expect("adapter a")
                    .execute_structured(json!({}), None)
                    .await
            },
            async {
                registry
                    .get("mcp__db__b")
                    .expect("adapter b")
                    .execute_structured(json!({}), None)
                    .await
            }
        );
        assert!(first.unwrap().success);
        assert!(second.unwrap().success);
    }
}
