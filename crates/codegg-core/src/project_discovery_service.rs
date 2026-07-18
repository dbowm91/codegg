//! Daemon-neutral coordinator and persistence seam for bounded discovery.
//!
//! This layer persists bounded scan metadata and delegates project/workspace
//! authority to the existing `ProjectStorage` and `WorkspaceRegistry` APIs.
//! It never writes below a discovered root and never activates workspace
//! services.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use codegg_config::{DiscoveryConfig, DiscoveryMode as ConfigDiscoveryMode, DiscoveryRootConfig};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use thiserror::Error;
use tokio::sync::{Mutex, Notify, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::identity::{ProjectId, WorkspaceId};
use crate::project_discovery::{
    reconcile_candidate, CandidateKind, DiscoveryCandidate, DiscoveryError, DiscoveryMode,
    DiscoveryPolicy, DiscoveryRoot, KnownProject, ObservationStatus, ReconciliationOutcome, Report,
    ScanLimits, ScanStatus, Scanner,
};
use crate::project_storage::ProjectStorage;
use crate::repository_lineage::RepositoryLineageEvidence;
use crate::workspace::{SqliteWorkspaceStore, WorkspaceRegistry};

/// Maximum rows returned by operator inspection APIs.
pub const MAX_DISCOVERY_INSPECTION_ROWS: usize = 256;
/// Maximum serialized diagnostic payload retained in one scan row.
pub const MAX_DISCOVERY_DIAGNOSTICS_JSON_BYTES: usize = 64 * 1024;

/// A persistence-ready root with explicit bounded scan limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryRootRecord {
    pub root: DiscoveryRoot,
    pub limits: ScanLimits,
    pub enabled: bool,
    pub revision: u64,
}

impl DiscoveryRootRecord {
    pub fn from_config(config: &DiscoveryRootConfig) -> Result<Self, DiscoveryServiceError> {
        let id = config.id().or_else(|| config.name()).ok_or_else(|| {
            DiscoveryServiceError::InvalidConfiguration("root requires id or name".into())
        })?;
        let path = config.path().ok_or_else(|| {
            DiscoveryServiceError::InvalidConfiguration("root path is required".into())
        })?;
        let mode = match config.mode() {
            ConfigDiscoveryMode::Git => DiscoveryMode::Git,
            ConfigDiscoveryMode::Directory => DiscoveryMode::Directory,
            ConfigDiscoveryMode::Mixed => DiscoveryMode::Mixed,
        };
        let root = DiscoveryRoot::new(
            id,
            PathBuf::from(path),
            mode,
            DiscoveryPolicy {
                ignore_names: config.ignore(),
                directory_markers: config.directory_markers(),
                direct_child_only: config.direct_child_only(),
                include_hidden: config.include_hidden(),
            },
        )?;
        let limits = ScanLimits {
            max_depth: config.max_depth(),
            max_entries: config.max_visited_entries(),
            max_candidates: config.max_candidates(),
            max_elapsed: std::time::Duration::from_millis(config.max_elapsed_ms()),
            // The config schema bounds these values; the scanner owns the
            // report/output bound so it cannot be omitted by a config layer.
            max_output_bytes: 256 * 1024,
            max_diagnostics: 128,
            stat_concurrency: config.stat_concurrency(),
            git_probe_concurrency: config.git_probe_concurrency(),
        };
        Ok(Self {
            root,
            limits,
            enabled: config.enabled(),
            revision: config.revision.unwrap_or(1).max(1),
        })
    }
}

/// Validate and convert all configured roots before a coordinator is started.
pub fn roots_from_config(
    config: &DiscoveryConfig,
) -> Result<Vec<DiscoveryRootRecord>, Vec<String>> {
    let mut errors = config.validate().err().unwrap_or_default();
    let mut roots = Vec::new();
    if errors.is_empty() {
        for (index, root) in config.roots().iter().enumerate() {
            match DiscoveryRootRecord::from_config(root) {
                Ok(record) => roots.push(record),
                Err(error) => errors.push(format!("discovery.roots[{index}]: {error}")),
            }
        }
    }
    if errors.is_empty() {
        Ok(roots)
    } else {
        Err(errors)
    }
}

/// A bounded validation result for an explicitly configured root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootValidation {
    pub root_id: String,
    pub canonical_path: Option<PathBuf>,
    pub available: bool,
    pub diagnostic: Option<String>,
}

/// Compact persisted-root inspection result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryRootSummary {
    pub id: String,
    pub name: String,
    pub canonical_root: PathBuf,
    pub mode: DiscoveryMode,
    pub enabled: bool,
    pub revision: u64,
    pub time_updated: i64,
}

/// Compact operation status, suitable for a future protocol adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanStatusRecord {
    pub operation_id: String,
    pub root_id: String,
    pub generation: u64,
    pub status: ScanStatus,
    pub report: Option<Report>,
}

/// Result returned by preview and refresh operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshResult {
    pub operation_id: String,
    pub scan_id: String,
    pub generation: u64,
    pub report: Report,
    pub reconciled_count: usize,
    pub ambiguous_count: usize,
    pub missing_count: usize,
}

/// An unresolved observation returned by operator inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedObservation {
    pub id: String,
    pub root_id: String,
    pub generation: u64,
    pub canonical_locator: PathBuf,
    pub status: ObservationStatus,
    pub outcome: String,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DiscoveryServiceError {
    #[error("invalid discovery configuration: {0}")]
    InvalidConfiguration(String),
    #[error("discovery storage error: {0}")]
    Storage(String),
    #[error("discovery operation error: {0}")]
    Operation(String),
    #[error("discovery root is already running: {0}")]
    AlreadyRunning(String),
}

impl From<DiscoveryError> for DiscoveryServiceError {
    fn from(error: DiscoveryError) -> Self {
        Self::InvalidConfiguration(error.to_string())
    }
}

struct RunningOperation {
    operation_id: String,
    token: CancellationToken,
    result: Mutex<Option<Result<RefreshResult, DiscoveryServiceError>>>,
    notify: Notify,
}

/// Daemon-owned bounded discovery coordinator.
#[derive(Clone)]
pub struct DiscoveryCoordinator {
    pool: SqlitePool,
    global_limit: Arc<Semaphore>,
    running: Arc<Mutex<HashMap<String, Arc<RunningOperation>>>>,
    operations: Arc<Mutex<BTreeMap<String, ScanStatusRecord>>>,
}

impl DiscoveryCoordinator {
    pub fn new(pool: SqlitePool, global_concurrency: usize) -> Result<Self, DiscoveryServiceError> {
        ScanLimits::default().validate()?;
        Ok(Self {
            pool,
            global_limit: Arc::new(Semaphore::new(global_concurrency.clamp(1, 16))),
            running: Arc::new(Mutex::new(HashMap::new())),
            operations: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    pub fn validate_root(&self, root: &DiscoveryRootRecord) -> RootValidation {
        match std::fs::canonicalize(&root.root.path) {
            Ok(path) if path.is_dir() => RootValidation {
                root_id: root.root.id.clone(),
                canonical_path: Some(path),
                available: true,
                diagnostic: None,
            },
            Ok(path) => RootValidation {
                root_id: root.root.id.clone(),
                canonical_path: Some(path),
                available: false,
                diagnostic: Some("configured root is not a directory".into()),
            },
            Err(error) => RootValidation {
                root_id: root.root.id.clone(),
                canonical_path: None,
                available: false,
                diagnostic: Some(format!("configured root is unavailable: {error}")),
            },
        }
    }

    pub fn validate_roots(&self, roots: &[DiscoveryRootRecord]) -> Vec<RootValidation> {
        roots.iter().map(|root| self.validate_root(root)).collect()
    }

    pub async fn list_roots(&self) -> Result<Vec<DiscoveryRootSummary>, DiscoveryServiceError> {
        let rows = sqlx::query(
            "SELECT id, name, canonical_root, mode, enabled, revision, time_updated FROM discovery_root ORDER BY id LIMIT ?",
        )
        .bind(MAX_DISCOVERY_INSPECTION_ROWS as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        rows.into_iter().map(root_summary_from_row).collect()
    }

    pub async fn get_root(
        &self,
        root_id: &str,
    ) -> Result<Option<DiscoveryRootSummary>, DiscoveryServiceError> {
        let row = sqlx::query(
            "SELECT id, name, canonical_root, mode, enabled, revision, time_updated FROM discovery_root WHERE id = ?",
        )
        .bind(root_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        row.map(root_summary_from_row).transpose()
    }

    pub async fn preview(
        &self,
        root: DiscoveryRootRecord,
        cancellation: CancellationToken,
    ) -> Result<Report, DiscoveryServiceError> {
        let scanner = Scanner::new(root.limits.clone())?;
        tokio::task::spawn_blocking(move || {
            scanner.scan_with_cancellation(&root.root, &cancellation)
        })
        .await
        .map_err(|error| DiscoveryServiceError::Operation(error.to_string()))
    }

    /// Refresh one root. Calls arriving while the same root is running await
    /// the same result rather than publishing competing generations.
    pub async fn refresh(
        &self,
        root: DiscoveryRootRecord,
    ) -> Result<RefreshResult, DiscoveryServiceError> {
        let operation = self.start_operation(root).await;
        loop {
            if let Some(result) = operation.result.lock().await.clone() {
                return result;
            }
            operation.notify.notified().await;
        }
    }

    /// Start a refresh and return its durable operation id immediately. This
    /// is the cancellation/status seam used by future protocol adapters.
    pub async fn start_refresh(
        &self,
        root: DiscoveryRootRecord,
    ) -> Result<String, DiscoveryServiceError> {
        Ok(self.start_operation(root).await.operation_id.clone())
    }

    async fn start_operation(&self, root: DiscoveryRootRecord) -> Arc<RunningOperation> {
        let root_id = root.root.id.clone();
        let (operation, leader) = loop {
            let mut running = self.running.lock().await;
            if let Some(existing) = running.get(&root_id) {
                let completed = existing
                    .result
                    .try_lock()
                    .map(|result| result.is_some())
                    .unwrap_or(false);
                if completed {
                    running.remove(&root_id);
                    continue;
                }
                break (existing.clone(), false);
            } else {
                let operation = Arc::new(RunningOperation {
                    operation_id: uuid::Uuid::new_v4().to_string(),
                    token: CancellationToken::new(),
                    result: Mutex::new(None),
                    notify: Notify::new(),
                });
                running.insert(root_id, operation.clone());
                break (operation, true);
            }
        };

        if leader {
            let this = self.clone();
            let operation_for_task = operation.clone();
            tokio::spawn(async move {
                let result = this.execute_refresh(root, operation_for_task.clone()).await;
                *operation_for_task.result.lock().await = Some(result);
                this.running
                    .lock()
                    .await
                    .remove(&operation_for_task.operation_id);
                // Remove the completed operation before waking waiters so a
                // subsequent refresh cannot accidentally receive stale data.
                operation_for_task.notify.notify_waiters();
            });
        }

        operation
    }

    pub async fn refresh_all(
        &self,
        roots: Vec<DiscoveryRootRecord>,
    ) -> Vec<Result<RefreshResult, DiscoveryServiceError>> {
        let mut tasks = Vec::new();
        for root in roots.into_iter().filter(|root| root.enabled) {
            let this = self.clone();
            tasks.push(tokio::spawn(async move { this.refresh(root).await }));
        }
        let mut results = Vec::new();
        for task in tasks {
            results.push(
                task.await.unwrap_or_else(|error| {
                    Err(DiscoveryServiceError::Operation(error.to_string()))
                }),
            );
        }
        results
    }

    pub async fn cancel(&self, operation_id: &str) -> bool {
        let running = self.running.lock().await;
        for operation in running.values() {
            if operation.operation_id == operation_id {
                operation.token.cancel();
                return true;
            }
        }
        false
    }

    pub async fn get_scan_status(
        &self,
        operation_id: &str,
    ) -> Result<Option<ScanStatusRecord>, DiscoveryServiceError> {
        if let Some(status) = self.operations.lock().await.get(operation_id).cloned() {
            return Ok(Some(status));
        }
        let row =
            sqlx::query("SELECT id, root_id, generation, status FROM discovery_scan WHERE id = ?")
                .bind(operation_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(storage_error)?;
        row.map(|row| {
            Ok(ScanStatusRecord {
                operation_id: row.try_get("id").map_err(row_error)?,
                root_id: row.try_get("root_id").map_err(row_error)?,
                generation: row.try_get::<i64, _>("generation").map_err(row_error)? as u64,
                status: parse_scan_status(row.try_get("status").map_err(row_error)?),
                report: None,
            })
        })
        .transpose()
    }

    pub async fn list_unresolved(
        &self,
        root_id: Option<&str>,
    ) -> Result<Vec<UnresolvedObservation>, DiscoveryServiceError> {
        let rows = if let Some(root_id) = root_id {
            sqlx::query(
                "SELECT id, root_id, generation, canonical_locator, status, outcome, diagnostic FROM discovery_observation WHERE root_id = ? AND status IN ('missing','ambiguous','inaccessible','stale') ORDER BY time_observed DESC LIMIT ?",
            )
            .bind(root_id)
            .bind(MAX_DISCOVERY_INSPECTION_ROWS as i64)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query(
                "SELECT id, root_id, generation, canonical_locator, status, outcome, diagnostic FROM discovery_observation WHERE status IN ('missing','ambiguous','inaccessible','stale') ORDER BY time_observed DESC LIMIT ?",
            )
            .bind(MAX_DISCOVERY_INSPECTION_ROWS as i64)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(storage_error)?;

        rows.into_iter()
            .map(|row| {
                Ok(UnresolvedObservation {
                    id: row.try_get("id").map_err(row_error)?,
                    root_id: row.try_get("root_id").map_err(row_error)?,
                    generation: row.try_get::<i64, _>("generation").map_err(row_error)? as u64,
                    canonical_locator: PathBuf::from(
                        row.try_get::<String, _>("canonical_locator")
                            .map_err(row_error)?,
                    ),
                    status: parse_observation_status(row.try_get("status").map_err(row_error)?),
                    outcome: row.try_get("outcome").map_err(row_error)?,
                    diagnostic: row.try_get("diagnostic").map_err(row_error)?,
                })
            })
            .collect()
    }

    /// Explicit association is intentionally narrow: it only rebinds an
    /// already registered workspace using its expected revision.
    pub async fn associate_workspace(
        &self,
        workspace_id: &WorkspaceId,
        project_id: &ProjectId,
        expected_revision: i64,
    ) -> Result<(), DiscoveryServiceError> {
        ProjectStorage::new(self.pool.clone())
            .rebind_workspace(
                workspace_id,
                project_id,
                None,
                expected_revision,
                "discovery-explicit-association",
            )
            .await
            .map(|_| ())
            .map_err(|error| DiscoveryServiceError::Operation(error.to_string()))
    }

    async fn execute_refresh(
        &self,
        root: DiscoveryRootRecord,
        operation: Arc<RunningOperation>,
    ) -> Result<RefreshResult, DiscoveryServiceError> {
        let _permit = self
            .global_limit
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| DiscoveryServiceError::Operation(error.to_string()))?;
        // Reuse the operation id as the durable scan id so status inspection
        // survives coordinator restart without a second operation-id table.
        let scan_id = operation.operation_id.clone();
        let generation = next_generation(&self.pool, &root.root.id).await?;
        upsert_root(&self.pool, &root).await?;
        insert_scan_start(&self.pool, &scan_id, &root.root.id, generation).await?;
        let scanner = Scanner::new(root.limits.clone())?;
        let root_for_scan = root.root.clone();
        let token = operation.token.clone();
        let report = tokio::task::spawn_blocking(move || {
            scanner.scan_with_cancellation(&root_for_scan, &token)
        })
        .await
        .map_err(|error| DiscoveryServiceError::Operation(error.to_string()))?;

        if report.status == ScanStatus::Cancelled {
            finish_scan(&self.pool, &scan_id, &report, "cancelled", 0, 0, 0).await?;
            self.record_status(
                &operation,
                &root.root.id,
                generation,
                ScanStatus::Cancelled,
                report.clone(),
            )
            .await;
            return Ok(RefreshResult {
                operation_id: operation.operation_id.clone(),
                scan_id,
                generation,
                report,
                reconciled_count: 0,
                ambiguous_count: 0,
                missing_count: 0,
            });
        }
        if report.status == ScanStatus::Unavailable || report.status == ScanStatus::Failed {
            finish_scan(
                &self.pool,
                &scan_id,
                &report,
                "failed",
                0,
                0,
                report.inaccessible_entries,
            )
            .await?;
            self.record_status(
                &operation,
                &root.root.id,
                generation,
                ScanStatus::Failed,
                report.clone(),
            )
            .await;
            return Ok(RefreshResult {
                operation_id: operation.operation_id.clone(),
                scan_id,
                generation,
                report,
                reconciled_count: 0,
                ambiguous_count: 0,
                missing_count: 0,
            });
        }

        let known = load_known_projects(&self.pool).await?;
        let registry =
            WorkspaceRegistry::load(Arc::new(SqliteWorkspaceStore::new(self.pool.clone())))
                .await
                .map_err(|error| DiscoveryServiceError::Operation(error.to_string()))?;
        let mut reconciled_count = 0;
        let mut ambiguous_count = 0;
        for candidate in &report.candidates {
            let planned = reconcile_candidate(candidate, &known, None);
            let (outcome, project_id, workspace_id) = match planned {
                ReconciliationOutcome::UniqueLineage { .. }
                | ReconciliationOutcome::NewCandidate => {
                    let workspace = registry
                        .get_or_register(&candidate.canonical_path)
                        .await
                        .map_err(|error| DiscoveryServiceError::Operation(error.to_string()))?;
                    let evidence = candidate
                        .lineage
                        .clone()
                        .unwrap_or(RepositoryLineageEvidence::NotRepository);
                    let binding = ProjectStorage::new(self.pool.clone())
                        .reconcile_workspace(&workspace, &evidence, "discovery")
                        .await
                        .map_err(|error| DiscoveryServiceError::Operation(error.to_string()))?
                        .binding;
                    let outcome = match planned {
                        ReconciliationOutcome::NewCandidate => {
                            ReconciliationOutcome::CreatedProject {
                                project_id: binding.project_id.clone(),
                                workspace_id: Some(binding.workspace_id.clone()),
                            }
                        }
                        ReconciliationOutcome::UniqueLineage { .. } => {
                            ReconciliationOutcome::UniqueLineage {
                                project_id: binding.project_id.clone(),
                                workspace_id: Some(binding.workspace_id.clone()),
                            }
                        }
                        _ => unreachable!(),
                    };
                    (
                        outcome,
                        Some(binding.project_id),
                        Some(binding.workspace_id),
                    )
                }
                other => {
                    if matches!(
                        other,
                        ReconciliationOutcome::AmbiguousLineage
                            | ReconciliationOutcome::ForkConflict { .. }
                            | ReconciliationOutcome::PlainDirectoryUnresolved
                    ) {
                        ambiguous_count += 1;
                    }
                    let project_id = other.project_id().cloned();
                    let workspace_id = match &other {
                        ReconciliationOutcome::ExactLocator { workspace_id, .. }
                        | ReconciliationOutcome::CanonicalAlias { workspace_id, .. }
                        | ReconciliationOutcome::UniqueLineage { workspace_id, .. } => {
                            workspace_id.clone()
                        }
                        ReconciliationOutcome::CreatedProject { workspace_id, .. } => {
                            workspace_id.clone()
                        }
                        _ => None,
                    };
                    (other, project_id, workspace_id)
                }
            };
            if project_id.is_some() {
                reconciled_count += 1;
            }
            insert_observation(
                &self.pool,
                &scan_id,
                &root.root.id,
                generation,
                candidate,
                ObservationStatus::Present,
                &outcome,
                project_id.as_ref(),
                workspace_id.as_ref(),
                None,
            )
            .await?;
        }
        let missing_count =
            insert_missing_observations(&self.pool, &root.root.id, generation, &scan_id, &report)
                .await?;
        let status = if report.status == ScanStatus::Truncated {
            "truncated"
        } else {
            "completed"
        };
        finish_scan(
            &self.pool,
            &scan_id,
            &report,
            status,
            reconciled_count,
            ambiguous_count,
            report.inaccessible_entries,
        )
        .await?;
        prune_old_scans(&self.pool, &root.root.id, generation).await?;
        self.record_status(
            &operation,
            &root.root.id,
            generation,
            report.status,
            report.clone(),
        )
        .await;
        Ok(RefreshResult {
            operation_id: operation.operation_id.clone(),
            scan_id,
            generation,
            report,
            reconciled_count,
            ambiguous_count,
            missing_count,
        })
    }

    async fn record_status(
        &self,
        operation: &RunningOperation,
        root_id: &str,
        generation: u64,
        status: ScanStatus,
        report: Report,
    ) {
        let mut operations = self.operations.lock().await;
        operations.insert(
            operation.operation_id.clone(),
            ScanStatusRecord {
                operation_id: operation.operation_id.clone(),
                root_id: root_id.to_owned(),
                generation,
                status,
                report: Some(report),
            },
        );
        while operations.len() > MAX_DISCOVERY_INSPECTION_ROWS {
            let first = operations.keys().next().cloned();
            if let Some(first) = first {
                operations.remove(&first);
            } else {
                break;
            }
        }
    }
}

const DISCOVERY_GENERATION_RETENTION: u64 = 20;

async fn prune_old_scans(
    pool: &SqlitePool,
    root_id: &str,
    latest_generation: u64,
) -> Result<(), DiscoveryServiceError> {
    let cutoff = latest_generation.saturating_sub(DISCOVERY_GENERATION_RETENTION);
    if cutoff == 0 {
        return Ok(());
    }
    sqlx::query("DELETE FROM discovery_scan WHERE root_id = ? AND generation < ?")
        .bind(root_id)
        .bind(cutoff as i64)
        .execute(pool)
        .await
        .map_err(storage_error)?;
    Ok(())
}

fn storage_error(error: sqlx::Error) -> DiscoveryServiceError {
    DiscoveryServiceError::Storage(error.to_string())
}

fn row_error(error: sqlx::Error) -> DiscoveryServiceError {
    DiscoveryServiceError::Storage(error.to_string())
}

fn parse_scan_status(value: String) -> ScanStatus {
    match value.as_str() {
        "queued" => ScanStatus::Queued,
        "running" => ScanStatus::Running,
        "completed" => ScanStatus::Completed,
        "cancelled" => ScanStatus::Cancelled,
        "truncated" => ScanStatus::Truncated,
        "failed" | "interrupted" => ScanStatus::Failed,
        _ => ScanStatus::Failed,
    }
}

fn parse_observation_status(value: String) -> ObservationStatus {
    match value.as_str() {
        "present" => ObservationStatus::Present,
        "moved" => ObservationStatus::Moved,
        "missing" => ObservationStatus::Missing,
        "ambiguous" => ObservationStatus::Ambiguous,
        "inaccessible" => ObservationStatus::Inaccessible,
        "ignored" => ObservationStatus::Ignored,
        _ => ObservationStatus::Stale,
    }
}

fn mode_as_str(mode: DiscoveryMode) -> &'static str {
    match mode {
        DiscoveryMode::Git => "git",
        DiscoveryMode::Directory => "directory",
        DiscoveryMode::Mixed => "mixed",
    }
}

fn root_summary_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<DiscoveryRootSummary, DiscoveryServiceError> {
    let mode = match row
        .try_get::<String, _>("mode")
        .map_err(row_error)?
        .as_str()
    {
        "git" => DiscoveryMode::Git,
        "directory" => DiscoveryMode::Directory,
        "mixed" => DiscoveryMode::Mixed,
        other => {
            return Err(DiscoveryServiceError::Storage(format!(
                "invalid discovery mode {other}"
            )))
        }
    };
    Ok(DiscoveryRootSummary {
        id: row.try_get("id").map_err(row_error)?,
        name: row.try_get("name").map_err(row_error)?,
        canonical_root: PathBuf::from(
            row.try_get::<String, _>("canonical_root")
                .map_err(row_error)?,
        ),
        mode,
        enabled: row.try_get::<i64, _>("enabled").map_err(row_error)? != 0,
        revision: row.try_get::<i64, _>("revision").map_err(row_error)? as u64,
        time_updated: row.try_get("time_updated").map_err(row_error)?,
    })
}

async fn upsert_root(
    pool: &SqlitePool,
    root: &DiscoveryRootRecord,
) -> Result<(), DiscoveryServiceError> {
    let now = chrono::Utc::now().timestamp_millis();
    let ignore = serde_json::to_string(&root.root.policy.ignore_names)
        .map_err(|e| DiscoveryServiceError::Storage(e.to_string()))?;
    let markers = serde_json::to_string(&root.root.policy.directory_markers)
        .map_err(|e| DiscoveryServiceError::Storage(e.to_string()))?;
    sqlx::query(
        "INSERT INTO discovery_root (id,name,canonical_root,mode,enabled,revision,max_depth,max_entries,max_candidates,max_duration_ms,stat_concurrency,git_probe_concurrency,include_hidden,follow_symlinks,ignore_names_json,directory_markers_json,direct_child_only,time_created,time_updated) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,canonical_root=excluded.canonical_root,mode=excluded.mode,enabled=excluded.enabled,revision=discovery_root.revision+1,max_depth=excluded.max_depth,max_entries=excluded.max_entries,max_candidates=excluded.max_candidates,max_duration_ms=excluded.max_duration_ms,stat_concurrency=excluded.stat_concurrency,git_probe_concurrency=excluded.git_probe_concurrency,include_hidden=excluded.include_hidden,ignore_names_json=excluded.ignore_names_json,directory_markers_json=excluded.directory_markers_json,direct_child_only=excluded.direct_child_only,time_updated=excluded.time_updated",
    )
    .bind(&root.root.id)
    .bind(&root.root.id)
    .bind(root.root.path.to_string_lossy().as_ref())
    .bind(mode_as_str(root.root.mode))
    .bind(if root.enabled { 1 } else { 0 })
    .bind(root.revision as i64)
    .bind(root.limits.max_depth as i64)
    .bind(root.limits.max_entries as i64)
    .bind(root.limits.max_candidates as i64)
    .bind(root.limits.max_elapsed.as_millis() as i64)
    .bind(root.limits.stat_concurrency as i64)
    .bind(root.limits.git_probe_concurrency as i64)
    .bind(if root.root.policy.include_hidden { 1 } else { 0 })
    .bind(0_i64)
    .bind(ignore)
    .bind(markers)
    .bind(if root.root.policy.direct_child_only { 1 } else { 0 })
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(storage_error)?;
    Ok(())
}

async fn next_generation(pool: &SqlitePool, root_id: &str) -> Result<u64, DiscoveryServiceError> {
    let value: Option<i64> =
        sqlx::query_scalar("SELECT MAX(generation) FROM discovery_scan WHERE root_id = ?")
            .bind(root_id)
            .fetch_one(pool)
            .await
            .map_err(storage_error)?;
    Ok(value.unwrap_or(0).max(0) as u64 + 1)
}

async fn insert_scan_start(
    pool: &SqlitePool,
    scan_id: &str,
    root_id: &str,
    generation: u64,
) -> Result<(), DiscoveryServiceError> {
    sqlx::query("INSERT INTO discovery_scan (id,root_id,generation,status,visited_entries,ignored_entries,candidate_count,reconciled_count,ambiguous_count,unavailable_count,error_count,duration_ms,truncated,diagnostics_json,started_at) VALUES (?,?,?,'running',0,0,0,0,0,0,0,0,0,'[]',?)")
        .bind(scan_id)
        .bind(root_id)
        .bind(generation as i64)
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(pool)
        .await
        .map_err(storage_error)?;
    Ok(())
}

async fn finish_scan(
    pool: &SqlitePool,
    scan_id: &str,
    report: &Report,
    status: &str,
    reconciled_count: usize,
    ambiguous_count: usize,
    unavailable_count: usize,
) -> Result<(), DiscoveryServiceError> {
    let diagnostics = bounded_json(&report.diagnostics, MAX_DISCOVERY_DIAGNOSTICS_JSON_BYTES);
    sqlx::query("UPDATE discovery_scan SET status=?,visited_entries=?,ignored_entries=?,candidate_count=?,reconciled_count=?,ambiguous_count=?,unavailable_count=?,error_count=?,duration_ms=?,truncated=?,diagnostics_json=?,completed_at=? WHERE id=?")
        .bind(status)
        .bind(report.visited_entries as i64)
        .bind(report.ignored_entries as i64)
        .bind(report.candidates.len() as i64)
        .bind(reconciled_count as i64)
        .bind(ambiguous_count as i64)
        .bind(unavailable_count as i64)
        .bind(report.inaccessible_entries as i64)
        .bind(report.duration_ms as i64)
        .bind(if report.is_truncated() { 1 } else { 0 })
        .bind(diagnostics)
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(scan_id)
        .execute(pool)
        .await
        .map_err(storage_error)?;
    Ok(())
}

async fn load_known_projects(
    pool: &SqlitePool,
) -> Result<Vec<KnownProject>, DiscoveryServiceError> {
    let rows = sqlx::query("SELECT wpb.project_id, wpb.workspace_id, w.canonical_root, r.lineage_key FROM workspace_project_binding wpb JOIN workspace w ON w.id = wpb.workspace_id LEFT JOIN project_repository pr ON pr.project_id = wpb.project_id LEFT JOIN repository r ON r.id = pr.repository_id WHERE wpb.status = 'resolved' ORDER BY wpb.project_id, wpb.workspace_id")
        .fetch_all(pool)
        .await
        .map_err(storage_error)?;
    let mut known = Vec::new();
    for row in rows {
        let project_id =
            ProjectId::parse(&row.try_get::<String, _>("project_id").map_err(row_error)?)
                .map_err(|e| DiscoveryServiceError::Storage(e.to_string()))?;
        let workspace_id = WorkspaceId::parse(
            &row.try_get::<String, _>("workspace_id")
                .map_err(row_error)?,
        )
        .map_err(|e| DiscoveryServiceError::Storage(e.to_string()))?;
        let canonical_root = PathBuf::from(
            row.try_get::<String, _>("canonical_root")
                .map_err(row_error)?,
        );
        let lineage_key = row
            .try_get::<Option<String>, _>("lineage_key")
            .map_err(row_error)?;
        known.push(KnownProject {
            project_id,
            workspace_id: Some(workspace_id),
            canonical_root,
            lineage_key,
            repository_fingerprint: None,
        });
    }
    Ok(known)
}

#[allow(clippy::too_many_arguments)]
async fn insert_observation(
    pool: &SqlitePool,
    scan_id: &str,
    root_id: &str,
    generation: u64,
    candidate: &DiscoveryCandidate,
    status: ObservationStatus,
    outcome: &ReconciliationOutcome,
    project_id: Option<&ProjectId>,
    workspace_id: Option<&WorkspaceId>,
    diagnostic: Option<&str>,
) -> Result<(), DiscoveryServiceError> {
    let status = observation_status_as_str(status);
    let outcome = serde_json::to_string(outcome)
        .map_err(|e| DiscoveryServiceError::Storage(e.to_string()))?;
    sqlx::query("INSERT INTO discovery_observation (id,root_id,scan_id,generation,canonical_locator,relative_path,candidate_kind,lineage_key,project_id,workspace_id,status,outcome,diagnostic,time_observed) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(root_id)
        .bind(scan_id)
        .bind(generation as i64)
        .bind(candidate.canonical_path.to_string_lossy().as_ref())
        .bind(candidate.relative_path.to_string_lossy().as_ref())
        .bind(match candidate.kind { CandidateKind::GitRepository => "git_repository", CandidateKind::Directory => "directory" })
        .bind(candidate.lineage_key())
        .bind(project_id.map(ProjectId::as_str))
        .bind(workspace_id.map(WorkspaceId::as_str))
        .bind(status)
        .bind(bounded_text(&outcome, 8 * 1024))
        .bind(diagnostic.map(|value| bounded_text(value, 512)))
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(pool)
        .await
        .map_err(storage_error)?;
    Ok(())
}

async fn insert_missing_observations(
    pool: &SqlitePool,
    root_id: &str,
    generation: u64,
    scan_id: &str,
    report: &Report,
) -> Result<usize, DiscoveryServiceError> {
    let previous_generation: Option<i64> = sqlx::query_scalar("SELECT MAX(generation) FROM discovery_scan WHERE root_id = ? AND generation < ? AND status IN ('completed','truncated')")
        .bind(root_id)
        .bind(generation as i64)
        .fetch_one(pool)
        .await
        .map_err(storage_error)?;
    let Some(previous_generation) = previous_generation else {
        return Ok(0);
    };
    let rows = sqlx::query("SELECT canonical_locator, relative_path, candidate_kind, lineage_key, project_id, workspace_id FROM discovery_observation WHERE root_id = ? AND generation = ? AND status IN ('present','moved')")
        .bind(root_id)
        .bind(previous_generation)
        .fetch_all(pool)
        .await
        .map_err(storage_error)?;
    let current: std::collections::HashSet<PathBuf> = report
        .candidates
        .iter()
        .map(|candidate| candidate.canonical_path.clone())
        .collect();
    let mut missing = 0;
    for row in rows {
        let locator = PathBuf::from(
            row.try_get::<String, _>("canonical_locator")
                .map_err(row_error)?,
        );
        if current.contains(&locator) {
            continue;
        }
        let kind = match row
            .try_get::<String, _>("candidate_kind")
            .map_err(row_error)?
            .as_str()
        {
            "git_repository" => CandidateKind::GitRepository,
            _ => CandidateKind::Directory,
        };
        let candidate = DiscoveryCandidate {
            source_root_id: root_id.to_owned(),
            observed_path: locator.clone(),
            canonical_path: locator,
            relative_path: PathBuf::from(
                row.try_get::<String, _>("relative_path")
                    .map_err(row_error)?,
            ),
            depth: 0,
            kind,
            evidence: Vec::new(),
            lineage: None,
            repository_fingerprint: None,
        };
        let outcome = ReconciliationOutcome::PlainDirectoryUnresolved;
        insert_observation(
            pool,
            scan_id,
            root_id,
            generation,
            &candidate,
            ObservationStatus::Missing,
            &outcome,
            None,
            None,
            Some("candidate absent from latest successful generation; catalog retained"),
        )
        .await?;
        missing += 1;
    }
    Ok(missing)
}

fn observation_status_as_str(status: ObservationStatus) -> &'static str {
    match status {
        ObservationStatus::Present => "present",
        ObservationStatus::Moved => "moved",
        ObservationStatus::Missing => "missing",
        ObservationStatus::Ambiguous => "ambiguous",
        ObservationStatus::Inaccessible => "inaccessible",
        ObservationStatus::Ignored => "ignored",
        ObservationStatus::Stale => "stale",
    }
}

fn bounded_text(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max.saturating_sub(3);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}

fn bounded_json<T: Serialize>(value: &T, max: usize) -> String {
    serde_json::to_string(value)
        .map(|value| bounded_text(&value, max))
        .unwrap_or_else(|_| "[]".into())
}

impl From<DiscoveryServiceError> for crate::error::StorageError {
    fn from(error: DiscoveryServiceError) -> Self {
        crate::error::StorageError::Database(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};
    use std::path::Path;

    fn test_root(path: &Path) -> DiscoveryRootRecord {
        DiscoveryRootRecord {
            root: DiscoveryRoot::new(
                "test-root",
                path,
                DiscoveryMode::Directory,
                DiscoveryPolicy {
                    ignore_names: vec![".git".into()],
                    directory_markers: vec!["Cargo.toml".into()],
                    direct_child_only: false,
                    include_hidden: false,
                },
            )
            .expect("valid test root"),
            limits: ScanLimits::default(),
            enabled: true,
            revision: 1,
        }
    }

    async fn test_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::session::schema::migrate(&pool)
            .await
            .expect("schema migration");
        pool
    }

    #[test]
    fn config_conversion_keeps_finite_defaults() {
        let config = DiscoveryRootConfig {
            id: Some("root".into()),
            path: Some("/tmp/projects".into()),
            ..Default::default()
        };
        let record = DiscoveryRootRecord::from_config(&config).expect("valid root");
        assert_eq!(record.limits.max_depth, 4);
        assert_eq!(record.limits.max_entries, 10_000);
        assert_eq!(record.limits.max_candidates, 1_000);
        assert!(record.limits.max_elapsed.as_millis() > 0);
    }

    #[test]
    fn invalid_config_is_rejected_before_service_creation() {
        let config = DiscoveryConfig {
            enabled: Some(true),
            roots: Some(vec![DiscoveryRootConfig {
                id: Some("bad\nroot".into()),
                path: Some("/tmp".into()),
                ..Default::default()
            }]),
            max_concurrent_scans: Some(0),
        };
        assert!(roots_from_config(&config).is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn preview_is_bounded_and_does_not_create_catalog_rows() {
        let temp = tempfile::tempdir().expect("tempdir");
        create_dir_all(temp.path().join("project")).expect("project");
        write(temp.path().join("project/Cargo.toml"), b"[package]\n").expect("marker");
        let pool = test_pool().await;
        let coordinator = DiscoveryCoordinator::new(pool.clone(), 1).expect("coordinator");
        let report = coordinator
            .preview(test_root(temp.path()), CancellationToken::new())
            .await
            .expect("preview");
        assert_eq!(report.candidate_count(), 1);
        let projects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logical_project")
            .fetch_one(&pool)
            .await
            .expect("project count");
        assert_eq!(projects, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn refresh_preserves_completed_generation_when_root_becomes_unavailable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = test_root(temp.path());
        let pool = test_pool().await;
        let coordinator = DiscoveryCoordinator::new(pool.clone(), 1).expect("coordinator");
        let first = coordinator
            .refresh(root.clone())
            .await
            .expect("first refresh");
        assert_eq!(first.generation, 1);
        drop(temp);
        let second = coordinator
            .refresh(root)
            .await
            .expect("unavailable refresh");
        assert_eq!(second.generation, 2);
        assert_eq!(second.report.status, ScanStatus::Unavailable);
        let statuses: Vec<String> =
            sqlx::query_scalar("SELECT status FROM discovery_scan ORDER BY generation")
                .fetch_all(&pool)
                .await
                .expect("scan statuses");
        assert_eq!(statuses, vec!["completed", "failed"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn start_cancel_and_same_root_refresh_share_one_operation() {
        let temp = tempfile::tempdir().expect("tempdir");
        for index in 0..256 {
            let path = temp.path().join(format!("project-{index:03}"));
            create_dir_all(&path).expect("project");
            write(path.join("Cargo.toml"), b"[package]\n").expect("marker");
        }
        let pool = test_pool().await;
        let coordinator = DiscoveryCoordinator::new(pool, 1).expect("coordinator");
        let root = test_root(temp.path());
        let operation_id = coordinator
            .start_refresh(root.clone())
            .await
            .expect("start refresh");
        assert!(coordinator.cancel(&operation_id).await);
        let result = coordinator.refresh(root).await.expect("joined refresh");
        assert_eq!(result.operation_id, operation_id);
        assert_eq!(result.report.status, ScanStatus::Cancelled);
    }
}
