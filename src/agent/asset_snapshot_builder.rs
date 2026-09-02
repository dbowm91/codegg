//! Production builder for [`ProjectAssetSnapshot`].
//!
//! [`ProjectAssetSnapshotBuilder`] is the single side-effect-bearing
//! constructor for runtime assets. It accepts an explicit
//! [`AssetContext`], runs each subsystem resolver once, computes stable
//! per-asset and combined digests, and returns an immutable snapshot.
//!
//! The builder does not perform publication or generation management.
//! Milestone 3 will own those concerns. The builder is safe to call
//! concurrently for different project/workspace contexts and produces
//! deterministic output for unchanged inputs.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;

use crate::agent::asset_context::AssetContext;
use crate::agent::asset_snapshot::{
    BuiltSnapshot, ProjectAssetSnapshot, SnapshotBuildError, SnapshotBuildMetadata,
};
use crate::agent::instructions::ProjectInstructionResolver;
use crate::agent::registry::{AgentRegistry, ResolvedAgent};
use crate::config::schema::Config;
use crate::skills::{AssetDiscoveryConfig, AssetRegistry};

/// Configuration for the snapshot builder.
#[derive(Debug, Clone, Default)]
pub struct SnapshotBuilderConfig {
    pub asset_discovery: AssetDiscoveryConfig,
}

/// Builder. Construct via [`ProjectAssetSnapshotBuilder::new`] and call
/// [`ProjectAssetSnapshotBuilder::build`] with an explicit context.
#[derive(Debug, Clone)]
pub struct ProjectAssetSnapshotBuilder {
    config: SnapshotBuilderConfig,
    /// Cached `Config` to thread into `AgentRegistry`. Required because
    /// the registry still needs to honor the existing
    /// built-in/global/project/config/session overlay order. The
    /// `Config` itself is unchanged; only its inputs are now
    /// context-bound.
    config_doc: Arc<Config>,
}

impl ProjectAssetSnapshotBuilder {
    pub fn new(config: SnapshotBuilderConfig, config_doc: Arc<Config>) -> Self {
        Self { config, config_doc }
    }

    pub fn with_default_config_doc(config_doc: Arc<Config>) -> Self {
        Self::new(SnapshotBuilderConfig::default(), config_doc)
    }

    /// Build an immutable snapshot for the explicit context.
    pub fn build(&self, ctx: &AssetContext) -> Result<ProjectAssetSnapshot, SnapshotBuildError> {
        let started = Instant::now();
        let context = Arc::new(ctx.clone());

        // 1. Skills (one scan).
        let skills = self.build_skills(ctx).map_err(SnapshotBuildError::Skill)?;

        // 2. Agents (one pass through the registry, context-bound).
        let (agents, agent_diagnostics) =
            self.build_agents(ctx).map_err(SnapshotBuildError::Agent)?;

        // 3. Instructions (one bounded walk).
        let instruction_resolution = ProjectInstructionResolver::with_defaults().resolve(ctx);
        let instructions = instruction_resolution.fragments.clone();
        let instruction_text = instruction_resolution.merged.clone();
        let instruction_diagnostics = instruction_resolution.diagnostics.clone();

        let build_metadata = SnapshotBuildMetadata {
            built_at: Utc::now(),
            build_duration: started.elapsed(),
            config_revision: ctx.config_revision(),
        };

        let built = BuiltSnapshot {
            agents,
            agent_diagnostics,
            skills: Arc::new(skills),
            instructions,
            instruction_text,
            instruction_diagnostics,
            build_metadata,
        };

        Ok(built.assemble(context))
    }

    fn build_skills(&self, ctx: &AssetContext) -> Result<AssetRegistry, String> {
        let global_roots = ctx.global_roots();
        // The legacy AssetRegistry::build expects &[PathBuf] which is
        // cheaply constructible from a slice of references.
        let global_root_refs: Vec<std::path::PathBuf> = global_roots.to_vec();
        let plugin_sources = ctx
            .plugin_contributions()
            .into_iter()
            .flat_map(|set| set.plugins.iter())
            .flat_map(|plugin| plugin.skills.iter())
            .cloned()
            .collect::<Vec<_>>();
        let registry = AssetRegistry::build_with_plugin_sources(
            &self.config.asset_discovery,
            ctx.workspace_root(),
            &global_root_refs,
            &plugin_sources,
        );
        Ok(registry)
    }

    fn build_agents(
        &self,
        ctx: &AssetContext,
    ) -> Result<
        (
            BTreeMap<String, ResolvedAgent>,
            Vec<crate::agent::registry::AgentDiagnostic>,
        ),
        String,
    > {
        let registry =
            AgentRegistry::load_for_context(&self.config_doc, ctx).map_err(|e| format!("{e}"))?;
        let mut agents: BTreeMap<String, ResolvedAgent> = BTreeMap::new();
        for ra in registry.list() {
            agents.insert(ra.agent.name.clone(), ra.clone());
        }
        let mut diagnostics = registry.diagnostics().to_vec();
        let plugin_sets = ctx
            .plugin_contributions()
            .into_iter()
            .flat_map(|set| set.plugins.iter());
        for plugin in plugin_sets {
            let mut files = Vec::new();
            for source in &plugin.agents {
                if source.path.is_dir() {
                    match load_plugin_agents_from_dir(&source.path) {
                        Ok(mut loaded) => files.append(&mut loaded),
                        Err(error) => {
                            diagnostics.push(crate::agent::registry::AgentDiagnostic::new(
                                crate::agent::registry::AgentDiagnosticSeverity::Error,
                                format!("plugin:{}", plugin.plugin_id),
                                format!("failed to load plugin agent directory: {error}"),
                            ))
                        }
                    }
                } else {
                    let result = match source.path.extension().and_then(|ext| ext.to_str()) {
                        Some("md") => crate::agent::load_agent_from_file(&source.path),
                        Some("toml") => crate::agent::load_agent_from_toml(&source.path),
                        _ => Err(crate::error::AgentError::Invalid(
                            "plugin agent must be TOML or Markdown".into(),
                        )),
                    };
                    match result {
                        Ok(Some(file_agent)) => files.push(file_agent),
                        Ok(None) => diagnostics.push(crate::agent::registry::AgentDiagnostic::new(
                            crate::agent::registry::AgentDiagnosticSeverity::Warning,
                            format!("plugin:{}", plugin.plugin_id),
                            format!("plugin agent '{}' has no definition", source.relative_path),
                        )),
                        Err(error) => {
                            diagnostics.push(crate::agent::registry::AgentDiagnostic::new(
                                crate::agent::registry::AgentDiagnosticSeverity::Error,
                                format!("plugin:{}", plugin.plugin_id),
                                format!(
                                    "failed to parse plugin agent '{}': {error}",
                                    source.relative_path
                                ),
                            ))
                        }
                    }
                }
            }
            files.sort_by(|a, b| a.source.cmp(&b.source));
            for file_agent in files {
                let raw_name = file_agent.agent.name.clone();
                let raw_plugin = plugin
                    .plugin_id
                    .strip_prefix("plugin:")
                    .unwrap_or(&plugin.plugin_id);
                let name = format!("plugin:{raw_plugin}:{raw_name}");
                let mut agent = file_agent.agent;
                agent.name = name.clone();
                let mut agent_diagnostics = file_agent.diagnostics;
                for diagnostic in &mut agent_diagnostics {
                    diagnostic.agent_name = name.clone();
                    diagnostic.source = Some(crate::agent::registry::AgentSourceKind::PluginFile);
                }
                let resolved = ResolvedAgent {
                    agent,
                    sources: vec![crate::agent::registry::AgentSource {
                        kind: crate::agent::registry::AgentSourceKind::PluginFile,
                        path: Some(PathBuf::from(file_agent.source)),
                        name: name.clone(),
                    }],
                    diagnostics: agent_diagnostics.clone(),
                };
                if agents.insert(name.clone(), resolved).is_some() {
                    diagnostics.push(crate::agent::registry::AgentDiagnostic::new(
                        crate::agent::registry::AgentDiagnosticSeverity::Warning,
                        name.clone(),
                        "duplicate plugin agent identity; later path wins deterministically",
                    ));
                }
                diagnostics.extend(agent_diagnostics);
            }
        }
        Ok((agents, diagnostics))
    }
}

fn load_plugin_agents_from_dir(
    dir: &std::path::Path,
) -> Result<Vec<crate::agent::FileAgent>, crate::error::AgentError> {
    let canonical_dir = dir
        .canonicalize()
        .map_err(|error| crate::error::AgentError::Invalid(error.to_string()))?;
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| crate::error::AgentError::Invalid(error.to_string()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("md" | "toml")
            )
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries.truncate(crate::plugin::manifest::PluginContributions::MAX_PATHS);
    let mut agents = Vec::new();
    for path in entries {
        let canonical = path
            .canonicalize()
            .map_err(|error| crate::error::AgentError::Invalid(error.to_string()))?;
        if !canonical.starts_with(&canonical_dir) {
            return Err(crate::error::AgentError::Invalid(format!(
                "plugin agent path escapes contribution root: {}",
                path.display()
            )));
        }
        let loaded = match path.extension().and_then(|ext| ext.to_str()) {
            Some("md") => crate::agent::load_agent_from_file(&path)?,
            Some("toml") => crate::agent::load_agent_from_toml(&path)?,
            _ => None,
        };
        if let Some(agent) = loaded {
            agents.push(agent);
        }
    }
    Ok(agents)
}

impl crate::agent::asset_snapshot::SnapshotBuilder for ProjectAssetSnapshotBuilder {
    fn build(
        &self,
        ctx: &AssetContext,
    ) -> Result<ProjectAssetSnapshot, crate::agent::asset_snapshot::SnapshotBuildError> {
        Self::build(self, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::asset_context::{AssetContextBuilder, ProjectId};
    use crate::skills::AssetDiscoveryConfig;
    use std::fs;
    use tempfile::TempDir;

    fn make_config() -> Arc<Config> {
        Arc::new(Config::default())
    }

    fn default_builder() -> ProjectAssetSnapshotBuilder {
        ProjectAssetSnapshotBuilder::new(
            SnapshotBuilderConfig {
                asset_discovery: AssetDiscoveryConfig::default(),
            },
            make_config(),
        )
    }

    #[test]
    fn empty_workspace_produces_builtins_only_snapshot() {
        let tmp = TempDir::new().unwrap();
        let ctx = AssetContextBuilder::new()
            .with_synthetic_project_id(ProjectId::new())
            .with_workspace_root(tmp.path())
            .build()
            .unwrap();
        let snapshot = default_builder().build(&ctx).unwrap();
        // Built-in agents are still loaded; project-local overlays are
        // empty. Milestone 2 keeps the existing overlay order, which
        // includes the compiled built-ins.
        assert!(snapshot.agent_count() > 0);
        assert!(snapshot.instruction_fragments().is_empty());
        assert!(snapshot.instruction_block().is_empty());
        assert!(!snapshot.fingerprint.is_empty());
    }

    #[test]
    fn two_contexts_produce_independent_snapshots() {
        let a = TempDir::new().unwrap();
        let b = TempDir::new().unwrap();
        fs::write(a.path().join("AGENTS.md"), "build agents for project a").unwrap();
        fs::write(b.path().join("AGENTS.md"), "build agents for project b").unwrap();
        let ctx_a = AssetContextBuilder::new()
            .with_synthetic_project_id(ProjectId::new())
            .with_workspace_root(a.path())
            .build()
            .unwrap();
        let ctx_b = AssetContextBuilder::new()
            .with_synthetic_project_id(ProjectId::new())
            .with_workspace_root(b.path())
            .build()
            .unwrap();
        let snap_a = default_builder().build(&ctx_a).unwrap();
        let snap_b = default_builder().build(&ctx_b).unwrap();
        assert_ne!(snap_a.fingerprint, snap_b.fingerprint);
        assert_eq!(snap_a.instruction_block(), "build agents for project a");
        assert_eq!(snap_b.instruction_block(), "build agents for project b");
    }

    #[test]
    fn identical_inputs_produce_identical_fingerprints() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("AGENTS.md"), "stable instructions").unwrap();
        let ctx = AssetContextBuilder::new()
            .with_synthetic_project_id(ProjectId::new())
            .with_workspace_root(tmp.path())
            .build()
            .unwrap();
        let snap1 = default_builder().build(&ctx).unwrap();
        let snap2 = default_builder().build(&ctx).unwrap();
        assert_eq!(snap1.fingerprint, snap2.fingerprint);
    }
}
