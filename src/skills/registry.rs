use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::candidate::{EffectiveSkill, ResolvedRegistry, ShadowedAlternative, SkillCandidate};
use super::diagnostic::Diagnostic;
use super::parser;
use super::resource::{ResourceError, ResourceHandle, ResourceReadLimits};
use super::source::{AssetDiscoveryConfig, SourceKind, SourceRoot, SourceSummary};

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
        Self::build_with_scoped_roots(config, &[project_root.to_path_buf()], global_roots, &[])
    }

    pub fn build_with_project_roots(
        config: &AssetDiscoveryConfig,
        project_roots: &[PathBuf],
        global_roots: &[PathBuf],
    ) -> Self {
        Self::build_with_scoped_roots(config, project_roots, global_roots, &[])
    }

    pub fn build_for_workspace_scope(
        config: &AssetDiscoveryConfig,
        workspace_root: &Path,
        global_roots: &[PathBuf],
    ) -> Self {
        let roots = workspace_skill_roots(workspace_root);
        Self::build_with_project_roots(config, &roots, global_roots)
    }

    pub fn build_with_plugin_sources(
        config: &AssetDiscoveryConfig,
        project_root: &Path,
        global_roots: &[PathBuf],
        plugin_sources: &[crate::plugin::PluginAssetPath],
    ) -> Self {
        Self::build_with_scoped_roots(
            config,
            &[project_root.to_path_buf()],
            global_roots,
            plugin_sources,
        )
    }

    pub fn build_with_project_roots_and_plugins(
        config: &AssetDiscoveryConfig,
        project_roots: &[PathBuf],
        global_roots: &[PathBuf],
        plugin_sources: &[crate::plugin::PluginAssetPath],
    ) -> Self {
        Self::build_with_scoped_roots(config, project_roots, global_roots, plugin_sources)
    }

    fn build_with_scoped_roots(
        config: &AssetDiscoveryConfig,
        project_roots: &[PathBuf],
        global_roots: &[PathBuf],
        plugin_sources: &[crate::plugin::PluginAssetPath],
    ) -> Self {
        let mut all_candidates: Vec<SkillCandidate> = Vec::new();
        let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
        let mut source_summaries: Vec<SourceSummary> = Vec::new();

        let mut source_roots = Vec::new();
        for (scope_rank, root) in project_roots.iter().enumerate() {
            for mut source_root in resolve_source_roots(config, root, &[]) {
                if source_root.kind.is_project_local() {
                    source_root.scope_rank = scope_rank as u32;
                    source_roots.push(source_root);
                }
            }
        }
        let global_start = source_roots.len();
        source_roots.extend(
            resolve_source_roots(
                config,
                project_roots
                    .first()
                    .map_or(Path::new(std::path::MAIN_SEPARATOR_STR), PathBuf::as_path),
                global_roots,
            )
            .into_iter()
            .filter(|root| !root.kind.is_project_local()),
        );
        for root in &mut source_roots[global_start..] {
            root.scope_rank = 100;
        }
        source_roots = deduplicate_source_roots(source_roots);
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
                    plugin_id: Some(source.plugin_id.clone()),
                    scope_rank: 0,
                    alias_paths: Vec::new(),
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
            let (candidates, diagnostics) = discover_in_root(source_root, config, None);
            let mut candidates = candidates;
            for candidate in &mut candidates {
                candidate.precedence_rank =
                    source_root.scope_rank * 1000 + source_root.kind.precedence_rank();
            }
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
                discovered_count: discovered,
                valid_count: valid,
                invalid_count: invalid,
                alias_paths: source_root.alias_paths.clone(),
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
                discovered_count: discovered,
                valid_count: candidates.len(),
                invalid_count: diagnostics
                    .iter()
                    .filter(|d| d.severity == super::diagnostic::Severity::Error)
                    .count(),
                alias_paths: source_root.alias_paths.clone(),
            });
            all_candidates.extend(candidates);
            all_diagnostics.extend(diagnostics);
        }

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
        prompt.push_str("The following skills are available. Activate one with the `skill` tool using `{\"name\": \"<skill-name>\"}`.\n\n");
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

pub(crate) fn workspace_skill_roots(workspace_root: &Path) -> Vec<PathBuf> {
    const MAX_ANCESTORS: usize = 16;
    let Ok(canonical) = workspace_root.canonicalize() else {
        return vec![workspace_root.to_path_buf()];
    };
    let mut roots = Vec::new();
    let mut current = Some(canonical.as_path());
    let mut depth = 0;
    while let Some(path) = current {
        roots.push(path.to_path_buf());
        let has_git_boundary = path.join(".git").exists();
        if has_git_boundary || depth + 1 >= MAX_ANCESTORS {
            break;
        }
        current = path.parent();
        depth += 1;
    }
    roots
}

fn resolve_source_roots(
    config: &AssetDiscoveryConfig,
    project_root: &Path,
    global_roots: &[PathBuf],
) -> Vec<SourceRoot> {
    let mut roots = Vec::new();

    if config.enabled_sources.contains(&SourceKind::CodeGGProject) {
        let path = project_root.join(".codegg").join("skills");
        if path.is_dir() {
            if let Ok(canonical) = path.canonicalize() {
                roots.push(SourceRoot {
                    kind: SourceKind::CodeGGProject,
                    display_path: path,
                    canonical_path: canonical,
                    plugin_id: None,
                    scope_rank: 0,
                    alias_paths: Vec::new(),
                });
            }
        }
    }

    if config.enabled_sources.contains(&SourceKind::AgentsProject) {
        let path = project_root.join(".agents").join("skills");
        if path.is_dir() {
            if let Ok(canonical) = path.canonicalize() {
                roots.push(SourceRoot {
                    kind: SourceKind::AgentsProject,
                    display_path: path,
                    canonical_path: canonical,
                    plugin_id: None,
                    scope_rank: 0,
                    alias_paths: Vec::new(),
                });
            }
        }
    }

    if config
        .enabled_sources
        .contains(&SourceKind::OpenCodeProject)
    {
        let path = project_root.join(".opencode").join("skills");
        if path.is_dir() {
            if let Ok(canonical) = path.canonicalize() {
                roots.push(SourceRoot {
                    kind: SourceKind::OpenCodeProject,
                    display_path: path,
                    canonical_path: canonical,
                    plugin_id: None,
                    scope_rank: 0,
                    alias_paths: Vec::new(),
                });
            }
        }
    }

    if config.enabled_sources.contains(&SourceKind::ClaudeProject) {
        let path = project_root.join(".claude").join("skills");
        if path.is_dir() {
            if let Ok(canonical) = path.canonicalize() {
                roots.push(SourceRoot {
                    kind: SourceKind::ClaudeProject,
                    display_path: path,
                    canonical_path: canonical,
                    plugin_id: None,
                    scope_rank: 0,
                    alias_paths: Vec::new(),
                });
            }
        }
    }

    for (kind, relative) in [
        (SourceKind::CursorProject, ".cursor/skills"),
        (SourceKind::GeminiProject, ".gemini/skills"),
        (SourceKind::CopilotProject, ".github/skills"),
        (SourceKind::CodexProject, ".codex/skills"),
    ] {
        if config.enabled_sources.contains(&kind) {
            push_source_root(&mut roots, kind, project_root.join(relative));
        }
    }

    for global_root in global_roots {
        if config.enabled_sources.contains(&SourceKind::CodeGGGlobal) {
            let path = global_root.join("codegg").join("skills");
            if path.is_dir() {
                if let Ok(canonical) = path.canonicalize() {
                    roots.push(SourceRoot {
                        kind: SourceKind::CodeGGGlobal,
                        display_path: path,
                        canonical_path: canonical,
                        plugin_id: None,
                        scope_rank: 0,
                        alias_paths: Vec::new(),
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
                        plugin_id: None,
                        scope_rank: 0,
                        alias_paths: Vec::new(),
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
                        plugin_id: None,
                        scope_rank: 0,
                        alias_paths: Vec::new(),
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
                        plugin_id: None,
                        scope_rank: 0,
                        alias_paths: Vec::new(),
                    });
                }
            }
        }
        for (kind, relative) in [
            (SourceKind::CursorGlobal, ".cursor/skills"),
            (SourceKind::GeminiGlobal, ".gemini/skills"),
            (SourceKind::CodexGlobal, ".codex/skills"),
            (SourceKind::CopilotGlobal, ".copilot/skills"),
        ] {
            if config.enabled_sources.contains(&kind) {
                push_source_root(&mut roots, kind, global_root.join(relative));
            }
        }
        // OpenCode's documented XDG location is ~/.config/opencode/skills.
        if config.enabled_sources.contains(&SourceKind::OpenCodeGlobal) {
            push_source_root(
                &mut roots,
                SourceKind::OpenCodeGlobal,
                global_root.join(".config/opencode/skills"),
            );
        }
        if config.enabled_sources.contains(&SourceKind::AgentsGlobal) {
            push_source_root(
                &mut roots,
                SourceKind::AgentsGlobal,
                global_root.join(".agents/skills"),
            );
        }
        if config.enabled_sources.contains(&SourceKind::ClaudeGlobal) {
            push_source_root(
                &mut roots,
                SourceKind::ClaudeGlobal,
                global_root.join(".claude/skills"),
            );
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
                        plugin_id: None,
                        scope_rank: 0,
                        alias_paths: Vec::new(),
                    });
                }
            }
        }
    }

    deduplicate_source_roots(roots)
}

fn deduplicate_source_roots(roots: Vec<SourceRoot>) -> Vec<SourceRoot> {
    let mut unique: Vec<SourceRoot> = Vec::new();
    let mut positions = HashMap::new();
    for root in roots {
        if let Some(index) = positions.get(&root.canonical_path).copied() {
            let existing: &mut SourceRoot = &mut unique[index];
            if existing.display_path != root.display_path {
                existing.alias_paths.push(root.display_path);
            }
            existing.alias_paths.extend(root.alias_paths);
        } else {
            positions.insert(root.canonical_path.clone(), unique.len());
            unique.push(root);
        }
    }
    unique
}

fn push_source_root(roots: &mut Vec<SourceRoot>, kind: SourceKind, path: PathBuf) {
    if !path.is_dir() {
        return;
    }
    if let Ok(canonical_path) = path.canonicalize() {
        roots.push(SourceRoot {
            kind,
            display_path: path,
            canonical_path,
            plugin_id: None,
            scope_rank: 0,
            alias_paths: Vec::new(),
        });
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
    let mut seen_skill_files = std::collections::HashSet::new();

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
                let Ok(canonical_file) = skill_file.canonicalize() else {
                    continue;
                };
                if !seen_skill_files.insert(canonical_file) {
                    continue;
                }
                match parser::parse_candidate(&skill_file, source_kind, config) {
                    Ok(mut candidate) => {
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
            let Ok(canonical_file) = path.canonicalize() else {
                continue;
            };
            if !seen_skill_files.insert(canonical_file) {
                continue;
            }
            let compat_kind = if source_kind == SourceKind::CodeGGProject {
                SourceKind::CodeGGNativeCompat
            } else {
                source_kind
            };
            match parser::parse_candidate(&path, compat_kind, config) {
                Ok(mut candidate) => {
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
        group.sort_by_key(|c| c.precedence_rank);

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
            source_path: winner.source_path.clone(),
            package_root: winner.package_root.clone(),
            content_digest: winner.content_digest.clone(),
            metadata: winner.metadata.clone(),
            resources: winner.resources.clone(),
            body: winner.body.clone(),
            precedence_rank: winner.precedence_rank,
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

        let registry = AssetRegistry::build(&config, project.path(), &[]);
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

        let registry = AssetRegistry::build(&config, project.path(), &[]);
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

        let registry = AssetRegistry::build(&config, project.path(), &[]);
        assert!(registry.effective.is_empty());
    }

    /// A non-directory entry in `skills.paths` is skipped, not trusted.
    #[test]
    fn nonexistent_configured_root_is_skipped() {
        let project = TempDir::new().unwrap();
        let mut config = test_config();
        config.configured_roots = vec![project.path().join("does-not-exist")];
        let registry = AssetRegistry::build(&config, project.path(), &[]);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[]);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[]);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[]);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[]);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[]);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[global_root]);
        assert_eq!(registry.effective.len(), 1);
        assert_eq!(registry.effective[0].source_kind, SourceKind::CodeGGGlobal);
    }

    #[test]
    fn discovers_cursor_gemini_copilot_and_home_global_roots_once() {
        let project = TempDir::new().unwrap();
        for (relative, name, kind) in [
            (
                ".cursor/skills/cursor/SKILL.md",
                "cursor",
                SourceKind::CursorProject,
            ),
            (
                ".gemini/skills/gemini/SKILL.md",
                "gemini",
                SourceKind::GeminiProject,
            ),
            (
                ".github/skills/copilot/SKILL.md",
                "copilot",
                SourceKind::CopilotProject,
            ),
            (
                ".codex/skills/codex/SKILL.md",
                "codex",
                SourceKind::CodexProject,
            ),
        ] {
            let path = project.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(
                &path,
                format!("---\nname: {name}\ndescription: fixture\n---\nBody"),
            )
            .unwrap();
            let registry = AssetRegistry::build(&test_config(), project.path(), &[]);
            assert_eq!(registry.get(name).unwrap().source_kind, kind);
        }

        let home = TempDir::new().unwrap();
        let claude = home.path().join(".claude/skills/home-skill/SKILL.md");
        fs::create_dir_all(claude.parent().unwrap()).unwrap();
        fs::write(
            &claude,
            "---\nname: home-skill\ndescription: home\n---\nBody",
        )
        .unwrap();
        let registry =
            AssetRegistry::build(&test_config(), project.path(), &[home.path().to_path_buf()]);
        assert_eq!(
            registry.get("home-skill").unwrap().source_kind,
            SourceKind::ClaudeGlobal
        );
        for (relative, name, kind) in [
            (
                ".agents/skills/codex/SKILL.md",
                "codex",
                SourceKind::AgentsGlobal,
            ),
            (
                ".config/opencode/skills/opencode/SKILL.md",
                "opencode",
                SourceKind::OpenCodeGlobal,
            ),
            (
                ".codex/skills/codex-user/SKILL.md",
                "codex-user",
                SourceKind::CodexGlobal,
            ),
            (
                ".copilot/skills/copilot-user/SKILL.md",
                "copilot-user",
                SourceKind::CopilotGlobal,
            ),
            (
                ".cursor/skills/cursor-user/SKILL.md",
                "cursor-user",
                SourceKind::CursorGlobal,
            ),
            (
                ".gemini/skills/gemini-user/SKILL.md",
                "gemini-user",
                SourceKind::GeminiGlobal,
            ),
        ] {
            let path = home.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(
                &path,
                format!("---\nname: {name}\ndescription: home\n---\nBody"),
            )
            .unwrap();
        }
        let registry =
            AssetRegistry::build(&test_config(), project.path(), &[home.path().to_path_buf()]);
        assert_eq!(
            registry.get("codex").unwrap().source_kind,
            SourceKind::AgentsGlobal
        );
        assert_eq!(
            registry.get("opencode").unwrap().source_kind,
            SourceKind::OpenCodeGlobal
        );
        assert_eq!(
            registry.get("codex-user").unwrap().source_kind,
            SourceKind::CodexGlobal
        );
        assert_eq!(
            registry.get("copilot-user").unwrap().source_kind,
            SourceKind::CopilotGlobal
        );
        assert_eq!(
            registry.get("cursor-user").unwrap().source_kind,
            SourceKind::CursorGlobal
        );
        assert_eq!(
            registry.get("gemini-user").unwrap().source_kind,
            SourceKind::GeminiGlobal
        );
    }

    #[test]
    fn nearest_scoped_project_wins_and_sibling_roots_are_not_loaded() {
        let repo = TempDir::new().unwrap();
        let nested = repo.path().join("packages/app");
        fs::create_dir_all(nested.join(".claude/skills/shared")).unwrap();
        fs::create_dir_all(repo.path().join(".codegg/skills/shared")).unwrap();
        fs::create_dir_all(repo.path().join("packages/other/.codegg/skills/sibling")).unwrap();
        fs::write(
            nested.join(".claude/skills/shared/SKILL.md"),
            "---\nname: shared\ndescription: nested\n---\nNested",
        )
        .unwrap();
        fs::write(
            repo.path().join(".codegg/skills/shared/SKILL.md"),
            "---\nname: shared\ndescription: parent\n---\nParent",
        )
        .unwrap();
        fs::write(
            repo.path()
                .join("packages/other/.codegg/skills/sibling/SKILL.md"),
            "---\nname: sibling\ndescription: sibling\n---\nSibling",
        )
        .unwrap();
        let registry = AssetRegistry::build_with_project_roots(
            &test_config(),
            &[nested.clone(), repo.path().to_path_buf()],
            &[],
        );
        assert_eq!(registry.get("shared").unwrap().body.trim(), "Nested");
        assert!(registry.get("sibling").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn physical_skill_file_alias_is_parsed_once() {
        let project = TempDir::new().unwrap();
        let skills = project.path().join(".codegg/skills");
        let package = skills.join("original");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("SKILL.md"),
            "---\nname: original\ndescription: once\n---\nBody",
        )
        .unwrap();
        std::os::unix::fs::symlink(package.join("SKILL.md"), skills.join("z-alias.md")).unwrap();
        let registry = AssetRegistry::build(&test_config(), project.path(), &[]);
        let skill = registry.get("original").unwrap();
        assert_eq!(skill.source_kind, SourceKind::CodeGGProject);
        assert!(skill.shadowed_alternatives.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn canonical_source_alias_is_reported_without_duplicate_shadow() {
        let project = TempDir::new().unwrap();
        let opencode = project.path().join(".opencode");
        let package = opencode.join("skills/shared");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("SKILL.md"),
            "---\nname: shared\ndescription: one physical source\n---\nBody",
        )
        .unwrap();
        std::os::unix::fs::symlink(&opencode, project.path().join(".agents")).unwrap();
        let registry = AssetRegistry::build(&test_config(), project.path(), &[]);
        assert!(registry
            .get("shared")
            .unwrap()
            .shadowed_alternatives
            .is_empty());
        assert!(registry.sources.iter().any(|source| {
            source.kind == SourceKind::AgentsProject && source.alias_paths.len() == 1
        }));
    }

    #[test]
    fn nested_resources_open_through_the_contained_handle() {
        let project = TempDir::new().unwrap();
        let package = project.path().join(".agents/skills/nested");
        fs::create_dir_all(package.join("references")).unwrap();
        fs::write(
            package.join("SKILL.md"),
            "---\nname: nested\ndescription: nested resource\n---\nBody",
        )
        .unwrap();
        fs::write(package.join("references/API.md"), "bounded reference").unwrap();
        let registry = AssetRegistry::build(&test_config(), project.path(), &[]);
        let resource = registry
            .get("nested")
            .unwrap()
            .resource_handle("references/API.md", ResourceReadLimits::default())
            .unwrap();
        assert_eq!(resource.read_text().unwrap(), "bounded reference");
    }

    #[test]
    fn workspace_scope_stops_at_git_boundary_and_is_bounded_without_git() {
        let repo = TempDir::new().unwrap();
        let selected = repo.path().join("a/b/c");
        fs::create_dir_all(&selected).unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        let roots = workspace_skill_roots(&selected);
        assert_eq!(roots.last().unwrap(), repo.path());
        assert_eq!(roots.len(), 4);

        let no_git = TempDir::new().unwrap();
        let deep = no_git
            .path()
            .join("0/1/2/3/4/5/6/7/8/9/10/11/12/13/14/15/16");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(workspace_skill_roots(&deep).len(), 16);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[joined]);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[global_root]);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[]);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[global_root]);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[]);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[]);
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
        let registry = AssetRegistry::build(&config, dir.path(), &[]);
        let prompt = registry.build_system_prompt();
        assert!(prompt.contains("prompt"));
        assert!(prompt.contains("`skill` tool"));
        assert!(prompt.contains("{\"name\": \"<skill-name>\"}"));
        assert!(!prompt.contains("/skill:"));
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
        let registry = AssetRegistry::build(&config, dir.path(), &[]);
        let body = registry.activate("act").unwrap();
        assert!(body.contains("Body content here"));
    }
}
