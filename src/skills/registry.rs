use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::candidate::{EffectiveSkill, ResolvedRegistry, ShadowedAlternative, SkillCandidate};
use super::diagnostic::Diagnostic;
use super::parser;
use super::resource::{ResourceError, ResourceHandle, ResourceReadLimits};
use super::source::{AssetDiscoveryConfig, SourceKind, SourceRoot, SourceSummary};

const MAX_GLOBAL_DISCOVERY_ROOTS: usize = 16;

#[derive(Debug)]
pub struct AssetRegistry {
    pub effective: Vec<EffectiveSkill>,
    pub diagnostics: Vec<Diagnostic>,
    pub sources: Vec<SourceSummary>,
}

impl AssetRegistry {
    pub fn build(
        config: &AssetDiscoveryConfig,
        project_root: &Path,
        global_roots: &[PathBuf],
    ) -> Self {
        Self::build_with_plugin_sources(config, project_root, global_roots, &[])
    }

    pub fn build_with_plugin_sources(
        config: &AssetDiscoveryConfig,
        project_root: &Path,
        global_roots: &[PathBuf],
        plugin_sources: &[crate::plugin::PluginAssetPath],
    ) -> Self {
        Self::build_with_home_and_plugin_sources(
            config,
            project_root,
            global_roots,
            dirs::home_dir(),
            plugin_sources,
        )
    }

    /// Build with an explicit home directory. Supplying `None` disables
    /// home-relative vendor roots, which keeps isolated callers and fixtures
    /// independent of the process user's private skill installation.
    pub fn build_with_home(
        config: &AssetDiscoveryConfig,
        project_root: &Path,
        global_roots: &[PathBuf],
        home_dir: Option<PathBuf>,
    ) -> Self {
        Self::build_with_home_and_plugin_sources(config, project_root, global_roots, home_dir, &[])
    }

    fn build_with_home_and_plugin_sources(
        config: &AssetDiscoveryConfig,
        project_root: &Path,
        global_roots: &[PathBuf],
        home_dir: Option<PathBuf>,
        plugin_sources: &[crate::plugin::PluginAssetPath],
    ) -> Self {
        let mut all_candidates: Vec<SkillCandidate> = Vec::new();
        let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
        let mut source_summaries: Vec<SourceSummary> = Vec::new();

        let source_roots =
            resolve_source_roots_with_home(config, project_root, global_roots, home_dir);
        let mut plugin_roots = Vec::new();
        for source in plugin_sources {
            let path = source
                .path
                .canonicalize()
                .unwrap_or_else(|_| source.path.clone());
            let (root_path, single_file) = if path.is_dir() {
                (path.clone(), None)
            } else {
                (
                    path.parent().unwrap_or(&path).to_path_buf(),
                    Some(path.clone()),
                )
            };
            plugin_roots.push((
                SourceRoot {
                    kind: SourceKind::Plugin,
                    canonical_path: root_path,
                    display_path: path,
                    workspace_depth: 0,
                    plugin_id: Some(source.plugin_id.clone()),
                },
                single_file,
            ));
        }
        plugin_roots.sort_by(|a, b| {
            a.0.plugin_id
                .cmp(&b.0.plugin_id)
                .then_with(|| a.0.display_path.cmp(&b.0.display_path))
        });

        for source_root in &source_roots {
            if let Some(summary) =
                source_summaries
                    .iter_mut()
                    .find(|summary: &&mut SourceSummary| {
                        summary.canonical_path == source_root.canonical_path
                    })
            {
                summary.alias_paths.push(source_root.display_path.clone());
                continue;
            }
            let (candidates, diagnostics) = discover_in_root(source_root, config, None);
            let discovered = candidates.len()
                + diagnostics
                    .iter()
                    .filter(|d| d.severity == super::diagnostic::Severity::Error)
                    .count();
            let valid = candidates.len();
            let invalid = diagnostics
                .iter()
                .filter(|d| d.severity == super::diagnostic::Severity::Error)
                .count();
            source_summaries.push(SourceSummary {
                kind: source_root.kind,
                canonical_path: source_root.canonical_path.clone(),
                alias_paths: Vec::new(),
                workspace_depth: source_root.workspace_depth,
                discovered_count: discovered,
                valid_count: valid,
                invalid_count: invalid,
            });
            all_candidates.extend(candidates);
            all_diagnostics.extend(diagnostics);
        }

        for (source_root, single_file) in &plugin_roots {
            let (candidates, diagnostics) =
                discover_in_root(source_root, config, single_file.as_deref());
            let discovered = candidates.len()
                + diagnostics
                    .iter()
                    .filter(|d| d.severity == super::diagnostic::Severity::Error)
                    .count();
            source_summaries.push(SourceSummary {
                kind: source_root.kind,
                canonical_path: source_root.canonical_path.clone(),
                alias_paths: Vec::new(),
                workspace_depth: source_root.workspace_depth,
                discovered_count: discovered,
                valid_count: candidates.len(),
                invalid_count: diagnostics
                    .iter()
                    .filter(|d| d.severity == super::diagnostic::Severity::Error)
                    .count(),
            });
            all_candidates.extend(candidates);
            all_diagnostics.extend(diagnostics);
        }

        all_candidates.sort_by(|left, right| {
            source_precedence_class(left.source_kind)
                .cmp(&source_precedence_class(right.source_kind))
                .then_with(|| left.workspace_depth.cmp(&right.workspace_depth))
                .then_with(|| {
                    left.source_kind
                        .precedence_rank()
                        .cmp(&right.source_kind.precedence_rank())
                })
                .then_with(|| left.source_path.cmp(&right.source_path))
        });
        let mut physical_packages = std::collections::HashSet::new();
        all_candidates.retain(|candidate| {
            candidate
                .source_path
                .canonicalize()
                .map(|path| physical_packages.insert(path))
                .unwrap_or(true)
        });
        let resolved = resolve(all_candidates, config);
        all_diagnostics.extend(resolved.diagnostics);

        Self {
            effective: resolved.effective,
            diagnostics: all_diagnostics,
            sources: source_summaries,
        }
    }

    pub fn get(&self, name: &str) -> Option<&EffectiveSkill> {
        let normalized = name.trim().to_lowercase();
        self.effective
            .iter()
            .find(|s| s.normalized_name == normalized)
    }

    pub fn list(&self) -> &[EffectiveSkill] {
        &self.effective
    }

    pub fn find_matching(&self, query: &str) -> Vec<&EffectiveSkill> {
        let query_lower = query.to_lowercase();
        self.effective
            .iter()
            .filter(|s| {
                s.normalized_name.contains(&query_lower)
                    || s.description.to_lowercase().contains(&query_lower)
                    || s.metadata.values().any(|v| {
                        v.as_str()
                            .map(|s| s.to_lowercase().contains(&query_lower))
                            .unwrap_or(false)
                    })
            })
            .collect()
    }

    pub fn build_system_prompt(&self) -> String {
        if self.effective.is_empty() {
            return String::new();
        }
        let mut prompt = String::from("## Available Skills\n\n");
        prompt.push_str(
            "The following skills are available. Activate one with the `skill` tool using its `name` argument.\n\n",
        );
        for skill in &self.effective {
            prompt.push_str(&format!("- **{}**: {}\n", skill.name, skill.description));
        }
        prompt.push('\n');
        prompt
    }

    pub fn activate(&self, name: &str) -> Option<String> {
        self.get(name).map(|s| s.body.clone())
    }

    pub fn resource_handle(
        &self,
        skill_name: &str,
        relative_path: impl AsRef<Path>,
        limits: ResourceReadLimits,
    ) -> Result<ResourceHandle, ResourceError> {
        let skill = self
            .get(skill_name)
            .ok_or_else(|| ResourceError::NotFound {
                skill: skill_name.to_string(),
                path: relative_path.as_ref().to_path_buf(),
            })?;
        skill.resource_handle(relative_path, limits)
    }
}

fn resolve_source_roots_with_home(
    config: &AssetDiscoveryConfig,
    project_root: &Path,
    global_roots: &[PathBuf],
    home_dir: Option<PathBuf>,
) -> Vec<SourceRoot> {
    let mut roots = Vec::new();

    let project_sources = [
        (SourceKind::CodeGGProject, ".codegg/skills"),
        (SourceKind::AgentsProject, ".agents/skills"),
        (SourceKind::OpenCodeProject, ".opencode/skills"),
        (SourceKind::PiProject, ".pi/skills"),
        (SourceKind::CursorProject, ".cursor/skills"),
        (SourceKind::GeminiProject, ".gemini/skills"),
        (SourceKind::CopilotProject, ".github/skills"),
        (SourceKind::ClineProject, ".cline/skills"),
        (SourceKind::ClineProject, ".clinerules/skills"),
        (SourceKind::RooProject, ".roo/skills"),
        (SourceKind::FactoryProject, ".factory/skills"),
        (SourceKind::ClaudeProject, ".claude/skills"),
    ];
    for (scope, depth) in project_scope_roots(project_root) {
        for (kind, relative) in project_sources {
            if !config.enabled_sources.contains(&kind) {
                continue;
            }
            let path = scope.join(relative);
            if path.is_dir() {
                if let Ok(canonical) = path.canonicalize() {
                    roots.push(SourceRoot {
                        kind,
                        display_path: path,
                        canonical_path: canonical,
                        workspace_depth: depth,
                        plugin_id: None,
                    });
                }
            }
        }
    }

    for global_root in global_roots.iter().take(MAX_GLOBAL_DISCOVERY_ROOTS) {
        if config.enabled_sources.contains(&SourceKind::CodeGGGlobal) {
            let path = global_root.join("codegg").join("skills");
            if path.is_dir() {
                if let Ok(canonical) = path.canonicalize() {
                    roots.push(SourceRoot {
                        kind: SourceKind::CodeGGGlobal,
                        display_path: path,
                        canonical_path: canonical,
                        workspace_depth: 0,
                        plugin_id: None,
                    });
                }
            }
        }
        if config.enabled_sources.contains(&SourceKind::AgentsGlobal) {
            let path = global_root.join("agents").join("skills");
            if path.is_dir() {
                if let Ok(canonical) = path.canonicalize() {
                    roots.push(SourceRoot {
                        kind: SourceKind::AgentsGlobal,
                        display_path: path,
                        canonical_path: canonical,
                        workspace_depth: 0,
                        plugin_id: None,
                    });
                }
            }
        }
        if config.enabled_sources.contains(&SourceKind::OpenCodeGlobal) {
            let path = global_root.join("opencode").join("skills");
            if path.is_dir() {
                if let Ok(canonical) = path.canonicalize() {
                    roots.push(SourceRoot {
                        kind: SourceKind::OpenCodeGlobal,
                        display_path: path,
                        canonical_path: canonical,
                        workspace_depth: 0,
                        plugin_id: None,
                    });
                }
            }
        }
        if config.enabled_sources.contains(&SourceKind::ClaudeGlobal) {
            let path = global_root.join("claude").join("skills");
            if path.is_dir() {
                if let Ok(canonical) = path.canonicalize() {
                    roots.push(SourceRoot {
                        kind: SourceKind::ClaudeGlobal,
                        display_path: path,
                        canonical_path: canonical,
                        workspace_depth: 0,
                        plugin_id: None,
                    });
                }
            }
        }
        for (kind, vendor) in [
            (SourceKind::ClineGlobal, "cline"),
            (SourceKind::PiGlobal, "pi"),
            (SourceKind::RooGlobal, "roo"),
            (SourceKind::CopilotGlobal, "copilot"),
            (SourceKind::FactoryGlobal, "factory"),
        ] {
            if config.enabled_sources.contains(&kind) {
                let path = global_root.join(vendor).join("skills");
                if path.is_dir() {
                    if let Ok(canonical) = path.canonicalize() {
                        roots.push(SourceRoot {
                            kind,
                            display_path: path,
                            canonical_path: canonical,
                            workspace_depth: 0,
                            plugin_id: None,
                        });
                    }
                }
            }
        }
    }

    // Common user-home locations are explicit candidates, never recursive
    // scans. Existing config-directory roots above remain compatibility paths.
    if let Some(home) = home_dir {
        let portable = [
            (SourceKind::AgentsGlobal, home.join(".agents/skills")),
            (SourceKind::ClaudeGlobal, home.join(".claude/skills")),
            (
                SourceKind::OpenCodeGlobal,
                home.join(".config/opencode/skills"),
            ),
            (SourceKind::ClineGlobal, home.join(".cline/skills")),
            (SourceKind::PiGlobal, home.join(".pi/agent/skills")),
            (SourceKind::RooGlobal, home.join(".roo/skills")),
            (SourceKind::CopilotGlobal, home.join(".copilot/skills")),
            (SourceKind::FactoryGlobal, home.join(".factory/skills")),
        ];
        for (kind, path) in portable {
            if config.enabled_sources.contains(&kind) && path.is_dir() {
                if let Ok(canonical) = path.canonicalize() {
                    roots.push(SourceRoot {
                        kind,
                        display_path: path,
                        canonical_path: canonical,
                        workspace_depth: 0,
                        plugin_id: None,
                    });
                }
            }
        }
    }

    // User-configured extra roots (`skills.paths`). Unlike the global roots
    // above, each entry is already a skills directory, so nothing is
    // appended. They are canonicalized through the same bounds, so a
    // non-directory or unreadable path is skipped rather than trusted.
    if config.enabled_sources.contains(&SourceKind::Configured) {
        for root in &config.configured_roots {
            if root.is_dir() {
                if let Ok(canonical) = root.canonicalize() {
                    roots.push(SourceRoot {
                        kind: SourceKind::Configured,
                        display_path: root.clone(),
                        canonical_path: canonical,
                        workspace_depth: 0,
                        plugin_id: None,
                    });
                }
            }
        }
    }

    roots.sort_by(|left, right| {
        source_precedence_class(left.kind)
            .cmp(&source_precedence_class(right.kind))
            .then_with(|| left.workspace_depth.cmp(&right.workspace_depth))
            .then_with(|| {
                left.kind
                    .precedence_rank()
                    .cmp(&right.kind.precedence_rank())
            })
            .then_with(|| left.canonical_path.cmp(&right.canonical_path))
    });
    roots
}

fn project_scope_roots(project_root: &Path) -> Vec<(PathBuf, u8)> {
    const MAX_ANCESTOR_DEPTH: usize = 8;
    let mut scopes = vec![(project_root.to_path_buf(), 0)];
    let mut current = project_root.to_path_buf();
    for depth in 1..=MAX_ANCESTOR_DEPTH {
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent.to_path_buf();
        scopes.push((current.clone(), depth as u8));
        if current.join(".git").exists() {
            break;
        }
    }
    if !scopes.iter().any(|(path, _)| path.join(".git").exists()) {
        scopes.truncate(1);
    }
    scopes
}

fn source_precedence_class(kind: SourceKind) -> u8 {
    if kind.is_project_local() {
        0
    } else if kind.is_global() {
        1
    } else {
        2
    }
}

fn discover_in_root(
    source_root: &SourceRoot,
    config: &AssetDiscoveryConfig,
    single_file: Option<&Path>,
) -> (Vec<SkillCandidate>, Vec<Diagnostic>) {
    let mut candidates = Vec::new();
    let mut diagnostics = Vec::new();
    let mut skill_count = 0;
    let mut parsed_targets = std::collections::HashSet::new();

    let entries = match std::fs::read_dir(&source_root.canonical_path) {
        Ok(e) => e,
        Err(e) => {
            diagnostics.push(Diagnostic::warning(
                format!("failed to read directory: {e}"),
                source_root.canonical_path.display().to_string(),
            ));
            return (candidates, diagnostics);
        }
    };

    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|a| a.file_name());

    for entry in &entries {
        if skill_count >= config.max_skills_per_root {
            diagnostics.push(Diagnostic::warning(
                format!(
                    "skill count truncated at {} per root",
                    config.max_skills_per_root
                ),
                source_root.canonical_path.display().to_string(),
            ));
            break;
        }

        let path = entry.path();
        if let Some(single_file) = single_file {
            if path != single_file {
                continue;
            }
        }
        let source_kind = source_root.kind;

        if path.is_dir() {
            let skill_file = path.join("SKILL.md");
            if skill_file.is_file() {
                match validate_symlink_boundary(&skill_file, &source_root.canonical_path) {
                    Ok(()) => {}
                    Err(diag) => {
                        diagnostics.push(diag);
                        continue;
                    }
                }
                let target = match skill_file.canonicalize() {
                    Ok(target) => target,
                    Err(_) => continue,
                };
                if !parsed_targets.insert(target) {
                    continue;
                }
                match parser::parse_candidate(&skill_file, source_kind, config) {
                    Ok(mut candidate) => {
                        candidate.workspace_depth = source_root.workspace_depth;
                        namespace_plugin_candidate(&mut candidate, source_root);
                        candidates.push(candidate);
                        skill_count += 1;
                    }
                    Err(diag) => {
                        diagnostics.push(diag);
                    }
                }
            }
        } else if (source_kind == SourceKind::CodeGGNativeCompat
            || source_kind == SourceKind::CodeGGProject
            || source_kind == SourceKind::Plugin)
            && path.extension().and_then(|e| e.to_str()) == Some("md")
        {
            match validate_symlink_boundary(&path, &source_root.canonical_path) {
                Ok(()) => {}
                Err(diag) => {
                    diagnostics.push(diag);
                    continue;
                }
            }
            let target = match path.canonicalize() {
                Ok(target) => target,
                Err(_) => continue,
            };
            if !parsed_targets.insert(target) {
                continue;
            }
            let compat_kind = if source_kind == SourceKind::CodeGGProject {
                SourceKind::CodeGGNativeCompat
            } else {
                source_kind
            };
            match parser::parse_candidate(&path, compat_kind, config) {
                Ok(mut candidate) => {
                    candidate.workspace_depth = source_root.workspace_depth;
                    namespace_plugin_candidate(&mut candidate, source_root);
                    candidates.push(candidate);
                    skill_count += 1;
                }
                Err(diag) => {
                    diagnostics.push(diag);
                }
            }
        }
    }

    (candidates, diagnostics)
}

fn namespace_plugin_candidate(candidate: &mut SkillCandidate, source_root: &SourceRoot) {
    let Some(plugin_id) = source_root.plugin_id.as_deref() else {
        return;
    };
    let name = candidate.name.clone();
    let raw_id = plugin_id.strip_prefix("plugin:").unwrap_or(plugin_id);
    candidate.name = format!("plugin:{raw_id}:{name}");
    candidate.normalized_name = candidate.name.to_lowercase();
    candidate.metadata.insert(
        "plugin_id".to_string(),
        serde_json::Value::String(plugin_id.to_string()),
    );
}

fn validate_symlink_boundary(file: &Path, root: &Path) -> Result<(), Diagnostic> {
    let location = file.display().to_string();
    let canonical = file.canonicalize().map_err(|e| {
        Diagnostic::error(
            format!("failed to canonicalize path: {e}"),
            location.clone(),
        )
    })?;
    if !canonical.starts_with(root) {
        return Err(Diagnostic::error(
            "symlink escapes source root boundary",
            location,
        ));
    }
    if let Some(parent) = file.parent() {
        let canonical_parent = parent.canonicalize().map_err(|e| {
            Diagnostic::error(
                format!("failed to canonicalize parent: {e}"),
                location.clone(),
            )
        })?;
        if !canonical_parent.starts_with(root) {
            return Err(Diagnostic::error(
                "symlink escapes source root boundary",
                location,
            ));
        }
    }
    Ok(())
}

fn resolve(candidates: Vec<SkillCandidate>, _config: &AssetDiscoveryConfig) -> ResolvedRegistry {
    let mut by_name: HashMap<String, Vec<SkillCandidate>> = HashMap::new();
    let mut diagnostics = Vec::new();

    for candidate in candidates {
        diagnostics.extend(candidate.diagnostics.clone());
        by_name
            .entry(candidate.normalized_name.clone())
            .or_default()
            .push(candidate);
    }

    let mut effective = Vec::new();

    for (_name, mut group) in by_name {
        group.sort_by(|left, right| {
            source_precedence_class(left.source_kind)
                .cmp(&source_precedence_class(right.source_kind))
                .then_with(|| left.workspace_depth.cmp(&right.workspace_depth))
                .then_with(|| {
                    left.source_kind
                        .precedence_rank()
                        .cmp(&right.source_kind.precedence_rank())
                })
                .then_with(|| left.source_path.cmp(&right.source_path))
        });

        let valid_candidates: Vec<_> = group
            .iter()
            .filter(|c| {
                c.diagnostics
                    .iter()
                    .all(|d| d.severity != super::diagnostic::Severity::Error)
            })
            .collect();

        if valid_candidates.is_empty() {
            if let Some(invalid) = group.first() {
                diagnostics.push(Diagnostic::warning(
                    format!(
                        "all candidates for '{}' are invalid; no effective skill produced",
                        invalid.normalized_name
                    ),
                    invalid.source_path.display().to_string(),
                ));
            }
            continue;
        }

        let winner = valid_candidates
            .into_iter()
            .next()
            .expect("valid_candidates is non-empty after the guard above");
        let shadowed: Vec<ShadowedAlternative> = group
            .iter()
            .filter(|c| {
                c.normalized_name == winner.normalized_name && c.source_path != winner.source_path
            })
            .map(|c| ShadowedAlternative {
                source_kind: c.source_kind,
                workspace_depth: c.workspace_depth,
                source_path: c.source_path.clone(),
                content_digest: c.content_digest.clone(),
                diagnostics: c.diagnostics.clone(),
            })
            .collect();

        if !shadowed.is_empty() {
            diagnostics.push(Diagnostic::info(
                format!(
                    "skill '{}' shadows {} alternative(s)",
                    winner.normalized_name,
                    shadowed.len()
                ),
                winner.source_path.display().to_string(),
            ));
        }

        effective.push(EffectiveSkill {
            name: winner.name.clone(),
            normalized_name: winner.normalized_name.clone(),
            description: winner.description.clone(),
            source_kind: winner.source_kind,
            workspace_depth: winner.workspace_depth,
            source_path: winner.source_path.clone(),
            package_root: winner.package_root.clone(),
            content_digest: winner.content_digest.clone(),
            metadata: winner.metadata.clone(),
            resources: winner.resources.clone(),
            body: winner.body.clone(),
            precedence_rank: winner.source_kind.precedence_rank(),
            shadowed_alternatives: shadowed,
        });
    }

    effective.sort_by(|a, b| a.normalized_name.cmp(&b.normalized_name));

    ResolvedRegistry {
        effective,
        diagnostics,
        sources: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn test_config() -> AssetDiscoveryConfig {
        AssetDiscoveryConfig::default()
    }

    fn build_isolated(
        config: &AssetDiscoveryConfig,
        project_root: &Path,
        global_roots: &[PathBuf],
    ) -> AssetRegistry {
        AssetRegistry::build_with_home(config, project_root, global_roots, None)
    }

    #[test]
    fn explicit_home_candidates_are_discovered_without_process_home_mutation() {
        let project = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let candidates = [
            (".agents/skills", SourceKind::AgentsGlobal),
            (".claude/skills", SourceKind::ClaudeGlobal),
            (".config/opencode/skills", SourceKind::OpenCodeGlobal),
            (".cline/skills", SourceKind::ClineGlobal),
            (".pi/agent/skills", SourceKind::PiGlobal),
            (".roo/skills", SourceKind::RooGlobal),
            (".copilot/skills", SourceKind::CopilotGlobal),
            (".factory/skills", SourceKind::FactoryGlobal),
        ];
        for (path, _) in candidates {
            fs::create_dir_all(home.path().join(path)).unwrap();
        }
        let roots = resolve_source_roots_with_home(
            &test_config(),
            project.path(),
            &[],
            Some(home.path().to_path_buf()),
        );
        for (path, kind) in candidates {
            assert!(roots.iter().any(|root| {
                root.kind == kind && root.canonical_path == home.path().join(path)
            }));
        }
    }

    #[test]
    fn home_skill_is_loaded_from_the_injected_user_root() {
        let project = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let skill = home.path().join(".agents/skills/from-home");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: from-home\ndescription: Home skill fixture\n---\nBody",
        )
        .unwrap();
        let registry = AssetRegistry::build_with_home(
            &test_config(),
            project.path(),
            &[],
            Some(home.path().to_path_buf()),
        );
        assert_eq!(
            registry.get("from-home").unwrap().source_kind,
            SourceKind::AgentsGlobal
        );
    }

    /// `skills.paths` entries are skills directories themselves, so the
    /// skill is discovered without any `<vendor>/skills` join.
    #[test]
    fn configured_root_is_used_as_a_skills_directory() {
        let project = TempDir::new().unwrap();
        let extra = TempDir::new().unwrap();
        let skill_dir = extra.path().join("from-config");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: from-config\ndescription: A skill discovered via skills.paths\n---\n\nBody.\n",
        )
        .unwrap();

        let mut config = test_config();
        config.configured_roots = vec![extra.path().to_path_buf()];

        let registry = build_isolated(&config, project.path(), &[]);
        let names: Vec<_> = registry.effective.iter().map(|s| s.name.clone()).collect();
        assert!(
            names.contains(&"from-config".to_string()),
            "expected configured root skill, got {names:?}"
        );
        assert!(
            registry
                .sources
                .iter()
                .any(|s| s.kind == SourceKind::Configured),
            "configured root should be reported as a source"
        );
    }

    /// A configured path must not be mistaken for a global *parent* root:
    /// `<root>/codegg/skills` must not be joined on.
    #[test]
    fn configured_root_is_not_treated_as_a_global_parent() {
        let project = TempDir::new().unwrap();
        let extra = TempDir::new().unwrap();
        // This layout is what a global-root parent would produce; under the
        // configured-root contract it must be ignored.
        let nested = extra.path().join("codegg").join("skills");
        fs::create_dir_all(nested.join("nested")).unwrap();
        fs::write(
            nested.join("nested").join("SKILL.md"),
            "---\nname: nested\ndescription: Must not be discovered\n---\n\nBody.\n",
        )
        .unwrap();

        let mut config = test_config();
        config.configured_roots = vec![extra.path().to_path_buf()];

        let registry = build_isolated(&config, project.path(), &[]);
        let names: Vec<_> = registry.effective.iter().map(|s| s.name.clone()).collect();
        assert!(
            !names.contains(&"nested".to_string()),
            "configured root must not double-join <vendor>/skills, got {names:?}"
        );
    }

    /// `skills.enabled = false` clears every source.
    #[test]
    fn disabled_sources_suppress_project_skills() {
        let project = TempDir::new().unwrap();
        let skill_dir = project.path().join(".codegg").join("skills").join("local");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: local\ndescription: A project skill\n---\n\nBody.\n",
        )
        .unwrap();

        let mut config = test_config();
        config.enabled_sources.clear();

        let registry = build_isolated(&config, project.path(), &[]);
        assert!(registry.effective.is_empty());
    }

    /// A non-directory entry in `skills.paths` is skipped, not trusted.
    #[test]
    fn nonexistent_configured_root_is_skipped() {
        let project = TempDir::new().unwrap();
        let mut config = test_config();
        config.configured_roots = vec![project.path().join("does-not-exist")];
        let registry = build_isolated(&config, project.path(), &[]);
        assert!(registry.effective.is_empty());
        assert!(
            !registry
                .sources
                .iter()
                .any(|s| s.kind == SourceKind::Configured),
            "a missing configured root must not be reported as a live source"
        );
    }

    #[test]
    fn empty_project_builds_empty_registry() {
        let dir = TempDir::new().unwrap();
        let config = test_config();
        let registry = build_isolated(&config, dir.path(), &[]);
        assert!(registry.effective.is_empty());
        assert!(registry.diagnostics.is_empty());
    }

    #[test]
    fn discover_codegg_project_skills() {
        let dir = TempDir::new().unwrap();
        let skills_dir = dir.path().join(".codegg").join("skills");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: project-skill\ndescription: A project skill\n---\nBody",
        )
        .unwrap();

        let config = test_config();
        let registry = build_isolated(&config, dir.path(), &[]);
        assert_eq!(registry.effective.len(), 1);
        assert_eq!(registry.effective[0].name, "project-skill");
    }

    #[test]
    fn discover_agents_project_skills() {
        let dir = TempDir::new().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills").join("my-skill");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: From agents\n---\nBody",
        )
        .unwrap();

        let config = test_config();
        let registry = build_isolated(&config, dir.path(), &[]);
        assert_eq!(registry.effective.len(), 1);
        assert_eq!(registry.effective[0].source_kind, SourceKind::AgentsProject);
    }

    #[test]
    fn discover_opencode_project_skills() {
        let dir = TempDir::new().unwrap();
        let skills_dir = dir.path().join(".opencode").join("skills").join("oc-skill");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: oc-skill\ndescription: From opencode\n---\nBody",
        )
        .unwrap();

        let config = test_config();
        let registry = build_isolated(&config, dir.path(), &[]);
        assert_eq!(registry.effective.len(), 1);
        assert_eq!(
            registry.effective[0].source_kind,
            SourceKind::OpenCodeProject
        );
    }

    #[test]
    fn discover_claude_project_skills() {
        let dir = TempDir::new().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills").join("cl-skill");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: cl-skill\ndescription: From claude\n---\nBody",
        )
        .unwrap();

        let config = test_config();
        let registry = build_isolated(&config, dir.path(), &[]);
        assert_eq!(registry.effective.len(), 1);
        assert_eq!(registry.effective[0].source_kind, SourceKind::ClaudeProject);
    }

    #[test]
    fn discover_global_skills() {
        let dir = TempDir::new().unwrap();
        let global_root = dir.path().join("global");
        let codegg_skills = global_root.join("codegg").join("skills").join("g-skill");
        fs::create_dir_all(&codegg_skills).unwrap();
        fs::write(
            codegg_skills.join("SKILL.md"),
            "---\nname: g-skill\ndescription: Global skill\n---\nBody",
        )
        .unwrap();

        let config = test_config();
        let registry = build_isolated(&config, dir.path(), &[global_root]);
        assert_eq!(registry.effective.len(), 1);
        assert_eq!(registry.effective[0].source_kind, SourceKind::CodeGGGlobal);
    }

    #[test]
    fn already_joined_global_root_discovers_nothing() {
        // Regression guard: `AssetRegistry::build` appends `<vendor>/skills`
        // to each root, so handing it an already-joined `…/codegg/skills`
        // path yields `…/codegg/skills/codegg/skills`. Because missing
        // directories are skipped silently, every global skill is dropped.
        // Callers must pass the configuration directory instead.
        let dir = TempDir::new().unwrap();
        let global_root = dir.path().join("global");
        let codegg_skills = global_root.join("codegg").join("skills").join("g-skill");
        fs::create_dir_all(&codegg_skills).unwrap();
        fs::write(
            codegg_skills.join("SKILL.md"),
            "---\nname: g-skill\ndescription: Global skill\n---\nBody",
        )
        .unwrap();

        let config = test_config();
        let joined = global_root.join("codegg").join("skills");
        let registry = build_isolated(&config, dir.path(), &[joined]);
        assert!(
            registry.effective.is_empty(),
            "already-joined root must not be treated as a parent dir"
        );
    }

    #[test]
    fn precedence_project_over_global() {
        let dir = TempDir::new().unwrap();
        let global_root = dir.path().join("global");

        let project_skills = dir.path().join(".codegg").join("skills").join("shared");
        fs::create_dir_all(&project_skills).unwrap();
        fs::write(
            project_skills.join("SKILL.md"),
            "---\nname: shared\ndescription: Project version\n---\nProject body",
        )
        .unwrap();

        let global_skills = global_root.join("codegg").join("skills").join("shared");
        fs::create_dir_all(&global_skills).unwrap();
        fs::write(
            global_skills.join("SKILL.md"),
            "---\nname: shared\ndescription: Global version\n---\nGlobal body",
        )
        .unwrap();

        let config = test_config();
        let registry = build_isolated(&config, dir.path(), &[global_root]);
        assert_eq!(registry.effective.len(), 1);
        assert_eq!(registry.effective[0].description, "Project version");
        assert_eq!(registry.effective[0].source_kind, SourceKind::CodeGGProject);
        assert_eq!(registry.effective[0].shadowed_alternatives.len(), 1);
    }

    #[test]
    fn native_compat_direct_md() {
        let dir = TempDir::new().unwrap();
        let skills_dir = dir.path().join(".codegg").join("skills");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(
            skills_dir.join("native.md"),
            "---\nname: native-skill\ndescription: Native direct md\n---\nBody",
        )
        .unwrap();

        let config = test_config();
        let registry = build_isolated(&config, dir.path(), &[]);
        assert_eq!(registry.effective.len(), 1);
        assert_eq!(
            registry.effective[0].source_kind,
            SourceKind::CodeGGNativeCompat
        );
    }

    #[test]
    fn invalid_higher_precedence_falls_back() {
        let dir = TempDir::new().unwrap();
        let global_root = dir.path().join("global");

        let project_skills = dir.path().join(".codegg").join("skills").join("fallback");
        fs::create_dir_all(&project_skills).unwrap();
        fs::write(
            project_skills.join("SKILL.md"),
            "---\nname: [{invalid yaml\ndescription: bad\n---\nBody",
        )
        .unwrap();

        let global_skills = global_root.join("codegg").join("skills").join("fallback");
        fs::create_dir_all(&global_skills).unwrap();
        fs::write(
            global_skills.join("SKILL.md"),
            "---\nname: fallback\ndescription: Valid global\n---\nBody",
        )
        .unwrap();

        let config = test_config();
        let registry = build_isolated(&config, dir.path(), &[global_root]);
        assert_eq!(registry.effective.len(), 1);
        assert_eq!(registry.effective[0].description, "Valid global");
        assert_eq!(registry.effective[0].source_kind, SourceKind::CodeGGGlobal);
    }

    #[test]
    fn disabled_source_not_discovered() {
        let dir = TempDir::new().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills").join("test");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: test\ndescription: test\n---\nBody",
        )
        .unwrap();

        let mut config = test_config();
        config.enabled_sources.remove(&SourceKind::AgentsProject);
        let registry = build_isolated(&config, dir.path(), &[]);
        assert!(registry.effective.is_empty());
    }

    #[test]
    fn get_returns_effective_skill() {
        let dir = TempDir::new().unwrap();
        let skills_dir = dir.path().join(".codegg").join("skills").join("lookup");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: lookup\ndescription: Lookup skill\n---\nBody",
        )
        .unwrap();

        let config = test_config();
        let registry = build_isolated(&config, dir.path(), &[]);
        assert!(registry.get("lookup").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn build_system_prompt_non_empty() {
        let dir = TempDir::new().unwrap();
        let skills_dir = dir.path().join(".codegg").join("skills").join("prompt");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: prompt\ndescription: Prompt skill\n---\nBody",
        )
        .unwrap();

        let config = test_config();
        let registry = build_isolated(&config, dir.path(), &[]);
        let prompt = registry.build_system_prompt();
        assert!(prompt.contains("prompt"));
    }

    #[test]
    fn activate_returns_body() {
        let dir = TempDir::new().unwrap();
        let skills_dir = dir.path().join(".codegg").join("skills").join("act");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: act\ndescription: Act skill\n---\nBody content here",
        )
        .unwrap();

        let config = test_config();
        let registry = build_isolated(&config, dir.path(), &[]);
        let body = registry.activate("act").unwrap();
        assert!(body.contains("Body content here"));
    }

    #[test]
    fn ancestor_scopes_are_bounded_and_nearer_scopes_win() {
        let dir = TempDir::new().unwrap();
        let outside = dir.path().join("outside");
        let repo = outside.join("repo");
        let project = repo.join("packages/app");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&project).unwrap();
        let write_skill = |root: &Path, name: &str, description: &str| {
            let skill = root.join(name);
            fs::create_dir_all(&skill).unwrap();
            fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {description}\n---\nBody"),
            )
            .unwrap();
        };
        let parent_root = repo.join(".codegg/skills");
        write_skill(&parent_root, "winner", "Parent version");
        write_skill(&parent_root, "parent-only", "Inherited from repo");
        write_skill(
            &outside.join(".codegg/skills"),
            "outside",
            "Outside git root",
        );
        write_skill(
            &repo.join("packages/sibling/.agents/skills"),
            "sibling",
            "Sibling",
        );
        let child_root = project.join(".agents/skills");
        write_skill(&child_root, "winner", "Nearest version");

        let registry = build_isolated(&test_config(), &project, &[]);
        let winner = registry.get("winner").unwrap();
        assert_eq!(winner.description, "Nearest version");
        assert_eq!(winner.workspace_depth, 0);
        assert_eq!(registry.get("parent-only").unwrap().workspace_depth, 2);
        assert!(registry.get("sibling").is_none());
        assert!(registry.get("outside").is_none());
    }

    #[test]
    fn ancestor_discovery_is_disabled_without_git_root() {
        let dir = TempDir::new().unwrap();
        let parent = dir.path().join("parent");
        let project = parent.join("child");
        fs::create_dir_all(&project).unwrap();
        let skill = parent.join(".codegg/skills/inherited");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: inherited\ndescription: Parent skill\n---\nBody",
        )
        .unwrap();
        let registry = build_isolated(&test_config(), &project, &[]);
        assert!(registry.get("inherited").is_none());
    }

    #[test]
    fn worktree_gitfile_marks_the_ancestor_boundary() {
        let dir = TempDir::new().unwrap();
        let repo = dir.path().join("worktree");
        let project = repo.join("nested/project");
        fs::create_dir_all(&project).unwrap();
        fs::write(repo.join(".git"), "gitdir: /unused/worktree-metadata").unwrap();
        let skill = repo.join(".agents/skills/from-worktree");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: from-worktree\ndescription: Worktree ancestor\n---\nBody",
        )
        .unwrap();
        let registry = build_isolated(&test_config(), &project, &[]);
        assert_eq!(registry.get("from-worktree").unwrap().workspace_depth, 2);
    }
}
