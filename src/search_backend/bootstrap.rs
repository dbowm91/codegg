//! Bootstrap: connect the eggsearch MCP server from `[search.eggsearch]`
//! (or the existing `[mcp.eggsearch]` if the user configured one
//! explicitly), then install the resolved `SearchConfig` and the
//! `McpService` into the search_backend state slot.
//!
//! Startup is intentionally non-fatal when eggsearch is missing: the
//! wrapper tools will return a clear actionable error and the
//! agent loop continues without the raw MCP tools exposed.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::schema::{Config, SearchBackendConfig, SearchConfig};
use crate::mcp::McpService;

use super::state;

/// Connect eggsearch from config and install shared state. Returns
/// the `Arc<RwLock<McpService>>` that the agent loop should use, or
/// `None` if no MCP service was created (e.g. when the search backend
/// is `disabled`).
///
/// Safe to call multiple times: the underlying `state` slots are
/// `OnceLock`s, so a second call from a different process entry point
/// (e.g. the daemon's `TurnSubmit` handler when the TUI has already
/// bootstrapped) is a no-op for state installation. The
/// `McpService::connect_stdio` step is skipped if a service is
/// already present.
pub async fn bootstrap_search_backend(
    config: &Config,
) -> (Option<Arc<RwLock<McpService>>>, BootstrapReport) {
    if state::mcp_service().is_some() {
        // Already bootstrapped; synthesize a minimal report.
        let effective = effective_search_config(config);
        let report = BootstrapReport {
            search_backend: Some(format!("{:?}", effective.backend()).to_lowercase()),
            expose_raw_mcp_tools: effective.expose_raw_mcp_tools(),
            fallback_to_builtin: effective.fallback_to_builtin(),
            max_search_output_chars: effective.max_search_output_chars(),
            max_fetch_output_chars: effective.max_fetch_output_chars(),
            max_repo_output_chars: effective.max_repo_output_chars(),
            max_repo_search_output_chars: effective.max_repo_search_output_chars(),
            max_repo_fetch_output_chars: effective.max_repo_fetch_output_chars(),
            max_repo_map_output_chars: effective.max_repo_map_output_chars(),
            max_security_output_chars: effective.max_security_output_chars(),
            max_research_output_chars: effective.max_research_output_chars(),
            max_batch_output_chars: effective.max_batch_output_chars(),
            max_evidence_output_chars: effective.max_evidence_output_chars(),
            note: Some("McpService already installed; reusing".to_string()),
            ..Default::default()
        };
        return (state::mcp_service(), report);
    }
    let report = bootstrap_eggsearch(config).await;
    (state::mcp_service(), report)
}

/// Connect eggsearch if it is the configured backend and the user has
/// not already registered an explicit `mcp.eggsearch` block.
///
/// Returns a summary of what happened so the doctor command can
/// surface it.
pub async fn bootstrap_eggsearch(config: &Config) -> BootstrapReport {
    let mut report = BootstrapReport::default();

    // Step 1: resolve the effective search config.
    let effective = effective_search_config(config);
    state::install_search_config(effective.clone());
    report.search_backend = Some(format!("{:?}", effective.backend()).to_lowercase());
    report.expose_raw_mcp_tools = effective.expose_raw_mcp_tools();
    report.fallback_to_builtin = effective.fallback_to_builtin();
    report.max_search_output_chars = effective.max_search_output_chars();
    report.max_fetch_output_chars = effective.max_fetch_output_chars();
    report.max_repo_output_chars = effective.max_repo_output_chars();
    report.max_repo_search_output_chars = effective.max_repo_search_output_chars();
    report.max_repo_fetch_output_chars = effective.max_repo_fetch_output_chars();
    report.max_repo_map_output_chars = effective.max_repo_map_output_chars();
    report.max_security_output_chars = effective.max_security_output_chars();
    report.max_research_output_chars = effective.max_research_output_chars();
    report.max_batch_output_chars = effective.max_batch_output_chars();
    report.max_evidence_output_chars = effective.max_evidence_output_chars();

    if !matches!(effective.backend(), SearchBackendConfig::Eggsearch) {
        report.note = Some(format!(
            "search backend is {:?}; not bootstrapping eggsearch",
            effective.backend()
        ));
        return report;
    }

    let egg_cfg = effective.eggsearch.clone().unwrap_or_default();
    if egg_cfg.enabled == Some(false) {
        report.note = Some("[search.eggsearch] enabled = false".to_string());
        return report;
    }
    let server_name = egg_cfg.server_name().to_string();
    report.server_name = Some(server_name.clone());

    // Step 2: build the McpService and connect.
    let mut mcp_service = McpService::new();
    connect_explicit_if_present(config, &mut mcp_service, &server_name, &mut report).await;

    if !report.already_connected {
        let command = egg_cfg.command().to_string();
        let args = egg_cfg.args();
        let env = egg_cfg.env();
        let timeout = egg_cfg.timeout_ms();
        report.command = Some(egg_cfg.command().to_string());

        match mcp_service
            .connect_stdio(&server_name, &command, &args, env.clone(), timeout)
            .await
        {
            Ok(()) => {
                report.connected = true;
                report.tools = list_tool_names(&mcp_service, &server_name).await;
            }
            Err(e) => {
                report.connection_error = Some(format!("{e}"));
            }
        }
    }

    // Step 3: record the effective default timeout.
    report.timeout_ms = Some(
        effective
            .eggsearch
            .as_ref()
            .map(|e| e.timeout_ms())
            .unwrap_or(60_000),
    );

    // Step 4: best-effort provider_status call (never break startup).
    if report.connected {
        let server = effective_server_name(&effective);
        let ps_timeout = effective
            .eggsearch
            .as_ref()
            .map(|e| e.timeout_ms_for(crate::config::schema::ToolTimeoutKind::ProviderStatus))
            .unwrap_or(15_000);
        match super::eggsearch::call_provider_status(&server, ps_timeout).await {
            Ok(raw) => {
                report.provider_status_ok = true;
                // Truncate to a reasonable size for doctor output.
                let display = if raw.len() > 512 {
                    format!("{}... (truncated)", &raw[..512])
                } else {
                    raw
                };
                report.provider_status_summary = Some(display);
            }
            Err(e) => {
                report.provider_status_summary = Some(format!("unavailable: {e}"));
            }
        }

        // Step 5: required/recommended tool coverage.
        let required = ["web_search", "web_fetch"];
        let recommended = [
            "repo_search",
            "repo_fetch",
            "repo_map",
            "security_search",
            "research_search",
            "batch_fetch",
            "build_evidence_bundle",
        ];
        let discovered: Vec<String> = report.tools.clone();
        for tool in required.iter().chain(recommended.iter()) {
            report
                .required_tool_coverage
                .push((tool.to_string(), discovered.iter().any(|t| t == tool)));
        }
    }

    let svc = Arc::new(RwLock::new(mcp_service));
    state::install_mcp_service(svc);
    report
}

async fn connect_explicit_if_present(
    config: &Config,
    mcp_service: &mut McpService,
    server_name: &str,
    report: &mut BootstrapReport,
) {
    let Some(entries) = config.mcp.as_ref() else {
        return;
    };
    let Some(entry) = entries.get(server_name) else {
        return;
    };
    if entry.enabled == Some(false) {
        return;
    }
    let Some(server_cfg) = entry.inner.as_ref() else {
        return;
    };
    let server_type = server_cfg.server_type.as_deref().unwrap_or("local");
    let timeout = server_cfg.timeout.unwrap_or(60_000);
    let env = server_cfg
        .env
        .clone()
        .or_else(|| server_cfg.environment.clone())
        .unwrap_or_default();
    let env: HashMap<String, String> = env.into_iter().collect();
    let result = mcp_service
        .connect_from_config(
            server_name,
            server_type,
            server_cfg.command.as_deref(),
            server_cfg.args.as_deref(),
            Some(env),
            server_cfg.url.as_deref(),
            server_cfg.headers.clone(),
            timeout,
        )
        .await;
    match result {
        Ok(()) => {
            report.already_connected = true;
            report.connected = true;
            report.tools = list_tool_names(mcp_service, server_name).await;
            if let Some(cmd) = &server_cfg.command {
                let args = server_cfg.args.clone().unwrap_or_default();
                report.command = Some(format!("{} {}", cmd, args.join(" ")));
            }
        }
        Err(e) => {
            report.connection_error = Some(format!("explicit mcp.{server_name}: {e}"));
        }
    }
}

async fn list_tool_names(mcp_service: &McpService, server: &str) -> Vec<String> {
    let tools = mcp_service.server_tools();
    tools
        .get(server)
        .map(|t| t.iter().map(|x| x.name.clone()).collect())
        .unwrap_or_default()
}

/// Return the effective `SearchConfig` after defaults. Currently this
/// just clones the user config; the helper exists so that future
/// migrations (e.g. synthesizing `[search]` from legacy keys) have a
/// single place to live.
pub fn effective_search_config(config: &Config) -> SearchConfig {
    config.search.clone().unwrap_or_default()
}

fn effective_server_name(cfg: &SearchConfig) -> String {
    cfg.eggsearch
        .as_ref()
        .and_then(|e| e.server_name.clone())
        .unwrap_or_else(|| "eggsearch".to_string())
}

#[derive(Debug, Default, Clone)]
pub struct BootstrapReport {
    pub search_backend: Option<String>,
    pub expose_raw_mcp_tools: bool,
    pub fallback_to_builtin: bool,
    pub max_search_output_chars: usize,
    pub max_fetch_output_chars: usize,
    pub max_repo_output_chars: usize,
    pub max_repo_search_output_chars: usize,
    pub max_repo_fetch_output_chars: usize,
    pub max_repo_map_output_chars: usize,
    pub max_security_output_chars: usize,
    pub max_research_output_chars: usize,
    pub max_batch_output_chars: usize,
    pub max_evidence_output_chars: usize,
    pub server_name: Option<String>,
    pub command: Option<String>,
    pub connected: bool,
    pub already_connected: bool,
    pub connection_error: Option<String>,
    pub tools: Vec<String>,
    pub note: Option<String>,
    /// Default timeout in milliseconds for tool calls.
    pub timeout_ms: Option<u64>,
    /// Best-effort provider_status summary (JSON string).
    pub provider_status_summary: Option<String>,
    /// Whether provider_status call succeeded.
    pub provider_status_ok: bool,
    /// List of required upstream tools (web_search, web_fetch) and whether
    /// they were discovered on the server.
    pub required_tool_coverage: Vec<(String, bool)>,
}

impl BootstrapReport {
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!(
            "Search backend: {}",
            self.search_backend.as_deref().unwrap_or("?")
        ));
        if let Some(cmd) = &self.command {
            lines.push(format!("Command: {cmd}"));
        }
        if let Some(name) = &self.server_name {
            lines.push(format!("Server name: {name}"));
        }
        lines.push(format!(
            "Eggsearch MCP: {}",
            if self.connected {
                if self.already_connected {
                    "connected (from explicit [mcp.eggsearch])".to_string()
                } else {
                    "connected".to_string()
                }
            } else if let Some(err) = &self.connection_error {
                format!("unavailable ({err})")
            } else {
                "not configured".to_string()
            }
        ));
        if !self.tools.is_empty() {
            lines.push(format!("Tools: {}", self.tools.join(", ")));
        } else if self.connected {
            lines.push("Tools: (none discovered)".to_string());
        }
        // Required/recommended tool coverage.
        if !self.required_tool_coverage.is_empty() {
            let missing: Vec<&str> = self
                .required_tool_coverage
                .iter()
                .filter(|(_, found)| !found)
                .map(|(name, _)| name.as_str())
                .collect();
            if missing.is_empty() {
                lines.push("Tool coverage: all required and recommended tools present".to_string());
            } else {
                lines.push(format!(
                    "Tool coverage: missing {} ({})",
                    missing.len(),
                    missing.join(", ")
                ));
            }
        }
        lines.push(format!(
            "Raw MCP tools exposed to model: {}",
            if self.expose_raw_mcp_tools {
                "yes"
            } else {
                "no"
            }
        ));
        lines.push(format!(
            "Fallback to built-in: {}",
            if self.fallback_to_builtin {
                "yes"
            } else {
                "no"
            }
        ));
        if let Some(timeout) = self.timeout_ms {
            lines.push(format!("Default timeout: {timeout}ms"));
        }
        lines.push(format!(
            "Output caps: search={} fetch={} repo_search={} repo_fetch={} repo_map={} \
             security={} research={} batch={} evidence={}",
            self.max_search_output_chars,
            self.max_fetch_output_chars,
            self.max_repo_search_output_chars,
            self.max_repo_fetch_output_chars,
            self.max_repo_map_output_chars,
            self.max_security_output_chars,
            self.max_research_output_chars,
            self.max_batch_output_chars,
            self.max_evidence_output_chars,
        ));
        // Provider status (best-effort).
        if self.provider_status_ok {
            lines.push("Provider status: available".to_string());
            if let Some(summary) = &self.provider_status_summary {
                lines.push(format!("Provider details: {summary}"));
            }
        } else if let Some(detail) = &self.provider_status_summary {
            lines.push(format!("Provider status: {detail}"));
        }
        if let Some(note) = &self.note {
            lines.push(format!("Note: {note}"));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::EggsearchConfig;

    #[test]
    fn effective_search_config_returns_default_when_unset() {
        let cfg = Config::default();
        let effective = effective_search_config(&cfg);
        assert_eq!(effective.backend(), SearchBackendConfig::Eggsearch);
    }

    #[test]
    fn report_summary_lists_key_fields() {
        let report = BootstrapReport {
            search_backend: Some("eggsearch".to_string()),
            command: Some("eggsearch mcp stdio".to_string()),
            server_name: Some("eggsearch".to_string()),
            connected: true,
            tools: vec!["web_search".to_string(), "web_fetch".to_string()],
            expose_raw_mcp_tools: false,
            fallback_to_builtin: false,
            max_search_output_chars: 12_000,
            max_fetch_output_chars: 20_000,
            max_repo_search_output_chars: 16_000,
            max_repo_fetch_output_chars: 24_000,
            max_repo_map_output_chars: 16_000,
            max_security_output_chars: 18_000,
            max_research_output_chars: 22_000,
            max_batch_output_chars: 30_000,
            max_evidence_output_chars: 30_000,
            timeout_ms: Some(60_000),
            provider_status_ok: true,
            provider_status_summary: Some(r#"{"duckduckgo":"ok"}"#.to_string()),
            required_tool_coverage: vec![
                ("web_search".to_string(), true),
                ("web_fetch".to_string(), true),
                ("repo_search".to_string(), false),
            ],
            ..Default::default()
        };
        let lines = report.summary_lines();
        let joined = lines.join("\n");
        assert!(joined.contains("Search backend: eggsearch"));
        assert!(joined.contains("Command: eggsearch mcp stdio"));
        assert!(joined.contains("Server name: eggsearch"));
        assert!(joined.contains("connected"));
        assert!(joined.contains("web_search, web_fetch"));
        assert!(joined.contains("Tool coverage: missing 1 (repo_search)"));
        assert!(joined.contains("Raw MCP tools exposed to model: no"));
        assert!(joined.contains("Fallback to built-in: no"));
        assert!(joined.contains("Default timeout: 60000ms"));
        assert!(joined.contains("search=12000"));
        assert!(joined.contains("fetch=20000"));
        assert!(joined.contains("repo_search=16000"));
        assert!(joined.contains("repo_fetch=24000"));
        assert!(joined.contains("repo_map=16000"));
        assert!(joined.contains("security=18000"));
        assert!(joined.contains("research=22000"));
        assert!(joined.contains("batch=30000"));
        assert!(joined.contains("evidence=30000"));
        assert!(joined.contains("Provider status: available"));
        assert!(joined.contains("Provider details:"));
    }

    #[test]
    fn report_summary_marks_unavailable_backend() {
        let report = BootstrapReport {
            search_backend: Some("eggsearch".to_string()),
            connection_error: Some("spawn failed".to_string()),
            ..Default::default()
        };
        let lines = report.summary_lines();
        let joined = lines.join("\n");
        assert!(joined.contains("unavailable"));
        assert!(joined.contains("spawn failed"));
    }

    #[test]
    fn default_search_config_uses_default_eggsearch_config() {
        let cfg = Config::default();
        let effective = effective_search_config(&cfg);
        assert_eq!(effective.backend(), SearchBackendConfig::Eggsearch);
        let egg = effective.eggsearch.clone().unwrap_or_default();
        assert_eq!(egg.server_name(), "eggsearch");
        assert_eq!(egg.command(), "eggsearch");
        assert_eq!(egg.args(), vec!["mcp".to_string(), "stdio".to_string()]);
    }

    #[tokio::test]
    async fn bootstrap_with_missing_binary_reports_command_and_error() {
        // Ensure a clean baseline so the bootstrap actually runs.
        let _cp = crate::search_backend::test_support::acquire_cross_process_lock();
        let _g = crate::search_backend::test_support::SHARED_TEST_LOCK
            .lock()
            .await;
        state::reset_for_tests();

        // Build a Config with a deliberately missing command
        let mut cfg = Config::default();
        let egg_cfg = EggsearchConfig {
            command: Some("definitely-missing-eggsearch-test-binary".to_string()),
            ..Default::default()
        };
        cfg.search = Some(SearchConfig {
            backend: Some(SearchBackendConfig::Eggsearch),
            eggsearch: Some(egg_cfg),
            ..Default::default()
        });

        // Ensure a clean baseline so the bootstrap actually runs.
        state::install_search_config(SearchConfig::default());

        let (_svc, report) = bootstrap_search_backend(&cfg).await;
        assert_eq!(report.search_backend.as_deref(), Some("eggsearch"));
        assert!(report.command.is_some());
        assert_eq!(
            report.command.as_deref(),
            Some("definitely-missing-eggsearch-test-binary")
        );
        assert!(!report.connected);
        assert!(
            report.connection_error.is_some(),
            "expected connection_error to be set, got report: {:#?}",
            report
        );
    }

    #[tokio::test]
    async fn bootstrap_with_default_config_attempts_eggsearch() {
        // Ensure a clean baseline so the bootstrap actually runs.
        let _cp = crate::search_backend::test_support::acquire_cross_process_lock();
        let _g = crate::search_backend::test_support::SHARED_TEST_LOCK
            .lock()
            .await;
        state::reset_for_tests();

        let cfg = Config::default();

        // Ensure a clean baseline so the bootstrap actually runs.
        state::install_search_config(SearchConfig::default());

        let (_svc, report) = bootstrap_search_backend(&cfg).await;
        assert_eq!(report.search_backend.as_deref(), Some("eggsearch"));
        assert_eq!(report.command.as_deref(), Some("eggsearch"));
        assert_eq!(report.server_name.as_deref(), Some("eggsearch"));
        // report.connected may be true or false depending on whether the eggsearch binary is installed,
        // but report.note should NOT be "no [search.eggsearch] section configured" anymore.
        let note = report.note.as_deref().unwrap_or("");
        assert!(!note.contains("no [search.eggsearch]"), "report.note should not be 'no [search.eggsearch] section configured' when default config is used. Got: {note}");
    }

    #[tokio::test]
    async fn reentrant_bootstrap_returns_existing_service() {
        let _cp = crate::search_backend::test_support::acquire_cross_process_lock();
        let _g = crate::search_backend::test_support::SHARED_TEST_LOCK
            .lock()
            .await;
        state::reset_for_tests();

        let cfg = Config::default();
        let (svc1, _report1) = bootstrap_search_backend(&cfg).await;
        assert!(svc1.is_some(), "first bootstrap should install McpService");

        let (svc2, report2) = bootstrap_search_backend(&cfg).await;
        assert!(svc2.is_some());
        assert!(
            report2.note.as_deref().unwrap_or("").contains("already installed")
                || report2.note.as_deref().unwrap_or("").contains("reusing"),
            "re-entrant bootstrap should note reuse, got: {:?}",
            report2.note
        );
    }

    #[tokio::test]
    async fn enabled_false_skips_eggsearch_connection() {
        let _cp = crate::search_backend::test_support::acquire_cross_process_lock();
        let _g = crate::search_backend::test_support::SHARED_TEST_LOCK
            .lock()
            .await;
        state::reset_for_tests();

        let mut cfg = Config::default();
        cfg.search = Some(SearchConfig {
            backend: Some(SearchBackendConfig::Eggsearch),
            eggsearch: Some(EggsearchConfig {
                enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        });

        let report = bootstrap_eggsearch(&cfg).await;
        assert!(!report.connected);
        assert!(
            report.note.as_deref().unwrap_or("").contains("enabled = false"),
            "expected enabled=false note, got: {:?}",
            report.note
        );
    }

    #[test]
    fn builtin_backend_skips_eggsearch_bootstrap() {
        let cfg = Config {
            search: Some(SearchConfig {
                backend: Some(SearchBackendConfig::Builtin),
                ..Default::default()
            }),
            ..Default::default()
        };
        let effective = effective_search_config(&cfg);
        assert_eq!(effective.backend(), SearchBackendConfig::Builtin);
    }

    #[test]
    fn disabled_backend_skips_eggsearch_bootstrap() {
        let cfg = Config {
            search: Some(SearchConfig {
                backend: Some(SearchBackendConfig::Disabled),
                ..Default::default()
            }),
            ..Default::default()
        };
        let effective = effective_search_config(&cfg);
        assert_eq!(effective.backend(), SearchBackendConfig::Disabled);
    }

    #[test]
    fn bootstrap_report_includes_all_caps() {
        let report = BootstrapReport {
            search_backend: Some("eggsearch".to_string()),
            max_search_output_chars: 12_000,
            max_fetch_output_chars: 20_000,
            max_repo_output_chars: 16_000,
            max_repo_search_output_chars: 16_000,
            max_repo_fetch_output_chars: 24_000,
            max_repo_map_output_chars: 16_000,
            max_security_output_chars: 18_000,
            max_research_output_chars: 22_000,
            max_batch_output_chars: 30_000,
            max_evidence_output_chars: 30_000,
            ..Default::default()
        };
        let lines = report.summary_lines();
        let joined = lines.join("\n");
        assert!(joined.contains("Output caps:"));
        assert!(joined.contains("search=12000"));
        assert!(joined.contains("fetch=20000"));
    }

    #[test]
    fn report_summary_includes_provider_status_when_failed() {
        let report = BootstrapReport {
            search_backend: Some("eggsearch".to_string()),
            provider_status_ok: false,
            provider_status_summary: Some("unavailable: timeout".to_string()),
            ..Default::default()
        };
        let lines = report.summary_lines();
        let joined = lines.join("\n");
        assert!(joined.contains("Provider status: unavailable: timeout"));
    }

    #[test]
    fn report_summary_always_connected_shows_explicit() {
        let report = BootstrapReport {
            connected: true,
            already_connected: true,
            ..Default::default()
        };
        let lines = report.summary_lines();
        let joined = lines.join("\n");
        assert!(joined.contains("explicit [mcp.eggsearch]"));
    }
}
