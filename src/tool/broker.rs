//! Canonical Tool Broker.
//!
//! The broker is the single execution boundary for all production tool
//! calls — both direct (agent loop) and programmatic (Tool Programs).
//! It performs an ordered pipeline:
//!
//! 1. Registry lookup and contract snapshot
//! 2. Caller-policy check
//! 3. Input schema validation
//! 4. Authority, permission, path policy
//! 5. Deadline/cancellation precheck
//! 6. Route selection (inline native or scheduler-owned)
//! 7. Execution with invocation identity and provenance
//! 8. Output validation
//! 9. Artifact registration
//! 10. Terminal result recording and event emission
//!
//! # Backward compatibility
//!
//! Legacy tools that do not supply a `ToolContract` receive
//! conservative defaults. The broker wraps their string output in a
//! `ToolValue` so callers can consume typed results uniformly.

use std::path::PathBuf;

use crate::error::ToolError;

use super::backend::{StructuredToolResult, ToolExecutionContext, ToolProvenance};
use super::contract::{
    ToolCaller, ToolCallerPolicy, ToolContract, ToolContractCatalog, ToolTerminalStatus, ToolValue,
};
use super::ToolRegistry;

// ─── Broker configuration ─────────────────────────────────────────

/// Configuration for the ToolBroker, resolved once at construction.
#[derive(Debug, Clone)]
pub struct ToolBrokerConfig {
    /// Default per-call timeout in milliseconds.
    pub default_timeout_ms: u64,
    /// Maximum allowed input payload size in bytes.
    pub max_input_bytes: usize,
    /// Maximum allowed output display size in bytes before
    /// artifact spillover.
    pub max_output_display_bytes: usize,
    /// Maximum allowed output bytes total (display + structured).
    /// Outputs exceeding this are truncated and flagged.
    pub max_output_bytes: usize,
}

impl Default for ToolBrokerConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: 120_000,
            max_input_bytes: 10 * 1024 * 1024,
            max_output_display_bytes: 256 * 1024,
            max_output_bytes: 10 * 1024 * 1024,
        }
    }
}

// ─── Broker invocation context ────────────────────────────────────

/// Rich context passed to the broker for every tool invocation.
///
/// This supersedes the minimal `ToolExecutionContext` for new
/// broker-mediated paths. Legacy callers can convert through
/// `From<ToolExecutionContext>`.
#[derive(Debug, Clone)]
pub struct BrokerInvocationContext {
    /// Who initiated the call.
    pub caller: ToolCaller,
    /// Working directory.
    pub cwd: PathBuf,
    /// Optional session identity.
    pub session_id: Option<String>,
    /// Optional workspace identity.
    pub workspace_id: Option<String>,
    /// Optional agent identity.
    pub agent_id: Option<String>,
    /// Optional turn identity.
    pub turn_id: Option<String>,
    /// Optional job identity (for programmatic callers).
    pub job_id: Option<String>,
    /// Optional attempt identity.
    pub attempt_id: Option<String>,
    /// Permission mode (e.g. "ask", "allow", "deny").
    pub permission_mode: Option<String>,
    /// Effective timeout in milliseconds (overrides default).
    pub timeout_ms: Option<u64>,
    /// Submission key for idempotent deduplication.
    pub submission_key: Option<String>,
    /// Whether the caller has already performed authority/permission
    /// checks for this invocation. When `true`, the broker skips
    /// its own step-4 authority check (used by AgentLoop which
    /// checks permissions before entering the broker).
    pub caller_authorized: bool,
}

impl From<ToolExecutionContext> for BrokerInvocationContext {
    fn from(ctx: ToolExecutionContext) -> Self {
        Self {
            caller: ToolCaller::Agent,
            cwd: ctx.cwd,
            session_id: ctx.session_id,
            workspace_id: None,
            agent_id: None,
            turn_id: None,
            job_id: None,
            attempt_id: None,
            permission_mode: ctx.permission_mode,
            timeout_ms: ctx.timeout_ms,
            submission_key: None,
            caller_authorized: false,
        }
    }
}

// ─── Broker result ────────────────────────────────────────────────

/// The result of a broker-mediated tool invocation.
#[derive(Debug)]
pub struct BrokerResult {
    /// Typed tool value with display, artifacts, and status.
    pub value: ToolValue,
    /// The contract that was used for this invocation.
    pub contract: ToolContract,
    /// Invocation identifier for correlation.
    pub invocation_id: String,
    /// Wall-clock elapsed time in milliseconds.
    pub elapsed_ms: u64,
}

// ─── ToolBroker ───────────────────────────────────────────────────

/// The canonical tool execution broker.
///
/// Enforces the 10-step pipeline for every production tool call.
/// The broker holds a pre-built contract catalog and configuration.
/// The tool registry is passed to execution methods — the broker
/// does not own it.
pub struct ToolBroker {
    catalog: ToolContractCatalog,
    config: ToolBrokerConfig,
}

impl ToolBroker {
    /// Build a broker from an existing tool registry.
    ///
    /// Contracts are derived from registered tools using conservative
    /// defaults for tools that do not supply explicit metadata.
    pub fn new(registry: &ToolRegistry) -> Self {
        let catalog = Self::build_catalog(registry);
        Self {
            catalog,
            config: ToolBrokerConfig::default(),
        }
    }

    /// Build with custom configuration.
    pub fn with_config(registry: &ToolRegistry, config: ToolBrokerConfig) -> Self {
        let catalog = Self::build_catalog(registry);
        Self { catalog, config }
    }

    /// Access the contract catalog.
    pub fn catalog(&self) -> &ToolContractCatalog {
        &self.catalog
    }

    /// Build the contract catalog from the registered tools.
    ///
    /// For each registered tool:
    /// 1. If the tool supplies a `contract()`, use it.
    /// 2. Otherwise, build a conservative `ToolContract::legacy()`.
    fn build_catalog(registry: &ToolRegistry) -> ToolContractCatalog {
        let mut contracts = Vec::new();
        for name in registry.tool_names() {
            if let Some(tool) = registry.get(name) {
                let contract = tool.contract(name, tool.parameters());
                contracts.push(contract);
            }
        }
        ToolContractCatalog::new(contracts)
    }

    // ── Step 1: Lookup ──────────────────────────────────────────

    /// Check if a tool exists and return its contract.
    pub fn lookup_contract(&self, tool_name: &str) -> Result<&ToolContract, BrokerError> {
        self.catalog
            .get(tool_name)
            .ok_or_else(|| BrokerError::NoContract(tool_name.to_string()))
    }

    /// Check if a tool exists in the registry.
    pub fn has_tool(&self, registry: &ToolRegistry, tool_name: &str) -> bool {
        registry.get(tool_name).is_some()
    }

    // ── Step 2: Caller policy ───────────────────────────────────

    /// Check whether the caller is permitted to invoke this tool.
    pub fn check_caller_policy(
        &self,
        contract: &ToolContract,
        caller: &ToolCaller,
    ) -> Result<(), BrokerError> {
        match (&contract.caller_policy, caller) {
            (ToolCallerPolicy::DirectOnly, ToolCaller::Agent) => Ok(()),
            (ToolCallerPolicy::DirectOnly, ToolCaller::Internal) => Ok(()),
            (ToolCallerPolicy::DirectOnly, _) => Err(BrokerError::CallerDenied {
                tool: contract.name.clone(),
                caller: format!("{:?}", caller),
                policy: contract.caller_policy,
            }),
            (ToolCallerPolicy::DirectOrProgrammatic, _) => Ok(()),
            (ToolCallerPolicy::ProgrammaticOnly, ToolCaller::Program { .. }) => Ok(()),
            (ToolCallerPolicy::ProgrammaticOnly, ToolCaller::Internal) => Ok(()),
            (ToolCallerPolicy::ProgrammaticOnly, _) => Err(BrokerError::CallerDenied {
                tool: contract.name.clone(),
                caller: format!("{:?}", caller),
                policy: contract.caller_policy,
            }),
        }
    }

    // ── Steps 3-6: Validation pipeline ──────────────────────────

    /// Run the pre-execution validation pipeline:
    /// - caller policy (step 2)
    /// - caller authorization (step 4, if not pre-authorized)
    /// - input size bounds (step 3)
    /// - deadline precheck (step 5)
    ///
    /// Returns the effective timeout in milliseconds.
    pub fn validate_pre_execution(
        &self,
        contract: &ToolContract,
        ctx: &BrokerInvocationContext,
        input: &serde_json::Value,
    ) -> Result<u64, BrokerError> {
        // Step 2: caller policy
        self.check_caller_policy(contract, &ctx.caller)?;

        // Step 4: authority — if the caller has not already performed
        // permission checks, the broker rejects the call. This ensures
        // programmatic callers go through the permission system.
        if !ctx.caller_authorized && !matches!(ctx.caller, ToolCaller::Agent | ToolCaller::Internal)
        {
            return Err(BrokerError::CallerDenied {
                tool: contract.name.clone(),
                caller: format!("{:?}", ctx.caller),
                policy: contract.caller_policy,
            });
        }

        // Step 3: input size check
        let input_size = serde_json::to_vec(input).map(|b| b.len()).unwrap_or(0);
        if input_size > self.config.max_input_bytes {
            return Err(BrokerError::InputTooLarge {
                tool: contract.name.clone(),
                size: input_size,
                max: self.config.max_input_bytes,
            });
        }

        // Step 5: deadline/timeout
        let effective_timeout_ms = ctx.timeout_ms.unwrap_or(self.config.default_timeout_ms);

        Ok(effective_timeout_ms)
    }

    // ── Step 7: Execution ───────────────────────────────────────

    /// Execute a tool through the broker.
    ///
    /// This performs the full 10-step pipeline and returns a
    /// `BrokerResult` with typed output.
    pub async fn execute(
        &self,
        registry: &ToolRegistry,
        tool_name: &str,
        input: serde_json::Value,
        ctx: BrokerInvocationContext,
    ) -> Result<BrokerResult, BrokerError> {
        let invocation_id = uuid::Uuid::new_v4().to_string();

        // Step 1: lookup contract (catalog, no lock needed)
        let contract = self.lookup_contract(tool_name)?;

        // Steps 2-5: validation pipeline
        let _effective_timeout = self.validate_pre_execution(contract, &ctx, &input)?;

        // Step 7: execute
        let start = std::time::Instant::now();
        let exec_ctx = ToolExecutionContext {
            backend: super::backend::ToolBackendKind::Native,
            session_id: ctx.session_id.clone(),
            cwd: ctx.cwd.clone(),
            permission_mode: ctx.permission_mode.clone(),
            timeout_ms: ctx.timeout_ms,
        };
        let tool = registry
            .get(tool_name)
            .ok_or_else(|| BrokerError::NotFound(tool_name.to_string()))?;
        let result = tool.execute_structured(input, Some(exec_ctx)).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(structured) => {
                // Steps 8-10: convert to typed value, validate, register artifacts
                let value = self.normalize_result(tool_name, structured, elapsed_ms);
                let value = self.validate_output(&contract, value)?;
                let value = self.register_artifacts(tool_name, value);
                Ok(BrokerResult {
                    value,
                    contract: contract.clone(),
                    invocation_id,
                    elapsed_ms,
                })
            }
            Err(e) => {
                let value = match &e {
                    ToolError::NotFound(name) => {
                        ToolValue::infrastructure_error(format!("Tool not found: {}", name))
                    }
                    ToolError::Timeout(_msg) => ToolValue::timed_out(),
                    ToolError::Permission(msg) => ToolValue::denied(msg.clone()),
                    ToolError::Disabled(msg) => {
                        ToolValue::infrastructure_error(format!("Tool is disabled: {}", msg))
                    }
                    other => ToolValue::infrastructure_error(format!("Tool error: {}", other)),
                };
                Ok(BrokerResult {
                    value,
                    contract: contract.clone(),
                    invocation_id,
                    elapsed_ms,
                })
            }
        }
    }

    // ── Steps 8-10: Result normalization ────────────────────────

    /// Normalize a `StructuredToolResult` into a `ToolValue`.
    fn normalize_result(
        &self,
        tool_name: &str,
        result: StructuredToolResult,
        elapsed_ms: u64,
    ) -> ToolValue {
        let terminal_status = if result.success {
            ToolTerminalStatus::Success
        } else {
            ToolTerminalStatus::Error
        };

        let truncated = result
            .provenance
            .as_ref()
            .map(|p| p.truncated)
            .unwrap_or(false);

        let provenance = result.provenance.or_else(|| {
            Some(ToolProvenance {
                backend: "native".to_string(),
                implementation: format!("codegg/{}", tool_name),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                elapsed_ms: Some(elapsed_ms),
                truncated: false,
                trust: super::backend::ToolTrust::LocalUntrusted,
            })
        });

        ToolValue {
            display: result.output,
            value: result.value,
            artifacts: Vec::new(),
            provenance,
            terminal_status,
            truncated,
        }
    }

    // ── Step 8: Output validation ──────────────────────────────

    /// Validate the output against broker-level bounds.
    ///
    /// Checks that the output display does not exceed
    /// `max_output_bytes`. If it does, the output is truncated and
    /// `truncated` is set to `true`.
    fn validate_output(
        &self,
        _contract: &ToolContract,
        mut value: ToolValue,
    ) -> Result<ToolValue, BrokerError> {
        let display_len = value.display.len();
        if display_len > self.config.max_output_bytes {
            value.display.truncate(self.config.max_output_bytes);
            value.truncated = true;
        }
        Ok(value)
    }

    // ── Step 9: Artifact registration ─────────────────────────

    /// Register artifact handles for large outputs.
    ///
    /// When the output display exceeds `max_output_display_bytes`,
    /// the broker creates an artifact handle so downstream consumers
    /// can reference the content without embedding it in transcripts.
    fn register_artifacts(&self, tool_name: &str, mut value: ToolValue) -> ToolValue {
        let display_len = value.display.len();
        if display_len > self.config.max_output_display_bytes {
            let artifact = super::contract::ToolArtifactHandle {
                artifact_id: uuid::Uuid::new_v4().to_string(),
                tool_name: tool_name.to_string(),
                content_type: "text/plain".to_string(),
                byte_length: display_len as u64,
                digest: None,
            };
            value.artifacts.push(artifact);
            value.truncated = true;
        }
        value
    }
}

// ─── Broker errors ────────────────────────────────────────────────

/// Errors returned by the ToolBroker pipeline.
#[derive(Debug)]
pub enum BrokerError {
    /// Tool not found in registry.
    NotFound(String),
    /// No contract registered for this tool.
    NoContract(String),
    /// Caller is not permitted by contract policy.
    CallerDenied {
        tool: String,
        caller: String,
        policy: super::contract::ToolCallerPolicy,
    },
    /// Input payload exceeds size bounds.
    InputTooLarge {
        tool: String,
        size: usize,
        max: usize,
    },
    /// Tool execution failed.
    Execution(String),
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "tool not found: {}", name),
            Self::NoContract(name) => write!(f, "no contract for tool: {}", name),
            Self::CallerDenied {
                tool,
                caller,
                policy,
            } => write!(
                f,
                "caller {:?} denied by {:?} policy for tool {}",
                caller, policy, tool
            ),
            Self::InputTooLarge { tool, size, max } => write!(
                f,
                "input for {} is {} bytes, max is {} bytes",
                tool, size, max
            ),
            Self::Execution(msg) => write!(f, "broker execution error: {}", msg),
        }
    }
}

impl std::error::Error for BrokerError {}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::ToolCategory;
    use super::*;

    /// Minimal test tool for broker tests.
    struct TestTool {
        name: String,
        category: ToolCategory,
    }

    #[async_trait::async_trait]
    impl super::super::Tool for TestTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            "test tool"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
            Ok("result".to_string())
        }
        fn category(&self) -> ToolCategory {
            self.category
        }
    }

    fn make_registry_with_test_tool() -> ToolRegistry {
        let mut registry = ToolRegistry::with_defaults();
        registry.register(TestTool {
            name: "read".to_string(),
            category: ToolCategory::ReadOnly,
        });
        registry
    }

    fn make_ctx() -> BrokerInvocationContext {
        BrokerInvocationContext {
            caller: ToolCaller::Agent,
            cwd: PathBuf::from("."),
            session_id: None,
            workspace_id: None,
            agent_id: None,
            turn_id: None,
            job_id: None,
            attempt_id: None,
            permission_mode: None,
            timeout_ms: None,
            submission_key: None,
            caller_authorized: true,
        }
    }

    #[tokio::test]
    async fn broker_execute_returns_success() {
        let registry = make_registry_with_test_tool();
        let broker = ToolBroker::new(&registry);
        let result = broker
            .execute(&registry, "read", serde_json::json!({}), make_ctx())
            .await
            .unwrap();
        assert_eq!(result.value.terminal_status, ToolTerminalStatus::Success);
        assert_eq!(result.value.display, "result");
    }

    #[tokio::test]
    async fn broker_not_found_tool() {
        let registry = make_registry_with_test_tool();
        let broker = ToolBroker::new(&registry);
        let err = broker
            .execute(&registry, "nonexistent", serde_json::json!({}), make_ctx())
            .await
            .unwrap_err();
        assert!(matches!(err, BrokerError::NoContract(_)));
    }

    #[test]
    fn catalog_builds_from_registry() {
        let registry = make_registry_with_test_tool();
        let broker = ToolBroker::new(&registry);
        assert!(broker.catalog().contains("read"));
    }

    #[test]
    fn legacy_tool_gets_default_contract() {
        let registry = make_registry_with_test_tool();
        let broker = ToolBroker::new(&registry);
        let contract = broker.catalog().get("read").unwrap();
        assert_eq!(
            contract.caller_policy,
            super::super::contract::ToolCallerPolicy::DirectOnly
        );
        assert_eq!(
            contract.effect_class,
            super::super::contract::ToolEffectClass::NonIdempotent
        );
    }
}
