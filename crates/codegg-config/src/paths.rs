use std::path::{Path, PathBuf};

use crate::error::ConfigError;
use crate::schema::Config;

macro_rules! merge_option {
    ($merged:expr, $config:expr, $($field:ident),*) => {
        $(if $config.$field.is_some() { $merged.$field.clone_from(&$config.$field); })*
    };
}

pub fn resolve_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(path) = std::env::var("CODEGG_TUI_CONFIG") {
        let p = PathBuf::from(path);
        if p.exists() {
            paths.push(p);
        }
    }

    if let Some(system_config) = system_config_path() {
        if system_config.exists() {
            paths.push(system_config);
        }
    }

    if let Some(global_config) = global_config_path() {
        if global_config.exists() {
            paths.push(global_config);
        }
    }

    if let Some(project_config) = find_project_config() {
        paths.push(project_config);
    }

    paths
}

pub fn find_project_config() -> Option<PathBuf> {
    let current = std::env::current_dir().ok()?;
    find_project_config_from(&current)
}

pub fn find_project_config_from(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        for dir_name in [".codegg", "codegg"] {
            for ext in ["jsonc", "json"] {
                let config_path = current.join(dir_name).join(format!("codegg.{}", ext));
                if config_path.exists() {
                    return Some(config_path);
                }
            }
        }
        if !current.pop() {
            break;
        }
    }
    None
}

pub fn global_config_path() -> Option<PathBuf> {
    let config_dir = dirs::config_dir()?;
    for file in ["codegg.jsonc", "codegg.json", "config.json"] {
        let p = config_dir.join("codegg").join(file);
        if p.exists() {
            return Some(p);
        }
    }
    Some(config_dir.join("codegg").join("codegg.jsonc"))
}

pub fn system_config_path() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(PathBuf::from(
            "/Library/Application Support/codegg/codegg.json",
        ))
    } else if cfg!(unix) {
        Some(PathBuf::from("/etc/codegg/codegg.json"))
    } else if cfg!(windows) {
        std::env::var("ProgramData")
            .ok()
            .map(|d| PathBuf::from(d).join("codegg").join("codegg.json"))
    } else {
        None
    }
}

pub fn load_config(path: &Path) -> Result<Config, ConfigError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ConfigError::NotFound(format!("{}: {}", path.display(), e)))?;
    let interpolated = interpolate_env_vars(&content);
    parse_config(&interpolated, path)
}

pub fn parse_config(content: &str, path: &Path) -> Result<Config, ConfigError> {
    let cleaned = strip_jsonc_comments(content);
    json5::from_str(&cleaned).map_err(|e| ConfigError::Parse(format!("{}: {}", path.display(), e)))
}

fn strip_jsonc_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escape_next = false;

    while let Some(c) = chars.next() {
        if escape_next {
            result.push(c);
            escape_next = false;
            continue;
        }

        if in_string {
            match c {
                '\\' => {
                    result.push(c);
                    escape_next = true;
                }
                '"' => {
                    result.push(c);
                    in_string = false;
                }
                _ => result.push(c),
            }
            continue;
        }

        match c {
            '"' => {
                result.push(c);
                in_string = true;
            }
            '/' => match chars.peek() {
                Some('/') => {
                    for nc in chars.by_ref() {
                        if nc == '\n' {
                            result.push(nc);
                            break;
                        }
                    }
                }
                Some('*') => {
                    chars.next();
                    let mut prev = '\0';
                    for nc in chars.by_ref() {
                        if prev == '*' && nc == '/' {
                            break;
                        }
                        prev = nc;
                    }
                }
                _ => result.push(c),
            },
            _ => result.push(c),
        }
    }

    result
}

pub fn merge_configs(configs: &[Config]) -> Config {
    let mut merged = Config::default();
    for config in configs {
        merge_option!(
            merged,
            config,
            schema,
            version,
            log_level,
            model,
            small_model,
            medium_model,
            auto_route_models,
            default_agent,
            username,
            share,
            autoupdate,
            disabled_providers,
            enabled_providers,
            permission,
            compaction,
            subagent,
            skills,
            templates,
            layout,
            tools,
            formatter,
            lsp,
            lsp_semantic_cache,
            snapshot,
            snapshot_config,
            plugin,
            enterprise,
            experimental,
            keybinds,
            vim_mode,
            hooks,
            notifications,
            catalog,
            context,
            context_packer,
            context_policy,
            tool_advisor,
            decision_engine,
            orchestration
        );
        if let Some(model_routers) = &config.model_routers {
            merged
                .model_routers
                .get_or_insert_with(Default::default)
                .extend(model_routers.clone());
        }
        if let Some(ref discovery) = config.discovery {
            match &mut merged.discovery {
                Some(ref mut existing) => existing.merge(discovery),
                None => merged.discovery = Some(discovery.clone()),
            }
        }
        if let Some(ref search) = config.search {
            match &mut merged.search {
                Some(ref mut existing) => {
                    if search.backend.is_some() {
                        existing.backend = search.backend;
                    }
                    if search.expose_raw_mcp_tools.is_some() {
                        existing.expose_raw_mcp_tools = search.expose_raw_mcp_tools;
                    }
                    if search.fallback_to_builtin.is_some() {
                        existing.fallback_to_builtin = search.fallback_to_builtin;
                    }
                    if search.max_search_output_chars.is_some() {
                        existing.max_search_output_chars = search.max_search_output_chars;
                    }
                    if search.max_fetch_output_chars.is_some() {
                        existing.max_fetch_output_chars = search.max_fetch_output_chars;
                    }
                    if search.max_repo_output_chars.is_some() {
                        existing.max_repo_output_chars = search.max_repo_output_chars;
                    }
                    if search.max_security_output_chars.is_some() {
                        existing.max_security_output_chars = search.max_security_output_chars;
                    }
                    if search.max_research_output_chars.is_some() {
                        existing.max_research_output_chars = search.max_research_output_chars;
                    }
                    if search.max_batch_output_chars.is_some() {
                        existing.max_batch_output_chars = search.max_batch_output_chars;
                    }
                    if search.max_evidence_output_chars.is_some() {
                        existing.max_evidence_output_chars = search.max_evidence_output_chars;
                    }
                    if search.max_repo_search_output_chars.is_some() {
                        existing.max_repo_search_output_chars = search.max_repo_search_output_chars;
                    }
                    if search.max_repo_fetch_output_chars.is_some() {
                        existing.max_repo_fetch_output_chars = search.max_repo_fetch_output_chars;
                    }
                    if search.max_repo_map_output_chars.is_some() {
                        existing.max_repo_map_output_chars = search.max_repo_map_output_chars;
                    }
                    if search.eggsearch.is_some() {
                        existing.eggsearch = search.eggsearch.clone();
                    }
                }
                None => merged.search = Some(search.clone()),
            }
        }
        if let Some(ref server) = config.server {
            match &mut merged.server {
                Some(ref mut existing) => existing.merge(server),
                None => merged.server = Some(server.clone()),
            }
        }
        if let Some(ref watcher) = config.watcher {
            match &mut merged.watcher {
                Some(ref mut existing) => {
                    if watcher.ignore.is_some() {
                        existing.ignore.clone_from(&watcher.ignore);
                    }
                    if watcher.debounce_duration_ms.is_some() {
                        existing
                            .debounce_duration_ms
                            .clone_from(&watcher.debounce_duration_ms);
                    }
                }
                None => merged.watcher = Some(watcher.clone()),
            }
        }
        if let Some(ref providers) = config.provider {
            match &mut merged.provider {
                Some(ref mut existing) => {
                    for (k, v) in providers {
                        if let Some(existing) = existing.get_mut(k) {
                            existing.merge(v);
                        } else {
                            existing.insert(k.clone(), v.clone());
                        }
                    }
                }
                None => merged.provider = Some(providers.clone()),
            }
        }
        if let Some(ref eggwork) = config.eggwork {
            match &mut merged.eggwork {
                Some(ref mut existing) => existing.merge(eggwork),
                None => merged.eggwork = Some(eggwork.clone()),
            }
        }
        if let Some(ref agents) = config.agent {
            match &mut merged.agent {
                Some(ref mut existing) => {
                    for (k, v) in agents {
                        existing.insert(k.clone(), v.clone());
                    }
                }
                None => merged.agent = Some(agents.clone()),
            }
        }
        if let Some(ref profiles) = config.model_profile {
            match &mut merged.model_profile {
                Some(existing) => {
                    for (name, profile) in profiles {
                        existing.insert(name.clone(), profile.clone());
                    }
                }
                None => merged.model_profile = Some(profiles.clone()),
            }
        }
        if let Some(ref mcp) = config.mcp {
            match &mut merged.mcp {
                Some(ref mut existing) => {
                    for (k, v) in mcp {
                        existing.insert(k.clone(), v.clone());
                    }
                }
                None => merged.mcp = Some(mcp.clone()),
            }
        }
        if let Some(ref commands) = config.commands {
            match &mut merged.commands {
                Some(ref mut existing) => {
                    for (k, v) in commands {
                        existing.insert(k.clone(), v.clone());
                    }
                }
                None => merged.commands = Some(commands.clone()),
            }
        }
        if let Some(ref instr) = config.instructions {
            merged
                .instructions
                .get_or_insert_with(Vec::new)
                .extend(instr.clone());
        }
        if let Some(ref modes) = config.mode {
            match &mut merged.mode {
                Some(ref mut existing) => {
                    for (k, v) in modes {
                        existing.insert(k.clone(), v.clone());
                    }
                }
                None => merged.mode = Some(modes.clone()),
            }
        }
        if let Some(ref theme) = config.theme {
            merged.theme = Some(theme.clone());
        }

        // Every remaining `Config` field is merged here. A field with no arm
        // below is silently discarded for every layer, so `scripts/
        // check_config_merge_coverage.py` fails the build if one is added.
        //
        // Two precedence shapes are used:
        //   * `merge()` — the section's top-level fields combine, so a later
        //     layer can override one key without restating the rest.
        //   * whole-value replace — reserved for sections whose fields are all
        //     non-`Option` (serde defaults bake in the absent case), where
        //     field-by-field combination cannot distinguish "unset" from
        //     "set to the default".
        if let Some(ref v) = config.approval_reviewer {
            match &mut merged.approval_reviewer {
                Some(existing) => existing.merge(v),
                None => merged.approval_reviewer = Some(v.clone()),
            }
        }
        if let Some(ref v) = config.daemon {
            match &mut merged.daemon {
                Some(existing) => existing.merge(v),
                None => merged.daemon = Some(v.clone()),
            }
        }
        if let Some(ref v) = config.scheduler {
            match &mut merged.scheduler {
                Some(existing) => existing.merge(v),
                None => merged.scheduler = Some(v.clone()),
            }
        }
        if let Some(ref v) = config.tool_deferral {
            match &mut merged.tool_deferral {
                Some(existing) => existing.merge(v),
                None => merged.tool_deferral = Some(v.clone()),
            }
        }
        if let Some(ref v) = config.research {
            match &mut merged.research {
                Some(existing) => existing.merge(v),
                None => merged.research = Some(v.clone()),
            }
        }
        if let Some(ref v) = config.tool_backends {
            match &mut merged.tool_backends {
                Some(existing) => existing.merge(v),
                None => merged.tool_backends = Some(v.clone()),
            }
        }
        if let Some(ref v) = config.human_shell {
            match &mut merged.human_shell {
                Some(existing) => existing.merge(v),
                None => merged.human_shell = Some(v.clone()),
            }
        }
        if let Some(ref v) = config.shell {
            match &mut merged.shell {
                Some(existing) => existing.merge(v),
                None => merged.shell = Some(v.clone()),
            }
        }
        if let Some(ref v) = config.preflight {
            match &mut merged.preflight {
                Some(existing) => existing.merge(v),
                None => merged.preflight = Some(v.clone()),
            }
        }
        if let Some(ref v) = config.command_intent {
            match &mut merged.command_intent {
                Some(existing) => existing.merge(v),
                None => merged.command_intent = Some(v.clone()),
            }
        }
        // Whole-value replace: all fields are non-`Option`.
        if let Some(ref v) = config.provider_connections {
            merged.provider_connections = Some(v.clone());
        }
        if let Some(ref v) = config.security {
            merged.security = Some(v.clone());
        }
        if let Some(ref v) = config.deterministic_tools {
            merged.deterministic_tools = Some(v.clone());
        }
    }
    merged
}

pub fn interpolate_env_vars(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next();
            let mut var_name = String::new();
            let mut terminated = false;
            while let Some(&nc) = chars.peek() {
                if nc == '}' {
                    chars.next();
                    terminated = true;
                    break;
                }
                var_name.push(nc);
                chars.next();
            }
            if !terminated {
                // Unterminated `${`: emit the literal text rather than
                // silently dropping the rest of the string.
                result.push_str("${");
                result.push_str(&var_name);
            } else if let Ok(val) = std::env::var(&var_name) {
                result.push_str(&val);
            }
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_strip_jsonc_line_comments() {
        let input = r#"{
  // this is a comment
  "key": "value"
}"#;
        let output = strip_jsonc_comments(input);
        assert!(output.contains("\"key\": \"value\""));
        assert!(!output.contains("// this is a comment"));
    }

    #[test]
    fn test_strip_jsonc_block_comments() {
        let input = r#"{
  /* block comment */
  "key": "value"
}"#;
        let output = strip_jsonc_comments(input);
        assert!(output.contains("\"key\": \"value\""));
        assert!(!output.contains("block comment"));
    }

    #[test]
    fn test_strip_jsonc_preserves_strings_with_slashes() {
        let input = r#"{"url": "http://example.com"}"#;
        let output = strip_jsonc_comments(input);
        assert!(output.contains("http://example.com"));
    }

    #[test]
    fn test_interpolate_env_vars() {
        std::env::set_var("TEST_CONFIG_VAR", "test_value");
        let input = r#"{"key": "${TEST_CONFIG_VAR}"}"#;
        let output = interpolate_env_vars(input);
        assert!(output.contains("test_value"));
        std::env::remove_var("TEST_CONFIG_VAR");
    }

    #[test]
    fn test_interpolate_env_vars_missing_var() {
        let input = r#"{"key": "${NONEXISTENT_VAR_12345}"}"#;
        let output = interpolate_env_vars(input);
        assert!(output.contains(r#""key": """#));
    }

    #[test]
    fn test_interpolate_env_vars_unterminated_keeps_literal() {
        let input = "prefix ${UNCLOSED and more";
        let output = interpolate_env_vars(input);
        assert_eq!(output, "prefix ${UNCLOSED and more");
        assert_eq!(interpolate_env_vars("a ${b"), "a ${b");
    }

    #[test]
    fn test_merge_configs_later_overrides_earlier() {
        let c1 = Config {
            log_level: Some("warn".to_string()),
            model: Some("provider/model1".to_string()),
            ..Default::default()
        };
        let c2 = Config {
            log_level: Some("debug".to_string()),
            ..Default::default()
        };
        let merged = merge_configs(&[c1, c2]);
        assert_eq!(merged.log_level, Some("debug".to_string()));
        assert_eq!(merged.model, Some("provider/model1".to_string()));
    }

    #[test]
    fn test_merge_configs_merges_model_profiles_and_orchestration() {
        let first = Config {
            model_profile: Some(std::collections::HashMap::from([(
                "fixture/model".into(),
                crate::schema::ModelProfileConfig {
                    prompt_profile: Some(crate::schema::PromptProfileKind::Reviewer),
                    ..Default::default()
                },
            )])),
            orchestration: Some(crate::schema::OrchestrationConfig {
                auto_convergence: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let second = Config {
            model_profile: Some(std::collections::HashMap::from([(
                "fixture/model".into(),
                crate::schema::ModelProfileConfig {
                    orchestration_tier: Some(crate::schema::OrchestrationTier::ConvergenceCapable),
                    ..Default::default()
                },
            )])),
            ..Default::default()
        };
        let merged = merge_configs(&[first, second]);
        assert_eq!(
            merged
                .model_profile
                .unwrap()
                .get("fixture/model")
                .unwrap()
                .orchestration_tier,
            Some(crate::schema::OrchestrationTier::ConvergenceCapable)
        );
        assert!(merged.orchestration.unwrap().auto_convergence);
    }

    #[test]
    fn test_merge_configs_merges_discovery_roots_by_id() {
        let first = Config {
            discovery: Some(crate::schema::DiscoveryConfig {
                enabled: Some(true),
                roots: Some(vec![crate::schema::DiscoveryRootConfig {
                    id: Some("work".to_string()),
                    path: Some("/workspaces".to_string()),
                    max_depth: Some(5),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let second = Config {
            discovery: Some(crate::schema::DiscoveryConfig {
                max_concurrent_scans: Some(2),
                roots: Some(vec![crate::schema::DiscoveryRootConfig {
                    id: Some("work".to_string()),
                    path: Some("/renamed".to_string()),
                    max_candidates: Some(25),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let merged = merge_configs(&[first, second]);
        let discovery = merged.discovery.expect("discovery config");
        assert_eq!(discovery.max_concurrent_scans(), 2);
        assert_eq!(discovery.roots().len(), 1);
        assert_eq!(discovery.roots()[0].path(), Some("/renamed"));
        assert_eq!(discovery.roots()[0].max_depth(), 5);
        assert_eq!(discovery.roots()[0].max_candidates(), 25);
    }

    #[test]
    fn test_merge_configs_merges_provider_maps() {
        let mut providers1 = HashMap::new();
        providers1.insert(
            "anthropic".to_string(),
            crate::schema::ProviderConfig {
                api_key: Some("key1".to_string()),
                ..Default::default()
            },
        );
        let c1 = Config {
            provider: Some(providers1),
            ..Default::default()
        };

        let mut providers2 = HashMap::new();
        providers2.insert(
            "openai".to_string(),
            crate::schema::ProviderConfig {
                api_key: Some("key2".to_string()),
                ..Default::default()
            },
        );
        let c2 = Config {
            provider: Some(providers2),
            ..Default::default()
        };

        let merged = merge_configs(&[c1, c2]);
        let providers = merged.provider.unwrap();
        assert!(providers.contains_key("anthropic"));
        assert!(providers.contains_key("openai"));
    }

    #[test]
    fn test_merge_configs_merges_agent_maps() {
        let mut agents1 = HashMap::new();
        agents1.insert(
            "build".to_string(),
            crate::schema::AgentConfig {
                model: Some("model1".to_string()),
                ..Default::default()
            },
        );
        let c1 = Config {
            agent: Some(agents1),
            ..Default::default()
        };

        let mut agents2 = HashMap::new();
        agents2.insert(
            "plan".to_string(),
            crate::schema::AgentConfig {
                model: Some("model2".to_string()),
                ..Default::default()
            },
        );
        let c2 = Config {
            agent: Some(agents2),
            ..Default::default()
        };

        let merged = merge_configs(&[c1, c2]);
        let agents = merged.agent.unwrap();
        assert!(agents.contains_key("build"));
        assert!(agents.contains_key("plan"));
    }

    #[test]
    fn test_parse_config_json5() {
        let input = r#"{
  log_level: "info",
  model: "anthropic/claude-sonnet-4-20250514",
}"#;
        let config = parse_config(input, Path::new("test.json")).unwrap();
        assert_eq!(config.log_level, Some("info".to_string()));
        assert_eq!(
            config.model,
            Some("anthropic/claude-sonnet-4-20250514".to_string())
        );
    }

    #[test]
    fn test_parse_config_with_comments() {
        let input = r#"{
  // log level comment
  "log_level": "debug",
  /* another comment */
  "model": "openai/gpt-4"
}"#;
        let config = parse_config(input, Path::new("test.json")).unwrap();
        assert_eq!(config.log_level, Some("debug".to_string()));
        assert_eq!(config.model, Some("openai/gpt-4".to_string()));
    }

    #[test]
    fn test_parse_config_with_env_interpolation() {
        std::env::set_var("MY_API_KEY", "secret123");
        let input = r#"{"provider": {"anthropic": {"api_key": "${MY_API_KEY}"}}}"#;
        let interpolated = interpolate_env_vars(input);
        let config = parse_config(&interpolated, Path::new("test.json")).unwrap();
        let providers = config.provider.unwrap();
        let anthropic = providers.get("anthropic").unwrap();
        assert_eq!(anthropic.api_key, Some("secret123".to_string()));
        std::env::remove_var("MY_API_KEY");
    }

    #[test]
    fn test_validate_log_level() {
        let config = Config {
            log_level: Some("invalid".to_string()),
            ..Default::default()
        };
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("log_level")));
    }

    #[test]
    fn test_validate_share() {
        let config = Config {
            share: Some("invalid".to_string()),
            ..Default::default()
        };
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("share")));
    }

    #[test]
    fn test_validate_model_format() {
        let config = Config {
            model: Some("just-model".to_string()),
            ..Default::default()
        };
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("model")));
    }

    #[test]
    fn test_validate_model_format_valid() {
        let config = Config {
            model: Some("anthropic/claude-sonnet-4".to_string()),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_medium_model_format() {
        let config = Config {
            medium_model: Some("just-model".to_string()),
            ..Default::default()
        };
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("medium_model")));
    }

    #[test]
    fn test_validate_medium_model_format_valid() {
        let config = Config {
            medium_model: Some("anthropic/claude-sonnet-4".to_string()),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_agent_mode() {
        let mut agents = HashMap::new();
        agents.insert(
            "test".to_string(),
            crate::schema::AgentConfig {
                mode: Some("invalid_mode".to_string()),
                ..Default::default()
            },
        );
        let config = Config {
            agent: Some(agents),
            ..Default::default()
        };
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("mode")));
    }

    #[test]
    fn test_validate_agent_mode_valid() {
        let mut agents = HashMap::new();
        agents.insert(
            "test".to_string(),
            crate::schema::AgentConfig {
                mode: Some("primary".to_string()),
                ..Default::default()
            },
        );
        let config = Config {
            agent: Some(agents),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_empty_config() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_find_project_config() {
        let project_dir = tempfile::tempdir().unwrap();
        let config_dir = project_dir.path().join(".codegg");
        std::fs::create_dir_all(&config_dir).unwrap();
        let config_file = config_dir.join("codegg.json");
        std::fs::write(&config_file, "{}").unwrap();

        let found = find_project_config_from(project_dir.path());
        assert_eq!(found, Some(config_file));
    }

    #[test]
    fn test_find_project_config_walks_up() {
        let project_dir = tempfile::tempdir().unwrap();
        let config_dir = project_dir.path().join(".codegg");
        let nested = project_dir.path().join("subdir").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        let config_file = config_dir.join("codegg.json");
        std::fs::write(&config_file, "{}").unwrap();

        let found = find_project_config_from(&nested);
        assert_eq!(found, Some(config_file));
    }

    #[test]
    fn test_merge_configs_empty() {
        let merged = merge_configs(&[]);
        assert_eq!(merged, Config::default());
    }

    #[test]
    fn test_merge_configs_single() {
        let c = Config {
            log_level: Some("info".to_string()),
            model: Some("provider/model".to_string()),
            ..Default::default()
        };
        let merged = merge_configs(&[c]);
        assert_eq!(merged.log_level, Some("info".to_string()));
        assert_eq!(merged.model, Some("provider/model".to_string()));
    }

    #[test]
    fn test_merge_configs_instructions_concat() {
        let c1 = Config {
            instructions: Some(vec!["instr1".to_string()]),
            ..Default::default()
        };
        let c2 = Config {
            instructions: Some(vec!["instr2".to_string()]),
            ..Default::default()
        };
        let merged = merge_configs(&[c1, c2]);
        assert_eq!(
            merged.instructions,
            Some(vec!["instr1".to_string(), "instr2".to_string()])
        );
    }

    #[test]
    fn test_merge_configs_merges_newly_covered_fields() {
        let mut templates = HashMap::new();
        templates.insert(
            "default".to_string(),
            crate::schema::SessionTemplate {
                name: "Default".to_string(),
                ..Default::default()
            },
        );
        let mut keybinds = HashMap::new();
        keybinds.insert("send".to_string(), "enter".to_string());

        let c1 = Config {
            medium_model: Some("provider/medium".to_string()),
            auto_route_models: Some(true),
            subagent: Some(crate::schema::SubagentConfig {
                max_concurrent: Some(7),
                max_depth: Some(3),
                ..Default::default()
            }),
            templates: Some(templates),
            snapshot_config: Some(crate::schema::SnapshotConfig {
                max_files: 123,
                max_file_bytes: 456,
                max_total_bytes: 789,
            }),
            keybinds: Some(keybinds),
            vim_mode: Some(true),
            hooks: Some(vec![crate::schema::HookConfigEntry::default()]),
            notifications: Some(crate::schema::NotificationConfig {
                enabled: Some(true),
                on_task_complete: Some(true),
                on_error: Some(false),
                audio: None,
                quiet_hours: None,
            }),
            catalog: Some(crate::schema::CatalogConfig {
                enabled: Some(true),
                deferred_tools: Some(vec!["webfetch".to_string()]),
                search_max_results: Some(25),
            }),
            ..Default::default()
        };

        let merged = merge_configs(&[c1]);
        assert_eq!(merged.medium_model, Some("provider/medium".to_string()));
        assert_eq!(merged.auto_route_models, Some(true));
        assert_eq!(
            merged.subagent.as_ref().and_then(|s| s.max_concurrent),
            Some(7)
        );
        assert!(merged
            .templates
            .as_ref()
            .is_some_and(|templates| templates.contains_key("default")));
        assert_eq!(
            merged.snapshot_config.as_ref().map(|s| s.max_total_bytes),
            Some(789)
        );
        assert_eq!(
            merged
                .keybinds
                .as_ref()
                .and_then(|k| k.get("send"))
                .map(String::as_str),
            Some("enter")
        );
        assert_eq!(merged.vim_mode, Some(true));
        assert_eq!(merged.hooks.as_ref().map(Vec::len), Some(1));
        assert_eq!(
            merged.notifications.as_ref().and_then(|n| n.enabled),
            Some(true)
        );
        assert_eq!(
            merged.catalog.as_ref().and_then(|c| c.search_max_results),
            Some(25)
        );
    }

    #[test]
    fn test_merge_configs_merges_provider_configs_field_by_field() {
        let mut providers1 = HashMap::new();
        providers1.insert(
            "openai".to_string(),
            crate::schema::ProviderConfig {
                api_key: Some("key1".to_string()),
                base_url: Some("https://api.openai.com".to_string()),
                ..Default::default()
            },
        );
        let c1 = Config {
            provider: Some(providers1),
            ..Default::default()
        };

        let mut providers2 = HashMap::new();
        providers2.insert(
            "openai".to_string(),
            crate::schema::ProviderConfig {
                api_key: Some("key2".to_string()),
                timeout: Some(crate::schema::ProviderTimeout::Ms(5000)),
                ..Default::default()
            },
        );
        let c2 = Config {
            provider: Some(providers2),
            ..Default::default()
        };

        let merged = merge_configs(&[c1, c2]);
        let providers = merged.provider.unwrap();
        let openai = providers.get("openai").unwrap();
        assert_eq!(openai.api_key.as_deref(), Some("key2"));
        assert_eq!(openai.base_url.as_deref(), Some("https://api.openai.com"));
        assert!(matches!(
            openai.timeout,
            Some(crate::schema::ProviderTimeout::Ms(5000))
        ));
    }

    /// Every section a user can write must survive a merge. This used to need
    /// two layers to fail; `merge_configs` is the only path from file to
    /// `Config`, so a section with no arm is discarded even for a single
    /// layer and the setting silently does nothing.
    #[test]
    fn test_merge_configs_preserves_single_layer_sections() {
        let raw = r#"{
            "approval_reviewer": { "model": "fixture/reviewer" },
            "daemon": { "enabled": true },
            "scheduler": { "enabled": true },
            "tool_deferral": { "defer_loading": true },
            "security": { "enabled": true },
            "research": { "search_provider": { "backend": "fixture" } },
            "tool_backends": { "lsp": { "backend": "native" } },
            "human_shell": { "enabled": true },
            "shell": { "output": { "retain_raw": false } },
            "deterministic_tools": { "enabled": true },
            "preflight": { "enabled": false },
            "command_intent": { "route_safe_commands": true },
            "provider_connections": { "background_refresh": true }
        }"#;
        let parsed = parse_config(raw, Path::new("fixture.jsonc")).expect("parses");

        let merged = merge_configs(&[parsed]);

        assert_eq!(
            merged
                .approval_reviewer
                .as_ref()
                .and_then(|c| c.model.as_deref()),
            Some("fixture/reviewer")
        );
        assert_eq!(merged.daemon.as_ref().and_then(|c| c.enabled), Some(true));
        assert_eq!(
            merged.scheduler.as_ref().and_then(|c| c.enabled),
            Some(true)
        );
        assert_eq!(
            merged.tool_deferral.as_ref().and_then(|c| c.defer_loading),
            Some(true)
        );
        assert!(merged.security.as_ref().expect("security survives").enabled);
        assert!(merged.research.is_some());
        assert!(merged.tool_backends.is_some());
        assert_eq!(
            merged.human_shell.as_ref().and_then(|c| c.enabled),
            Some(true)
        );
        assert!(merged.shell.is_some());
        assert!(
            merged
                .deterministic_tools
                .as_ref()
                .expect("deterministic_tools survives")
                .enabled
        );
        assert_eq!(
            merged.preflight.as_ref().and_then(|c| c.enabled),
            Some(false)
        );
        assert_eq!(
            merged
                .command_intent
                .as_ref()
                .and_then(|c| c.route_safe_commands),
            Some(true)
        );
        assert!(
            merged
                .provider_connections
                .as_ref()
                .expect("provider_connections survives")
                .background_refresh
        );
    }

    /// All-`Option` sections combine key-by-key across layers, so a later
    /// layer can override one key without restating the rest of the block.
    #[test]
    fn test_merge_configs_combines_optional_sections_field_by_field() {
        let base = Config {
            daemon: Some(crate::schema::DaemonConfig {
                enabled: Some(true),
                startup_timeout_ms: Some(1_500),
                ..Default::default()
            }),
            human_shell: Some(crate::schema::HumanShellConfig {
                max_history_entries: Some(42),
                ..Default::default()
            }),
            ..Default::default()
        };
        let overlay = Config {
            daemon: Some(crate::schema::DaemonConfig {
                startup_timeout_ms: Some(9_000),
                ..Default::default()
            }),
            human_shell: Some(crate::schema::HumanShellConfig {
                enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };

        let merged = merge_configs(&[base, overlay]);

        let daemon = merged.daemon.expect("daemon section");
        assert_eq!(daemon.enabled, Some(true), "unset key survives from base");
        assert_eq!(daemon.startup_timeout_ms, Some(9_000), "later layer wins");

        let human_shell = merged.human_shell.expect("human_shell section");
        assert_eq!(human_shell.enabled, Some(false));
        assert_eq!(human_shell.max_history_entries, Some(42));
    }

    /// Sections whose fields are all non-`Option` cannot be combined without
    /// an `Option`-backed shadow, so they replace wholesale.
    #[test]
    fn test_merge_configs_replaces_default_backed_sections_whole() {
        let base = Config {
            security: Some(crate::schema::SecurityConfig {
                enabled: true,
                ..Default::default()
            }),
            deterministic_tools: Some(crate::schema::DeterministicToolsConfig {
                enabled: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let overlay = Config {
            security: Some(crate::schema::SecurityConfig {
                enabled: false,
                ..Default::default()
            }),
            deterministic_tools: Some(crate::schema::DeterministicToolsConfig {
                enabled: false,
                ..Default::default()
            }),
            ..Default::default()
        };

        let merged = merge_configs(&[base, overlay]);

        assert!(!merged.security.expect("security section").enabled);
        assert!(
            !merged
                .deterministic_tools
                .expect("deterministic_tools section")
                .enabled
        );
    }
}
