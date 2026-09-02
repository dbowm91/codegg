//! Resolution of passive plugin contributions.
//!
//! This module is the translation seam between the durable plugin activation
//! authority and the existing runtime-asset/MCP owners. It performs bounded,
//! read-only validation and never invokes a plugin runtime.

use std::path::{Path, PathBuf};

use super::activation::ResolvedPluginActivationSet;
use super::manifest::PluginMcpServerContribution;
use super::registry::PluginInfo;

const MAX_RESOLVED_ASSETS: usize = 64;

#[derive(Debug, Clone, Default)]
pub struct ResolvedPluginContributionSet {
    pub plugins: Vec<ResolvedPluginContribution>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedPluginContribution {
    pub plugin_id: String,
    pub plugin_version: String,
    pub install_root: PathBuf,
    pub skills: Vec<PluginAssetPath>,
    pub agents: Vec<PluginAssetPath>,
    pub instructions: Vec<PluginAssetPath>,
    pub mcp_servers: Vec<ResolvedPluginMcpServer>,
}

#[derive(Debug, Clone)]
pub struct PluginAssetPath {
    pub plugin_id: String,
    pub plugin_version: String,
    pub relative_path: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ResolvedPluginMcpServer {
    pub plugin_id: String,
    pub plugin_version: String,
    pub canonical_name: String,
    pub declaration: PluginMcpServerContribution,
}

impl ResolvedPluginContributionSet {
    /// Resolve only active plugins from an immutable activation view.
    /// Plugins without a canonical installed directory cannot contribute
    /// filesystem assets; their MCP declarations are also rejected because
    /// the install identity is part of the provenance boundary.
    pub fn resolve(plugins: &[PluginInfo], activation: &ResolvedPluginActivationSet) -> Self {
        let mut result = Self::default();
        for plugin in plugins {
            if !activation.is_active(&plugin.id)
                || plugin.manifest.contributions == Default::default()
            {
                continue;
            }
            let Some(source) = plugin.source.as_ref() else {
                result.diagnostics.push(format!(
                    "plugin '{}' contributions disabled: install root is unavailable",
                    plugin.id
                ));
                continue;
            };
            let Some(root) = source.install_path.as_ref() else {
                result.diagnostics.push(format!(
                    "plugin '{}' contributions disabled: install root is unavailable",
                    plugin.id
                ));
                continue;
            };
            let Ok(root) = root.canonicalize() else {
                result.diagnostics.push(format!(
                    "plugin '{}' contributions disabled: install root is unavailable",
                    plugin.id
                ));
                continue;
            };
            if let Err(error) = plugin.manifest.validate_contributions() {
                result
                    .diagnostics
                    .push(format!("plugin '{}': {error}", plugin.id));
                continue;
            }

            let mut contribution = ResolvedPluginContribution {
                plugin_id: plugin.id.clone(),
                plugin_version: plugin.manifest.version.clone(),
                install_root: root.clone(),
                skills: Vec::new(),
                agents: Vec::new(),
                instructions: Vec::new(),
                mcp_servers: Vec::new(),
            };
            for (target, label) in [
                (&mut contribution.skills, "skill"),
                (&mut contribution.agents, "agent"),
                (&mut contribution.instructions, "instruction"),
            ] {
                let declared = match label {
                    "skill" => &plugin.manifest.contributions.skills,
                    "agent" => &plugin.manifest.contributions.agents,
                    _ => &plugin.manifest.contributions.instructions,
                };
                for relative in declared.iter().take(MAX_RESOLVED_ASSETS) {
                    match resolve_asset_path(&root, relative) {
                        Ok(path) => target.push(PluginAssetPath {
                            plugin_id: plugin.id.clone(),
                            plugin_version: plugin.manifest.version.clone(),
                            relative_path: relative.clone(),
                            path,
                        }),
                        Err(error) => result.diagnostics.push(format!(
                            "plugin '{}' {label} contribution '{relative}' disabled: {error}",
                            plugin.id
                        )),
                    }
                }
            }
            for declaration in plugin.manifest.contributions.mcp_servers.iter().cloned() {
                let canonical_name = canonical_mcp_name(&plugin.id, &declaration.name);
                contribution.mcp_servers.push(ResolvedPluginMcpServer {
                    plugin_id: plugin.id.clone(),
                    plugin_version: plugin.manifest.version.clone(),
                    canonical_name,
                    declaration,
                });
            }
            result.plugins.push(contribution);
        }
        result.plugins.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        result
    }

    pub fn mcp_servers(&self) -> impl Iterator<Item = &ResolvedPluginMcpServer> {
        self.plugins
            .iter()
            .flat_map(|plugin| plugin.mcp_servers.iter())
    }

    pub fn inventory(&self) -> Vec<String> {
        self.plugins
            .iter()
            .flat_map(|plugin| {
                [
                    format!("{} skill(s)", plugin.skills.len()),
                    format!("{} agent(s)", plugin.agents.len()),
                    format!("{} instruction(s)", plugin.instructions.len()),
                    format!("{} MCP server(s)", plugin.mcp_servers.len()),
                ]
            })
            .collect()
    }
}

pub fn canonical_mcp_name(plugin_id: &str, declared_name: &str) -> String {
    let plugin_name = plugin_id.strip_prefix("plugin:").unwrap_or(plugin_id);
    format!("plugin:{plugin_name}:{declared_name}")
}

fn resolve_asset_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("path is absolute or escapes the plugin root".into());
    }
    let candidate = root.join(path);
    let canonical = candidate
        .canonicalize()
        .map_err(|error| format!("path is missing or unreadable: {error}"))?;
    if !canonical.starts_with(root) {
        return Err("symlink escapes the installed plugin root".into());
    }
    if !canonical.is_dir() && !canonical.is_file() {
        return Err("path is not a regular file or directory".into());
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_deterministically_namespaced() {
        assert_eq!(
            canonical_mcp_name("plugin:docs", "server"),
            "plugin:docs:server"
        );
        assert_eq!(canonical_mcp_name("docs", "server"), "plugin:docs:server");
    }

    #[test]
    fn relative_path_rejects_escape() {
        let root = tempfile::tempdir().unwrap();
        assert!(resolve_asset_path(root.path(), "../outside").is_err());
    }
}
