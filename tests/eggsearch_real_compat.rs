//! Opt-in local compatibility smoke for the documented eggsearch baseline.
//!
//! Run with `CODEGG_EGGSEARCH_BIN=/path/to/eggsearch cargo test
//! --test eggsearch_real_compat -- --ignored --nocapture`. This is deliberately
//! not part of routine CI: provider/network failures are reported separately
//! from MCP/request compatibility failures.

use codegg::config::schema::{Config, EggsearchConfig, SearchBackendConfig, SearchConfig};
use codegg::search_backend::bootstrap::bootstrap_eggsearch;
use codegg::search_backend::state;
use codegg::tool::batch_fetch::BatchFetchTool;
use codegg::tool::evidence_bundle::EvidenceBundleTool;
use codegg::tool::repo_fetch::RepoFetchTool;
use codegg::tool::repo_map::RepoMapTool;
use codegg::tool::repo_search::RepoSearchTool;
use codegg::tool::research_search::ResearchSearchTool;
use codegg::tool::security_search::SecuritySearchTool;
use codegg::tool::webfetch::WebFetchTool;
use codegg::tool::websearch::WebSearchTool;
use codegg::tool::Tool;

fn smoke_config(binary: String) -> Config {
    Config {
        search: Some(SearchConfig {
            backend: Some(SearchBackendConfig::Eggsearch),
            fallback_to_builtin: Some(false),
            eggsearch: Some(EggsearchConfig {
                command: Some(binary),
                args: Some(vec!["mcp".to_string(), "stdio".to_string()]),
                timeout_ms: Some(30_000),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[tokio::test]
#[ignore = "local opt-in real eggsearch process smoke; not network CI"]
async fn current_eggsearch_process_is_compatible_with_all_wrappers() {
    let binary = std::env::var("CODEGG_EGGSEARCH_BIN")
        .expect("set CODEGG_EGGSEARCH_BIN to the exact eggsearch binary under test");
    state::reset_for_tests();
    let report = bootstrap_eggsearch(&smoke_config(binary)).await;
    println!("bootstrap: {:?}", report.summary_lines());
    println!("tool_inventory: {:?}", report.tools);
    assert!(
        report.connected,
        "MCP bootstrap failed: {:?}",
        report.connection_error
    );
    assert_eq!(report.tool_coverage_status(), "complete");

    let cases: Vec<(Box<dyn Tool>, serde_json::Value)> = vec![
        (
            Box::new(WebSearchTool::default()),
            serde_json::json!({"query": "Rust language"}),
        ),
        (
            Box::new(WebFetchTool::default()),
            serde_json::json!({"url": "https://www.rust-lang.org/"}),
        ),
        (
            Box::new(RepoSearchTool),
            serde_json::json!({"query": "async runtime", "owner": "tokio-rs", "repo": "tokio"}),
        ),
        (
            Box::new(RepoFetchTool),
            serde_json::json!({"owner": "tokio-rs", "repo": "tokio", "path": "README.md"}),
        ),
        (
            Box::new(RepoMapTool),
            serde_json::json!({"owner": "tokio-rs", "repo": "tokio"}),
        ),
        (
            Box::new(SecuritySearchTool),
            serde_json::json!({"query": "CVE-2024-0001"}),
        ),
        (
            Box::new(ResearchSearchTool),
            serde_json::json!({"query": "Rust async runtime"}),
        ),
        (
            Box::new(BatchFetchTool),
            serde_json::json!({"items": [{"type": "web", "url": "https://www.rust-lang.org/"}]}),
        ),
        (
            Box::new(EvidenceBundleTool),
            serde_json::json!({"sources": [{"id": "rust", "url": "https://www.rust-lang.org/", "title": "Rust"}]}),
        ),
    ];

    for (tool, input) in cases {
        let name = tool.name().to_string();
        match tool.execute_structured(input, None).await {
            Ok(result) => println!(
                "wrapper={name} disposition=ok structured={}",
                result.value.is_some()
            ),
            Err(error) => {
                let message = error.to_string();
                println!("wrapper={name} disposition=provider_or_network_failure error={message}");
                assert!(
                    !message.to_ascii_lowercase().contains("invalid params")
                        && !message.to_ascii_lowercase().contains("invalid_params")
                        && !message.to_ascii_lowercase().contains("unknown field"),
                    "wrapper {name} request was rejected by eggsearch: {message}"
                );
            }
        }
    }
}
