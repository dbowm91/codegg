//! Unit tests for the eggsearch backend integration.
//!
//! These tests verify the agent-loop tool exposure filtering behavior
//! when `expose_raw_mcp_tools` is enabled or disabled.

#[cfg(test)]
mod agent_loop_filtering_tests {
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
            filtered.iter().all(|t| !t.name.starts_with("mcp__eggsearch__")),
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
            filtered.iter().any(|t| t.name == "mcp__eggsearch__web_search"),
            "web_search should be present"
        );
        assert!(
            filtered.iter().any(|t| t.name == "mcp__eggsearch__web_fetch"),
            "web_fetch should be present"
        );
        assert!(
            filtered.iter().any(|t| t.name == "mcp__eggsearch__provider_status"),
            "provider_status should be present"
        );
        assert!(
            filtered.iter().any(|t| t.name == "other_tool"),
            "other_tool should be present"
        );
    }

    #[test]
    fn expose_raw_uses_default_server_name() {
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
            filtered.iter().all(|t| !t.name.starts_with("mcp__eggsearch__")),
            "with default server name, all eggsearch tools should be filtered"
        );
    }

    #[test]
    fn expose_raw_with_custom_server_name() {
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