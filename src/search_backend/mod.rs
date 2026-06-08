//! Search/fetch backend abstraction.
//!
//! Codegg exposes stable native `websearch` and `webfetch` tools to
//! the agent. Internally they delegate to a pluggable backend, with
//! `eggsearch` (an external MCP server) as the default. The legacy
//! in-tree implementation is kept as an explicit fallback.
//!
//! The two wrapper tools read the resolved `SearchConfig` and the
//! shared `McpService` from the `state` module, which is populated at
//! startup by `bootstrap::bootstrap_eggsearch`.
//!
//! Public submodules:
//!
//! - [`state`]: process-wide slots for the `McpService` and
//!   `SearchConfig`. Populated once at startup.
//! - [`bootstrap`]: connect eggsearch from config and install the
//!   shared state. Non-fatal if eggsearch is missing.
//! - [`eggsearch`]: adapter that calls the eggsearch MCP tools.
//! - [`legacy`]: adapter that calls the in-tree built-in providers.
//! - [`framing`]: `external_untrusted` framing helpers and output
//!   clamping.

pub mod bootstrap;
pub mod eggsearch;
pub mod framing;
pub mod legacy;
pub mod state;

use serde_json::Value;

use crate::config::schema::{SearchBackendConfig, SearchConfig};
use crate::error::ToolError;

/// Run a native `websearch` call against the configured backend.
///
/// Returns the framed, capped output. The caller is responsible for
/// surfacing the result to the model.
pub async fn dispatch_web_search(input: &Value) -> Result<String, ToolError> {
    let cfg = state::search_config();
    let max_chars = cfg.max_search_output_chars();
    match cfg.backend() {
        SearchBackendConfig::Disabled => Err(ToolError::Execution(
            "web search is disabled ([search].backend = \"disabled\")".to_string(),
        )),
        SearchBackendConfig::Builtin => {
            let registry = legacy::legacy_registry();
            legacy::call_web_search_legacy(registry, input, max_chars, 60).await
        }
        SearchBackendConfig::Eggsearch => match state::mcp_service() {
            None => Err(eggsearch::eggsearch_unavailable(
                "McpService is not initialized",
            )),
            Some(_) => {
                let server = effective_server_name(&cfg);
                match eggsearch::call_web_search(&server, input, max_chars).await {
                    Ok(s) => Ok(s),
                    Err(e) if cfg.fallback_to_builtin() => {
                        tracing::warn!(
                            "eggsearch web_search failed ({}), falling back to built-in",
                            e
                        );
                        let registry = legacy::legacy_registry();
                        legacy::call_web_search_legacy(registry, input, max_chars, 60).await
                    }
                    Err(e) => Err(e),
                }
            }
        },
    }
}

/// Run a native `webfetch` call against the configured backend.
pub async fn dispatch_web_fetch(input: &Value) -> Result<String, ToolError> {
    let cfg = state::search_config();
    let max_chars = cfg.max_fetch_output_chars();
    match cfg.backend() {
        SearchBackendConfig::Disabled => Err(ToolError::Execution(
            "web fetch is disabled ([search].backend = \"disabled\")".to_string(),
        )),
        SearchBackendConfig::Builtin => {
            // Use the built-in reqwest-based path via a private helper.
            crate::tool::webfetch::execute_builtin(input, max_chars).await
        }
        SearchBackendConfig::Eggsearch => match state::mcp_service() {
            None => Err(eggsearch::eggsearch_unavailable(
                "McpService is not initialized",
            )),
            Some(_) => {
                let server = effective_server_name(&cfg);
                match eggsearch::call_web_fetch(&server, input, max_chars).await {
                    Ok(s) => Ok(s),
                    Err(e) if cfg.fallback_to_builtin() => {
                        tracing::warn!(
                            "eggsearch web_fetch failed ({}), falling back to built-in",
                            e
                        );
                        crate::tool::webfetch::execute_builtin(input, max_chars).await
                    }
                    Err(e) => Err(e),
                }
            }
        },
    }
}

fn effective_server_name(cfg: &SearchConfig) -> String {
    cfg.eggsearch
        .as_ref()
        .and_then(|e| e.server_name.clone())
        .unwrap_or_else(|| "eggsearch".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{SearchBackendConfig, SearchConfig};

    #[test]
    fn dispatch_disabled_backend_errors() {
        let mut cfg = SearchConfig::default();
        cfg.backend = Some(SearchBackendConfig::Disabled);
        state::install_search_config(cfg);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let res = rt.block_on(dispatch_web_search(&Value::String("hello".to_string())));
        assert!(matches!(res, Err(ToolError::Execution(_))));
    }

    #[test]
    fn dispatch_disabled_backend_errors_for_fetch() {
        let mut cfg = SearchConfig::default();
        cfg.backend = Some(SearchBackendConfig::Disabled);
        state::install_search_config(cfg);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let res = rt.block_on(dispatch_web_fetch(&serde_json::json!({"url": "https://x"})));
        assert!(matches!(res, Err(ToolError::Execution(_))));
    }

    #[test]
    fn effective_server_name_uses_default_when_unset() {
        let cfg = SearchConfig::default();
        assert_eq!(effective_server_name(&cfg), "eggsearch");
    }

    #[test]
    fn effective_server_name_uses_eggsearch_config_value() {
        let mut cfg = SearchConfig::default();
        cfg.eggsearch = Some(crate::config::schema::EggsearchConfig {
            server_name: Some("myegg".to_string()),
            ..Default::default()
        });
        assert_eq!(effective_server_name(&cfg), "myegg");
    }
}
