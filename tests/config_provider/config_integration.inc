use codegg::config::Config;
use codegg::config::{load_config, merge_configs, parse_config};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_config(dir: &TempDir, name: &str, content: &str) -> String {
    let path = dir.path().join(name);
    fs::write(&path, content).unwrap();
    path.to_string_lossy().to_string()
}

#[test]
fn test_load_config_json() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.json",
        r#"{
            "log_level": "debug",
            "model": "anthropic/claude-sonnet-4"
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    assert_eq!(config.log_level, Some("debug".to_string()));
    assert_eq!(config.model, Some("anthropic/claude-sonnet-4".to_string()));
}

#[test]
fn test_load_config_jsonc_with_comments() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.jsonc",
        r#"{
            // This is a comment
            "log_level": "info",
            /* block comment */
            "model": "openai/gpt-4"
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    assert_eq!(config.log_level, Some("info".to_string()));
    assert_eq!(config.model, Some("openai/gpt-4".to_string()));
}

#[test]
fn test_load_config_missing_file() {
    let result = load_config(Path::new("/nonexistent/codegg.json"));
    assert!(result.is_err());
}

#[test]
fn test_load_config_invalid_json() {
    let dir = TempDir::new().unwrap();
    let path = write_config(&dir, "codegg.json", "{invalid json content}");
    let result = load_config(Path::new(&path));
    assert!(result.is_err());
}

#[test]
fn test_load_config_with_provider() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.json",
        r#"{
            "provider": {
                "anthropic": {
                    "api_key": "sk-test-key",
                    "base_url": "https://api.anthropic.com"
                }
            }
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let providers = config.provider.unwrap();
    let anthropic = providers.get("anthropic").unwrap();
    assert_eq!(anthropic.api_key, Some("sk-test-key".to_string()));
    assert_eq!(
        anthropic.base_url,
        Some("https://api.anthropic.com".to_string())
    );
}

#[test]
fn test_load_config_with_agent() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.json",
        r#"{
            "agent": {
                "reviewer": {
                    "name": "Code Reviewer",
                    "mode": "primary",
                    "model": "anthropic/claude-sonnet-4"
                }
            }
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let agents = config.agent.unwrap();
    let reviewer = agents.get("reviewer").unwrap();
    assert_eq!(reviewer.name, Some("Code Reviewer".to_string()));
    assert_eq!(reviewer.mode, Some("primary".to_string()));
}

#[test]
fn test_load_config_with_mcp() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.json",
        r#"{
            "mcp": {
                "filesystem": {
                    "type": "local",
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
                }
            }
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let mcp = config.mcp.unwrap();
    let fs_server = mcp.get("filesystem").unwrap();
    let inner = fs_server.inner.as_ref().unwrap();
    assert_eq!(inner.server_type, Some("local".to_string()));
    assert_eq!(inner.command, Some("npx".to_string()));
}

#[test]
fn test_load_config_with_permissions() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.json",
        r#"{
            "permission": {
                "default": "allow",
                "bash": "deny",
                "paths": ["/tmp/*", "/var/log/*"]
            }
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let perm = config.permission.unwrap();
    assert_eq!(perm.default, Some("allow".to_string()));
}

#[test]
fn test_merge_configs_multiple_files() {
    let dir = TempDir::new().unwrap();
    let path1 = write_config(
        &dir,
        "base.json",
        r#"{
            "log_level": "warn",
            "model": "anthropic/claude-sonnet-4"
        }"#,
    );
    let path2 = write_config(
        &dir,
        "override.json",
        r#"{
            "log_level": "debug"
        }"#,
    );

    let config1 = load_config(Path::new(&path1)).unwrap();
    let config2 = load_config(Path::new(&path2)).unwrap();
    let merged = merge_configs(&[config1, config2]);

    assert_eq!(merged.log_level, Some("debug".to_string()));
    assert_eq!(merged.model, Some("anthropic/claude-sonnet-4".to_string()));
}

#[test]
fn test_merge_configs_provider_merging() {
    let dir = TempDir::new().unwrap();
    let path1 = write_config(
        &dir,
        "base.json",
        r#"{
            "provider": {
                "anthropic": { "api_key": "key1" }
            }
        }"#,
    );
    let path2 = write_config(
        &dir,
        "override.json",
        r#"{
            "provider": {
                "openai": { "api_key": "key2" }
            }
        }"#,
    );

    let config1 = load_config(Path::new(&path1)).unwrap();
    let config2 = load_config(Path::new(&path2)).unwrap();
    let merged = merge_configs(&[config1, config2]);

    let providers = merged.provider.unwrap();
    assert!(providers.contains_key("anthropic"));
    assert!(providers.contains_key("openai"));
}

#[test]
fn test_merge_configs_agent_merging() {
    let dir = TempDir::new().unwrap();
    let path1 = write_config(
        &dir,
        "base.json",
        r#"{
            "agent": {
                "build": { "model": "anthropic/claude-sonnet-4" }
            }
        }"#,
    );
    let path2 = write_config(
        &dir,
        "override.json",
        r#"{
            "agent": {
                "plan": { "model": "openai/gpt-4" }
            }
        }"#,
    );

    let config1 = load_config(Path::new(&path1)).unwrap();
    let config2 = load_config(Path::new(&path2)).unwrap();
    let merged = merge_configs(&[config1, config2]);

    let agents = merged.agent.unwrap();
    assert!(agents.contains_key("build"));
    assert!(agents.contains_key("plan"));
}

#[test]
fn test_load_config_with_env_interpolation() {
    std::env::set_var("TEST_API_KEY", "secret-from-env");
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.json",
        r#"{
            "provider": {
                "anthropic": { "api_key": "${TEST_API_KEY}" }
            }
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let providers = config.provider.unwrap();
    let anthropic = providers.get("anthropic").unwrap();
    assert_eq!(anthropic.api_key, Some("secret-from-env".to_string()));
    std::env::remove_var("TEST_API_KEY");
}

#[test]
fn test_load_config_compaction() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.json",
        r#"{
            "compaction": {
                "enabled": true,
                "auto": true,
                "max_tokens": 100000,
                "threshold": 0.8
            }
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let compaction = config.compaction.unwrap();
    assert_eq!(compaction.enabled, Some(true));
    assert_eq!(compaction.auto, Some(true));
    assert_eq!(compaction.max_tokens, Some(100000));
}

#[test]
fn test_load_config_tools() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.json",
        r#"{
            "tools": {
                "bash": true,
                "edit": false
            }
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let tools = config.tools.unwrap();
    assert_eq!(tools.get("bash"), Some(&true));
    assert_eq!(tools.get("edit"), Some(&false));
}

#[test]
fn test_load_config_commands() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.json",
        r#"{
            "commands": {
                "deploy": {
                    "template": "deploy {{input}}",
                    "description": "Deploy the application"
                }
            }
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let commands = config.commands.unwrap();
    let deploy = commands.get("deploy").unwrap();
    assert_eq!(deploy.template, "deploy {{input}}");
    assert_eq!(
        deploy.description,
        Some("Deploy the application".to_string())
    );
}

#[test]
fn test_load_config_yaml_extension() {
    let dir = TempDir::new().unwrap();
    let config_dir = dir.path().join(".codegg");
    fs::create_dir_all(&config_dir).unwrap();
    let path = config_dir.join("codegg.json");
    fs::write(
        &path,
        r#"{
            "log_level": "trace"
        }"#,
    )
    .unwrap();
    let config = load_config(&path).unwrap();
    assert_eq!(config.log_level, Some("trace".to_string()));
}

#[test]
fn test_parse_config_empty_object() {
    let config = parse_config("{}", Path::new("test.json")).unwrap();
    assert_eq!(config, Config::default());
}

#[test]
fn test_parse_config_with_nested_objects() {
    let content = r#"{
        "server": {
            "port": 8080,
            "hostname": "localhost",
            "mdns": true
        }
    }"#;
    let config = parse_config(content, Path::new("test.json")).unwrap();
    let server = config.server.unwrap();
    assert_eq!(server.port, Some(8080));
    assert_eq!(server.hostname, Some("localhost".to_string()));
    assert_eq!(server.mdns, Some(true));
}

#[test]
fn test_load_config_with_skills() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.json",
        r#"{
            "skills": {
                "enabled": true,
                "paths": ["./skills"],
                "urls": ["https://example.com/skill"]
            }
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let skills = config.skills.unwrap();
    assert_eq!(skills.enabled, Some(true));
    assert_eq!(skills.paths, Some(vec!["./skills".to_string()]));
}

#[test]
fn test_load_config_with_autoupdate() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.json",
        r#"{
            "autoupdate": false
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    assert!(matches!(
        config.autoupdate.unwrap(),
        codegg::config::schema::AutoupdateConfig::Bool(false)
    ));
}

#[test]
fn test_load_config_with_watcher() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        "codegg.json",
        r#"{
            "watcher": {
                "ignore": ["node_modules", ".git", "target"]
            }
        }"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let watcher = config.watcher.unwrap();
    assert_eq!(
        watcher.ignore,
        Some(vec![
            "node_modules".to_string(),
            ".git".to_string(),
            "target".to_string()
        ])
    );
}
