//! Authoritative project/workspace/session context resolution.
//!
//! This module is the core boundary for turning already-typed domain identity
//! into executable context. A directory is accepted only as a compatibility
//! locator for an existing, uniquely resolvable binding; it never creates or
//! derives a [`ProjectId`] or [`WorkspaceId`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::identity::{IdentityParseError, ProjectId, WorkspaceId};
use crate::project_catalog::{CatalogError, ProjectCatalog};
use crate::project_storage::{
    BindingStatus, ProjectStorage, ProjectStorageError, SessionBindingRecord,
    WorkspaceBindingRecord,
};
use crate::workspace::{WorkspaceError, WorkspaceStore};

/// Maximum byte length accepted for a session identifier at this boundary.
pub const MAX_SESSION_ID_LENGTH: usize = 128;

/// Maximum byte length accepted for a compatibility directory locator.
pub const MAX_DIRECTORY_LOCATOR_LENGTH: usize = 4_096;

/// Maximum number of candidates returned for an ambiguous compatibility
/// lookup. The outcome remains ambiguous when this bound is reached.
pub const MAX_DIRECTORY_COMPATIBILITY_CANDIDATES: usize = 32;

const MAX_ERROR_MESSAGE_LENGTH: usize = 512;

/// A bounded, opaque session identifier used when validating an optional
/// canonical session binding. Session IDs are not domain identity sources.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    pub fn parse(value: &str) -> Result<Self, ContextInputError> {
        if value.is_empty() {
            return Err(ContextInputError::InvalidSessionId {
                reason: "session ID is empty",
            });
        }
        if value.len() > MAX_SESSION_ID_LENGTH {
            return Err(ContextInputError::InvalidSessionId {
                reason: "session ID exceeds the maximum length",
            });
        }
        if value.bytes().any(|byte| byte == b'\0') {
            return Err(ContextInputError::InvalidSessionId {
                reason: "session ID contains a NUL byte",
            });
        }
        if value.chars().any(char::is_control) {
            return Err(ContextInputError::InvalidSessionId {
                reason: "session ID contains a control character",
            });
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SessionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A bounded directory locator. It is deliberately not convertible to a
/// project or workspace identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct DirectoryLocator(PathBuf);

impl DirectoryLocator {
    pub fn parse(path: impl AsRef<Path>) -> Result<Self, ContextInputError> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(ContextInputError::InvalidDirectory {
                reason: "directory is empty",
            });
        }
        let length = path.to_string_lossy().len();
        if length > MAX_DIRECTORY_LOCATOR_LENGTH {
            return Err(ContextInputError::InvalidDirectory {
                reason: "directory exceeds the maximum length",
            });
        }
        if path.to_string_lossy().contains('\0') {
            return Err(ContextInputError::InvalidDirectory {
                reason: "directory contains a NUL byte",
            });
        }
        Ok(Self(path.to_path_buf()))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for DirectoryLocator {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl<'de> Deserialize<'de> for DirectoryLocator {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = PathBuf::deserialize(deserializer)?;
        Self::parse(path).map_err(serde::de::Error::custom)
    }
}

/// Explicit request for an authoritative project/workspace context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectContextRequest {
    pub project_id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub session_id: Option<SessionId>,
}

impl ProjectContextRequest {
    pub fn new(project_id: ProjectId, workspace_id: WorkspaceId) -> Self {
        Self {
            project_id,
            workspace_id,
            session_id: None,
        }
    }

    pub fn with_session_id(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Parse untrusted request fields before any storage access.
    pub fn from_raw(
        project_id: &str,
        workspace_id: &str,
        session_id: Option<&str>,
    ) -> Result<Self, ContextInputError> {
        Ok(Self {
            project_id: ProjectId::parse(project_id).map_err(ContextInputError::ProjectId)?,
            workspace_id: WorkspaceId::parse(workspace_id)
                .map_err(ContextInputError::WorkspaceId)?,
            session_id: session_id.map(SessionId::parse).transpose()?,
        })
    }
}

/// Typed input failures. The values intentionally contain bounded reasons,
/// not untrusted ID or path text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ContextInputError {
    #[error("invalid project ID: {0}")]
    ProjectId(IdentityParseError),
    #[error("invalid workspace ID: {0}")]
    WorkspaceId(IdentityParseError),
    #[error("invalid session ID: {reason}")]
    InvalidSessionId { reason: &'static str },
    #[error("invalid directory locator: {reason}")]
    InvalidDirectory { reason: &'static str },
}

/// The resolved, executable project/workspace context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectContext {
    pub project_id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub session_id: Option<SessionId>,
    pub workspace_root: PathBuf,
    pub workspace_display_name: String,
    pub project_display_name: String,
    pub repository_id: Option<crate::identity::RepositoryId>,
    pub binding_status: BindingStatus,
    pub binding_revision: i64,
}

impl ProjectContext {
    pub fn is_resolved(&self) -> bool {
        self.binding_status == BindingStatus::Resolved
    }
}

/// Existing project/workspace mapping found through a directory compatibility
/// lookup. The IDs are returned from durable records; no ID is derived from
/// [`DirectoryLocator`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryCompatibilityCandidate {
    pub project_id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub directory: DirectoryLocator,
}

/// Deterministic result of looking up an existing directory locator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DirectoryCompatibilityOutcome {
    Unique(DirectoryCompatibilityCandidate),
    None,
    Ambiguous(Vec<DirectoryCompatibilityCandidate>),
}

/// Typed failures returned while validating an explicit context.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContextResolutionError {
    #[error("invalid context input: {0}")]
    InvalidInput(#[from] ContextInputError),
    #[error("project was not found")]
    ProjectNotFound,
    #[error("project is archived")]
    ProjectArchived,
    #[error("workspace was not found")]
    WorkspaceNotFound,
    #[error("workspace is archived")]
    WorkspaceArchived,
    #[error("workspace has no project binding")]
    BindingMissing,
    #[error("workspace is bound to a different project")]
    BindingProjectMismatch,
    #[error("workspace binding is not resolved: {status:?}")]
    BindingNotResolved { status: BindingStatus },
    #[error("session binding was not found")]
    SessionBindingMissing,
    #[error("session binding is not resolved: {status:?}")]
    SessionBindingNotResolved { status: BindingStatus },
    #[error("session binding does not match the requested context")]
    SessionBindingMismatch,
    #[error("directory does not map to an existing context")]
    DirectoryNotFound,
    #[error("directory maps to multiple existing contexts")]
    DirectoryAmbiguous(Vec<DirectoryCompatibilityCandidate>),
    #[error("project catalog failure: {0}")]
    CatalogFailure(String),
    #[error("project storage failure: {0}")]
    StorageFailure(String),
    #[error("workspace store failure: {0}")]
    WorkspaceStoreFailure(String),
}

/// Reusable core authority for resolving project/workspace/session context.
///
/// The resolver owns only core domain services. It performs no authorization,
/// filesystem discovery, UI work, or server-specific translation.
pub struct ProjectContextResolver {
    project_storage: ProjectStorage,
    project_catalog: ProjectCatalog,
    workspace_store: Arc<dyn WorkspaceStore>,
}

impl ProjectContextResolver {
    pub fn new(
        project_storage: ProjectStorage,
        project_catalog: ProjectCatalog,
        workspace_store: Arc<dyn WorkspaceStore>,
    ) -> Self {
        Self {
            project_storage,
            project_catalog,
            workspace_store,
        }
    }

    /// Resolve an explicit typed request. Every returned context has an
    /// active project, an existing non-archived workspace, and a resolved
    /// project/workspace binding. A session, when supplied, must also match.
    pub async fn resolve(
        &self,
        request: ProjectContextRequest,
    ) -> Result<ProjectContext, ContextResolutionError> {
        validate_typed_request(&request)?;
        let project = match self.project_catalog.get_project(&request.project_id).await {
            Ok(project) => project,
            Err(CatalogError::NotFound(_)) => return Err(ContextResolutionError::ProjectNotFound),
            Err(error) => return Err(ContextResolutionError::CatalogFailure(bounded_error(error))),
        };
        if project.lifecycle != crate::project_storage::ProjectLifecycle::Active {
            return Err(ContextResolutionError::ProjectArchived);
        }

        let workspace = self
            .workspace_store
            .load_by_id(request.workspace_id.as_str())
            .await
            .map_err(map_workspace_error)?
            .ok_or(ContextResolutionError::WorkspaceNotFound)?;
        if workspace.is_archived() {
            return Err(ContextResolutionError::WorkspaceArchived);
        }

        let binding = self
            .project_storage
            .workspace_binding(&request.workspace_id)
            .await
            .map_err(map_storage_error)?
            .ok_or(ContextResolutionError::BindingMissing)?;
        validate_workspace_binding(&request, &binding)?;

        if let Some(session_id) = &request.session_id {
            let session = self
                .project_storage
                .session_binding(session_id.as_str())
                .await
                .map_err(map_storage_error)?
                .ok_or(ContextResolutionError::SessionBindingMissing)?;
            validate_session_binding(&request, &session)?;
        }

        Ok(ProjectContext {
            project_id: request.project_id,
            workspace_id: request.workspace_id,
            session_id: request.session_id,
            workspace_root: workspace.canonical_root,
            workspace_display_name: workspace.display_name,
            project_display_name: project.display_name,
            repository_id: binding.repository_id,
            binding_status: binding.status,
            binding_revision: binding.revision,
        })
    }

    /// Resolve raw request fields after bounded identity parsing.
    pub async fn resolve_raw(
        &self,
        project_id: &str,
        workspace_id: &str,
        session_id: Option<&str>,
    ) -> Result<ProjectContext, ContextResolutionError> {
        let request = ProjectContextRequest::from_raw(project_id, workspace_id, session_id)?;
        self.resolve(request).await
    }

    /// Look up an existing context by directory for legacy compatibility.
    ///
    /// Only existing workspace records and resolved bindings are considered.
    /// Matching candidates are sorted by typed project/workspace IDs, and no
    /// project identity is ever constructed from the directory.
    pub async fn lookup_directory(
        &self,
        directory: impl AsRef<Path>,
    ) -> Result<DirectoryCompatibilityOutcome, ContextResolutionError> {
        let directory = DirectoryLocator::parse(directory)?;
        let requested_directory = comparable_directory(directory.as_path());
        let workspaces = self
            .workspace_store
            .list(true)
            .await
            .map_err(map_workspace_error)?;

        let mut candidates = BTreeMap::new();
        for workspace in workspaces {
            if comparable_directory(workspace.canonical_root.as_path()) != requested_directory {
                continue;
            }
            let Some(binding) = self
                .project_storage
                .workspace_binding(&workspace.id)
                .await
                .map_err(map_storage_error)?
            else {
                continue;
            };
            if binding.status != BindingStatus::Resolved || workspace.is_archived() {
                continue;
            }
            let project = match self.project_catalog.get_project(&binding.project_id).await {
                Ok(project) => project,
                Err(CatalogError::NotFound(_)) => continue,
                Err(error) => {
                    return Err(ContextResolutionError::CatalogFailure(bounded_error(error)))
                }
            };
            if project.lifecycle != crate::project_storage::ProjectLifecycle::Active {
                continue;
            }
            candidates.insert(
                (binding.project_id.clone(), workspace.id.clone()),
                DirectoryCompatibilityCandidate {
                    project_id: binding.project_id,
                    workspace_id: workspace.id,
                    directory: DirectoryLocator::parse(workspace.canonical_root)?,
                },
            );
        }

        let candidates: Vec<_> = candidates
            .into_values()
            .take(MAX_DIRECTORY_COMPATIBILITY_CANDIDATES)
            .collect();
        Ok(match candidates.as_slice() {
            [] => DirectoryCompatibilityOutcome::None,
            [candidate] => DirectoryCompatibilityOutcome::Unique(candidate.clone()),
            _ => DirectoryCompatibilityOutcome::Ambiguous(candidates),
        })
    }

    /// Resolve a directory compatibility request only when it maps uniquely
    /// to an existing active context.
    pub async fn resolve_directory(
        &self,
        directory: impl AsRef<Path>,
    ) -> Result<ProjectContext, ContextResolutionError> {
        match self.lookup_directory(directory).await? {
            DirectoryCompatibilityOutcome::Unique(candidate) => {
                self.resolve(ProjectContextRequest::new(
                    candidate.project_id,
                    candidate.workspace_id,
                ))
                .await
            }
            DirectoryCompatibilityOutcome::None => Err(ContextResolutionError::DirectoryNotFound),
            DirectoryCompatibilityOutcome::Ambiguous(candidates) => {
                Err(ContextResolutionError::DirectoryAmbiguous(candidates))
            }
        }
    }
}

fn validate_workspace_binding(
    request: &ProjectContextRequest,
    binding: &WorkspaceBindingRecord,
) -> Result<(), ContextResolutionError> {
    if binding.project_id != request.project_id {
        return Err(ContextResolutionError::BindingProjectMismatch);
    }
    if binding.status != BindingStatus::Resolved {
        return Err(ContextResolutionError::BindingNotResolved {
            status: binding.status,
        });
    }
    Ok(())
}

fn validate_typed_request(request: &ProjectContextRequest) -> Result<(), ContextResolutionError> {
    ProjectId::parse(request.project_id.as_str())
        .map_err(ContextInputError::ProjectId)
        .map_err(ContextResolutionError::InvalidInput)?;
    WorkspaceId::parse(request.workspace_id.as_str())
        .map_err(ContextInputError::WorkspaceId)
        .map_err(ContextResolutionError::InvalidInput)?;
    Ok(())
}

fn validate_session_binding(
    request: &ProjectContextRequest,
    binding: &SessionBindingRecord,
) -> Result<(), ContextResolutionError> {
    if binding.status != BindingStatus::Resolved {
        return Err(ContextResolutionError::SessionBindingNotResolved {
            status: binding.status,
        });
    }
    if binding.project_id.as_ref() != Some(&request.project_id)
        || binding.workspace_id.as_ref() != Some(&request.workspace_id)
    {
        return Err(ContextResolutionError::SessionBindingMismatch);
    }
    Ok(())
}

fn comparable_directory(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn bounded_error(error: impl std::fmt::Display) -> String {
    let mut message = error.to_string();
    if message.len() > MAX_ERROR_MESSAGE_LENGTH {
        message.truncate(MAX_ERROR_MESSAGE_LENGTH);
    }
    message
}

fn map_storage_error(error: ProjectStorageError) -> ContextResolutionError {
    ContextResolutionError::StorageFailure(bounded_error(error))
}

fn map_workspace_error(error: WorkspaceError) -> ContextResolutionError {
    ContextResolutionError::WorkspaceStoreFailure(bounded_error(error))
}

/// Compatibility aliases for callers using the shorter domain terminology.
pub type ContextResolver = ProjectContextResolver;
pub type ProjectWorkspaceContextResolver = ProjectContextResolver;
pub type ContextRequest = ProjectContextRequest;
pub type ResolvedContext = ProjectContext;
pub type ProjectWorkspaceContext = ProjectContext;
pub type ContextError = ContextResolutionError;
pub type ProjectContextError = ContextResolutionError;
pub type DirectoryLookupOutcome = DirectoryCompatibilityOutcome;
