//! File-based agent loading (global and project agent files).
//!
//! Physical decomposition of [`super`](crate::agent) module root (M003):
//! markdown/TOML agent-file parsing, overlay flags, structured permission
//! specs, prompt-file resolution, and the file-agent lookup helpers.
//! Resolution layering stays owned by [`super::definition`] and
//! [`super::registry`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::schema::AgentConfig;
use crate::error::AgentError;

use super::definition::{agent_from_config, parse_mode, Agent, AgentRuntimeKind};
use super::registry;

#[derive(Debug, Clone)]
pub struct FileAgent {
    pub agent: Agent,
    pub source: String,
    /// Overlay control flags from the TOML file.
    pub overlay: OverlayFlags,
    /// Declarative spec preserving field-level explicitness for merge operations.
    pub spec: registry::AgentSpec,
    /// Diagnostics emitted during file parsing.
    pub diagnostics: Vec<registry::AgentDiagnostic>,
}

pub fn load_agents_from_dir(dir: &Path) -> Result<Vec<FileAgent>, AgentError> {
    let mut agents = Vec::new();

    if !dir.is_dir() {
        return Ok(agents);
    }

    for entry in std::fs::read_dir(dir).map_err(|e| AgentError::Invalid(e.to_string()))? {
        let entry = entry.map_err(|e| AgentError::Invalid(e.to_string()))?;
        let path = entry.path();

        let ext = path.extension().and_then(|e| e.to_str());
        match ext {
            Some("md") => {
                if let Some(file_agent) = load_agent_from_file(&path)? {
                    agents.push(file_agent);
                }
            }
            Some("toml") => {
                if let Some(file_agent) = load_agent_from_toml(&path)? {
                    agents.push(file_agent);
                }
            }
            _ => continue,
        }
    }

    Ok(agents)
}

pub fn load_agent_from_file(path: &Path) -> Result<Option<FileAgent>, AgentError> {
    let content = std::fs::read_to_string(path).map_err(|e| AgentError::Invalid(e.to_string()))?;

    let Some((frontmatter, body)) = parse_frontmatter(&content) else {
        return Ok(None);
    };

    let mut agent_cfg: AgentConfig =
        codegg_config::parse_yaml(path.display().to_string(), frontmatter.as_bytes())
            .map_err(|e| AgentError::Invalid(e.to_string()))?;

    // Body-as-prompt: use markdown body as prompt when no explicit prompt or prompt_file
    let body = body.trim().to_string();
    if agent_cfg.prompt.is_none() && agent_cfg.prompt_file.is_none() && !body.is_empty() {
        agent_cfg.prompt = Some(body);
    }

    let name = agent_cfg.name.clone().unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string())
    });

    // Resolve prompt_file: load content into prompt if prompt is not already set
    let mut file_diags = Vec::new();

    // Check for TOML-only keys in markdown frontmatter
    {
        let raw: serde_json::Value =
            codegg_config::parse_yaml(path.display().to_string(), frontmatter.as_bytes())
                .map_err(|e| AgentError::Invalid(e.to_string()))?;
        if let Some(mapping) = raw.as_object() {
            // TOML-only features that have no effect in markdown files.
            // 'disable' is NOT here — it's a valid AgentConfig field.
            let toml_only_keys = [
                ("replace", "overlay flag — markdown files always merge"),
                ("merge", "overlay flag — markdown files always merge"),
                ("bash_permission", "structured permission section"),
                ("path_permission", "structured permission section"),
            ];
            for (key, hint) in &toml_only_keys {
                if mapping.contains_key(*key) {
                    file_diags.push(
                        registry::AgentDiagnostic::new(
                            registry::AgentDiagnosticSeverity::Warning,
                            &name,
                            format!("'{key}' is a TOML-only feature ({hint}). Use TOML format for full structured control."),
                        )
                        .with_field(*key)
                        .with_suggestion("convert to .toml format or remove this key"),
                    );
                }
            }
        }
    }

    if agent_cfg.prompt.is_none() {
        if let Some(ref prompt_file) = agent_cfg.prompt_file.clone() {
            let resolved_path = if Path::new(prompt_file).is_absolute() {
                PathBuf::from(prompt_file)
            } else if let Some(parent) = path.parent() {
                parent.join(prompt_file)
            } else {
                PathBuf::from(prompt_file)
            };
            match std::fs::read_to_string(&resolved_path) {
                Ok(prompt_content) => {
                    agent_cfg.prompt = Some(prompt_content);
                }
                Err(_) => {
                    file_diags.push(
                        registry::AgentDiagnostic::new(
                            registry::AgentDiagnosticSeverity::Warning,
                            &name,
                            format!("prompt_file '{prompt_file}' not found, agent loaded without prompt"),
                        )
                        .with_field("prompt_file"),
                    );
                }
            }
        }
    }

    let agent = agent_from_config(&name, &agent_cfg)?;

    let source = path.to_string_lossy().to_string();

    // Build a spec from the original config, preserving which fields were set.
    let spec = registry::AgentSpec::from_agent_config(&name, &agent_cfg)?;

    // Markdown files always use merge overlay (no replace/disable flags)
    Ok(Some(FileAgent {
        agent,
        source,
        overlay: OverlayFlags::default(),
        spec,
        diagnostics: file_diags,
    }))
}

/// Apply structured bash permission spec to agent permissions.
/// Converts BashPermissionSpec into flat permission entries.
fn apply_bash_permission_spec(agent: &mut Agent, spec: &BashPermissionSpec) {
    // Set the default bash action
    if let Some(ref action) = spec.action {
        agent.permissions.insert("bash".to_string(), action.clone());
    }

    // Store allow/deny patterns as structured entries
    // These will be converted to ToolRules during permission_ruleset()
    if let Some(ref allow_patterns) = spec.allow_patterns {
        for pattern in allow_patterns {
            agent
                .permissions
                .insert(format!("bash:allow:{}", pattern), "allow".to_string());
        }
    }
    if let Some(ref deny_patterns) = spec.deny_patterns {
        for pattern in deny_patterns {
            agent
                .permissions
                .insert(format!("bash:deny:{}", pattern), "deny".to_string());
        }
    }
}

/// Apply structured path permission spec to agent permissions.
/// Converts PathPermissionSpec into flat permission entries.
fn apply_path_permission_spec(agent: &mut Agent, spec: &PathPermissionSpec) {
    if let Some(ref allow_patterns) = spec.allow {
        for pattern in allow_patterns {
            agent
                .permissions
                .insert(format!("path:allow:{}", pattern), "allow".to_string());
        }
    }
    if let Some(ref deny_patterns) = spec.deny {
        for pattern in deny_patterns {
            agent
                .permissions
                .insert(format!("path:deny:{}", pattern), "deny".to_string());
        }
    }
}

fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let content = content.trim_start();

    if !content.starts_with("---") {
        return None;
    }

    let rest = &content[3..];
    let end = rest.find("---")?;
    let frontmatter = rest[..end].trim().to_string();
    let body = rest[end + 3..].to_string();

    Some((frontmatter, body))
}

/// Overlay control flags for TOML agent files.
#[derive(serde::Deserialize, Debug, Clone, Default)]
pub struct OverlayFlags {
    /// When true, completely replaces the existing agent definition instead of merging.
    pub replace: Option<bool>,
    /// When true, disables the agent (prevents it from appearing in resolution).
    pub disable: Option<bool>,
    /// Explicitly merge into existing definition (default behavior, can be used for clarity).
    pub merge: Option<bool>,
}

/// Structured bash permission spec for agent definitions.
#[derive(serde::Deserialize, Debug, Clone, Default)]
pub struct BashPermissionSpec {
    /// Default action for bash: "allow", "deny", or "ask".
    pub action: Option<String>,
    /// Glob patterns that are explicitly allowed.
    pub allow_patterns: Option<Vec<String>>,
    /// Glob patterns that are explicitly denied.
    pub deny_patterns: Option<Vec<String>>,
}

/// Structured path permission spec for agent definitions.
#[derive(serde::Deserialize, Debug, Clone, Default)]
pub struct PathPermissionSpec {
    /// Glob patterns for allowed paths.
    pub allow: Option<Vec<String>>,
    /// Glob patterns for denied paths.
    pub deny: Option<Vec<String>>,
}

/// Rich permission spec supporting both simple strings and structured rules.
#[derive(serde::Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum AgentPermissionSpec {
    /// Simple action string: "allow", "deny", or "ask"
    Simple(String),
    /// Structured bash permission
    Bash(BashPermissionSpec),
    /// Structured path permission
    Paths(PathPermissionSpec),
}

/// TOML agent file: supports both flat format and `[agent]` wrapped format.
#[derive(serde::Deserialize, Debug, Default)]
#[serde(default)]
struct TomlAgentFile {
    schema_version: Option<u32>,
    /// Overlay control flags
    replace: Option<bool>,
    disable: Option<bool>,
    merge: Option<bool>,
    extends: Option<String>,
    /// Wrapped format: `[agent]` section
    agent: Option<TomlAgentInner>,
    // Flat format: top-level keys
    name: Option<String>,
    role: Option<String>,
    description: Option<String>,
    mode: Option<String>,
    model: Option<String>,
    fallback_model: Option<String>,
    variant: Option<String>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    prompt: Option<String>,
    prompt_file: Option<String>,
    color: Option<String>,
    steps: Option<u32>,
    hidden: Option<bool>,
    runtime_kind: Option<String>,
    // Flat format: `[permission]` section — simple string values only
    permission: Option<HashMap<String, String>>,
    // Structured permission sub-tables: `[bash_permission]` and `[path_permission]`
    bash_permission: Option<BashPermissionSpec>,
    path_permission: Option<PathPermissionSpec>,
}

/// Inner struct for `[agent]` wrapped TOML format.
#[derive(serde::Deserialize, Debug, Default)]
#[serde(default)]
struct TomlAgentInner {
    extends: Option<String>,
    name: Option<String>,
    role: Option<String>,
    description: Option<String>,
    mode: Option<String>,
    model: Option<String>,
    fallback_model: Option<String>,
    variant: Option<String>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    prompt: Option<String>,
    prompt_file: Option<String>,
    color: Option<String>,
    steps: Option<u32>,
    hidden: Option<bool>,
    disable: Option<bool>,
    permissions: Option<HashMap<String, String>>,
    runtime_kind: Option<String>,
}

impl TomlAgentFile {
    /// Extract overlay control flags from the top-level fields.
    fn overlay_flags(&self) -> OverlayFlags {
        OverlayFlags {
            replace: self.replace,
            disable: self.disable,
            merge: self.merge,
        }
    }

    /// Convert simple permission strings to PermissionRule for AgentConfig.
    fn simplify_permissions(
        perms: &Option<HashMap<String, String>>,
    ) -> Option<HashMap<String, crate::config::schema::PermissionRule>> {
        perms.as_ref().map(|m| {
            m.iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        crate::config::schema::PermissionRule::Action(v.clone()),
                    )
                })
                .collect()
        })
    }

    /// Convert to AgentConfig, preferring `[agent]` section over flat fields.
    fn into_agent_config(self) -> AgentConfig {
        if let Some(inner) = self.agent {
            // Wrapped format: use inner fields
            let permission = Self::simplify_permissions(&inner.permissions);
            AgentConfig {
                name: inner.name,
                role: inner.role,
                description: inner.description,
                mode: inner.mode,
                model: inner.model,
                fallback_model: inner.fallback_model,
                variant: inner.variant,
                temperature: inner.temperature,
                top_p: inner.top_p,
                prompt: inner.prompt,
                prompt_file: inner.prompt_file,
                color: inner.color,
                steps: inner.steps,
                hidden: inner.hidden,
                disable: inner.disable,
                permission,
                tools: None,
                options: None,
                runtime_kind: inner.runtime_kind,
            }
        } else {
            // Flat format: use top-level fields
            let permission = Self::simplify_permissions(&self.permission);
            AgentConfig {
                name: self.name,
                role: self.role,
                description: self.description,
                mode: self.mode,
                model: self.model,
                fallback_model: self.fallback_model,
                variant: self.variant,
                temperature: self.temperature,
                top_p: self.top_p,
                prompt: self.prompt,
                prompt_file: self.prompt_file,
                color: self.color,
                steps: self.steps,
                hidden: self.hidden,
                disable: self.disable,
                permission,
                tools: None,
                options: None,
                runtime_kind: self.runtime_kind,
            }
        }
    }

    /// Extract structured bash permission from dedicated section.
    fn structured_bash_permission(&self) -> Option<BashPermissionSpec> {
        if let Some(ref bash) = self.bash_permission {
            return Some(bash.clone());
        }
        None
    }

    /// Extract structured path permission from dedicated section.
    fn structured_path_permission(&self) -> Option<PathPermissionSpec> {
        if let Some(ref paths) = self.path_permission {
            return Some(paths.clone());
        }
        None
    }
}

pub fn load_agent_from_toml(path: &Path) -> Result<Option<FileAgent>, AgentError> {
    let content = std::fs::read_to_string(path).map_err(|e| AgentError::Invalid(e.to_string()))?;

    let toml_file: TomlAgentFile =
        toml::from_str(&content).map_err(|e| AgentError::Invalid(e.to_string()))?;

    // Check for unknown top-level TOML keys
    let mut file_diags = Vec::new();
    {
        let raw: toml::Value =
            toml::from_str(&content).map_err(|e| AgentError::Invalid(e.to_string()))?;
        if let Some(table) = raw.as_table() {
            let known_toml_keys = [
                "schema_version",
                "replace",
                "disable",
                "merge",
                "extends",
                "agent",
                "name",
                "role",
                "description",
                "mode",
                "model",
                "fallback_model",
                "variant",
                "temperature",
                "top_p",
                "prompt",
                "prompt_file",
                "color",
                "steps",
                "hidden",
                "runtime_kind",
                "permission",
                "bash_permission",
                "path_permission",
            ];
            let unknown: Vec<&str> = table
                .keys()
                .filter(|k| !known_toml_keys.contains(&k.as_str()))
                .map(|k| k.as_str())
                .collect();
            if !unknown.is_empty() {
                file_diags.push(
                    registry::AgentDiagnostic::new(
                        registry::AgentDiagnosticSeverity::Warning,
                        path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown"),
                        format!("unknown TOML keys: {}", unknown.join(", ")),
                    )
                    .with_source(registry::AgentSourceKind::GlobalFile),
                );
            }
        }
    }

    let overlay = toml_file.overlay_flags();
    let extends = toml_file
        .agent
        .as_ref()
        .and_then(|inner| inner.extends.clone())
        .or_else(|| toml_file.extends.clone());
    let bash_spec = toml_file.structured_bash_permission();
    let path_spec = toml_file.structured_path_permission();
    let mut agent_cfg = toml_file.into_agent_config();

    let name = agent_cfg.name.clone().unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string())
    });

    // Validate mode
    if let Some(ref mode_str) = agent_cfg.mode {
        if parse_mode(mode_str).is_err() {
            file_diags.push(
                registry::AgentDiagnostic::new(
                    registry::AgentDiagnosticSeverity::Error,
                    &name,
                    format!("invalid mode: {mode_str}"),
                )
                .with_field("mode")
                .with_suggestion("use one of: primary, subagent, all"),
            );
        }
    }

    // Validate runtime_kind
    if let Some(ref rk) = agent_cfg.runtime_kind {
        if rk.parse::<AgentRuntimeKind>().is_err() {
            file_diags.push(
                registry::AgentDiagnostic::new(
                    registry::AgentDiagnosticSeverity::Error,
                    &name,
                    format!("invalid runtime_kind: {rk}"),
                )
                .with_field("runtime_kind")
                .with_suggestion(
                    "use one of: standard, security_review, research, compaction, title, summary",
                ),
            );
        }
    }

    // Validate permission actions
    if let Some(ref perms) = agent_cfg.permission {
        for (tool, rule) in perms {
            let action = match rule {
                crate::config::schema::PermissionRule::Action(s) => s.as_str(),
                crate::config::schema::PermissionRule::Object(obj) => obj
                    .get("default")
                    .or_else(|| obj.get("action"))
                    .map(|s| s.as_str())
                    .unwrap_or("ask"),
            };
            if !matches!(action, "allow" | "deny" | "ask") {
                file_diags.push(
                    registry::AgentDiagnostic::new(
                        registry::AgentDiagnosticSeverity::Error,
                        &name,
                        format!("invalid permission action '{action}' for tool '{tool}'"),
                    )
                    .with_field("permission")
                    .with_suggestion("use one of: allow, deny, ask"),
                );
            }
        }
    }

    // Resolve prompt_file: load content into prompt if prompt is not already set
    if agent_cfg.prompt.is_none() {
        if let Some(ref prompt_file) = agent_cfg.prompt_file.clone() {
            let resolved_path = if Path::new(prompt_file).is_absolute() {
                PathBuf::from(prompt_file)
            } else if let Some(parent) = path.parent() {
                parent.join(prompt_file)
            } else {
                PathBuf::from(prompt_file)
            };
            match std::fs::read_to_string(&resolved_path) {
                Ok(prompt_content) => {
                    agent_cfg.prompt = Some(prompt_content);
                }
                Err(_) => {
                    file_diags.push(
                        registry::AgentDiagnostic::new(
                            registry::AgentDiagnosticSeverity::Warning,
                            &name,
                            format!("prompt_file '{prompt_file}' not found, agent loaded without prompt"),
                        )
                        .with_field("prompt_file"),
                    );
                }
            }
        }
    }

    let mut agent = agent_from_config(&name, &agent_cfg)?;

    // Apply structured bash permissions to agent.permissions
    if let Some(ref bash) = bash_spec {
        apply_bash_permission_spec(&mut agent, bash);
    }

    // Apply structured path permissions to agent.permissions
    if let Some(ref paths) = path_spec {
        apply_path_permission_spec(&mut agent, paths);
    }

    let source = path.to_string_lossy().to_string();

    // Build a spec from the original config, preserving which fields were set.
    let mut spec = registry::AgentSpec::from_agent_config(&name, &agent_cfg)?;
    spec.extends = extends;

    // Apply structured permissions into the spec as well
    if let Some(ref bash) = bash_spec {
        if let Some(ref action) = bash.action {
            spec.permission
                .get_or_insert_with(HashMap::new)
                .insert("bash".to_string(), action.clone());
        }
        if let Some(ref allow_patterns) = bash.allow_patterns {
            let perms = spec.permission.get_or_insert_with(HashMap::new);
            for pattern in allow_patterns {
                perms.insert(format!("bash:allow:{}", pattern), "allow".to_string());
            }
        }
        if let Some(ref deny_patterns) = bash.deny_patterns {
            let perms = spec.permission.get_or_insert_with(HashMap::new);
            for pattern in deny_patterns {
                perms.insert(format!("bash:deny:{}", pattern), "deny".to_string());
            }
        }
    }
    if let Some(ref paths) = path_spec {
        if let Some(ref allow_patterns) = paths.allow {
            let perms = spec.permission.get_or_insert_with(HashMap::new);
            for pattern in allow_patterns {
                perms.insert(format!("path:allow:{}", pattern), "allow".to_string());
            }
        }
        if let Some(ref deny_patterns) = paths.deny {
            let perms = spec.permission.get_or_insert_with(HashMap::new);
            for pattern in deny_patterns {
                perms.insert(format!("path:deny:{}", pattern), "deny".to_string());
            }
        }
    }

    Ok(Some(FileAgent {
        agent,
        source,
        overlay,
        spec,
        diagnostics: file_diags,
    }))
}

pub fn find_default_agent(agents: &[Agent]) -> Option<&Agent> {
    agents
        .iter()
        .find(|a| a.name == "build")
        .or_else(|| agents.iter().find(|a| !a.hidden))
        .or_else(|| agents.first())
}

pub fn find_agent_by_name<'a>(agents: &'a [Agent], name: &str) -> Option<&'a Agent> {
    agents.iter().find(|a| a.name == name)
}

pub fn list_visible_agents(agents: &[Agent]) -> Vec<&Agent> {
    agents.iter().filter(|a| !a.hidden).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::definition::{Agent, AgentMode};
    use crate::permission;
    use std::collections::HashMap;

    #[test]
    fn test_load_agents_from_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert!(agents.is_empty());
    }

    #[test]
    fn test_load_agents_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"---
name: TestAgent
mode: primary
description: A test agent
---
Some body content
"#;
        std::fs::write(tmp.path().join("test.md"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent.name, "TestAgent");
    }

    #[test]
    fn test_load_agent_no_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("nofm.md"), "Just content").unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert!(agents.is_empty());
    }

    #[test]
    fn test_malformed_yaml_reports_source_location() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("broken.md"),
            "---\nname: [broken\n---\nbody",
        )
        .unwrap();
        let error = load_agent_from_file(&tmp.path().join("broken.md")).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("YAML compatibility"));
        assert!(message.contains("line"));
    }

    #[test]
    fn test_load_agent_uses_filename() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "---\nmode: primary\n---\nbody";
        std::fs::write(tmp.path().join("myagent.md"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents[0].agent.name, "myagent");
    }

    #[test]
    fn test_markdown_unsupported_keys_emit_warnings() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"---
name: test-warn
mode: subagent
replace: true
merge: true
bash_permission:
  action: ask
path_permission:
  allow: ["src/**"]
---
You are a test agent.
"#;
        std::fs::write(tmp.path().join("warn.md"), content).unwrap();
        let file_agent = load_agent_from_file(&tmp.path().join("warn.md"))
            .unwrap()
            .expect("should load");
        assert_eq!(file_agent.agent.name, "test-warn");
        assert!(
            !file_agent.diagnostics.is_empty(),
            "should emit warnings for TOML-only keys"
        );
        let warning_keys: Vec<_> = file_agent
            .diagnostics
            .iter()
            .filter(|d| d.severity == registry::AgentDiagnosticSeverity::Warning)
            .filter_map(|d| d.field.as_deref())
            .collect();
        assert!(warning_keys.contains(&"replace"), "replace should warn");
        assert!(warning_keys.contains(&"merge"), "merge should warn");
        assert!(
            warning_keys.contains(&"bash_permission"),
            "bash_permission should warn"
        );
        assert!(
            warning_keys.contains(&"path_permission"),
            "path_permission should warn"
        );
    }

    #[test]
    fn test_markdown_supported_keys_work() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"---
name: full-agent
mode: subagent
model: tier.frontier
temperature: 0.7
color: blue
steps: 10
hidden: false
description: A full markdown agent
permission:
  read: allow
  bash: ask
  write: deny
---
The full body prompt.
"#;
        std::fs::write(tmp.path().join("full.md"), content).unwrap();
        let file_agent = load_agent_from_file(&tmp.path().join("full.md"))
            .unwrap()
            .expect("should load");
        assert_eq!(file_agent.agent.name, "full-agent");
        assert_eq!(file_agent.agent.mode, AgentMode::Subagent);
        assert_eq!(file_agent.agent.description, "A full markdown agent");
        assert_eq!(
            file_agent.agent.permissions.get("read"),
            Some(&"allow".to_string())
        );
        assert_eq!(
            file_agent.agent.permissions.get("bash"),
            Some(&"ask".to_string())
        );
        assert_eq!(
            file_agent.agent.permissions.get("write"),
            Some(&"deny".to_string())
        );
        assert_eq!(
            file_agent.agent.system_prompt,
            Some("The full body prompt.".to_string())
        );
        assert!(
            file_agent.diagnostics.is_empty(),
            "supported keys should not emit warnings"
        );
    }

    #[test]
    fn test_markdown_example_file_is_valid() {
        let example_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples/agents/markdown-agent.md");
        if !example_path.exists() {
            eprintln!("skipping: example file not found at {example_path:?}");
            return;
        }
        let file_agent = load_agent_from_file(&example_path)
            .unwrap()
            .expect("example markdown-agent.md should load");
        assert_eq!(file_agent.agent.name, "markdown-agent");
        assert_eq!(file_agent.agent.mode, AgentMode::Subagent);
        assert!(
            file_agent.agent.system_prompt.is_some(),
            "body should become prompt"
        );
        assert!(
            file_agent.diagnostics.is_empty(),
            "example should have no warnings"
        );
    }

    #[test]
    fn test_load_toml_agent_flat_format() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
name = "my-agent"
mode = "subagent"
description = "A custom agent"
prompt = "You are a helpful assistant."
"#;
        std::fs::write(tmp.path().join("my-agent.toml"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent.name, "my-agent");
        assert_eq!(agents[0].agent.mode, AgentMode::Subagent);
        assert_eq!(
            agents[0].agent.system_prompt.as_deref(),
            Some("You are a helpful assistant.")
        );
    }

    #[test]
    fn test_load_toml_agent_wrapped_format() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
[agent]
name = "wrapped-agent"
mode = "primary"
description = "Wrapped format agent"
"#;
        std::fs::write(tmp.path().join("wrapped.toml"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent.name, "wrapped-agent");
        assert_eq!(agents[0].agent.mode, AgentMode::Primary);
    }

    #[test]
    fn test_load_toml_agent_with_permissions() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
name = "perm-agent"
mode = "subagent"
description = "Agent with permissions"

[permission]
read = "allow"
bash = "ask"
write = "deny"
"#;
        std::fs::write(tmp.path().join("perm.toml"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(
            agents[0].agent.permissions.get("read"),
            Some(&"allow".to_string())
        );
        assert_eq!(
            agents[0].agent.permissions.get("bash"),
            Some(&"ask".to_string())
        );
        assert_eq!(
            agents[0].agent.permissions.get("write"),
            Some(&"deny".to_string())
        );
    }

    #[test]
    fn test_load_toml_agent_uses_filename_as_name() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
mode = "subagent"
description = "No name in file"
"#;
        std::fs::write(tmp.path().join("from-file.toml"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent.name, "from-file");
    }

    #[test]
    fn test_load_toml_invalid_toml_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("bad.toml"), "not valid {{{ toml").unwrap();
        let result = load_agents_from_dir(tmp.path());
        assert!(result.is_err());
    }

    // --- Markdown body-as-prompt tests ---

    #[test]
    fn test_md_body_becomes_prompt() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"---
name: body-agent
mode: subagent
description: Agent with body prompt
---

You are a focused code reviewer.
Check for safety issues.
"#;
        std::fs::write(tmp.path().join("body.md"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        let prompt = agents[0].agent.system_prompt.as_deref().unwrap();
        assert!(prompt.contains("You are a focused code reviewer."));
        assert!(prompt.contains("Check for safety issues."));
    }

    #[test]
    fn test_md_explicit_prompt_overrides_body() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"---
name: override-agent
mode: subagent
description: Agent with explicit prompt
prompt: "Explicit prompt wins"
---

Body content that should be ignored
"#;
        std::fs::write(tmp.path().join("override.md"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(
            agents[0].agent.system_prompt.as_deref(),
            Some("Explicit prompt wins")
        );
    }

    // --- Prompt file resolution tests ---

    #[test]
    fn test_prompt_file_resolved_relative_to_agent_file() {
        let tmp = tempfile::tempdir().unwrap();
        // Create prompt file in same directory as agent file
        std::fs::write(tmp.path().join("my-prompt.md"), "Prompt from file content").unwrap();
        let content = r#"---
name: file-prompt-agent
mode: subagent
description: Agent with prompt_file
prompt_file: my-prompt.md
---"#;
        std::fs::write(tmp.path().join("agent.md"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(
            agents[0].agent.system_prompt.as_deref(),
            Some("Prompt from file content")
        );
    }

    #[test]
    fn test_toml_prompt_file_resolved_relative() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("prompt.md"), "TOML prompt file content").unwrap();
        let content = r#"
name = "toml-file-prompt"
mode = "subagent"
description = "TOML agent with prompt_file"
prompt_file = "prompt.md"
"#;
        std::fs::write(tmp.path().join("agent.toml"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(
            agents[0].agent.system_prompt.as_deref(),
            Some("TOML prompt file content")
        );
    }

    #[test]
    fn test_prompt_file_missing_agent_still_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"---
name: missing-prompt-agent
mode: subagent
description: Agent with missing prompt_file
prompt_file: nonexistent.md
---"#;
        std::fs::write(tmp.path().join("agent.md"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        // Agent loads but prompt is None (file not found)
        assert!(agents[0].agent.system_prompt.is_none());
    }

    // --- Mixed format directory tests ---

    #[test]
    fn test_load_mixed_md_and_toml_agents() {
        let tmp = tempfile::tempdir().unwrap();
        let md_content = r#"---
name: md-agent
mode: primary
description: Markdown agent
---"#;
        let toml_content = r#"
name = "toml-agent"
mode = "subagent"
description = "TOML agent"
"#;
        std::fs::write(tmp.path().join("md-agent.md"), md_content).unwrap();
        std::fs::write(tmp.path().join("toml-agent.toml"), toml_content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 2);
        let names: Vec<&str> = agents.iter().map(|a| a.agent.name.as_str()).collect();
        assert!(names.contains(&"md-agent"));
        assert!(names.contains(&"toml-agent"));
    }

    // --- Registry integration tests ---

    #[test]
    fn test_registry_loads_toml_from_global_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
name = "global-toml-agent"
mode = "subagent"
description = "Global TOML agent"
"#;
        std::fs::write(tmp.path().join("global.toml"), content).unwrap();

        // We can't easily test the real global dir, but we can test
        // that load_agents_from_dir works with TOML files
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent.name, "global-toml-agent");
    }

    // --- Milestone 6: Overlay merge behavior tests ---

    #[test]
    fn test_overlay_merge_preserves_base_fields() {
        let base = Agent {
            name: "security-review".to_string(),
            role: Some("reviewer".to_string()),
            description: "Built-in security reviewer".to_string(),
            mode: AgentMode::Subagent,
            mode_name: None,
            model: Some("tier.frontier".to_string()),
            variant: None,
            temperature: Some(0.1),
            top_p: None,
            color: None,
            steps: None,
            system_prompt: Some("Review for security issues.".to_string()),
            permissions: HashMap::from([
                ("read".to_string(), "allow".to_string()),
                ("bash".to_string(), "deny".to_string()),
            ]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        // Overlay only changes temperature
        let overlay = Agent {
            name: "security-review".to_string(),
            role: None,
            description: String::new(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: Some(0.05),
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::new(),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        let merged = base.merge_overlay(&overlay, false);
        // Temperature replaced
        assert_eq!(merged.temperature, Some(0.05));
        // Other fields preserved from base
        assert_eq!(merged.name, "security-review");
        assert_eq!(merged.description, "Built-in security reviewer");
        assert_eq!(merged.mode, AgentMode::Subagent);
        assert_eq!(
            merged.system_prompt.as_deref(),
            Some("Review for security issues.")
        );
        assert_eq!(merged.permissions.get("read"), Some(&"allow".to_string()));
        assert_eq!(merged.permissions.get("bash"), Some(&"deny".to_string()));
    }

    #[test]
    fn test_overlay_merge_permissions_per_tool() {
        let base = Agent {
            name: "test".to_string(),
            role: None,
            description: "Base".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::from([
                ("read".to_string(), "allow".to_string()),
                ("bash".to_string(), "deny".to_string()),
            ]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        // Overlay changes bash to ask
        let overlay = Agent {
            name: "test".to_string(),
            role: None,
            description: String::new(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::from([("bash".to_string(), "ask".to_string())]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        let merged = base.merge_overlay(&overlay, false);
        assert_eq!(merged.permissions.get("read"), Some(&"allow".to_string()));
        assert_eq!(merged.permissions.get("bash"), Some(&"ask".to_string()));
    }

    #[test]
    fn test_overlay_replace_discards_base() {
        let base = Agent {
            name: "test".to_string(),
            role: Some("base-role".to_string()),
            description: "Base description".to_string(),
            mode: AgentMode::Subagent,
            mode_name: None,
            model: Some("old-model".to_string()),
            variant: None,
            temperature: Some(0.5),
            top_p: None,
            color: None,
            steps: None,
            system_prompt: Some("Base prompt".to_string()),
            permissions: HashMap::from([("read".to_string(), "allow".to_string())]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        let overlay = Agent {
            name: "test".to_string(),
            role: None,
            description: "Overlay description".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: Some("new-model".to_string()),
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: Some("Overlay prompt".to_string()),
            permissions: HashMap::from([("bash".to_string(), "allow".to_string())]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };

        let merged = base.merge_overlay(&overlay, true);
        // replace=true: overlay is used as-is
        assert_eq!(merged.description, "Overlay description");
        assert_eq!(merged.model, Some("new-model".to_string()));
        assert_eq!(merged.system_prompt.as_deref(), Some("Overlay prompt"));
        assert_eq!(merged.permissions.get("bash"), Some(&"allow".to_string()));
        // Base permissions are gone
        assert!(!merged.permissions.contains_key("read"));
    }

    #[test]
    fn test_overlay_disable_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
name = "my-agent"
mode = "subagent"
description = "Should be disabled"
disable = true
"#;
        std::fs::write(tmp.path().join("disabled.toml"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].overlay.disable, Some(true));
    }

    #[test]
    fn test_overlay_replace_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
name = "my-agent"
mode = "subagent"
description = "Replace mode"
replace = true
"#;
        std::fs::write(tmp.path().join("replace.toml"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].overlay.replace, Some(true));
    }

    // --- Milestone 6: Rich permission tests ---

    #[test]
    fn test_toml_structured_bash_permissions() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
name = "bash-agent"
mode = "subagent"
description = "Agent with structured bash permissions"

[bash_permission]
action = "ask"
allow_patterns = ["git diff*", "git status*", "cargo test*"]
deny_patterns = ["curl*", "wget*", "rm *"]
"#;
        std::fs::write(tmp.path().join("bash.toml"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        let agent = &agents[0].agent;
        // Default bash action
        assert_eq!(agent.permissions.get("bash"), Some(&"ask".to_string()));
        // Structured allow patterns stored as prefixed keys
        assert_eq!(
            agent.permissions.get("bash:allow:git diff*"),
            Some(&"allow".to_string())
        );
        assert_eq!(
            agent.permissions.get("bash:allow:cargo test*"),
            Some(&"allow".to_string())
        );
        // Structured deny patterns stored as prefixed keys
        assert_eq!(
            agent.permissions.get("bash:deny:curl*"),
            Some(&"deny".to_string())
        );
        assert_eq!(
            agent.permissions.get("bash:deny:rm *"),
            Some(&"deny".to_string())
        );
    }

    #[test]
    fn test_toml_structured_path_permissions() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
name = "path-agent"
mode = "subagent"
description = "Agent with structured path permissions"

[path_permission]
allow = ["src/**", "crates/**"]
deny = [".git/**", "target/**"]
"#;
        std::fs::write(tmp.path().join("paths.toml"), content).unwrap();
        let agents = load_agents_from_dir(tmp.path()).unwrap();
        assert_eq!(agents.len(), 1);
        let agent = &agents[0].agent;
        assert_eq!(
            agent.permissions.get("path:allow:src/**"),
            Some(&"allow".to_string())
        );
        assert_eq!(
            agent.permissions.get("path:deny:.git/**"),
            Some(&"deny".to_string())
        );
    }

    #[test]
    fn test_structured_bash_to_ruleset_conversion() {
        let agent = Agent {
            name: "test".to_string(),
            role: None,
            description: "Test".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::from([
                ("bash".to_string(), "ask".to_string()),
                ("bash:allow:git diff*".to_string(), "allow".to_string()),
                ("bash:allow:cargo test*".to_string(), "allow".to_string()),
                ("bash:deny:rm *".to_string(), "deny".to_string()),
                ("bash:deny:curl*".to_string(), "deny".to_string()),
            ]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };
        let ruleset = agent.permission_ruleset();
        // Should have: bash ask rule, deny patterns rule, allow patterns rule
        let bash_rules: Vec<_> = ruleset
            .tool_rules
            .iter()
            .filter(|r| r.tool == "bash")
            .collect();
        assert!(
            bash_rules.len() >= 2,
            "should have bash deny+allow pattern rules"
        );
        // Deny rule should have patterns
        let deny_rule = bash_rules
            .iter()
            .find(|r| r.level == permission::PermissionLevel::Deny)
            .unwrap();
        assert!(deny_rule.bash_patterns.is_some());
        let patterns = deny_rule.bash_patterns.as_ref().unwrap();
        assert!(patterns.contains(&"rm *".to_string()));
        assert!(patterns.contains(&"curl*".to_string()));
        // Allow rule should have patterns
        let allow_rule = bash_rules
            .iter()
            .find(|r| r.level == permission::PermissionLevel::Allow)
            .unwrap();
        assert!(allow_rule.bash_patterns.is_some());
        let patterns = allow_rule.bash_patterns.as_ref().unwrap();
        assert!(patterns.contains(&"git diff*".to_string()));
        assert!(patterns.contains(&"cargo test*".to_string()));
    }

    #[test]
    fn test_structured_path_to_ruleset_conversion() {
        let agent = Agent {
            name: "test".to_string(),
            role: None,
            description: "Test".to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::from([
                ("path:allow:src/**".to_string(), "allow".to_string()),
                ("path:deny:.git/**".to_string(), "deny".to_string()),
            ]),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        };
        let ruleset = agent.permission_ruleset();
        assert_eq!(ruleset.path_rules.len(), 2);
        let allow_rule = ruleset
            .path_rules
            .iter()
            .find(|r| r.level == permission::PermissionLevel::Allow)
            .unwrap();
        assert_eq!(allow_rule.pattern, "src/**");
        let deny_rule = ruleset
            .path_rules
            .iter()
            .find(|r| r.level == permission::PermissionLevel::Deny)
            .unwrap();
        assert_eq!(deny_rule.pattern, ".git/**");
    }

    #[test]
    fn test_example_agents_parse() {
        let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("agents");
        if !examples_dir.exists() {
            eprintln!("Skipping example agents test: examples/agents/ not found");
            return;
        }

        let result = load_agents_from_dir(&examples_dir);
        assert!(
            result.is_ok(),
            "Failed to load example agents: {:?}",
            result.err()
        );

        let agents = result.unwrap();
        // Should have at least the 5 agent files (4 TOML + 1 MD)
        assert!(
            agents.len() >= 5,
            "Expected at least 5 example agents, got {}",
            agents.len()
        );

        // All agents should have names
        for fa in &agents {
            assert!(
                !fa.agent.name.is_empty(),
                "Agent from {} has empty name",
                fa.source
            );
            // All agents should have descriptions
            assert!(
                !fa.agent.description.is_empty(),
                "Agent '{}' has no description",
                fa.agent.name
            );
            // All agents should have valid modes
            // (parse_mode would have failed if mode was invalid)
        }
    }

    #[test]
    fn test_each_example_agent_file_parses() {
        let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("agents");
        if !examples_dir.exists() {
            return;
        }

        let entries: Vec<_> = std::fs::read_dir(&examples_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let p = e.path();
                p.extension()
                    .is_some_and(|ext| ext == "toml" || ext == "md")
            })
            .collect();

        assert!(!entries.is_empty(), "No example agent files found");

        for entry in &entries {
            let path = entry.path();
            let result = if path.extension().unwrap() == "toml" {
                load_agent_from_toml(&path)
            } else {
                load_agent_from_file(&path)
            };

            assert!(
                result.is_ok(),
                "Failed to parse {}: {:?}",
                path.display(),
                result.err()
            );
            let agent_opt = result.unwrap();
            if let Some(fa) = agent_opt {
                assert!(
                    !fa.agent.name.is_empty(),
                    "Agent from {} has empty name",
                    path.display()
                );
            }
        }
    }

    // Phase 4: Safety envelope integration tests for custom agents
}
