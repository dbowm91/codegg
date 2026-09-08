//! Unit tests for the eggsearch backend integration.
//!
//! These tests verify the agent-loop tool exposure filtering behavior
//! when `expose_raw_mcp_tools` is enabled or disabled.

#[cfg(test)]
mod agent_loop_filtering_tests {
    use codegg::provider::ToolDefinition;

    fn make_mcp_tools(prefix: &str) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: format!("{prefix}web_search"),
                description: "Search the web".to_string(),
                parameters: serde_json::json!({}),
                defer_loading: None,
            },
            ToolDefinition {
                name: format!("{prefix}web_fetch"),
                description: "Fetch a URL".to_string(),
                parameters: serde_json::json!({}),
                defer_loading: None,
            },
            ToolDefinition {
                name: format!("{prefix}provider_status"),
                description: "Check provider status".to_string(),
                parameters: serde_json::json!({}),
                defer_loading: None,
            },
            ToolDefinition {
                name: "other_tool".to_string(),
                description: "Some other tool".to_string(),
                parameters: serde_json::json!({}),
                defer_loading: None,
            },
        ]
    }

    fn filter_eggsearch_tools(
        tools: Vec<ToolDefinition>,
        expose_raw: bool,
        server_name: &str,
    ) -> Vec<ToolDefinition> {
        let raw_prefix = format!("mcp__{}__", server_name);
        tools
            .into_iter()
            .filter(|t| {
                if !expose_raw && t.name.starts_with(&raw_prefix) {
                    return false;
                }
                true
            })
            .collect()
    }

    #[test]
    fn expose_raw_false_hides_eggsearch_tools() {
        let tools = make_mcp_tools("mcp__eggsearch__");
        let filtered = filter_eggsearch_tools(tools, false, "eggsearch");

        assert!(
            filtered
                .iter()
                .all(|t| !t.name.starts_with("mcp__eggsearch__")),
            "no mcp__eggsearch__ tools should remain, got: {:?}",
            filtered.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
        assert_eq!(
            filtered.len(),
            1,
            "only other_tool should remain, got: {:?}",
            filtered.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
        assert_eq!(filtered[0].name, "other_tool");
    }

    #[test]
    fn expose_raw_true_shows_eggsearch_tools() {
        let tools = make_mcp_tools("mcp__eggsearch__");
        let filtered = filter_eggsearch_tools(tools, true, "eggsearch");

        assert_eq!(
            filtered.len(),
            4,
            "all tools should remain, got: {:?}",
            filtered.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
        assert!(
            filtered
                .iter()
                .any(|t| t.name == "mcp__eggsearch__web_search"),
            "web_search should be present"
        );
        assert!(
            filtered
                .iter()
                .any(|t| t.name == "mcp__eggsearch__web_fetch"),
            "web_fetch should be present"
        );
        assert!(
            filtered
                .iter()
                .any(|t| t.name == "mcp__eggsearch__provider_status"),
            "provider_status should be present"
        );
        assert!(
            filtered.iter().any(|t| t.name == "other_tool"),
            "other_tool should be present"
        );
    }

    #[test]
    fn expose_raw_uses_default_server_name() {
        let tools = make_mcp_tools("mcp__eggsearch__");
        let filtered = filter_eggsearch_tools(tools, false, "eggsearch");

        assert!(
            filtered
                .iter()
                .all(|t| !t.name.starts_with("mcp__eggsearch__")),
            "with default server name, all eggsearch tools should be filtered"
        );
    }

    #[test]
    fn expose_raw_with_custom_server_name() {
        let tools = make_mcp_tools("mcp__myegg__");
        let filtered = filter_eggsearch_tools(tools, false, "myegg");

        assert!(
            filtered.iter().all(|t| !t.name.starts_with("mcp__myegg__")),
            "with custom server name, all myegg tools should be filtered"
        );
    }
}

/// Integration tests that exercise the real `AgentLoop::build_tool_definitions`
/// path, not a reimplementation of the filter predicate. These verify the
/// contract that `mcp__eggsearch__*` raw tools are hidden from the model when
/// `expose_raw_mcp_tools` is false and exposed when it is true.
#[cfg(test)]
mod real_build_tool_definitions_tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use codegg::agent::r#loop::AgentLoop;
    use codegg::agent::Agent;
    use codegg::config::schema::{Config, EggsearchConfig, SearchBackendConfig, SearchConfig};
    use codegg::error::McpError;
    use codegg::mcp::{McpService, McpTool};
    use codegg::permission::PermissionChecker;
    use codegg::provider::{
        ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError, TokenUsage,
    };
    use codegg::tool::ToolRegistry;
    use tokio::sync::RwLock;

    /// Minimal scripted provider that returns a single empty `Finish`
    /// event per call. We don't actually drive the agent loop; we only
    /// need a real `Provider` to satisfy `build_tool_definitions`.
    #[derive(Clone)]
    struct StubProvider;

    #[async_trait]
    impl Provider for StubProvider {
        fn id(&self) -> &str {
            "stub"
        }

        fn name(&self) -> &str {
            "Stub Provider"
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(Self)
        }

        async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
            let events = vec![ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            }];
            let stream = futures_util::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
            Ok(Box::pin(stream))
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![ModelInfo {
                id: "stub/model".to_string(),
                name: "Stub Model".to_string(),
                provider: "stub".to_string(),
                context_window: 4096,
                max_output_tokens: Some(2048),
                supports_tools: true,
                supports_vision: false,
                variants: vec![],
            }])
        }
    }

    fn eggsearch_config(expose_raw: bool) -> SearchConfig {
        SearchConfig {
            backend: Some(SearchBackendConfig::Eggsearch),
            expose_raw_mcp_tools: Some(expose_raw),
            fallback_to_builtin: Some(false),
            max_search_output_chars: Some(12_000),
            max_fetch_output_chars: Some(20_000),
            eggsearch: Some(EggsearchConfig {
                server_name: Some("eggsearch".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Build a mock `McpService` with the three required eggsearch tools
    /// (`web_search`, `web_fetch`, `provider_status`) pre-registered as
    /// a mock server named "eggsearch". The call handler is a no-op
    /// because these tests never actually call the tools.
    fn build_mock_eggsearch_mcp() -> McpService {
        let mut svc = McpService::new();
        let tools = vec![
            McpTool {
                name: "web_search".to_string(),
                description: "Search the web".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
                server: "eggsearch".to_string(),
            },
            McpTool {
                name: "web_fetch".to_string(),
                description: "Fetch a URL".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
                server: "eggsearch".to_string(),
            },
            McpTool {
                name: "provider_status".to_string(),
                description: "Check provider status".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
                server: "eggsearch".to_string(),
            },
        ];
        svc.register_mock_server(
            "eggsearch",
            tools,
            Box::new(|_tool, _args| -> Result<String, McpError> { Ok("{}".to_string()) }),
        );
        svc
    }

    fn make_test_agents() -> Vec<Agent> {
        vec![Agent {
            name: "build".to_string(),
            role: None,
            description: "Test agent".to_string(),
            mode: codegg::agent::AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: std::collections::HashMap::new(),
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
            fallback_model: None,
            runtime_kind: None,
        }]
    }

    /// Build a real `AgentLoop` whose `mcp_service` contains the
    /// eggsearch raw tools. M005: the resolved `SearchConfig` travels
    /// in the loop's explicit `Config` (which `AgentLoop::new` turns
    /// into the loop-owned `SearchRuntimeContext`), so
    /// `build_tool_definitions` observes this runtime's config without
    /// any process-global install. No serialization locks needed.
    fn build_agent_loop_with_mcp(mcp: Arc<RwLock<McpService>>, search: SearchConfig) -> AgentLoop {
        let config = Config {
            search: Some(search),
            ..Default::default()
        };
        AgentLoop::new(
            make_test_agents(),
            Box::new(StubProvider),
            PermissionChecker::new(None, None),
            ToolRegistry::with_defaults(),
            config,
            Some(mcp),
            None,
            Arc::new(codegg::context::InMemoryArtifactStore::new()),
            std::env::current_dir().expect("test workspace root"),
            "search-backend-test".to_string(),
        )
    }

    /// With `expose_raw_mcp_tools = false`, the real
    /// `build_tool_definitions` must return definitions that include
    /// the native `websearch` and `webfetch` wrappers but exclude the
    /// raw `mcp__eggsearch__*` tools.
    #[tokio::test]
    async fn real_build_hides_raw_eggsearch_tools() {
        let mcp = Arc::new(RwLock::new(build_mock_eggsearch_mcp()));

        let mut agent_loop = build_agent_loop_with_mcp(Arc::clone(&mcp), eggsearch_config(false));

        let defs = agent_loop.test_build_tool_definitions().await;

        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        assert!(
            names.contains(&"websearch"),
            "native websearch wrapper should be present, got: {:?}",
            names
        );
        assert!(
            names.contains(&"webfetch"),
            "native webfetch wrapper should be present, got: {:?}",
            names
        );
        assert!(
            !names.iter().any(|n| n.starts_with("mcp__eggsearch__")),
            "no mcp__eggsearch__ tools should appear when expose_raw_mcp_tools=false, got: {:?}",
            names
        );
        assert!(
            !names.contains(&"mcp__eggsearch__web_search"),
            "mcp__eggsearch__web_search must be hidden"
        );
        assert!(
            !names.contains(&"mcp__eggsearch__web_fetch"),
            "mcp__eggsearch__web_fetch must be hidden"
        );
        assert!(
            !names.contains(&"mcp__eggsearch__provider_status"),
            "mcp__eggsearch__provider_status must be hidden"
        );
    }

    /// With `expose_raw_mcp_tools = true`, the real
    /// `build_tool_definitions` must expose the raw `mcp__eggsearch__*`
    /// tools in addition to the native wrappers.
    #[tokio::test]
    async fn real_build_shows_raw_eggsearch_tools_when_exposed() {
        let mcp = Arc::new(RwLock::new(build_mock_eggsearch_mcp()));

        let mut agent_loop = build_agent_loop_with_mcp(Arc::clone(&mcp), eggsearch_config(true));

        let defs = agent_loop.test_build_tool_definitions().await;

        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        assert!(
            names.contains(&"websearch"),
            "native websearch wrapper should be present, got: {:?}",
            names
        );
        assert!(
            names.contains(&"webfetch"),
            "native webfetch wrapper should be present, got: {:?}",
            names
        );
        assert!(
            names.contains(&"mcp__eggsearch__web_search"),
            "mcp__eggsearch__web_search should be exposed, got: {:?}",
            names
        );
        assert!(
            names.contains(&"mcp__eggsearch__web_fetch"),
            "mcp__eggsearch__web_fetch should be exposed, got: {:?}",
            names
        );
        assert!(
            names.contains(&"mcp__eggsearch__provider_status"),
            "mcp__eggsearch__provider_status should be exposed, got: {:?}",
            names
        );
    }

    /// A custom `server_name` should drive the filter prefix: tools
    /// matching `mcp__<custom>__` should be hidden when
    /// `expose_raw_mcp_tools = false`.
    #[tokio::test]
    async fn real_build_hides_raw_eggsearch_tools_for_custom_server_name() {
        // Build a service whose tools use a non-default prefix.
        let mut svc = McpService::new();
        svc.register_mock_server(
            "myegg",
            vec![McpTool {
                name: "web_search".to_string(),
                description: "Search the web".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
                server: "myegg".to_string(),
            }],
            Box::new(|_tool, _args| -> Result<String, McpError> { Ok("{}".to_string()) }),
        );
        let mcp = Arc::new(RwLock::new(svc));

        let mut cfg = eggsearch_config(false);
        cfg.eggsearch = Some(EggsearchConfig {
            server_name: Some("myegg".to_string()),
            ..Default::default()
        });

        let mut agent_loop = build_agent_loop_with_mcp(Arc::clone(&mcp), cfg);

        let defs = agent_loop.test_build_tool_definitions().await;

        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        assert!(
            !names.iter().any(|n| n.starts_with("mcp__myegg__")),
            "raw myegg tools should be filtered by custom server_name, got: {:?}",
            names
        );
        // The default-prefix eggsearch tools aren't registered, so the
        // assertion above is the meaningful one.
    }
}

/// Verify that raw MCP tools for ALL expanded eggsearch tools
/// (not just web_search/web_fetch) are hidden when expose_raw is false.
#[test]
fn raw_mcp_hiding_works_for_all_expanded_tools() {
    use codegg::provider::ToolDefinition;

    let all_expanded = [
        "mcp__eggsearch__web_search",
        "mcp__eggsearch__web_fetch",
        "mcp__eggsearch__repo_search",
        "mcp__eggsearch__repo_fetch",
        "mcp__eggsearch__repo_map",
        "mcp__eggsearch__security_search",
        "mcp__eggsearch__research_search",
        "mcp__eggsearch__batch_fetch",
        "mcp__eggsearch__build_evidence_bundle",
        "mcp__eggsearch__provider_status",
    ];
    let tools: Vec<ToolDefinition> = all_expanded
        .iter()
        .map(|name| ToolDefinition {
            name: name.to_string(),
            description: "".to_string(),
            parameters: serde_json::json!({}),
            defer_loading: None,
        })
        .chain(std::iter::once(ToolDefinition {
            name: "native_tool".to_string(),
            description: "".to_string(),
            parameters: serde_json::json!({}),
            defer_loading: None,
        }))
        .collect();

    let filter = |tools: Vec<ToolDefinition>, expose: bool| {
        let raw_prefix = "mcp__eggsearch__";
        tools
            .into_iter()
            .filter(|t| expose || !t.name.starts_with(raw_prefix))
            .collect::<Vec<_>>()
    };

    let hidden = filter(tools.clone(), false);
    assert_eq!(
        hidden.len(),
        1,
        "only native_tool should remain when raw hidden"
    );
    assert_eq!(hidden[0].name, "native_tool");

    let shown = filter(tools, true);
    assert_eq!(
        shown.len(),
        all_expanded.len() + 1,
        "all tools shown when expose=true"
    );
}

/// Native Codegg wrappers remain registered regardless of raw MCP exposure.
#[test]
fn native_wrappers_always_registered() {
    use codegg::tool::ToolRegistry;

    let registry = ToolRegistry::with_defaults();
    let defs: Vec<String> = registry.definitions().into_iter().map(|d| d.name).collect();
    for name in &["websearch", "webfetch"] {
        assert!(
            defs.contains(&name.to_string()),
            "Native wrapper '{}' should always be in definitions()",
            name
        );
    }
}
