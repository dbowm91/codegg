//! Centralized runtime config resolution for integrated tool backends.
//!
//! This module bridges the config-time schema types (`SearchConfig`,
//! `DeterministicToolsConfig`, `PreflightConfig`) to runtime structs
//! consumed by `ToolRegistry::with_options()`. It fills defaults,
//! validates values, and emits warnings for suspicious configurations.
//!
//! See `plans/eggsearch-eggsact-phase-06-backend-config-policy.md`.

use crate::config::schema::Config;

/// Runtime config for evidence-backed search tools (websearch,
/// webfetch, repo_search, etc.). Derived from `[search]` config.
#[derive(Debug, Clone)]
pub struct EvidenceBackendRuntimeConfig {
    /// Whether the eggsearch MCP backend is enabled.
    pub enabled: bool,
    /// Whether to expose raw `mcp__eggsearch__*` tools to the model.
    pub expose_raw_mcp_tools: bool,
    /// Whether to fall back to built-in when eggsearch is unavailable.
    pub fallback_to_builtin: bool,
    /// Output caps per tool family.
    pub max_search_output_chars: usize,
    pub max_fetch_output_chars: usize,
    pub max_repo_output_chars: usize,
    pub max_security_output_chars: usize,
    pub max_research_output_chars: usize,
    pub max_batch_output_chars: usize,
    pub max_evidence_output_chars: usize,
}

impl Default for EvidenceBackendRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            expose_raw_mcp_tools: false,
            fallback_to_builtin: false,
            max_search_output_chars: 12_000,
            max_fetch_output_chars: 20_000,
            max_repo_output_chars: 15_000,
            max_security_output_chars: 12_000,
            max_research_output_chars: 12_000,
            max_batch_output_chars: 15_000,
            max_evidence_output_chars: 20_000,
        }
    }
}

/// Runtime config for eggsact-backed deterministic tools exposed
/// to the model. Derived from `[deterministic_tools]` config.
#[derive(Debug, Clone)]
pub struct DeterministicToolsRuntimeConfig {
    /// Whether deterministic tools are enabled.
    pub enabled: bool,
    /// Backend kind ("native" or "disabled").
    pub backend: String,
    /// Eggsact profile name (e.g. "codegg_core").
    pub profile: String,
    /// Tool audience for model-facing calls.
    pub model_audience: String,
    /// Tool audience for harness-side calls.
    pub harness_audience: String,
    /// Whether to expose expert-tier tools.
    pub expose_expert_tools: bool,
    /// Maximum output characters before truncation.
    pub max_output_chars: usize,
}

impl Default for DeterministicToolsRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            backend: "native".to_string(),
            profile: "codegg_core".to_string(),
            model_audience: "model".to_string(),
            harness_audience: "harness".to_string(),
            expose_expert_tools: false,
            max_output_chars: 12_000,
        }
    }
}

/// Runtime config for harness-side preflight checks. Derived from
/// `[preflight]` config.
#[derive(Debug, Clone)]
pub struct PreflightRuntimeConfig {
    /// Whether preflight is enabled.
    pub enabled: bool,
    /// Operating mode label ("off", "observe", "warn", "block_on_definite").
    pub mode: String,
    /// Enable patch/edit preflights.
    pub patch: bool,
    /// Enable config write preflights.
    pub config: bool,
    /// Enable shell command preflights.
    pub shell: bool,
    /// Enable unicode/identifier safety checks.
    pub unicode: bool,
    /// Log findings to tracing.
    pub log_findings: bool,
    /// Include findings in model-visible tool output.
    pub model_visible_findings: bool,
    /// Eggsact profile name for harness audience.
    pub profile: String,
}

impl Default for PreflightRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "warn".to_string(),
            patch: true,
            config: true,
            shell: true,
            unicode: true,
            log_findings: true,
            model_visible_findings: true,
            profile: "codegg_core".to_string(),
        }
    }
}

/// Aggregate of all integrated tool backend runtime configs.
#[derive(Debug, Clone, Default)]
pub struct IntegratedToolRuntimeConfig {
    pub evidence: Option<EvidenceBackendRuntimeConfig>,
    pub deterministic: Option<DeterministicToolsRuntimeConfig>,
    pub preflight: Option<PreflightRuntimeConfig>,
}

/// Known eggsact profiles. Used to validate user config and warn
/// on typos or unsupported profile names.
const KNOWN_EGGSACT_PROFILES: &[&str] = &["codegg_core", "codegg_core_min", "default", "full"];

/// Resolve all integrated tool runtime configs from a loaded `Config`.
///
/// Fills defaults for missing fields, validates values, and logs
/// warnings for suspicious configurations (e.g. unknown profiles,
/// disabled tools with non-default settings).
pub fn resolve_integrated_config(config: &Config) -> IntegratedToolRuntimeConfig {
    let evidence = resolve_evidence_config(config);
    let deterministic = resolve_deterministic_config(config);
    let preflight = resolve_preflight_config(config);

    IntegratedToolRuntimeConfig {
        evidence: Some(evidence),
        deterministic: Some(deterministic),
        preflight: Some(preflight),
    }
}

fn resolve_evidence_config(config: &Config) -> EvidenceBackendRuntimeConfig {
    let search = &config.search;
    let sc = search.as_ref();
    let enabled = sc
        .map(|s| !matches!(s.backend(), crate::config::schema::SearchBackendConfig::Disabled))
        .unwrap_or(true);

    let expose_raw_mcp_tools = sc
        .map(|s| s.expose_raw_mcp_tools())
        .unwrap_or(false);

    let fallback_to_builtin = sc
        .map(|s| s.fallback_to_builtin())
        .unwrap_or(false);

    let max_search_output_chars = sc
        .and_then(|s| s.max_search_output_chars)
        .unwrap_or(12_000);
    let max_fetch_output_chars = sc
        .and_then(|s| s.max_fetch_output_chars)
        .unwrap_or(20_000);
    let max_repo_output_chars = sc
        .and_then(|s| s.max_repo_output_chars)
        .unwrap_or(15_000);
    let max_security_output_chars = sc
        .and_then(|s| s.max_security_output_chars)
        .unwrap_or(12_000);
    let max_research_output_chars = sc
        .and_then(|s| s.max_research_output_chars)
        .unwrap_or(12_000);
    let max_batch_output_chars = sc
        .and_then(|s| s.max_batch_output_chars)
        .unwrap_or(15_000);
    let max_evidence_output_chars = sc
        .and_then(|s| s.max_evidence_output_chars)
        .unwrap_or(20_000);

    if !enabled {
        tracing::info!("evidence backend disabled via [search].backend");
    }

    EvidenceBackendRuntimeConfig {
        enabled,
        expose_raw_mcp_tools,
        fallback_to_builtin,
        max_search_output_chars,
        max_fetch_output_chars,
        max_repo_output_chars,
        max_security_output_chars,
        max_research_output_chars,
        max_batch_output_chars,
        max_evidence_output_chars,
    }
}

fn resolve_deterministic_config(config: &Config) -> DeterministicToolsRuntimeConfig {
    let dt = match config.deterministic_tools.as_ref() {
        Some(dt) => dt,
        None => return DeterministicToolsRuntimeConfig::default(),
    };

    let profile = dt.profile.clone();
    if !KNOWN_EGGSACT_PROFILES.contains(&profile.as_str()) {
        tracing::warn!(
            profile = %profile,
            known = ?KNOWN_EGGSACT_PROFILES,
            "unknown eggsact profile; falling back to Default profile"
        );
    }

    if !dt.enabled {
        tracing::info!("deterministic tools disabled via [deterministic_tools].enabled");
    }

    DeterministicToolsRuntimeConfig {
        enabled: dt.enabled,
        backend: dt.backend.clone(),
        profile,
        model_audience: dt.model_audience.clone(),
        harness_audience: dt.harness_audience.clone(),
        expose_expert_tools: dt.expose_expert_tools,
        max_output_chars: dt.max_output_chars,
    }
}

fn resolve_preflight_config(config: &Config) -> PreflightRuntimeConfig {
    let pf = match config.preflight.as_ref() {
        Some(pf) => pf,
        None => return PreflightRuntimeConfig::default(),
    };

    let enabled = pf.enabled.unwrap_or(true);
    let mode_label = match pf.mode {
        Some(crate::config::schema::PreflightMode::Off) => "off",
        Some(crate::config::schema::PreflightMode::Observe) => "observe",
        Some(crate::config::schema::PreflightMode::Warn) => "warn",
        Some(crate::config::schema::PreflightMode::BlockOnDefinite) => "block_on_definite",
        None => "warn",
    }
    .to_string();

    if !enabled {
        tracing::info!("preflight disabled via [preflight].enabled");
    }

    PreflightRuntimeConfig {
        enabled,
        mode: mode_label,
        patch: pf.patch.unwrap_or(true),
        config: pf.config.unwrap_or(true),
        shell: pf.shell.unwrap_or(true),
        unicode: pf.unicode.unwrap_or(true),
        log_findings: pf.log_findings.unwrap_or(true),
        model_visible_findings: pf.model_visible_findings.unwrap_or(true),
        profile: "codegg_core".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_defaults_when_config_is_empty() {
        let config = Config::default();
        let resolved = resolve_integrated_config(&config);

        let evidence = resolved.evidence.unwrap();
        assert!(evidence.enabled);
        assert!(!evidence.expose_raw_mcp_tools);
        assert_eq!(evidence.max_search_output_chars, 12_000);

        let deterministic = resolved.deterministic.unwrap();
        assert!(deterministic.enabled);
        assert_eq!(deterministic.profile, "codegg_core");
        assert_eq!(deterministic.max_output_chars, 12_000);

        let preflight = resolved.preflight.unwrap();
        assert!(preflight.enabled);
        assert_eq!(preflight.mode, "warn");
        assert!(preflight.patch);
    }

    #[test]
    fn resolve_from_explicit_config() {
        use crate::config::schema::{
            DeterministicToolsConfig, PreflightConfig, PreflightMode, SearchBackendConfig,
            SearchConfig,
        };

        let config = Config {
            search: Some(SearchConfig {
                backend: Some(SearchBackendConfig::Builtin),
                expose_raw_mcp_tools: Some(true),
                max_search_output_chars: Some(8_000),
                ..Default::default()
            }),
            deterministic_tools: Some(DeterministicToolsConfig {
                enabled: false,
                profile: "codegg_core_min".to_string(),
                max_output_chars: 6_000,
                ..Default::default()
            }),
            preflight: Some(PreflightConfig {
                mode: Some(PreflightMode::BlockOnDefinite),
                patch: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };

        let resolved = resolve_integrated_config(&config);

        let evidence = resolved.evidence.unwrap();
        assert!(evidence.enabled);
        assert!(evidence.expose_raw_mcp_tools);
        assert_eq!(evidence.max_search_output_chars, 8_000);

        let deterministic = resolved.deterministic.unwrap();
        assert!(!deterministic.enabled);
        assert_eq!(deterministic.profile, "codegg_core_min");
        assert_eq!(deterministic.max_output_chars, 6_000);

        let preflight = resolved.preflight.unwrap();
        assert_eq!(preflight.mode, "block_on_definite");
        assert!(!preflight.patch);
    }

    #[test]
    fn disabled_search_marks_evidence_disabled() {
        use crate::config::schema::{SearchBackendConfig, SearchConfig};

        let config = Config {
            search: Some(SearchConfig {
                backend: Some(SearchBackendConfig::Disabled),
                ..Default::default()
            }),
            ..Default::default()
        };

        let resolved = resolve_integrated_config(&config);
        let evidence = resolved.evidence.unwrap();
        assert!(!evidence.enabled);
    }

    #[test]
    fn unknown_profile_emits_warning() {
        use crate::config::schema::DeterministicToolsConfig;

        let config = Config {
            deterministic_tools: Some(DeterministicToolsConfig {
                profile: "nonexistent_profile".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };

        // The warning is logged via tracing::warn; we just verify
        // the config resolves without panicking.
        let resolved = resolve_integrated_config(&config);
        let deterministic = resolved.deterministic.unwrap();
        assert_eq!(deterministic.profile, "nonexistent_profile");
    }
}
