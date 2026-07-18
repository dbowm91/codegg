use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::candidate::{EffectiveSkill, ResolvedRegistry, ShadowedAlternative, SkillCandidate};
use super::diagnostic::Diagnostic;
use super::parser;
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
        let mut all_candidates: Vec<SkillCandidate> = Vec::new();
        let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
        let mut source_summaries: Vec<SourceSummary> = Vec::new();

        let source_roots = resolve_source_roots(config, project_root, global_roots);

        for source_root in &source_roots {
            let (candidates, diagnostics) = discover_in_root(source_root, config);
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
        prompt.push_str(
            "The following skills are available. Use /skill:<name> to activate a specific skill.\n\n",
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
                });
            }
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
                    });
                }
            }
        }
    }

    roots
}

fn discover_in_root(
    source_root: &SourceRoot,
    config: &AssetDiscoveryConfig,
) -> (Vec<SkillCandidate>, Vec<Diagnostic>) {
    let mut candidates = Vec::new();
    let mut diagnostics = Vec::new();
    let mut skill_count = 0;

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
                match parser::parse_candidate(&skill_file, source_kind, config) {
                    Ok(candidate) => {
                        candidates.push(candidate);
                        skill_count += 1;
                    }
                    Err(diag) => {
                        diagnostics.push(diag);
                    }
                }
            }
        } else if (source_kind == SourceKind::CodeGGNativeCompat
            || source_kind == SourceKind::CodeGGProject)
            && path.extension().and_then(|e| e.to_str()) == Some("md")
        {
            match validate_symlink_boundary(&path, &source_root.canonical_path) {
                Ok(()) => {}
                Err(diag) => {
                    diagnostics.push(diag);
                    continue;
                }
            }
            let compat_kind = if source_kind == SourceKind::CodeGGProject {
                SourceKind::CodeGGNativeCompat
            } else {
                source_kind
            };
            match parser::parse_candidate(&path, compat_kind, config) {
                Ok(candidate) => {
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
        group.sort_by_key(|c| c.source_kind.precedence_rank());

        let mut valid_candidates: Vec<_> = group
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

        let winner = valid_candidates.remove(0);
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
