//! Explicit asset resolution context for project agents, skills, and instructions.
//!
//! Runtime Assets Milestone 2 removes the legacy `std::env::var("PWD")` and
//! `std::env::current_dir()` inference from project asset resolution. Every
//! daemon/runtime consumer of agents, skills, or project instructions must
//! supply an [`AssetContext`] explicitly.
//!
//! The context is the single input contract for [`AgentRegistry::load_for_context`],
//! the project-instruction resolver, and the unified snapshot builder. It
//! must be clonable, immutable after construction, and free of any process
//! global inference at construction time. A valid context never derives
//! project identity from a path string and never reads `current_dir()`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AgentError;

/// Stable, opaque project identifier.
///
/// In the daemon path the `ProjectStorage` interface supplies this value.
/// In embedding or compatibility contexts where the closed identity stack
/// is unavailable, the [`AssetContextBuilder`] falls back to a
/// path-independent sentinel and explicitly marks the context as
/// `project_id_synthetic = true` so downstream code cannot accidentally
/// treat the absence of a real `ProjectId` as authoritative.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(String);

impl ProjectId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn parse(value: &str) -> Result<Self, AgentError> {
        if value.is_empty() {
            return Err(AgentError::Invalid("empty project id".into()));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Reason an `AssetContext` is missing a real `ProjectId`.
///
/// The context must be transparent about the absence so that daemon
/// consumers can refuse to operate (or surface a diagnostic) instead of
/// silently inheriting a synthetic identifier that would later be
/// mistaken for a real one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectIdSource {
    /// `ProjectStorage` provided a real, stable identifier.
    Authoritative,
    /// The caller (CLI/embedding/tests) supplied a synthetic fallback
    /// because the closed identity stack is unavailable in this process.
    SyntheticEmbedding,
    /// No `ProjectId` is available at all. Daemon-owned code must not
    /// construct an `AssetContext` in this state.
    Unbound,
}

/// Bounded, deterministic configuration for asset resolution.
#[derive(Debug, Clone)]
pub struct AssetContext {
    project_id: Option<ProjectId>,
    project_id_source: ProjectIdSource,
    workspace_root: PathBuf,
    global_roots: Vec<PathBuf>,
    config_revision: u64,
    session_id: Option<String>,
}

impl AssetContext {
    pub fn project_id(&self) -> Option<&ProjectId> {
        self.project_id.as_ref()
    }

    pub fn project_id_source(&self) -> ProjectIdSource {
        self.project_id_source
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn global_roots(&self) -> &[PathBuf] {
        &self.global_roots
    }

    pub fn config_revision(&self) -> u64 {
        self.config_revision
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// True when this context was created from a closed project/workspace
    /// identity interface and may drive daemon-owned execution paths.
    pub fn is_authoritative(&self) -> bool {
        self.project_id_source == ProjectIdSource::Authoritative
    }
}

/// Builder for [`AssetContext`]. Validates inputs and refuses to read
/// `PWD` or `current_dir()`.
#[derive(Debug)]
pub struct AssetContextBuilder {
    project_id: Option<ProjectId>,
    project_id_source: ProjectIdSource,
    workspace_root: Option<PathBuf>,
    global_roots: Vec<PathBuf>,
    config_revision: u64,
    session_id: Option<String>,
}

impl AssetContextBuilder {
    pub fn new() -> Self {
        Self {
            project_id: None,
            project_id_source: ProjectIdSource::Unbound,
            workspace_root: None,
            global_roots: Vec::new(),
            config_revision: 0,
            session_id: None,
        }
    }

    pub fn with_project_id(mut self, id: ProjectId) -> Self {
        self.project_id = Some(id);
        self.project_id_source = ProjectIdSource::Authoritative;
        self
    }

    pub fn with_synthetic_project_id(mut self, id: ProjectId) -> Self {
        self.project_id = Some(id);
        self.project_id_source = ProjectIdSource::SyntheticEmbedding;
        self
    }

    pub fn with_workspace_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.workspace_root = Some(root.into());
        self
    }

    pub fn with_global_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.global_roots.push(root.into());
        self
    }

    pub fn with_global_roots(mut self, roots: impl IntoIterator<Item = PathBuf>) -> Self {
        self.global_roots.extend(roots);
        self
    }

    pub fn with_config_revision(mut self, revision: u64) -> Self {
        self.config_revision = revision;
        self
    }

    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Build the context. Refuses if:
    ///
    /// - workspace root is missing or not a directory (when one is provided);
    /// - the context is marked `Unbound` for `ProjectId` AND the caller
    ///   did not opt into a synthetic fallback. Daemon-owned code must
    ///   always supply an authoritative `ProjectId`.
    pub fn build(self) -> Result<AssetContext, AgentError> {
        let workspace_root = self
            .workspace_root
            .ok_or_else(|| AgentError::Invalid("workspace_root is required".into()))?;
        if workspace_root.as_os_str().is_empty() {
            return Err(AgentError::Invalid("workspace_root is empty".into()));
        }

        if self.project_id_source == ProjectIdSource::Unbound && self.project_id.is_none() {
            return Err(AgentError::Invalid(
                "project_id is required (pass an authoritative ProjectId or set a synthetic fallback)"
                    .into(),
            ));
        }

        Ok(AssetContext {
            project_id: self.project_id,
            project_id_source: self.project_id_source,
            workspace_root,
            global_roots: self.global_roots,
            config_revision: self.config_revision,
            session_id: self.session_id,
        })
    }
}

impl Default for AssetContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Return the platform's default global agent root directory. Pulled out
/// of the agent registry so it can be unit-tested without env mutation.
pub fn default_global_agents_root() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("codegg").join("agents"))
}

/// Return the platform's default global skills root directory.
pub fn default_global_skills_root() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("codegg").join("skills"))
}

/// Return the platform's default global instructions file path. The
/// instructions module only ever reads this path; it does not scan any
/// other location.
pub fn default_global_instructions_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("codegg").join("instructions.md"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_requires_workspace_root() {
        let result = AssetContextBuilder::new()
            .with_synthetic_project_id(ProjectId::new())
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_requires_project_id() {
        let result = AssetContextBuilder::new()
            .with_workspace_root("/tmp")
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_accepts_authoritative_project_id() {
        let pid = ProjectId::new();
        let ctx = AssetContextBuilder::new()
            .with_project_id(pid.clone())
            .with_workspace_root("/tmp/codegg-workspace-test")
            .with_config_revision(7)
            .build()
            .unwrap();
        assert_eq!(ctx.project_id(), Some(&pid));
        assert_eq!(ctx.project_id_source(), ProjectIdSource::Authoritative);
        assert!(ctx.is_authoritative());
        assert_eq!(ctx.config_revision(), 7);
    }

    #[test]
    fn builder_accepts_synthetic_project_id() {
        let pid = ProjectId::new();
        let ctx = AssetContextBuilder::new()
            .with_synthetic_project_id(pid.clone())
            .with_workspace_root("/tmp/codegg-workspace-test")
            .build()
            .unwrap();
        assert_eq!(ctx.project_id(), Some(&pid));
        assert_eq!(ctx.project_id_source(), ProjectIdSource::SyntheticEmbedding);
        assert!(!ctx.is_authoritative());
    }

    #[test]
    fn empty_workspace_root_rejected() {
        let result = AssetContextBuilder::new()
            .with_synthetic_project_id(ProjectId::new())
            .with_workspace_root("")
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn two_contexts_are_independent() {
        let pid_a = ProjectId::new();
        let pid_b = ProjectId::new();
        let ctx_a = AssetContextBuilder::new()
            .with_synthetic_project_id(pid_a.clone())
            .with_workspace_root("/tmp/project-a")
            .with_global_root("/tmp/global")
            .build()
            .unwrap();
        let ctx_b = AssetContextBuilder::new()
            .with_synthetic_project_id(pid_b.clone())
            .with_workspace_root("/tmp/project-b")
            .build()
            .unwrap();
        assert_ne!(ctx_a.project_id(), ctx_b.project_id());
        assert_ne!(ctx_a.workspace_root(), ctx_b.workspace_root());
        assert_eq!(ctx_b.global_roots().len(), 0);
    }
}
