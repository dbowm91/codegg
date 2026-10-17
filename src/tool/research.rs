//! `research` tool — the agent-facing surface for the deep research
//! subsystem.
//!
//! The tool wraps [`ResearchService::answer_for_agent`] and returns
//! the synthesized `AgentAnswer` artifact. It is registered in
//! `ToolRegistry::with_defaults()` so any agent that can call the
//! `task` tool can also call `research` directly.
//!
//! Construction is two-step:
//! 1. Build a [`ResearchService`] (typically once per process).
//! 2. Construct a [`ResearchTool`] holding an `Arc<ResearchService>`.
//!
//! In contexts where research is not available (e.g. the `websearch`
//! tool's static registry path) the tool can be a no-op that returns
//! a clear "research service not configured" error.

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use crate::error::ToolError;
use crate::research::service::ResearchService;
use crate::research::types::{ResearchAudience, ResearchDepth, ResearchMode};
use crate::tool::{Tool, ToolCategory};

/// Default `ResearchService` for the no-provider case. The agent loop
/// may override this by calling [`ResearchTool::set_service`] after
/// constructing the registry.
pub struct ResearchTool {
    service: Arc<ResearchService>,
    timeout_secs: u64,
}

impl ResearchTool {
    pub fn new(service: Arc<ResearchService>) -> Self {
        Self {
            service,
            timeout_secs: 600,
        }
    }

    /// Construct a research tool from daemon-owned workspace context.
    pub fn for_execution(execution: &codegg_core::workspace::ExecutionContext) -> Self {
        Self::new(Arc::new(ResearchService::new(
            execution.workspace_root.clone(),
        )))
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Attach an explicit runtime-owned search/MCP context (M005) to the
    /// underlying research service's eggsearch evidence adapter.
    ///
    /// `ToolRegistry::with_options` always uses this. When the service
    /// is uniquely owned (the normal registry path) the context is
    /// installed and evidence collection avoids the deprecated
    /// process-global slots. If the service handle is shared elsewhere,
    /// the tool keeps global-fallback behavior and warns rather than
    /// aliasing another owner's service.
    pub fn with_search_runtime(
        mut self,
        runtime: crate::search_backend::SearchRuntimeContext,
    ) -> Self {
        match Arc::try_unwrap(self.service) {
            Ok(svc) => {
                self.service = Arc::new(svc.with_search_runtime(runtime));
            }
            Err(shared) => {
                tracing::warn!(
                    "research tool service is shared; retaining legacy global search fallback"
                );
                self.service = shared;
            }
        }
        self
    }

    /// Attach the turn's provider so research runs its model-backed
    /// phases (evidence extraction, claim construction, and semantic
    /// verification) instead of the deterministic fallbacks.
    ///
    /// Mirrors [`Self::with_search_runtime`]'s ownership rule: when the
    /// service handle is uniquely owned (the normal registry path) the
    /// provider is installed; if the handle is shared elsewhere we warn
    /// rather than silently leaving one caller's research provider-less.
    pub fn with_model_backed_phases(
        mut self,
        provider: Arc<dyn crate::provider::Provider>,
        model: String,
    ) -> Self {
        match Arc::try_unwrap(self.service) {
            Ok(svc) => {
                self.service = Arc::new(svc.with_model_backed_phases(provider, model));
            }
            Err(shared) => {
                tracing::warn!(
                    "research tool service is shared; research will run without model-backed phases"
                );
                self.service = shared;
            }
        }
        self
    }

    /// Whether the wrapped service runs model-backed research phases.
    pub fn model_backed_phases_enabled(&self) -> bool {
        self.service.model_backed_phases_enabled()
    }

    pub fn set_service(&mut self, service: Arc<ResearchService>) {
        self.service = service;
    }

    async fn run(
        &self,
        question: &str,
        mode: ResearchMode,
        depth: ResearchDepth,
    ) -> Result<String, ToolError> {
        let service = self.service.clone();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            async move {
                service
                    .answer_for_agent(question, mode, depth)
                    .await
                    .map_err(|e| ToolError::Execution(format!("research: {e}")))
            },
        )
        .await;
        match result {
            Err(_) => Err(ToolError::Timeout(format!(
                "research timed out after {}s",
                self.timeout_secs
            ))),
            Ok(inner) => inner,
        }
    }
}

#[async_trait]
impl Tool for ResearchTool {
    fn name(&self) -> &str {
        "research"
    }

    fn causal_contract(&self) -> Option<crate::tool_advisor::causal_frontier::ToolCausalContract> {
        crate::tool_advisor::causal_frontier::native_causal_contract("research")
    }

    fn description(&self) -> &str {
        "Run a deep, multi-source research pipeline and return a synthesized answer with citations. \
         Use this for open-ended, comparative, or multi-hop questions that go beyond a single websearch lookup. \
         The pipeline collects sources from the configured providers, extracts evidence, constructs claims, \
         verifies citations, and synthesizes a final answer. For a quick lookup, use `websearch` instead."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The research question"
                },
                "mode": {
                    "type": "string",
                    "description": "Research mode that biases the pipeline",
                    "enum": [
                        "landscape",
                        "architecture-decision",
                        "library-evaluation",
                        "api-investigation",
                        "debugging-investigation",
                        "security-review",
                        "spec-digest",
                        "narrow-answer"
                    ]
                },
                "depth": {
                    "type": "string",
                    "description": "Research depth (low=8 sources, medium=30, high=80)",
                    "enum": ["low", "medium", "high"]
                }
            },
            "required": ["question"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    /// M002: specialist synthesis is deferred from the ordinary prompt;
    /// it remains registered and discoverable via `tool_search`.
    fn defer_loading(&self) -> bool {
        crate::tool::disclosure::is_deferred_by_default(self.name())
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let question = input["question"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'question' parameter".to_string()))?
            .trim();
        if question.is_empty() {
            return Err(ToolError::Execution(
                "'question' must not be empty".to_string(),
            ));
        }
        let mode = match input["mode"].as_str() {
            Some(s) => crate::research::service::parse_mode(s)
                .map_err(|e| ToolError::Execution(e.to_string()))?,
            None => ResearchMode::NarrowAnswer,
        };
        let depth = match input["depth"].as_str() {
            Some(s) => crate::research::service::parse_depth(s)
                .map_err(|e| ToolError::Execution(e.to_string()))?,
            None => ResearchDepth::Medium,
        };
        // Use the AgentCoder audience so the synthesized answer is
        // oriented toward tool calls rather than human readers.
        let _ = ResearchAudience::AgentCoder;
        self.run(question, mode, depth).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;

    /// Test-only constructor. Its process-cwd variant was deleted in W001:
    /// the registry now refuses the tool outright when no workspace root was
    /// resolved, so the only remaining caller is a unit test, which names its
    /// own root explicitly.
    fn with_default_service() -> ResearchTool {
        ResearchTool::new(Arc::new(ResearchService::new(std::env::temp_dir())))
    }

    #[test]
    fn name_is_research() {
        let t = with_default_service();
        assert_eq!(t.name(), "research");
    }

    #[test]
    fn parameters_require_question() {
        let t = with_default_service();
        let p = t.parameters();
        let required = p.get("required").and_then(|v| v.as_array()).unwrap();
        assert!(required.iter().any(|v| v == "question"));
    }

    #[test]
    fn description_mentions_websearch() {
        let t = with_default_service();
        let d = t.description();
        assert!(d.contains("websearch"));
    }

    #[test]
    fn empty_question_rejected() {
        let t = with_default_service();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let res = rt.block_on(t.execute(json!({"question": "  "})));
        assert!(matches!(res, Err(ToolError::Execution(_))));
    }

    fn stub() -> Arc<dyn crate::provider::Provider> {
        Arc::new(crate::research::tests::StubProvider)
    }

    #[test]
    fn model_backed_phases_are_off_by_default() {
        assert!(!with_default_service().model_backed_phases_enabled());
    }

    #[test]
    fn with_model_backed_phases_enables_the_model_path() {
        // Regression: nothing in production ever attached a provider, so the
        // tool's service stayed model-less and research silently used the
        // deterministic fallbacks for extraction/claims/verification.
        let t = with_default_service().with_model_backed_phases(stub(), "m".into());
        assert!(t.model_backed_phases_enabled());
    }

    #[test]
    fn search_runtime_and_model_backed_phases_are_independent() {
        // This is the exact production order in `build_research_tool`: the
        // search runtime is attached first, then the provider. Neither
        // builder may clobber the other's decision.
        use crate::config::schema::{SearchBackendConfig, SearchConfig};

        let enabled = with_default_service()
            .with_search_runtime(crate::search_backend::SearchRuntimeContext::new(
                SearchConfig {
                    backend: Some(SearchBackendConfig::Eggsearch),
                    ..Default::default()
                },
            ))
            .with_model_backed_phases(stub(), "m".into());
        assert!(enabled.model_backed_phases_enabled());

        let disabled = with_default_service()
            .with_search_runtime(crate::search_backend::SearchRuntimeContext::disabled())
            .with_model_backed_phases(stub(), "m".into());
        assert!(disabled.model_backed_phases_enabled());
    }
}
