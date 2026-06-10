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
            context_policy
        );
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
            while let Some(&nc) = chars.peek() {
                if nc == '}' {
                    chars.next();
                    break;
                }
                var_name.push(nc);
                chars.next();
            }
            if let Ok(val) = std::env::var(&var_name) {
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
        let tmp = std::env::temp_dir();
        let project_dir = tmp.join("codegg_test_project");
        let config_dir = project_dir.join(".codegg");
        std::fs::create_dir_all(&config_dir).unwrap();
        let config_file = config_dir.join("codegg.json");
        std::fs::write(&config_file, "{}").unwrap();

        let found = find_project_config_from(&project_dir);
        assert_eq!(found, Some(config_file));

        std::fs::remove_dir_all(&project_dir).ok();
    }

    #[test]
    fn test_find_project_config_walks_up() {
        let tmp = std::env::temp_dir();
        let project_dir = tmp.join("codegg_test_walkup");
        let config_dir = project_dir.join(".codegg");
        let nested = project_dir.join("subdir").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        let config_file = config_dir.join("codegg.json");
        std::fs::write(&config_file, "{}").unwrap();

        let found = find_project_config_from(&nested);
        assert_eq!(found, Some(config_file));

        std::fs::remove_dir_all(&project_dir).ok();
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
}
