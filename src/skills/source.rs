use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SourceKind {
    CodeGGProject = 0,
    AgentsProject = 10,
    OpenCodeProject = 20,
    ClaudeProject = 30,
    CodeGGGlobal = 40,
    AgentsGlobal = 50,
    OpenCodeGlobal = 60,
    ClaudeGlobal = 70,
    CodeGGNativeCompat = 80,
}

impl SourceKind {
    pub fn precedence_rank(self) -> u32 {
        self as u32
    }

    pub fn is_project_local(self) -> bool {
        matches!(
            self,
            SourceKind::CodeGGProject
                | SourceKind::AgentsProject
                | SourceKind::OpenCodeProject
                | SourceKind::ClaudeProject
                | SourceKind::CodeGGNativeCompat
        )
    }

    pub fn is_global(self) -> bool {
        matches!(
            self,
            SourceKind::CodeGGGlobal
                | SourceKind::AgentsGlobal
                | SourceKind::OpenCodeGlobal
                | SourceKind::ClaudeGlobal
        )
    }

    pub fn directory_name(self) -> &'static str {
        match self {
            SourceKind::CodeGGProject
            | SourceKind::CodeGGGlobal
            | SourceKind::CodeGGNativeCompat => "codegg",
            SourceKind::AgentsProject | SourceKind::AgentsGlobal => "agents",
            SourceKind::OpenCodeProject | SourceKind::OpenCodeGlobal => "opencode",
            SourceKind::ClaudeProject | SourceKind::ClaudeGlobal => "claude",
        }
    }

    pub fn is_foreign(self) -> bool {
        matches!(
            self,
            SourceKind::AgentsProject
                | SourceKind::AgentsGlobal
                | SourceKind::OpenCodeProject
                | SourceKind::OpenCodeGlobal
                | SourceKind::ClaudeProject
                | SourceKind::ClaudeGlobal
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRoot {
    pub kind: SourceKind,
    pub canonical_path: PathBuf,
    pub display_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSummary {
    pub kind: SourceKind,
    pub canonical_path: PathBuf,
    pub discovered_count: usize,
    pub valid_count: usize,
    pub invalid_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetDiscoveryConfig {
    pub max_skill_file_size: u64,
    pub max_frontmatter_size: usize,
    pub max_skills_per_root: usize,
    pub max_resources_per_skill: usize,
    pub max_skill_name_length: usize,
    pub max_description_length: usize,
    pub enabled_sources: HashSet<SourceKind>,
}

impl Default for AssetDiscoveryConfig {
    fn default() -> Self {
        let mut enabled_sources = HashSet::new();
        enabled_sources.insert(SourceKind::CodeGGProject);
        enabled_sources.insert(SourceKind::AgentsProject);
        enabled_sources.insert(SourceKind::OpenCodeProject);
        enabled_sources.insert(SourceKind::ClaudeProject);
        enabled_sources.insert(SourceKind::CodeGGGlobal);
        enabled_sources.insert(SourceKind::AgentsGlobal);
        enabled_sources.insert(SourceKind::OpenCodeGlobal);
        enabled_sources.insert(SourceKind::ClaudeGlobal);
        enabled_sources.insert(SourceKind::CodeGGNativeCompat);

        Self {
            max_skill_file_size: 256 * 1024,
            max_frontmatter_size: 64 * 1024,
            max_skills_per_root: 256,
            max_resources_per_skill: 64,
            max_skill_name_length: 128,
            max_description_length: 2048,
            enabled_sources,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_kind_precedence_order() {
        let kinds = [
            SourceKind::CodeGGProject,
            SourceKind::AgentsProject,
            SourceKind::OpenCodeProject,
            SourceKind::ClaudeProject,
            SourceKind::CodeGGGlobal,
            SourceKind::AgentsGlobal,
            SourceKind::OpenCodeGlobal,
            SourceKind::ClaudeGlobal,
            SourceKind::CodeGGNativeCompat,
        ];
        for window in kinds.windows(2) {
            assert!(
                window[0].precedence_rank() < window[1].precedence_rank(),
                "Expected {:?} ({}) < {:?} ({})",
                window[0],
                window[0].precedence_rank(),
                window[1],
                window[1].precedence_rank()
            );
        }
    }

    #[test]
    fn project_vs_global_classification() {
        assert!(SourceKind::CodeGGProject.is_project_local());
        assert!(SourceKind::CodeGGNativeCompat.is_project_local());
        assert!(!SourceKind::CodeGGGlobal.is_project_local());
        assert!(SourceKind::CodeGGGlobal.is_global());
        assert!(!SourceKind::AgentsProject.is_global());
        assert!(SourceKind::AgentsProject.is_project_local());
    }

    #[test]
    fn foreign_harness_classification() {
        assert!(!SourceKind::CodeGGProject.is_foreign());
        assert!(!SourceKind::CodeGGNativeCompat.is_foreign());
        assert!(SourceKind::AgentsProject.is_foreign());
        assert!(SourceKind::OpenCodeGlobal.is_foreign());
        assert!(SourceKind::ClaudeGlobal.is_foreign());
    }

    #[test]
    fn directory_name_mapping() {
        assert_eq!(SourceKind::CodeGGProject.directory_name(), "codegg");
        assert_eq!(SourceKind::AgentsGlobal.directory_name(), "agents");
        assert_eq!(SourceKind::OpenCodeProject.directory_name(), "opencode");
        assert_eq!(SourceKind::ClaudeGlobal.directory_name(), "claude");
    }
}
