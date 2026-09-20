//! Portable plugin package detection and passive translation.
//!
//! This module parses package metadata only. It never runs package scripts or
//! opens MCP connections; the resulting passive contributions are consumed by
//! the existing asset and MCP owners.

use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::manifest::{
    PluginContributions, PluginManifest, PluginMcpServerContribution, PluginRuntimeSpec,
};

const MAX_JSON_BYTES: u64 = 1024 * 1024;
const MAX_SKILLS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageFormat {
    Codegg,
    AgentPluginsV1,
    ClaudeCompat,
}

impl PackageFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Codegg => "codegg",
            Self::AgentPluginsV1 => "agent-plugins-1.0",
            Self::ClaudeCompat => "claude-compat",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedPackage {
    pub format: PackageFormat,
    pub schema: Option<String>,
    pub root: PathBuf,
    pub manifest: PluginManifest,
    pub diagnostics: Vec<String>,
    pub unsupported: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PortableManifest {
    #[serde(rename = "$schema")]
    schema: Option<String>,
    name: String,
    version: String,
    description: Option<String>,
    author: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    license: Option<String>,
    keywords: Option<Vec<String>>,
    #[serde(default)]
    commands: Option<Value>,
    #[serde(default)]
    hooks: Option<Value>,
    #[serde(default)]
    lsp: Option<Value>,
    #[serde(default)]
    extensions: Option<Value>,
}

pub fn detect_and_load(root: &Path) -> Result<LoadedPackage, String> {
    let root = std::fs::canonicalize(root).map_err(|error| format!("package root: {error}"))?;
    if !root.is_dir() {
        return Err("package root is not a directory".into());
    }
    let native = root.join("manifest.toml");
    if native.is_file() {
        let raw = std::fs::read_to_string(&native).map_err(|error| error.to_string())?;
        let manifest = PluginManifest::from_toml_str(&raw)?;
        manifest.validate_contributions()?;
        return Ok(LoadedPackage {
            format: PackageFormat::Codegg,
            schema: None,
            root,
            manifest,
            diagnostics: Vec::new(),
            unsupported: Vec::new(),
        });
    }
    let (format, manifest_path) = if root.join("plugin.json").is_file() {
        (PackageFormat::AgentPluginsV1, root.join("plugin.json"))
    } else if root.join(".claude-plugin/plugin.json").is_file() {
        (
            PackageFormat::ClaudeCompat,
            root.join(".claude-plugin/plugin.json"),
        )
    } else {
        return Err("no supported plugin manifest (manifest.toml or plugin.json)".into());
    };
    load_portable(root, format, &manifest_path)
}

fn load_portable(
    root: PathBuf,
    format: PackageFormat,
    manifest_path: &Path,
) -> Result<LoadedPackage, String> {
    let value = read_json_bounded(manifest_path)?;
    let portable: PortableManifest = serde_json::from_value(value.clone())
        .map_err(|error| format!("{} manifest: {error}", format.as_str()))?;
    let schema = portable.schema.clone();
    if format == PackageFormat::AgentPluginsV1
        && !schema
            .as_deref()
            .is_some_and(|value| value.contains("agent-plugins") && value.contains("1.0.0"))
    {
        return Err("unsupported Agent Plugins $schema; expected 1.0.0".into());
    }
    if portable.name.trim().is_empty()
        || portable.name.len() > 128
        || portable.name.contains('/')
        || portable.name.contains('\\')
        || portable.version.trim().is_empty()
    {
        return Err("portable package name/version is invalid".into());
    }
    let mut diagnostics = Vec::new();
    let mut unsupported = Vec::new();
    if portable.commands.is_some() {
        unsupported.push("commands".into());
    }
    if portable.hooks.is_some() {
        unsupported.push("hooks".into());
    }
    if portable.lsp.is_some() {
        unsupported.push("lsp".into());
    }
    if portable.extensions.is_some() {
        diagnostics.push(
            "extensions are ignored because no portable extension semantics are implemented".into(),
        );
    }
    if !unsupported.is_empty() {
        diagnostics.push(format!(
            "unsupported passive components detected and ignored: {}",
            unsupported.join(", ")
        ));
    }
    let skills = discover_skills(&root, &mut diagnostics);
    let mcp_servers = parse_mcp(&root, &mut diagnostics);
    let contributions = PluginContributions {
        skills,
        agents: Vec::new(),
        instructions: Vec::new(),
        mcp_servers,
    };
    contributions.validate()?;
    let manifest = PluginManifest {
        name: portable.name,
        version: portable.version,
        runtime: PluginRuntimeSpec::Passive,
        contributions,
        description: portable.description,
        author: portable.author,
        homepage: portable.homepage.or(portable.repository),
        license: portable.license,
        ..PluginManifest::default()
    };
    let _ = portable.keywords;
    Ok(LoadedPackage {
        format,
        schema,
        root,
        manifest,
        diagnostics,
        unsupported,
    })
}

fn read_json_bounded(path: &Path) -> Result<Value, String> {
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_JSON_BYTES {
        return Err(format!("{} exceeds JSON size bound", path.display()));
    }
    let raw = std::fs::read(path).map_err(|error| error.to_string())?;
    serde_json::from_slice(&raw).map_err(|error| error.to_string())
}

fn discover_skills(root: &Path, diagnostics: &mut Vec<String>) -> Vec<String> {
    let skills_root = root.join("skills");
    let Ok(entries) = std::fs::read_dir(&skills_root) else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in entries.flatten().take(MAX_SKILLS) {
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_dir() {
            diagnostics.push(format!("skill '{}' is not a directory", path.display()));
            continue;
        }
        let skill_file = path.join("SKILL.md");
        let Ok(skill_metadata) = std::fs::symlink_metadata(&skill_file) else {
            diagnostics.push(format!("skill '{}' has no SKILL.md", path.display()));
            continue;
        };
        if !skill_metadata.is_file()
            || skill_file
                .canonicalize()
                .ok()
                .is_none_or(|p| !p.starts_with(root))
        {
            diagnostics.push(format!(
                "skill '{}' escapes or is not a regular file",
                path.display()
            ));
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        skills.push(relative);
    }
    skills
}

fn parse_mcp(root: &Path, diagnostics: &mut Vec<String>) -> Vec<PluginMcpServerContribution> {
    let path = root.join("mcp.json");
    if !path.is_file() {
        return Vec::new();
    }
    let value = match read_json_bounded(&path) {
        Ok(value) => value,
        Err(error) => {
            diagnostics.push(format!("mcp.json ignored: {error}"));
            return Vec::new();
        }
    };
    let Some(servers) = value
        .get("mcpServers")
        .or_else(|| value.get("servers"))
        .and_then(Value::as_object)
    else {
        diagnostics.push("mcp.json has no mcpServers object".into());
        return Vec::new();
    };
    let mut result = Vec::new();
    for (name, value) in servers.iter().take(32) {
        let Some(object) = value.as_object() else {
            diagnostics.push(format!("MCP server '{name}' is not an object"));
            continue;
        };
        let kind = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("stdio");
        let mut env = object
            .get("env")
            .and_then(Value::as_object)
            .map(|values| {
                values
                    .iter()
                    .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();
        env.entry("PLUGIN_ROOT".into())
            .or_insert_with(|| "${PLUGIN_ROOT}".into());
        env.entry("PLUGIN_DATA".into())
            .or_insert_with(|| "${PLUGIN_DATA}".into());
        let declaration = match kind {
            "stdio" | "local" => PluginMcpServerContribution {
                name: name.clone(),
                server_type: "local".into(),
                command: object
                    .get("command")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                args: string_array(object.get("args")),
                env,
                url: None,
                headers: HashMap::new(),
                timeout: object.get("timeout").and_then(Value::as_u64),
            },
            "streamable-http" | "http" | "remote" => PluginMcpServerContribution {
                name: name.clone(),
                server_type: "remote".into(),
                command: None,
                args: Vec::new(),
                env: HashMap::new(),
                url: object
                    .get("url")
                    .or_else(|| object.get("httpUrl"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                headers: object
                    .get("headers")
                    .and_then(Value::as_object)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|(key, value)| {
                                Some((key.clone(), value.as_str()?.to_string()))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                timeout: object.get("timeout").and_then(Value::as_u64),
            },
            other => {
                diagnostics.push(format!(
                    "MCP server '{name}' transport '{other}' is unsupported"
                ));
                continue;
            }
        };
        if declaration.command.is_none() && declaration.url.is_none() {
            diagnostics.push(format!("MCP server '{name}' has no usable endpoint"));
            continue;
        }
        result.push(declaration);
    }
    result
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .take(64)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn portable_package_loads_skills_and_mcp_without_runtime() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("skills")).unwrap();
        fs::create_dir(temp.path().join("skills/demo")).unwrap();
        fs::write(
            temp.path().join("skills/demo/SKILL.md"),
            "---\nname: demo\n---\nhello",
        )
        .unwrap();
        fs::write(temp.path().join("plugin.json"), r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"demo","version":"1.0.0"}"#).unwrap();
        let package = detect_and_load(temp.path()).unwrap();
        assert_eq!(package.format, PackageFormat::AgentPluginsV1);
        assert!(matches!(
            package.manifest.runtime,
            PluginRuntimeSpec::Passive
        ));
        assert_eq!(package.manifest.contributions.skills, vec!["skills/demo"]);
    }

    #[test]
    fn unsupported_components_are_diagnostic_only() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("plugin.json"), r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"demo","version":"1","hooks":{}}"#).unwrap();
        let package = detect_and_load(temp.path()).unwrap();
        assert_eq!(package.unsupported, vec!["hooks"]);
        assert!(package.manifest.capabilities.is_empty());
    }
}
