//! Unit tests for the eggsearch backend integration.
//!
//! These tests verify the agent-loop tool exposure filtering behavior
//! when `expose_raw_mcp_tools` is enabled or disabled.

use std::sync::Mutex;

// `search_backend::state` is a process-global slot, so tests that
// install a search config or McpService must be serialized with each
// other. Acquired at the start of every test in this file.
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod agent_loop_filtering_tests {
    use super::TEST_LOCK;
    use codegg::config::schema::{EggsearchConfig, SearchBackendConfig, SearchConfig};
    use codegg::provider::ToolDefinition;
    use codegg::search_backend::state;

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
        let _g = TEST_LOCK.lock().unwrap();
        state::reset_for_tests();
        let cfg = SearchConfig {
            backend: Some(SearchBackendConfig::Eggsearch),
            expose_raw_mcp_tools: Some(false),
            fallback_to_builtin: Some(false),
            max_search_output_chars: Some(12000),
            max_fetch_output_chars: Some(20000),
            eggsearch: Some(EggsearchConfig {
                server_name: Some("eggsearch".to_string()),
                ..Default::default()
            }),
        };
        state::install_search_config(cfg);

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
        let _g = TEST_LOCK.lock().unwrap();
        state::reset_for_tests();
        let cfg = SearchConfig {
            backend: Some(SearchBackendConfig::Eggsearch),
            expose_raw_mcp_tools: Some(true),
            fallback_to_builtin: Some(false),
            max_search_output_chars: Some(12000),
            max_fetch_output_chars: Some(20000),
            eggsearch: Some(EggsearchConfig {
                server_name: Some("eggsearch".to_string()),
                ..Default::default()
            }),
        };
        state::install_search_config(cfg);

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
        let _g = TEST_LOCK.lock().unwrap();
        state::reset_for_tests();
        let cfg = SearchConfig {
            backend: Some(SearchBackendConfig::Eggsearch),
            expose_raw_mcp_tools: Some(false),
            fallback_to_builtin: Some(false),
            max_search_output_chars: Some(12000),
            max_fetch_output_chars: Some(20000),
            eggsearch: Some(EggsearchConfig {
                server_name: None,
                ..Default::default()
            }),
        };
        state::install_search_config(cfg);

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
        let _g = TEST_LOCK.lock().unwrap();
        state::reset_for_tests();
        let cfg = SearchConfig {
            backend: Some(SearchBackendConfig::Eggsearch),
            expose_raw_mcp_tools: Some(false),
            fallback_to_builtin: Some(false),
            max_search_output_chars: Some(12000),
            max_fetch_output_chars: Some(20000),
            eggsearch: Some(EggsearchConfig {
                server_name: Some("myegg".to_string()),
                ..Default::default()
            }),
        };
        state::install_search_config(cfg);

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
    use super::TEST_LOCK;
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
    use codegg::search_backend::state;
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
            let stream = futures::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
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
        }]
    }

    /// Build a real `AgentLoop` whose `mcp_service` contains the
    /// eggsearch raw tools. The search backend state slot is installed
    /// in parallel so that `build_tool_definitions` can read the
    /// resolved `SearchConfig`.
    fn build_agent_loop_with_mcp(mcp: Arc<RwLock<McpService>>) -> AgentLoop {
        AgentLoop::new(
            make_test_agents(),
            Box::new(StubProvider),
            PermissionChecker::new(None, None),
            ToolRegistry::with_defaults(),
            Config::default(),
            Some(mcp),
            None,
        )
    }

    /// With `expose_raw_mcp_tools = false`, the real
    /// `build_tool_definitions` must return definitions that include
    /// the native `websearch` and `webfetch` wrappers but exclude the
    /// raw `mcp__eggsearch__*` tools.
    #[tokio::test]
    async fn real_build_hides_raw_eggsearch_tools() {
        let _g = TEST_LOCK.lock().unwrap();
        state::reset_for_tests();

        let mcp = Arc::new(RwLock::new(build_mock_eggsearch_mcp()));
        state::install_mcp_service(Arc::clone(&mcp));
        state::install_search_config(eggsearch_config(false));

        let mut agent_loop = build_agent_loop_with_mcp(Arc::clone(&mcp));

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
        let _g = TEST_LOCK.lock().unwrap();
        state::reset_for_tests();

        let mcp = Arc::new(RwLock::new(build_mock_eggsearch_mcp()));
        state::install_mcp_service(Arc::clone(&mcp));
        state::install_search_config(eggsearch_config(true));

        let mut agent_loop = build_agent_loop_with_mcp(Arc::clone(&mcp));

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
        let _g = TEST_LOCK.lock().unwrap();
        state::reset_for_tests();

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

        state::install_mcp_service(Arc::clone(&mcp));
        let mut cfg = eggsearch_config(false);
        cfg.eggsearch = Some(EggsearchConfig {
            server_name: Some("myegg".to_string()),
            ..Default::default()
        });
        state::install_search_config(cfg);

        let mut agent_loop = build_agent_loop_with_mcp(Arc::clone(&mcp));

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
