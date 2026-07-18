//! Bounded, metadata-only discovery of local project candidates.
//!
//! This module is intentionally a coordinator/persistence-neutral slice.  It
//! only observes explicitly supplied local roots and returns bounded facts and
//! reconciliation decisions.  It does not register workspaces, create
//! projects, write below candidate roots, or activate any other subsystem.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::identity::{ProjectId, WorkspaceId};
use crate::repository_lineage::{
    inspect_repository_lineage, RepositoryLineageError, RepositoryLineageEvidence,
};

/// Bound for identifiers and diagnostic labels supplied to this module.
pub const MAX_DISCOVERY_TEXT_BYTES: usize = 128;
/// Bound for one retained diagnostic or path rendering.
pub const MAX_DISCOVERY_DIAGNOSTIC_BYTES: usize = 512;

/// How a root classifies directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMode {
    /// Discover directories with a `.git` marker.
    Git,
    /// Discover only directories selected by [`DiscoveryPolicy`].
    Directory,
    /// Discover Git repositories and explicitly marked directories.
    Mixed,
}

/// Policy for the conservative directory mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryPolicy {
    /// Directory names skipped before metadata inspection.
    pub ignore_names: Vec<String>,
    /// Marker names whose presence makes a directory a candidate.
    pub directory_markers: Vec<String>,
    /// If enabled, every direct child directory is a directory candidate.
    pub direct_child_only: bool,
    /// Whether hidden entries are considered. Symlinks are never followed.
    pub include_hidden: bool,
}

impl Default for DiscoveryPolicy {
    fn default() -> Self {
        Self {
            ignore_names: [
                ".git",
                ".codegg",
                ".cache",
                ".venv",
                "build",
                "dist",
                "node_modules",
                "target",
                "vendor",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            directory_markers: [
                "Cargo.toml",
                "go.mod",
                "Makefile",
                "package.json",
                "pom.xml",
                "pyproject.toml",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            direct_child_only: false,
            include_hidden: false,
        }
    }
}

impl DiscoveryPolicy {
    fn validate(&self) -> Result<(), DiscoveryError> {
        validate_name_list(&self.ignore_names, "ignore name")?;
        validate_name_list(&self.directory_markers, "directory marker")?;
        Ok(())
    }

    fn is_ignored(&self, name: &str) -> bool {
        self.ignore_names.iter().any(|ignored| ignored == name)
    }
}

/// An explicitly configured local discovery root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryRoot {
    /// Stable caller-owned identity. It is never derived from the path.
    pub id: String,
    /// The configured local path. The scanner canonicalizes it before use.
    pub path: PathBuf,
    pub mode: DiscoveryMode,
    pub policy: DiscoveryPolicy,
}

impl DiscoveryRoot {
    /// Construct and validate one explicit local root.
    pub fn new(
        id: impl Into<String>,
        path: impl Into<PathBuf>,
        mode: DiscoveryMode,
        policy: DiscoveryPolicy,
    ) -> Result<Self, DiscoveryError> {
        let root = Self {
            id: id.into(),
            path: path.into(),
            mode,
            policy,
        };
        root.validate()?;
        Ok(root)
    }

    pub fn validate(&self) -> Result<(), DiscoveryError> {
        validate_text(&self.id, "discovery root id")?;
        if self.path.as_os_str().is_empty() {
            return Err(DiscoveryError::InvalidConfiguration(
                "discovery root path is empty".to_owned(),
            ));
        }
        if self.path.as_os_str().to_string_lossy().contains('\0') {
            return Err(DiscoveryError::InvalidConfiguration(
                "discovery root path contains NUL".to_owned(),
            ));
        }
        self.policy.validate()
    }
}

/// Explicit finite work limits for one scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanLimits {
    /// Maximum relative directory depth. The root is depth zero.
    pub max_depth: usize,
    /// Maximum number of entries whose metadata is inspected, including root.
    pub max_entries: usize,
    /// Maximum number of candidates retained in the report.
    pub max_candidates: usize,
    /// Maximum wall-clock time spent traversing and probing.
    pub max_elapsed: Duration,
    /// Maximum retained report bytes (paths, evidence labels, and diagnostics).
    pub max_output_bytes: usize,
    /// Maximum number of retained diagnostics.
    pub max_diagnostics: usize,
    /// Maximum metadata/stat concurrency permitted by the caller. The current
    /// deterministic scanner performs work serially, which is stricter than
    /// this bound; the field is persisted for a future parallel backend.
    pub stat_concurrency: usize,
    /// Maximum concurrent Git probes permitted by the caller. The current
    /// scanner performs probes serially, which is stricter than this bound.
    pub git_probe_concurrency: usize,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_depth: 4,
            max_entries: 10_000,
            max_candidates: 1_000,
            max_elapsed: Duration::from_secs(5),
            max_output_bytes: 256 * 1024,
            max_diagnostics: 128,
            stat_concurrency: 4,
            git_probe_concurrency: 2,
        }
    }
}

impl ScanLimits {
    pub fn validate(&self) -> Result<(), DiscoveryError> {
        if self.max_entries == 0 {
            return Err(DiscoveryError::InvalidConfiguration(
                "max_entries must be greater than zero".to_owned(),
            ));
        }
        if self.max_output_bytes == 0 {
            return Err(DiscoveryError::InvalidConfiguration(
                "max_output_bytes must be greater than zero".to_owned(),
            ));
        }
        if self.max_diagnostics == 0 {
            return Err(DiscoveryError::InvalidConfiguration(
                "max_diagnostics must be greater than zero".to_owned(),
            ));
        }
        if self.stat_concurrency == 0 || self.stat_concurrency > 256 {
            return Err(DiscoveryError::InvalidConfiguration(
                "stat_concurrency must be in 1..=256".to_owned(),
            ));
        }
        if self.git_probe_concurrency == 0 || self.git_probe_concurrency > 64 {
            return Err(DiscoveryError::InvalidConfiguration(
                "git_probe_concurrency must be in 1..=64".to_owned(),
            ));
        }
        Ok(())
    }
}

/// A candidate's conservative classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    GitRepository,
    Directory,
}

/// Bounded, non-secret evidence attached to a candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Evidence {
    GitLineage { lineage: RepositoryLineageEvidence },
    DirectoryMarker { name: String },
    DirectChild,
}

/// One metadata-only candidate returned by a scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryCandidate {
    pub source_root_id: String,
    /// The path used to observe the entry, retained only for alias decisions.
    pub observed_path: PathBuf,
    /// Canonical path after containment checks.
    pub canonical_path: PathBuf,
    pub relative_path: PathBuf,
    pub depth: usize,
    pub kind: CandidateKind,
    pub evidence: Vec<Evidence>,
    pub lineage: Option<RepositoryLineageEvidence>,
    /// Optional local repository fact supplied by a repository-aware layer.
    /// The metadata-only scanner leaves this unset.
    pub repository_fingerprint: Option<String>,
}

impl DiscoveryCandidate {
    fn output_size(&self) -> usize {
        self.observed_path.as_os_str().len()
            + self.canonical_path.as_os_str().len()
            + self.relative_path.as_os_str().len()
            + self
                .evidence
                .iter()
                .map(|evidence| format!("{evidence:?}").len())
                .sum::<usize>()
    }

    /// A unique lineage key is safe to use as reconciliation evidence.
    pub fn lineage_key(&self) -> Option<String> {
        self.lineage
            .as_ref()
            .and_then(RepositoryLineageEvidence::equality_key)
    }
}

/// Lifecycle of a scan operation/report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Queued,
    Running,
    Completed,
    Cancelled,
    Failed,
    Truncated,
    Unavailable,
}

/// Bounded scan output. No item in this report authorizes activation or writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub root_id: String,
    pub canonical_root: Option<PathBuf>,
    pub status: ScanStatus,
    pub visited_entries: usize,
    pub ignored_entries: usize,
    pub inaccessible_entries: usize,
    pub candidates: Vec<DiscoveryCandidate>,
    pub diagnostics: Vec<String>,
    pub duration_ms: u128,
    pub output_bytes: usize,
}

/// Descriptive alias for persistence/protocol layers.
pub type DiscoveryReport = Report;

impl Report {
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    pub fn is_truncated(&self) -> bool {
        self.status == ScanStatus::Truncated
    }
}

/// Status of an observation relative to a completed generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    Present,
    Moved,
    Missing,
    Ambiguous,
    Inaccessible,
    Ignored,
    Stale,
}

/// A persistence-ready observation; this module does not persist it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub generation: u64,
    pub candidate: Option<DiscoveryCandidate>,
    pub status: ObservationStatus,
    pub outcome: Option<ReconciliationOutcome>,
    pub diagnostics: Vec<String>,
}

/// Compatibility spelling for callers that use the longer domain name.
pub type DiscoveryObservation = Observation;

/// Existing durable identity facts supplied by a later catalog/storage layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownProject {
    pub project_id: ProjectId,
    pub workspace_id: Option<WorkspaceId>,
    pub canonical_root: PathBuf,
    pub lineage_key: Option<String>,
    /// Optional local fact supplied by a repository-aware persistence layer.
    /// Different values prevent a remote-only match from merging a fork.
    pub repository_fingerprint: Option<String>,
}

/// Typed, conservative result of the pure reconciliation decision engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ReconciliationOutcome {
    ExactLocator {
        project_id: ProjectId,
        workspace_id: Option<WorkspaceId>,
    },
    CanonicalAlias {
        project_id: ProjectId,
        workspace_id: Option<WorkspaceId>,
    },
    UniqueLineage {
        project_id: ProjectId,
        workspace_id: Option<WorkspaceId>,
    },
    ExplicitAssociation {
        project_id: ProjectId,
    },
    CreatedProject {
        project_id: ProjectId,
        workspace_id: Option<WorkspaceId>,
    },
    NewCandidate,
    AmbiguousLineage,
    ForkConflict {
        project_ids: Vec<ProjectId>,
    },
    PlainDirectoryUnresolved,
}

impl ReconciliationOutcome {
    pub fn project_id(&self) -> Option<&ProjectId> {
        match self {
            Self::ExactLocator { project_id, .. }
            | Self::CanonicalAlias { project_id, .. }
            | Self::UniqueLineage { project_id, .. }
            | Self::ExplicitAssociation { project_id }
            | Self::CreatedProject { project_id, .. } => Some(project_id),
            Self::NewCandidate
            | Self::AmbiguousLineage
            | Self::ForkConflict { .. }
            | Self::PlainDirectoryUnresolved => None,
        }
    }
}

/// Errors in explicit root/limit configuration. Filesystem failures are
/// represented as bounded report diagnostics so one inaccessible child does
/// not discard the rest of a safe scan.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DiscoveryError {
    #[error("invalid discovery configuration: {0}")]
    InvalidConfiguration(String),
}

/// Reusable scanner configuration. A scanner is read-only and synchronous so
/// callers can choose their own coordinator/runtime boundary later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scanner {
    limits: ScanLimits,
}

/// Descriptive alias for callers that prefer the domain-qualified name.
pub type DiscoveryScanner = Scanner;

impl Scanner {
    pub fn new(limits: ScanLimits) -> Result<Self, DiscoveryError> {
        limits.validate()?;
        Ok(Self { limits })
    }

    pub fn limits(&self) -> &ScanLimits {
        &self.limits
    }

    pub fn scan(&self, root: &DiscoveryRoot) -> Report {
        self.scan_with_cancellation(root, &CancellationToken::new())
    }

    pub fn scan_with_cancellation(
        &self,
        root: &DiscoveryRoot,
        cancellation: &CancellationToken,
    ) -> Report {
        let started = Instant::now();
        let mut report = Report {
            root_id: root.id.clone(),
            canonical_root: None,
            status: ScanStatus::Running,
            visited_entries: 0,
            ignored_entries: 0,
            inaccessible_entries: 0,
            candidates: Vec::new(),
            diagnostics: Vec::new(),
            duration_ms: 0,
            output_bytes: 0,
        };

        if let Err(error) = root.validate() {
            report.status = ScanStatus::Failed;
            add_diagnostic(&mut report, &self.limits, error.to_string());
            finish_report(&mut report, started);
            return report;
        }

        let root_metadata = match fs::symlink_metadata(&root.path) {
            Ok(metadata) if metadata.file_type().is_dir() => metadata,
            Ok(metadata) if metadata.file_type().is_symlink() => {
                report.status = ScanStatus::Unavailable;
                add_diagnostic(
                    &mut report,
                    &self.limits,
                    "discovery root is a symlink; symlink roots are not followed".to_owned(),
                );
                finish_report(&mut report, started);
                return report;
            }
            Ok(_) => {
                report.status = ScanStatus::Unavailable;
                add_diagnostic(
                    &mut report,
                    &self.limits,
                    "discovery root is not a directory".to_owned(),
                );
                finish_report(&mut report, started);
                return report;
            }
            Err(error) => {
                report.status = ScanStatus::Unavailable;
                add_diagnostic(
                    &mut report,
                    &self.limits,
                    format!("discovery root is unavailable: {error}"),
                );
                finish_report(&mut report, started);
                return report;
            }
        };
        let _ = root_metadata;

        let canonical_root = match fs::canonicalize(&root.path) {
            Ok(path) if path.is_dir() => path,
            Ok(_) => {
                report.status = ScanStatus::Unavailable;
                add_diagnostic(
                    &mut report,
                    &self.limits,
                    "canonical discovery root is not a directory".to_owned(),
                );
                finish_report(&mut report, started);
                return report;
            }
            Err(error) => {
                report.status = ScanStatus::Unavailable;
                add_diagnostic(
                    &mut report,
                    &self.limits,
                    format!("discovery root could not be canonicalized: {error}"),
                );
                finish_report(&mut report, started);
                return report;
            }
        };
        report.canonical_root = Some(canonical_root.clone());

        let mut pending = VecDeque::from([WorkItem {
            observed_path: root.path.clone(),
            canonical_path: canonical_root.clone(),
            relative_path: PathBuf::new(),
            depth: 0,
        }]);
        let mut truncated = false;
        let mut cancelled = false;

        while let Some(item) = pending.pop_front() {
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }
            if started.elapsed() >= self.limits.max_elapsed {
                truncated = true;
                add_diagnostic(
                    &mut report,
                    &self.limits,
                    "scan elapsed-time bound reached".to_owned(),
                );
                break;
            }
            if report.visited_entries >= self.limits.max_entries {
                truncated = true;
                add_diagnostic(
                    &mut report,
                    &self.limits,
                    "scan entry bound reached".to_owned(),
                );
                break;
            }
            report.visited_entries += 1;

            let is_root = item.depth == 0;
            let (candidate, is_git_repository) = match classify_directory(
                root,
                &item,
                is_root,
                cancellation,
                &mut report,
                &self.limits,
            ) {
                Some(result) => result,
                None => {
                    if cancellation.is_cancelled() {
                        cancelled = true;
                        break;
                    }
                    continue;
                }
            };

            if started.elapsed() >= self.limits.max_elapsed {
                truncated = true;
                add_diagnostic(
                    &mut report,
                    &self.limits,
                    "scan elapsed-time bound reached".to_owned(),
                );
                break;
            }

            if let Some(candidate) = candidate {
                if report.candidates.len() >= self.limits.max_candidates {
                    truncated = true;
                    add_diagnostic(
                        &mut report,
                        &self.limits,
                        "scan candidate bound reached".to_owned(),
                    );
                    break;
                }
                let candidate_size = candidate.output_size();
                if report.output_bytes.saturating_add(candidate_size) > self.limits.max_output_bytes
                {
                    truncated = true;
                    add_diagnostic(
                        &mut report,
                        &self.limits,
                        "scan output bound reached".to_owned(),
                    );
                    break;
                }
                report.output_bytes += candidate_size;
                report.candidates.push(candidate);
            }

            if is_git_repository {
                continue;
            }
            if item.depth >= self.limits.max_depth {
                continue;
            }

            let mut children = match fs::read_dir(&item.canonical_path) {
                Ok(entries) => entries.filter_map(Result::ok).collect::<Vec<_>>(),
                Err(error) => {
                    report.inaccessible_entries += 1;
                    add_diagnostic(
                        &mut report,
                        &self.limits,
                        format!(
                            "could not read {}: {error}",
                            display_path(&item.canonical_path)
                        ),
                    );
                    continue;
                }
            };
            children.sort_by(|left, right| {
                left.file_name()
                    .to_string_lossy()
                    .cmp(&right.file_name().to_string_lossy())
            });

            for entry in children {
                if cancellation.is_cancelled() {
                    cancelled = true;
                    break;
                }
                if started.elapsed() >= self.limits.max_elapsed {
                    truncated = true;
                    break;
                }
                if report.visited_entries >= self.limits.max_entries {
                    truncated = true;
                    break;
                }

                let observed_path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if (!root.policy.include_hidden && name.starts_with('.'))
                    || root.policy.is_ignored(&name)
                {
                    report.ignored_entries += 1;
                    continue;
                }

                let metadata = match fs::symlink_metadata(&observed_path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        report.inaccessible_entries += 1;
                        add_diagnostic(
                            &mut report,
                            &self.limits,
                            format!(
                                "could not inspect {}: {error}",
                                display_path(&observed_path)
                            ),
                        );
                        continue;
                    }
                };
                report.visited_entries += 1;
                if metadata.file_type().is_symlink() {
                    report.ignored_entries += 1;
                    add_diagnostic(
                        &mut report,
                        &self.limits,
                        format!(
                            "ignored symlink outside traversal policy: {}",
                            display_path(&observed_path)
                        ),
                    );
                    continue;
                }
                if !metadata.file_type().is_dir() {
                    continue;
                }

                let canonical_path = match fs::canonicalize(&observed_path) {
                    Ok(path) if path.is_dir() && is_within(&canonical_root, &path) => path,
                    Ok(path) => {
                        report.ignored_entries += 1;
                        add_diagnostic(
                            &mut report,
                            &self.limits,
                            format!(
                                "ignored path outside discovery root: {}",
                                display_path(&path)
                            ),
                        );
                        continue;
                    }
                    Err(error) => {
                        report.inaccessible_entries += 1;
                        add_diagnostic(
                            &mut report,
                            &self.limits,
                            format!(
                                "could not canonicalize {}: {error}",
                                display_path(&observed_path)
                            ),
                        );
                        continue;
                    }
                };
                let relative_path = canonical_path
                    .strip_prefix(&canonical_root)
                    .unwrap_or(Path::new(""))
                    .to_path_buf();
                pending.push_back(WorkItem {
                    observed_path,
                    canonical_path,
                    relative_path,
                    depth: item.depth + 1,
                });
            }
            if cancelled {
                break;
            }
            if truncated {
                break;
            }
        }

        report.candidates.sort_by(|left, right| {
            left.relative_path
                .to_string_lossy()
                .cmp(&right.relative_path.to_string_lossy())
        });
        report.status = if cancelled {
            ScanStatus::Cancelled
        } else if truncated {
            ScanStatus::Truncated
        } else {
            ScanStatus::Completed
        };
        finish_report(&mut report, started);
        report
    }
}

/// Convenience entry point for callers that do not need a reusable scanner.
pub fn scan(
    root: &DiscoveryRoot,
    limits: ScanLimits,
    cancellation: &CancellationToken,
) -> Result<Report, DiscoveryError> {
    Scanner::new(limits).map(|scanner| scanner.scan_with_cancellation(root, cancellation))
}

/// Apply the deterministic reconciliation order without performing writes.
/// Storage/coordinator code can use the result to call its existing authority.
pub fn reconcile_candidate(
    candidate: &DiscoveryCandidate,
    known_projects: &[KnownProject],
    explicit_project_id: Option<&ProjectId>,
) -> ReconciliationOutcome {
    let exact = known_projects
        .iter()
        .filter(|known| known.canonical_root == candidate.observed_path)
        .collect::<Vec<_>>();
    let exact_projects = distinct_projects(&exact);
    if exact_projects.len() == 1 {
        return ReconciliationOutcome::ExactLocator {
            project_id: exact_projects[0].project_id.clone(),
            workspace_id: exact_projects[0].workspace_id.clone(),
        };
    }
    if exact_projects.len() > 1 {
        return ReconciliationOutcome::AmbiguousLineage;
    }

    let aliases = known_projects
        .iter()
        .filter(|known| known.canonical_root == candidate.canonical_path)
        .collect::<Vec<_>>();
    let alias_projects = distinct_projects(&aliases);
    if alias_projects.len() == 1 {
        return ReconciliationOutcome::CanonicalAlias {
            project_id: alias_projects[0].project_id.clone(),
            workspace_id: alias_projects[0].workspace_id.clone(),
        };
    }
    if alias_projects.len() > 1 {
        return ReconciliationOutcome::AmbiguousLineage;
    }

    if let Some(lineage_key) = candidate.lineage_key() {
        let lineage_matches = known_projects
            .iter()
            .filter(|known| known.lineage_key.as_deref() == Some(lineage_key.as_str()))
            .collect::<Vec<_>>();
        let lineage_projects = distinct_projects(&lineage_matches);
        if lineage_projects.len() > 1 {
            return ReconciliationOutcome::AmbiguousLineage;
        }
        if let Some(known) = lineage_projects.first() {
            if let (Some(candidate_fingerprint), Some(known_fingerprint)) = (
                candidate.repository_fingerprint(),
                known.repository_fingerprint.as_deref(),
            ) {
                if candidate_fingerprint != known_fingerprint {
                    return ReconciliationOutcome::ForkConflict {
                        project_ids: vec![known.project_id.clone()],
                    };
                }
            }
            return ReconciliationOutcome::UniqueLineage {
                project_id: known.project_id.clone(),
                workspace_id: known.workspace_id.clone(),
            };
        }
        if let Some(project_id) = explicit_project_id {
            return ReconciliationOutcome::ExplicitAssociation {
                project_id: project_id.clone(),
            };
        }
        return ReconciliationOutcome::NewCandidate;
    }

    if let Some(project_id) = explicit_project_id {
        return ReconciliationOutcome::ExplicitAssociation {
            project_id: project_id.clone(),
        };
    }
    if candidate.kind == CandidateKind::Directory {
        ReconciliationOutcome::PlainDirectoryUnresolved
    } else {
        ReconciliationOutcome::AmbiguousLineage
    }
}

fn distinct_projects<'a>(projects: &[&'a KnownProject]) -> Vec<&'a KnownProject> {
    let mut distinct = Vec::new();
    for project in projects {
        if distinct
            .iter()
            .all(|known: &&KnownProject| known.project_id != project.project_id)
        {
            distinct.push(*project);
        }
    }
    distinct
}

impl DiscoveryCandidate {
    pub fn repository_fingerprint(&self) -> Option<&str> {
        self.repository_fingerprint.as_deref()
    }
}

#[derive(Debug, Clone)]
struct WorkItem {
    observed_path: PathBuf,
    canonical_path: PathBuf,
    relative_path: PathBuf,
    depth: usize,
}

fn classify_directory(
    root: &DiscoveryRoot,
    item: &WorkItem,
    is_root: bool,
    cancellation: &CancellationToken,
    report: &mut Report,
    limits: &ScanLimits,
) -> Option<(Option<DiscoveryCandidate>, bool)> {
    if cancellation.is_cancelled() {
        return None;
    }

    let git_marker = has_regular_git_marker(&item.canonical_path);
    if git_marker && matches!(root.mode, DiscoveryMode::Git | DiscoveryMode::Mixed) {
        match inspect_repository_lineage(&item.canonical_path) {
            Ok(lineage) if !matches!(lineage, RepositoryLineageEvidence::NotRepository) => {
                let candidate = DiscoveryCandidate {
                    source_root_id: root.id.clone(),
                    observed_path: item.observed_path.clone(),
                    canonical_path: item.canonical_path.clone(),
                    relative_path: item.relative_path.clone(),
                    depth: item.depth,
                    kind: CandidateKind::GitRepository,
                    evidence: vec![Evidence::GitLineage {
                        lineage: lineage.clone(),
                    }],
                    lineage: Some(lineage),
                    repository_fingerprint: None,
                };
                return Some((Some(candidate), true));
            }
            Ok(_) => {
                add_diagnostic(
                    report,
                    limits,
                    format!(
                        "Git marker did not identify a repository: {}",
                        display_path(&item.canonical_path)
                    ),
                );
            }
            Err(error) => add_diagnostic(
                report,
                limits,
                format!(
                    "Git probe failed for {}: {error}",
                    display_path(&item.canonical_path)
                ),
            ),
        }
    }

    if !is_root && matches!(root.mode, DiscoveryMode::Directory | DiscoveryMode::Mixed) {
        if root.policy.direct_child_only && item.depth != 1 {
            return Some((None, false));
        }
        let mut evidence = Vec::new();
        if item.depth == 1 && root.policy.direct_child_only {
            evidence.push(Evidence::DirectChild);
        }
        for marker in &root.policy.directory_markers {
            let marker_path = item.canonical_path.join(marker);
            let Ok(metadata) = fs::symlink_metadata(marker_path) else {
                continue;
            };
            if !metadata.file_type().is_symlink() {
                evidence.push(Evidence::DirectoryMarker {
                    name: marker.clone(),
                });
            }
        }
        if !evidence.is_empty() || (item.depth == 1 && root.policy.direct_child_only) {
            return Some((
                Some(DiscoveryCandidate {
                    source_root_id: root.id.clone(),
                    observed_path: item.observed_path.clone(),
                    canonical_path: item.canonical_path.clone(),
                    relative_path: item.relative_path.clone(),
                    depth: item.depth,
                    kind: CandidateKind::Directory,
                    evidence,
                    lineage: None,
                    repository_fingerprint: None,
                }),
                false,
            ));
        }
    }

    Some((None, false))
}

fn has_regular_git_marker(path: &Path) -> bool {
    fs::symlink_metadata(path.join(".git"))
        .map(|metadata| !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn is_within(root: &Path, candidate: &Path) -> bool {
    candidate == root || candidate.starts_with(root)
}

fn validate_text(value: &str, field: &str) -> Result<(), DiscoveryError> {
    if value.is_empty() || value.len() > MAX_DISCOVERY_TEXT_BYTES {
        return Err(DiscoveryError::InvalidConfiguration(format!(
            "{field} must be 1..={} bytes",
            MAX_DISCOVERY_TEXT_BYTES
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(DiscoveryError::InvalidConfiguration(format!(
            "{field} contains control characters"
        )));
    }
    Ok(())
}

fn validate_name_list(values: &[String], field: &str) -> Result<(), DiscoveryError> {
    if values.len() > 64 {
        return Err(DiscoveryError::InvalidConfiguration(format!(
            "too many {field}s"
        )));
    }
    for value in values {
        validate_text(value, field)?;
        if value == "." || value == ".." || value.contains('/') || value.contains('\\') {
            return Err(DiscoveryError::InvalidConfiguration(format!(
                "{field} must be one path component"
            )));
        }
    }
    Ok(())
}

fn add_diagnostic(report: &mut Report, limits: &ScanLimits, message: String) {
    if report.diagnostics.len() >= limits.max_diagnostics {
        return;
    }
    let message = truncate_text(&message, MAX_DISCOVERY_DIAGNOSTIC_BYTES);
    if report.output_bytes.saturating_add(message.len()) <= limits.max_output_bytes {
        report.output_bytes += message.len();
        report.diagnostics.push(message);
    }
}

fn truncate_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes.saturating_sub(3);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}

fn display_path(path: &Path) -> String {
    truncate_text(&path.display().to_string(), MAX_DISCOVERY_DIAGNOSTIC_BYTES)
}

fn finish_report(report: &mut Report, started: Instant) {
    report.duration_ms = started.elapsed().as_millis();
}

impl From<RepositoryLineageError> for DiscoveryError {
    fn from(error: RepositoryLineageError) -> Self {
        Self::InvalidConfiguration(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};

    fn root(path: &Path, mode: DiscoveryMode, policy: DiscoveryPolicy) -> DiscoveryRoot {
        DiscoveryRoot::new("test-root", path, mode, policy).expect("valid root")
    }

    fn scanner(limits: ScanLimits) -> Scanner {
        Scanner::new(limits).expect("valid limits")
    }

    #[test]
    fn limits_truncate_entries_and_candidates() {
        let temp = tempfile::tempdir().expect("tempdir");
        for name in ["a", "b", "c"] {
            create_dir_all(temp.path().join(name)).expect("directory");
            write(temp.path().join(name).join("Cargo.toml"), b"[package]\n").expect("marker");
        }
        let report = scanner(ScanLimits {
            max_depth: 2,
            max_entries: 3,
            max_candidates: 10,
            ..ScanLimits::default()
        })
        .scan(&root(
            temp.path(),
            DiscoveryMode::Directory,
            DiscoveryPolicy::default(),
        ));
        assert_eq!(report.status, ScanStatus::Truncated);
        assert!(report.visited_entries <= 3);

        let report = scanner(ScanLimits {
            max_candidates: 1,
            ..ScanLimits::default()
        })
        .scan(&root(
            temp.path(),
            DiscoveryMode::Directory,
            DiscoveryPolicy::default(),
        ));
        assert_eq!(report.status, ScanStatus::Truncated);
        assert_eq!(report.candidate_count(), 1);
    }

    #[test]
    fn traversal_and_candidates_are_sorted() {
        let temp = tempfile::tempdir().expect("tempdir");
        for name in ["zeta", "alpha", "middle"] {
            create_dir_all(temp.path().join(name)).expect("directory");
            write(temp.path().join(name).join("Cargo.toml"), b"[package]\n").expect("marker");
        }
        let report = scanner(ScanLimits::default()).scan(&root(
            temp.path(),
            DiscoveryMode::Directory,
            DiscoveryPolicy::default(),
        ));
        let names = report
            .candidates
            .iter()
            .map(|candidate| candidate.relative_path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["alpha", "middle", "zeta"]);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directory_is_not_followed_or_reported() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        create_dir_all(outside.path().join("project")).expect("outside project");
        write(
            outside.path().join("project").join("Cargo.toml"),
            b"[package]\n",
        )
        .expect("marker");
        symlink(outside.path().join("project"), temp.path().join("alias")).expect("symlink");
        let report = scanner(ScanLimits::default()).scan(&root(
            temp.path(),
            DiscoveryMode::Directory,
            DiscoveryPolicy {
                include_hidden: true,
                ..DiscoveryPolicy::default()
            },
        ));
        assert!(report.candidates.is_empty());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("symlink")));
    }

    fn candidate(path: &Path, kind: CandidateKind, lineage: Option<&str>) -> DiscoveryCandidate {
        let lineage = lineage.map(|key| RepositoryLineageEvidence::Unique {
            remote: crate::repository_lineage::NormalizedRemote {
                host: "example.test".to_owned(),
                path: key.to_owned(),
            },
        });
        DiscoveryCandidate {
            source_root_id: "root".to_owned(),
            observed_path: path.to_path_buf(),
            canonical_path: path.to_path_buf(),
            relative_path: PathBuf::from("candidate"),
            depth: 1,
            kind,
            evidence: Vec::new(),
            lineage,
            repository_fingerprint: None,
        }
    }

    #[test]
    fn reconciliation_handles_alias_lineage_fork_and_plain_directory() {
        let project = ProjectId::new();
        let workspace = WorkspaceId::new();
        let known_path = PathBuf::from("/tmp/catalog-project");
        let known = KnownProject {
            project_id: project.clone(),
            workspace_id: Some(workspace.clone()),
            canonical_root: known_path.clone(),
            lineage_key: Some("git:example.test/team/repo".to_owned()),
            repository_fingerprint: None,
        };

        let alias = candidate(&known_path, CandidateKind::GitRepository, None);
        assert!(matches!(
            reconcile_candidate(&alias, std::slice::from_ref(&known), None),
            ReconciliationOutcome::ExactLocator { .. }
        ));

        let mut canonical_alias = alias.clone();
        canonical_alias.observed_path = PathBuf::from("/tmp/catalog-project-alias");
        assert!(matches!(
            reconcile_candidate(&canonical_alias, std::slice::from_ref(&known), None),
            ReconciliationOutcome::CanonicalAlias { .. }
        ));

        let moved = candidate(
            Path::new("/tmp/catalog-project-moved"),
            CandidateKind::GitRepository,
            Some("team/repo"),
        );
        assert!(matches!(
            reconcile_candidate(&moved, std::slice::from_ref(&known), None),
            ReconciliationOutcome::UniqueLineage { .. }
        ));

        let mut fork = candidate(
            Path::new("/tmp/catalog-fork"),
            CandidateKind::GitRepository,
            Some("team/repo"),
        );
        fork.repository_fingerprint = Some("different-object-fingerprint".to_owned());
        let mut fork_known = known;
        fork_known.repository_fingerprint = Some("object-fingerprint".to_owned());
        assert!(matches!(
            reconcile_candidate(&fork, &[fork_known], None),
            ReconciliationOutcome::ForkConflict { .. }
        ));

        let plain = candidate(
            Path::new("/tmp/plain-moved"),
            CandidateKind::Directory,
            None,
        );
        assert_eq!(
            reconcile_candidate(&plain, &[], None),
            ReconciliationOutcome::PlainDirectoryUnresolved
        );
    }

    #[test]
    fn cancellation_is_reported_without_partial_completion() {
        let temp = tempfile::tempdir().expect("tempdir");
        let token = CancellationToken::new();
        token.cancel();
        let report = scanner(ScanLimits::default()).scan_with_cancellation(
            &root(
                temp.path(),
                DiscoveryMode::Directory,
                DiscoveryPolicy::default(),
            ),
            &token,
        );
        assert_eq!(report.status, ScanStatus::Cancelled);
    }
}
