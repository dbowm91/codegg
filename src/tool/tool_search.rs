//! Tool search for on-demand tool discovery.
//!
//! This tool allows the LLM to search for tools by name or description,
//! enabling on-demand tool discovery without loading all tools at once.
//!
//! Discovery is monotonic: results are restricted to the already-allowed
//! policy set (`available_tools` installed by the agent loop after
//! deny/plan/disable/backend/ceiling filtering). Discovery never turns a
//! prohibited tool into a callable one. Hidden/internal tools are never
//! returned, even when the allow-list is unset (unit-test construction).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;
use crate::tool::catalog::ToolCatalog;
use crate::tool::{Tool, ToolCategory};
use crate::tool_advisor::{
    project_discovery, AdvisorMode, NoopAdvisor, ToolAdvisor, ToolAdvisorCandidate,
    ToolAdvisorInput,
};

/// Maximum tools returned per search so a broad query cannot move prompt
/// bloat into the search result.
pub const MAX_SEARCH_RESULTS: usize = 10;

/// Tool for searching available tools by query.
///
/// This enables on-demand tool discovery - the LLM can search for tools
/// based on the current context, and only relevant tools need to be
/// sent to the LLM.
#[derive(Clone)]
pub struct ToolSearchTool {
    catalog: Arc<ToolCatalog>,
    available_tools: Option<Vec<String>>,
    advisor: Arc<dyn ToolAdvisor>,
    advisor_mode: AdvisorMode,
    advisor_threshold: f64,
    advisor_max_promotions: usize,
}

impl ToolSearchTool {
    /// Create a new ToolSearchTool with the given catalog.
    pub fn new(catalog: Arc<ToolCatalog>) -> Self {
        Self {
            catalog,
            available_tools: None,
            advisor: Arc::new(NoopAdvisor),
            advisor_mode: AdvisorMode::Off,
            advisor_threshold: 0.5,
            advisor_max_promotions: 2,
        }
    }

    /// Set the list of tool names that are currently available (after filtering).
    /// When set, search results are restricted to these tools.
    pub fn set_available_tools(&mut self, tools: Vec<String>) {
        self.available_tools = Some(tools);
    }

    /// Configure the optional advisory projection. The caller is responsible
    /// for constructing the advisor only after the normal policy surface is
    /// known; this method never changes that surface itself.
    pub fn set_advisor(&mut self, advisor: Arc<dyn ToolAdvisor>, mode: AdvisorMode) {
        self.advisor = advisor;
        self.advisor_mode = mode;
    }

    pub fn set_advisor_policy(&mut self, threshold: f64, max_promotions: usize) {
        self.advisor_threshold = threshold.clamp(0.0, 1.0);
        self.advisor_max_promotions = max_promotions.min(4);
    }

    fn policy_allows(&self, metadata: &crate::tool::catalog::ToolMetadata) -> bool {
        if crate::tool::disclosure::is_hidden(&metadata.name) {
            return false;
        }
        self.available_tools
            .as_ref()
            .map(|available| available.iter().any(|name| name == &metadata.name))
            .unwrap_or(true)
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "tool_search"
    }

    fn description(&self) -> &str {
        "Search for available tools by name or description. \
         Returns a list of tools matching the query. \
         Use this to discover tools available for on-demand use."
    }

    fn set_available_tools(&mut self, tools: Vec<String>) {
        self.available_tools = Some(tools);
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to find relevant tools (searches name and description)"
                },
                "name": {
                    "type": "string",
                    "description": "Optional exact canonical tool name to describe"
                },
                "detail": {
                    "type": "string",
                    "enum": ["summary", "schema"],
                    "description": "Use schema only after selecting one exact tool; broad searches stay compact"
                }
            },
            "required": ["query"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("query required".into()))?;
        let exact_name = input["name"]
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty());
        let detail = input["detail"].as_str().unwrap_or("summary");
        if !matches!(detail, "summary" | "schema") {
            return Err(ToolError::Execution(
                "detail must be either 'summary' or 'schema'".into(),
            ));
        }

        // An empty query would match the whole catalog in keyword mode;
        // return no results instead of dumping registered capability.
        if query.trim().is_empty() {
            return Ok(json!({
                "status": "no_results",
                "query": query,
                "tools": []
            })
            .to_string());
        }

        let results = match exact_name {
            Some(name) => self.catalog.get(name).into_iter().collect(),
            None => self.catalog.search(query),
        };

        // Policy filtering first: only tools the current agent/session
        // policy allows are discoverable. When no allow-list is installed
        // (direct unit construction), still exclude hidden/internal tools.
        let filtered: Vec<crate::tool::catalog::ToolMetadata> = results
            .into_iter()
            .filter(|metadata| self.policy_allows(metadata))
            .collect();

        if filtered.is_empty() && self.advisor_mode != AdvisorMode::Promote {
            return Ok(json!({
                "status": "no_results",
                "query": query,
                "name": exact_name,
                "tools": []
            })
            .to_string());
        }

        if exact_name.is_some() && detail == "schema" {
            // Exact expansion is intentionally policy-gated by the same
            // allow-list and hidden-tool filter as broad discovery. The
            // catalog is live, so the schema is the current registration,
            // not a stale request-time snapshot.
            let metadata = &filtered[0];
            let risk =
                crate::tool::risk::classify_tool_risk(&metadata.name, &serde_json::json!({}));
            let tool = json!({
                "canonical_name": metadata.name,
                "name": metadata.name,
                "description": metadata.description,
                "parameters": metadata.parameters,
                "defer_load": metadata.defer_load,
                "category": metadata.category,
                "risk": format!("{risk:?}"),
                "disclosure": metadata.disclosure,
                "schema_truncated": false
            });
            return Ok(json!({
                "status": "success",
                "query": query,
                "name": metadata.name,
                "detail": "schema",
                "count": 1,
                "total_matches": 1,
                "tools": [tool]
            })
            .to_string());
        }

        // Cap the current shortlist before advice so reranking cannot expand
        // the normal discovery result set. Promotion gets a separate,
        // policy-filtered deferred universe below.
        let total_matches = filtered.len();
        let current: Vec<_> = filtered.into_iter().take(MAX_SEARCH_RESULTS).collect();
        let deferred_allowed: Vec<_> = if self.advisor_mode == AdvisorMode::Promote {
            self.catalog
                .deferred_tools()
                .into_iter()
                .filter(|metadata| self.policy_allows(metadata))
                .collect()
        } else {
            Vec::new()
        };
        let candidates = current
            .iter()
            .chain(deferred_allowed.iter())
            .map(ToolAdvisorCandidate::from_metadata)
            .collect();
        let projection = project_discovery(
            &current,
            &deferred_allowed,
            &ToolAdvisorInput {
                case_id: "tool-search".to_string(),
                context: query.to_string(),
                candidates,
                surface_fingerprint: format!("query:{}", query),
            },
            self.advisor.as_ref(),
            self.advisor_mode,
            self.advisor_threshold,
            self.advisor_max_promotions,
        );
        let total_matches = total_matches + projection.promoted.len();
        let tools: Vec<serde_json::Value> = projection
            .ordered
            .into_iter()
            .map(|metadata| {
                // Selection metadata only: canonical name, purpose,
                // category/risk/disclosure for correct choice among related
                // research/evidence tools. No backend config, endpoints,
                // credentials, reasoning, or plugin internals are returned.
                let risk =
                    crate::tool::risk::classify_tool_risk(&metadata.name, &serde_json::json!({}));
                json!({
                    "canonical_name": metadata.name,
                    "name": metadata.name,
                    "description": metadata.description,
                    "defer_load": metadata.defer_load,
                    "schema_available": true,
                    "category": metadata.category,
                    "risk": format!("{risk:?}"),
                    "disclosure": metadata.disclosure
                })
            })
            .collect();

        Ok(json!({
            "status": "success",
            "query": query,
            "count": tools.len(),
            "total_matches": total_matches,
            "advisor": {
                "mode": format!("{:?}", self.advisor_mode).to_lowercase(),
                "promoted": projection.promoted,
                "abstained": projection.abstained,
                "fallback": projection.fallback,
            },
            "tools": tools
        })
        .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct MockTool {
        name: &'static str,
        hidden: bool,
        deferred: bool,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            "A deliberately large semantic tool for deterministic discovery tests"
        }

        fn parameters(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {
                    "operation": {"type": "string", "enum": ["status", "diff", "log"]},
                    "content": {"type": "string"}
                },
                "required": ["operation"]
            })
        }

        async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
            Ok(String::new())
        }

        fn expose_in_definitions(&self) -> bool {
            !self.hidden
        }

        fn defer_loading(&self) -> bool {
            self.deferred
        }
    }

    struct FixedSearchAdvisor;

    impl ToolAdvisor for FixedSearchAdvisor {
        fn score(
            &self,
            input: &ToolAdvisorInput,
        ) -> anyhow::Result<crate::tool_advisor::ToolAdvisorPrediction> {
            let mut ranked = input
                .candidates
                .iter()
                .map(|candidate| crate::tool_advisor::RankedCandidate {
                    name: candidate.name.clone(),
                    score: if candidate.name == "git_diff" {
                        0.9
                    } else if candidate.name == "git_deferred" {
                        0.8
                    } else {
                        0.1
                    },
                })
                .collect::<Vec<_>>();
            ranked.sort_by(|left, right| {
                right
                    .score
                    .partial_cmp(&left.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            Ok(crate::tool_advisor::ToolAdvisorPrediction {
                schema_version: crate::tool_advisor::PREDICTION_SCHEMA_VERSION,
                case_id: input.case_id.clone(),
                ranked,
                abstain_probability: Some(0.1),
                mode: "test".into(),
            })
        }
    }

    fn test_search() -> ToolSearchTool {
        let mut catalog = ToolCatalog::new();
        catalog.register(&MockTool {
            name: "git_query",
            hidden: false,
            deferred: false,
        });
        catalog.register(&MockTool {
            name: "git_diff",
            hidden: false,
            deferred: false,
        });
        catalog.register(&MockTool {
            name: "git_deferred",
            hidden: false,
            deferred: true,
        });
        catalog.register(&MockTool {
            name: "git_read",
            hidden: true,
            deferred: false,
        });
        let mut search = ToolSearchTool::new(Arc::new(catalog));
        search.set_available_tools(vec![
            "git_query".to_string(),
            "git_diff".to_string(),
            "git_deferred".to_string(),
            "git_read".to_string(),
        ]);
        search
    }

    #[tokio::test]
    async fn broad_search_is_compact_and_schema_expansion_is_exact() {
        let search = test_search();

        let broad = search
            .execute(json!({"query": "git"}))
            .await
            .expect("broad search");
        let broad: serde_json::Value = serde_json::from_str(&broad).expect("json");
        let result = &broad["tools"][0];
        assert!(result.get("schema_available").is_some());
        assert!(result.get("parameters").is_none());

        let expanded = search
            .execute(json!({"query": "git", "name": "git_query", "detail": "schema"}))
            .await
            .expect("schema expansion");
        let expanded: serde_json::Value = serde_json::from_str(&expanded).expect("json");
        assert_eq!(expanded["count"], 1);
        assert!(expanded["tools"][0].get("parameters").is_some());
        assert_eq!(expanded["tools"][0]["name"], "git_query");
    }

    #[tokio::test]
    async fn hidden_and_denied_tools_cannot_be_described() {
        let search = test_search();
        let hidden = search
            .execute(json!({
                "query": "internal",
                "name": "git_read",
                "detail": "schema"
            }))
            .await
            .expect("hidden search");
        let hidden: serde_json::Value = serde_json::from_str(&hidden).expect("json");
        assert_eq!(hidden["status"], "no_results");

        let mut search = test_search();
        search.set_available_tools(vec!["some_other_tool".to_string()]);
        let denied = search
            .execute(json!({"query": "git", "name": "git_query", "detail": "schema"}))
            .await
            .expect("denied search");
        let denied: serde_json::Value = serde_json::from_str(&denied).expect("json");
        assert_eq!(denied["status"], "no_results");
    }

    #[tokio::test]
    async fn rerank_is_opt_in_and_promotion_is_bounded_to_allowed_deferred_tools() {
        let mut search = test_search();
        let baseline = search
            .execute(json!({"query": "git"}))
            .await
            .expect("baseline search");
        let baseline: serde_json::Value = serde_json::from_str(&baseline).expect("baseline json");
        assert_eq!(baseline["advisor"]["mode"], "off");

        search.set_advisor(Arc::new(FixedSearchAdvisor), AdvisorMode::Rerank);
        let reranked = search
            .execute(json!({"query": "git"}))
            .await
            .expect("reranked search");
        let reranked: serde_json::Value = serde_json::from_str(&reranked).expect("reranked json");
        assert_eq!(reranked["tools"][0]["name"], "git_diff");
        assert_eq!(reranked["count"], baseline["count"]);

        search.set_advisor(Arc::new(FixedSearchAdvisor), AdvisorMode::Promote);
        search.set_advisor_policy(0.7, 1);
        let promoted = search
            .execute(json!({"query": "unmatched"}))
            .await
            .expect("promoted search");
        let promoted: serde_json::Value = serde_json::from_str(&promoted).expect("promoted json");
        assert_eq!(promoted["count"], 1);
        assert_eq!(promoted["tools"][0]["name"], "git_deferred");
        assert_eq!(promoted["advisor"]["promoted"][0], "git_deferred");
    }

    #[test]
    fn schema_census_records_large_surface_and_compact_selection() {
        let registry = crate::tool::ToolRegistry::with_defaults();
        for name in ["git", "git_query", "lsp", "task", "work_order"] {
            if let Some(metadata) = registry.catalog().get(name) {
                let bytes = serde_json::to_vec(&json!({
                    "description": metadata.description,
                    "parameters": metadata.parameters,
                }))
                .expect("schema census serialization")
                .len();
                eprintln!("m003 schema census {name}: {bytes} bytes");
            }
        }

        let broad_bytes = registry
            .catalog()
            .search("lsp")
            .into_iter()
            .take(MAX_SEARCH_RESULTS)
            .map(|metadata| {
                serde_json::to_vec(&json!({
                    "name": metadata.name,
                    "description": metadata.description,
                    "category": metadata.category,
                    "disclosure": metadata.disclosure,
                    "schema_available": true,
                }))
                .expect("compact census serialization")
                .len()
            })
            .sum::<usize>();
        let full_bytes = registry
            .catalog()
            .search("lsp")
            .into_iter()
            .take(MAX_SEARCH_RESULTS)
            .map(|metadata| {
                serde_json::to_vec(&json!({
                    "name": metadata.name,
                    "description": metadata.description,
                    "parameters": metadata.parameters,
                    "category": metadata.category,
                    "disclosure": metadata.disclosure,
                }))
                .expect("full census serialization")
                .len()
            })
            .sum::<usize>();
        assert!(broad_bytes < full_bytes);
        eprintln!(
            "m003 broad lsp search: {full_bytes} -> {broad_bytes} bytes ({}% reduction)",
            100usize.saturating_sub(broad_bytes.saturating_mul(100) / full_bytes.max(1))
        );
    }
}
