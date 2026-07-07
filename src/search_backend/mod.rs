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
pub mod test_support;

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

/// Run a `repo_search` call against the eggsearch backend.
/// Requires `backend = "eggsearch"` — no builtin fallback.
pub async fn dispatch_repo_search(input: &Value) -> Result<String, ToolError> {
    let cfg = state::search_config();
    match cfg.backend() {
        SearchBackendConfig::Disabled => Err(ToolError::Execution(
            "repo_search is disabled ([search].backend = \"disabled\")".to_string(),
        )),
        SearchBackendConfig::Builtin => Err(ToolError::Execution(
            "repo_search requires the eggsearch backend ([search].backend = \"eggsearch\")"
                .to_string(),
        )),
        SearchBackendConfig::Eggsearch => match state::mcp_service() {
            None => Err(eggsearch::eggsearch_unavailable(
                "McpService is not initialized",
            )),
            Some(_) => {
                let server = effective_server_name(&cfg);
                let max_chars = cfg.max_repo_output_chars();
                eggsearch::call_repo_search(&server, input, max_chars).await
            }
        },
    }
}

/// Run a `repo_fetch` call against the eggsearch backend.
pub async fn dispatch_repo_fetch(input: &Value) -> Result<String, ToolError> {
    let cfg = state::search_config();
    match cfg.backend() {
        SearchBackendConfig::Disabled => Err(ToolError::Execution(
            "repo_fetch is disabled ([search].backend = \"disabled\")".to_string(),
        )),
        SearchBackendConfig::Builtin => Err(ToolError::Execution(
            "repo_fetch requires the eggsearch backend ([search].backend = \"eggsearch\")"
                .to_string(),
        )),
        SearchBackendConfig::Eggsearch => match state::mcp_service() {
            None => Err(eggsearch::eggsearch_unavailable(
                "McpService is not initialized",
            )),
            Some(_) => {
                let server = effective_server_name(&cfg);
                let max_chars = cfg.max_repo_output_chars();
                eggsearch::call_repo_fetch(&server, input, max_chars).await
            }
        },
    }
}

/// Run a `repo_map` call against the eggsearch backend.
pub async fn dispatch_repo_map(input: &Value) -> Result<String, ToolError> {
    let cfg = state::search_config();
    match cfg.backend() {
        SearchBackendConfig::Disabled => Err(ToolError::Execution(
            "repo_map is disabled ([search].backend = \"disabled\")".to_string(),
        )),
        SearchBackendConfig::Builtin => Err(ToolError::Execution(
            "repo_map requires the eggsearch backend ([search].backend = \"eggsearch\")"
                .to_string(),
        )),
        SearchBackendConfig::Eggsearch => match state::mcp_service() {
            None => Err(eggsearch::eggsearch_unavailable(
                "McpService is not initialized",
            )),
            Some(_) => {
                let server = effective_server_name(&cfg);
                let max_chars = cfg.max_repo_output_chars();
                eggsearch::call_repo_map(&server, input, max_chars).await
            }
        },
    }
}

/// Run a `security_search` call against the eggsearch backend.
pub async fn dispatch_security_search(input: &Value) -> Result<String, ToolError> {
    let cfg = state::search_config();
    match cfg.backend() {
        SearchBackendConfig::Disabled => Err(ToolError::Execution(
            "security_search is disabled ([search].backend = \"disabled\")".to_string(),
        )),
        SearchBackendConfig::Builtin => Err(ToolError::Execution(
            "security_search requires the eggsearch backend ([search].backend = \"eggsearch\")"
                .to_string(),
        )),
        SearchBackendConfig::Eggsearch => match state::mcp_service() {
            None => Err(eggsearch::eggsearch_unavailable(
                "McpService is not initialized",
            )),
            Some(_) => {
                let server = effective_server_name(&cfg);
                let max_chars = cfg.max_security_output_chars();
                eggsearch::call_security_search(&server, input, max_chars).await
            }
        },
    }
}

/// Run a `research_search` call against the eggsearch backend.
pub async fn dispatch_research_search(input: &Value) -> Result<String, ToolError> {
    let cfg = state::search_config();
    match cfg.backend() {
        SearchBackendConfig::Disabled => Err(ToolError::Execution(
            "research_search is disabled ([search].backend = \"disabled\")".to_string(),
        )),
        SearchBackendConfig::Builtin => Err(ToolError::Execution(
            "research_search requires the eggsearch backend ([search].backend = \"eggsearch\")"
                .to_string(),
        )),
        SearchBackendConfig::Eggsearch => match state::mcp_service() {
            None => Err(eggsearch::eggsearch_unavailable(
                "McpService is not initialized",
            )),
            Some(_) => {
                let server = effective_server_name(&cfg);
                let max_chars = cfg.max_research_output_chars();
                eggsearch::call_research_search(&server, input, max_chars).await
            }
        },
    }
}

/// Run a `batch_fetch` call against the eggsearch backend.
pub async fn dispatch_batch_fetch(input: &Value) -> Result<String, ToolError> {
    let cfg = state::search_config();
    match cfg.backend() {
        SearchBackendConfig::Disabled => Err(ToolError::Execution(
            "batch_fetch is disabled ([search].backend = \"disabled\")".to_string(),
        )),
        SearchBackendConfig::Builtin => Err(ToolError::Execution(
            "batch_fetch requires the eggsearch backend ([search].backend = \"eggsearch\")"
                .to_string(),
        )),
        SearchBackendConfig::Eggsearch => match state::mcp_service() {
            None => Err(eggsearch::eggsearch_unavailable(
                "McpService is not initialized",
            )),
            Some(_) => {
                let server = effective_server_name(&cfg);
                let max_chars = cfg.max_batch_output_chars();
                eggsearch::call_batch_fetch(&server, input, max_chars).await
            }
        },
    }
}

/// Run a `build_evidence_bundle` call against the eggsearch backend.
pub async fn dispatch_evidence_bundle(input: &Value) -> Result<String, ToolError> {
    let cfg = state::search_config();
    match cfg.backend() {
        SearchBackendConfig::Disabled => Err(ToolError::Execution(
            "evidence_bundle is disabled ([search].backend = \"disabled\")".to_string(),
        )),
        SearchBackendConfig::Builtin => Err(ToolError::Execution(
            "evidence_bundle requires the eggsearch backend ([search].backend = \"eggsearch\")"
                .to_string(),
        )),
        SearchBackendConfig::Eggsearch => match state::mcp_service() {
            None => Err(eggsearch::eggsearch_unavailable(
                "McpService is not initialized",
            )),
            Some(_) => {
                let server = effective_server_name(&cfg);
                let max_chars = cfg.max_evidence_output_chars();
                eggsearch::call_build_evidence_bundle(&server, input, max_chars).await
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

/// Build a `ToolProvenance` describing the current `websearch`
/// backend. Returns `None` only if the resolved `SearchConfig` cannot
/// be read (which should not happen in production).
pub fn provenance_for_search() -> Option<crate::tool::ToolProvenance> {
    Some(provenance_for_backend("websearch", None))
}

/// Build a `ToolProvenance` describing the current `webfetch`
/// backend.
pub fn provenance_for_fetch() -> Option<crate::tool::ToolProvenance> {
    Some(provenance_for_backend("webfetch", None))
}

/// Build a `ToolProvenance` describing the current `repo_search`
/// backend.
pub fn provenance_for_repo_search() -> Option<crate::tool::ToolProvenance> {
    Some(provenance_for_backend("repo_search", None))
}

/// Build a `ToolProvenance` describing the current `repo_fetch`
/// backend.
pub fn provenance_for_repo_fetch() -> Option<crate::tool::ToolProvenance> {
    Some(provenance_for_backend("repo_fetch", None))
}

/// Build a `ToolProvenance` describing the current `repo_map`
/// backend.
pub fn provenance_for_repo_map() -> Option<crate::tool::ToolProvenance> {
    Some(provenance_for_backend("repo_map", None))
}

/// Build a `ToolProvenance` describing the current `security_search`
/// backend.
pub fn provenance_for_security_search() -> Option<crate::tool::ToolProvenance> {
    Some(provenance_for_backend("security_search", None))
}

/// Build a `ToolProvenance` describing the current `research_search`
/// backend.
pub fn provenance_for_research_search() -> Option<crate::tool::ToolProvenance> {
    Some(provenance_for_backend("research_search", None))
}

/// Build a `ToolProvenance` describing the current `batch_fetch`
/// backend.
pub fn provenance_for_batch_fetch() -> Option<crate::tool::ToolProvenance> {
    Some(provenance_for_backend("batch_fetch", None))
}

/// Build a `ToolProvenance` describing the current
/// `build_evidence_bundle` backend.
pub fn provenance_for_evidence_bundle() -> Option<crate::tool::ToolProvenance> {
    Some(provenance_for_backend("build_evidence_bundle", None))
}

fn provenance_for_backend(_tool: &str, elapsed_ms: Option<u64>) -> crate::tool::ToolProvenance {
    use crate::tool::{ToolBackendKind, ToolProvenance, ToolTrust};
    let cfg = state::search_config();
    let server = effective_server_name(&cfg);
    let (backend, implementation, trust) = match cfg.backend() {
        SearchBackendConfig::Disabled => (
            ToolBackendKind::BuiltinLegacy.label().to_lowercase(),
            "disabled".to_string(),
            ToolTrust::LocalTrusted,
        ),
        SearchBackendConfig::Builtin => (
            ToolBackendKind::BuiltinLegacy.label().to_lowercase(),
            "codegg/legacy".to_string(),
            ToolTrust::ExternalUntrusted,
        ),
        SearchBackendConfig::Eggsearch => {
            let connected = state::mcp_service().is_some();
            let impl_label = if connected {
                format!("{}/search", server)
            } else {
                format!("{}/search (unavailable)", server)
            };
            let trust = if connected {
                ToolTrust::ExternalUntrusted
            } else {
                ToolTrust::LocalUntrusted
            };
            (
                ToolBackendKind::Mcp.label().to_lowercase(),
                impl_label,
                trust,
            )
        }
    };
    ToolProvenance {
        backend,
        implementation,
        version: None,
        elapsed_ms,
        truncated: false,
        trust,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{SearchBackendConfig, SearchConfig};

    #[test]
    fn dispatch_disabled_backend_errors() {
        let _cp = crate::search_backend::test_support::acquire_cross_process_lock();
        let _g = crate::search_backend::test_support::SHARED_TEST_LOCK.blocking_lock();
        state::reset_for_tests();
        let cfg = SearchConfig {
            backend: Some(SearchBackendConfig::Disabled),
            ..Default::default()
        };
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
        let _cp = crate::search_backend::test_support::acquire_cross_process_lock();
        let _g = crate::search_backend::test_support::SHARED_TEST_LOCK.blocking_lock();
        state::reset_for_tests();
        let cfg = SearchConfig {
            backend: Some(SearchBackendConfig::Disabled),
            ..Default::default()
        };
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
        let cfg = SearchConfig {
            eggsearch: Some(crate::config::schema::EggsearchConfig {
                server_name: Some("myegg".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(effective_server_name(&cfg), "myegg");
    }

    #[test]
    fn provenance_for_search_reflects_backend() {
        let _cp = crate::search_backend::test_support::acquire_cross_process_lock();
        let _g = crate::search_backend::test_support::SHARED_TEST_LOCK.blocking_lock();
        state::reset_for_tests();
        // Disabled backend.
        let cfg = SearchConfig {
            backend: Some(SearchBackendConfig::Disabled),
            ..Default::default()
        };
        state::install_search_config(cfg);
        let p = provenance_for_search().unwrap();
        assert_eq!(p.implementation, "disabled");

        // Builtin backend.
        state::reset_for_tests();
        let cfg = SearchConfig {
            backend: Some(SearchBackendConfig::Builtin),
            ..Default::default()
        };
        state::install_search_config(cfg);
        let p = provenance_for_search().unwrap();
        assert_eq!(p.implementation, "codegg/legacy");
        assert_eq!(p.backend, "builtinlegacy");

        // Eggsearch backend, not connected.
        state::reset_for_tests();
        let cfg = SearchConfig::default();
        state::install_search_config(cfg);
        let p = provenance_for_search().unwrap();
        assert_eq!(p.backend, "mcp");
        assert!(p.implementation.starts_with("eggsearch"));
    }
}
