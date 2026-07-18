use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::diagnostic::Diagnostic;
use super::resource::{ResourceError, ResourceHandle, ResourceReadLimits};
use super::source::SourceKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDescriptor {
    pub name: String,
    pub relative_path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCandidate {
    pub name: String,
    pub normalized_name: String,
    pub description: String,
    pub source_kind: SourceKind,
    pub source_path: PathBuf,
    pub package_root: PathBuf,
    pub content_digest: String,
    pub frontmatter_raw: String,
    pub body: String,
    pub metadata: HashMap<String, serde_yaml::Value>,
    pub resources: Vec<ResourceDescriptor>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveSkill {
    pub name: String,
    pub normalized_name: String,
    pub description: String,
    pub source_kind: SourceKind,
    pub source_path: PathBuf,
    pub package_root: PathBuf,
    pub content_digest: String,
    pub metadata: HashMap<String, serde_yaml::Value>,
    pub resources: Vec<ResourceDescriptor>,
    pub body: String,
    pub precedence_rank: u32,
    pub shadowed_alternatives: Vec<ShadowedAlternative>,
}

impl EffectiveSkill {
    /// Open one of the resources inventoried during metadata-only discovery.
    /// The resource body remains unread until `ResourceHandle::read_*` is
    /// called.
    pub fn resource_handle(
        &self,
        relative_path: impl AsRef<std::path::Path>,
        limits: ResourceReadLimits,
    ) -> Result<ResourceHandle, ResourceError> {
        let relative_path = relative_path.as_ref();
        ResourceHandle::validate_relative_path(relative_path)?;
        if !self
            .resources
            .iter()
            .any(|resource| std::path::Path::new(&resource.relative_path) == relative_path)
        {
            return Err(ResourceError::NotFound {
                skill: self.name.clone(),
                path: relative_path.to_path_buf(),
            });
        }
        ResourceHandle::new(&self.package_root, relative_path, limits)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowedAlternative {
    pub source_kind: SourceKind,
    pub source_path: PathBuf,
    pub content_digest: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRegistry {
    pub effective: Vec<EffectiveSkill>,
    pub diagnostics: Vec<Diagnostic>,
    pub sources: Vec<super::source::SourceSummary>,
}
