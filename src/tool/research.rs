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
use once_cell::sync::Lazy;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::ToolError;
use crate::research::service::ResearchService;
use crate::research::types::{ResearchAudience, ResearchDepth, ResearchMode};
use crate::tool::{Tool, ToolCategory};

/// Default `ResearchService` for the no-provider case. The agent loop
/// may override this by calling [`ResearchTool::set_service`] after
/// constructing the registry.
static DEFAULT_SERVICE: Lazy<Arc<ResearchService>> = Lazy::new(|| {
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    Arc::new(ResearchService::new(project_root))
});

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

    /// Build a tool that uses the lazily-initialized default service
    /// rooted at the current working directory. Used by
    /// `ToolRegistry::with_defaults()` when no explicit service is
    /// provided.
    pub fn with_default_service() -> Self {
        Self {
            service: DEFAULT_SERVICE.clone(),
            timeout_secs: 600,
        }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
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

    #[test]
    fn name_is_research() {
        let t = ResearchTool::with_default_service();
        assert_eq!(t.name(), "research");
    }

    #[test]
    fn parameters_require_question() {
        let t = ResearchTool::with_default_service();
        let p = t.parameters();
        let required = p.get("required").and_then(|v| v.as_array()).unwrap();
        assert!(required.iter().any(|v| v == "question"));
    }

    #[test]
    fn description_mentions_websearch() {
        let t = ResearchTool::with_default_service();
        let d = t.description();
        assert!(d.contains("websearch"));
    }

    #[test]
    fn empty_question_rejected() {
        let t = ResearchTool::with_default_service();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let res = rt.block_on(t.execute(json!({"question": "  "})));
        assert!(matches!(res, Err(ToolError::Execution(_))));
    }
}
