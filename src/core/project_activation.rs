//! Project/workspace-scoped lazy activation and bounded health.
//!
//! This module is the Project Catalog Milestone 3 seam. It owns the
//! project-level lease lifecycle while delegating workspace bundle
//! construction to [`WorkspaceServiceRegistry`]. Runtime-asset publication is
//! deliberately not implemented here; [`CoreDaemon`] calls its existing
//! daemon-owned refresh seam after acquiring a lease.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;

use crate::agent::asset_refresh::{RefreshOutcome, RefreshReport, RefreshStatus};
use codegg_core::project_catalog::HealthStatus as CatalogHealthStatus;
use codegg_core::workspace::WorkspaceId;
use codegg_core::workspace_services::{
    WorkspaceServiceError, WorkspaceServiceRegistry, WorkspaceServiceSnapshot,
    WorkspaceServicesLease,
};

pub const MAX_ACTIVATION_IDENTIFIER_LENGTH: usize = 128;
pub const MAX_HEALTH_DIAGNOSTICS: usize = 16;
pub const MAX_HEALTH_TEXT_LENGTH: usize = 192;

/// Policy bounding project activation leases and the in-memory activation
/// table. Workspace bundle eviction remains governed by the lower-level
/// [`WorkspaceServiceRegistry`] policy.
#[derive(Debug, Clone)]
pub struct ProjectActivationPolicy {
    pub lease_ttl: Duration,
    pub max_active_leases: usize,
}

impl Default for ProjectActivationPolicy {
    fn default() -> Self {
        Self {
            lease_ttl: Duration::from_secs(30 * 60),
            max_active_leases: 64,
        }
    }
}

#[derive(Debug, Error)]
pub enum ProjectActivationError {
    #[error("invalid activation {field}: {message}")]
    InvalidInput {
        field: &'static str,
        message: String,
    },
    #[error("workspace service activation failed: {0}")]
    Workspace(#[from] WorkspaceServiceError),
    #[error("project activation lease capacity is exhausted")]
    Capacity,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ActivationKey {
    project_id: String,
    workspace_id: String,
    owner: String,
}

struct ActiveActivation {
    key: ActivationKey,
    lease_id: String,
    acquired_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    handles: AtomicUsize,
    expired: AtomicBool,
    services: Mutex<Option<WorkspaceServicesLease>>,
}

impl ActiveActivation {
    fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expired.load(Ordering::Acquire) || now >= self.expires_at
    }

    fn retain(&self, now: DateTime<Utc>) -> bool {
        if self.is_expired(now) {
            self.expired.store(true, Ordering::Release);
            return false;
        }
        self.handles.fetch_add(1, Ordering::AcqRel);
        true
    }

    fn release_services(&self) {
        self.expired.store(true, Ordering::Release);
        self.services.lock().take();
    }

    fn service_snapshot(&self) -> Option<WorkspaceServiceSnapshot> {
        self.services
            .lock()
            .as_ref()
            .map(|lease| lease.services().snapshot())
    }
}

/// Explicit owner handle for one project/workspace activation.
///
/// The handle is intentionally non-serializable and owns the underlying
/// workspace-service lease. Dropping or explicitly releasing it decrements
/// service lease accounting. Expiry is enforced by
/// [`ProjectActivationRegistry::evict_expired`], which also releases the
/// underlying service lease even if a caller retained a stale handle.
pub struct ProjectActivationLease {
    registry: Arc<ProjectActivationRegistry>,
    activation: Arc<ActiveActivation>,
    released: AtomicBool,
}

impl std::fmt::Debug for ProjectActivationLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProjectActivationLease")
            .field("lease_id", &self.lease_id())
            .field("project_id", &self.project_id())
            .field("workspace_id", &self.workspace_id())
            .field("owner", &self.owner())
            .field("expires_at", &self.expires_at())
            .field("active", &self.is_active())
            .finish()
    }
}

impl ProjectActivationLease {
    pub fn lease_id(&self) -> &str {
        &self.activation.lease_id
    }

    pub fn project_id(&self) -> &str {
        &self.activation.key.project_id
    }

    pub fn workspace_id(&self) -> &str {
        &self.activation.key.workspace_id
    }

    pub fn owner(&self) -> &str {
        &self.activation.key.owner
    }

    pub fn acquired_at(&self) -> DateTime<Utc> {
        self.activation.acquired_at
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.activation.expires_at
    }

    pub fn is_active(&self) -> bool {
        !self.released.load(Ordering::Acquire)
            && !self.activation.is_expired(Utc::now())
            && self.activation.services.lock().is_some()
    }

    pub fn service_snapshot(&self) -> Option<WorkspaceServiceSnapshot> {
        if self.is_active() {
            self.activation.service_snapshot()
        } else {
            None
        }
    }

    /// Release this activation before its bounded lease lifetime ends.
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        if self.released.swap(true, Ordering::AcqRel) {
            return;
        }
        if self.activation.handles.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.registry
                .active
                .remove_if(&self.activation.key, |_key, current| {
                    Arc::ptr_eq(current, &self.activation)
                });
            if !self.activation.expired.load(Ordering::Acquire) {
                self.activation.services.lock().take();
            }
        }
    }
}

impl Drop for ProjectActivationLease {
    fn drop(&mut self) {
        self.release_inner();
    }
}

/// Result of a bounded activation eviction pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivationEvictionReport {
    pub evaluated: usize,
    pub evicted_lease_ids: Vec<String>,
}

/// Result returned after the daemon has acquired a project activation lease,
/// refreshed runtime assets, and assembled the bounded health aggregate.
#[derive(Debug)]
pub struct ProjectActivation {
    pub lease: ProjectActivationLease,
    pub refresh: RefreshReport,
    pub health: ProjectHealthSnapshot,
    pub binding_revision: i64,
    pub diagnostics: Vec<String>,
}

/// Daemon-owned project activation registry.
pub struct ProjectActivationRegistry {
    workspace_services: Arc<WorkspaceServiceRegistry>,
    active: DashMap<ActivationKey, Arc<ActiveActivation>>,
    activation_locks: DashMap<ActivationKey, Arc<AsyncMutex<()>>>,
    capacity_lock: AsyncMutex<()>,
    policy: ProjectActivationPolicy,
}

impl std::fmt::Debug for ProjectActivationRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProjectActivationRegistry")
            .field("active_count", &self.active.len())
            .field("max_active_leases", &self.policy.max_active_leases)
            .finish()
    }
}

impl ProjectActivationRegistry {
    pub fn new(
        workspace_services: Arc<WorkspaceServiceRegistry>,
        policy: ProjectActivationPolicy,
    ) -> Arc<Self> {
        Arc::new(Self {
            workspace_services,
            active: DashMap::new(),
            activation_locks: DashMap::new(),
            capacity_lock: AsyncMutex::new(()),
            policy,
        })
    }

    /// Acquire an owner-scoped project activation. Repeated acquisition by
    /// the same owner for the same project/workspace is idempotent and shares
    /// one bounded activation record. Different owners receive independent
    /// leases while still sharing the one workspace service bundle.
    pub async fn acquire(
        self: &Arc<Self>,
        project_id: &str,
        workspace_id: &str,
        owner: &str,
    ) -> Result<ProjectActivationLease, ProjectActivationError> {
        let project_id = bounded_identifier(project_id, "project_id")?;
        let workspace_id = bounded_identifier(workspace_id, "workspace_id")?;
        let owner = bounded_identifier(owner, "owner")?;
        let key = ActivationKey {
            project_id,
            workspace_id,
            owner,
        };
        let activation_lock = self
            .activation_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone();
        let _guard = activation_lock.lock().await;
        let now = Utc::now();

        if let Some(existing) = self.active.get(&key).map(|entry| entry.clone()) {
            if existing.retain(now) {
                return Ok(ProjectActivationLease {
                    registry: Arc::clone(self),
                    activation: existing,
                    released: AtomicBool::new(false),
                });
            }
            self.active
                .remove_if(&key, |_key, current| Arc::ptr_eq(current, &existing));
            existing.release_services();
        }

        let typed_workspace_id = WorkspaceId::new_unchecked(key.workspace_id.clone());
        let services = self.workspace_services.acquire(&typed_workspace_id).await?;
        // Distinct activation keys use distinct single-flight locks. The
        // capacity check and insertion therefore need a registry-wide lock so
        // concurrent owners cannot all observe the same available slot.
        let _capacity_guard = self.capacity_lock.lock().await;
        self.evict_expired(now);
        if self.active.len() >= self.policy.max_active_leases {
            drop(_capacity_guard);
            drop(services);
            return Err(ProjectActivationError::Capacity);
        }
        let activation = Arc::new(ActiveActivation {
            key: key.clone(),
            lease_id: uuid::Uuid::new_v4().to_string(),
            acquired_at: now,
            expires_at: now
                + chrono::Duration::from_std(self.policy.lease_ttl)
                    .unwrap_or_else(|_| chrono::Duration::minutes(30)),
            handles: AtomicUsize::new(1),
            expired: AtomicBool::new(false),
            services: Mutex::new(Some(services)),
        });
        self.active.insert(key, activation.clone());
        Ok(ProjectActivationLease {
            registry: Arc::clone(self),
            activation,
            released: AtomicBool::new(false),
        })
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn list_active(&self) -> Vec<ActivationLeaseSnapshot> {
        self.active
            .iter()
            .map(|entry| ActivationLeaseSnapshot {
                lease_id: entry.value().lease_id.clone(),
                project_id: entry.value().key.project_id.clone(),
                workspace_id: entry.value().key.workspace_id.clone(),
                owner: entry.value().key.owner.clone(),
                acquired_at: entry.value().acquired_at,
                expires_at: entry.value().expires_at,
                active: !entry.value().is_expired(Utc::now()),
            })
            .collect()
    }

    /// Release expired project leases and their underlying workspace leases.
    /// The caller can run this from a bounded daemon maintenance tick.
    pub fn evict_expired(&self, now: DateTime<Utc>) -> ActivationEvictionReport {
        let candidates: Vec<(ActivationKey, Arc<ActiveActivation>)> = self
            .active
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();
        let mut report = ActivationEvictionReport {
            evaluated: candidates.len(),
            ..Default::default()
        };
        for (key, activation) in candidates {
            if !activation.is_expired(now) {
                continue;
            }
            self.active
                .remove_if(&key, |_key, current| Arc::ptr_eq(current, &activation));
            activation.release_services();
            report.evicted_lease_ids.push(activation.lease_id.clone());
        }
        report
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationLeaseSnapshot {
    pub lease_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub owner: String,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub active: bool,
}

/// Bounded health state for one catalog activation layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Available,
    Stale,
    Unavailable,
    Contended,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthLayer {
    pub state: HealthState,
    pub code: Option<String>,
    pub message: Option<String>,
}

impl HealthLayer {
    pub fn available() -> Self {
        Self {
            state: HealthState::Available,
            code: None,
            message: None,
        }
    }

    pub fn with_issue(state: HealthState, code: &str, message: &str) -> Self {
        Self {
            state,
            code: Some(bounded_text(code)),
            message: Some(bounded_text(message)),
        }
    }
}

/// Bounded aggregate that deliberately contains no filesystem paths,
/// credentials, or asset bodies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectHealthSnapshot {
    pub project_id: String,
    pub workspace_id: String,
    pub overall: HealthState,
    pub catalog: HealthLayer,
    pub workspace: HealthLayer,
    pub assets: HealthLayer,
    pub services: HealthLayer,
    pub diagnostics: Vec<String>,
}

impl ProjectHealthSnapshot {
    pub fn catalog_status(&self) -> CatalogHealthStatus {
        match self.overall {
            HealthState::Available => CatalogHealthStatus::Available,
            HealthState::Stale | HealthState::Contended => CatalogHealthStatus::Stale,
            HealthState::Unavailable => CatalogHealthStatus::Unavailable,
            HealthState::Error => CatalogHealthStatus::Error,
        }
    }
}

/// Aggregate catalog, workspace, runtime-asset, and service health using a
/// fixed precedence order. The input is already bounded by its owning layer.
pub fn aggregate_health(
    project_id: &str,
    workspace_id: &str,
    catalog: HealthLayer,
    workspace: HealthLayer,
    services: HealthLayer,
    assets: HealthLayer,
) -> ProjectHealthSnapshot {
    let catalog = sanitize_layer(catalog);
    let workspace = sanitize_layer(workspace);
    let services = sanitize_layer(services);
    let assets = sanitize_layer(assets);
    let layers = [&catalog, &workspace, &assets, &services];
    let overall = [
        HealthState::Error,
        HealthState::Unavailable,
        HealthState::Contended,
        HealthState::Stale,
        HealthState::Available,
    ]
    .into_iter()
    .find(|candidate| layers.iter().any(|layer| layer.state == *candidate))
    .unwrap_or(HealthState::Unavailable);
    let mut diagnostics = Vec::new();
    for (name, layer) in [
        ("catalog", &catalog),
        ("workspace", &workspace),
        ("assets", &assets),
        ("services", &services),
    ] {
        if layer.state != HealthState::Available {
            if let Some(message) = layer.message.as_deref() {
                diagnostics.push(bounded_text(&format!("{name}: {message}")));
            } else {
                diagnostics.push(bounded_text(&format!("{name}: {:?}", layer.state)));
            }
        }
    }
    diagnostics.truncate(MAX_HEALTH_DIAGNOSTICS);
    ProjectHealthSnapshot {
        project_id: bounded_text(project_id),
        workspace_id: bounded_text(workspace_id),
        overall,
        catalog,
        workspace,
        assets,
        services,
        diagnostics,
    }
}

fn sanitize_layer(mut layer: HealthLayer) -> HealthLayer {
    layer.code = layer.code.as_deref().map(bounded_text);
    layer.message = layer.message.as_deref().map(bounded_text);
    layer
}

pub fn catalog_health_layer(status: Option<CatalogHealthStatus>) -> HealthLayer {
    match status.unwrap_or(CatalogHealthStatus::Available) {
        CatalogHealthStatus::Unknown => HealthLayer::with_issue(
            HealthState::Stale,
            "catalog_unknown",
            "catalog health is unknown",
        ),
        CatalogHealthStatus::Available => HealthLayer::available(),
        CatalogHealthStatus::Unavailable => HealthLayer::with_issue(
            HealthState::Unavailable,
            "catalog_unavailable",
            "catalog reports the project unavailable",
        ),
        CatalogHealthStatus::Unsupported => HealthLayer::with_issue(
            HealthState::Unavailable,
            "catalog_unsupported",
            "catalog reports the project unsupported",
        ),
        CatalogHealthStatus::Stale => HealthLayer::with_issue(
            HealthState::Stale,
            "catalog_stale",
            "catalog health is stale",
        ),
        CatalogHealthStatus::Error => HealthLayer::with_issue(
            HealthState::Error,
            "catalog_error",
            "catalog reports an error",
        ),
    }
}

pub fn workspace_health_layer(available: bool) -> HealthLayer {
    if available {
        HealthLayer::available()
    } else {
        HealthLayer::with_issue(
            HealthState::Unavailable,
            "workspace_unavailable",
            "workspace is not available",
        )
    }
}

pub fn service_health_layer(snapshot: Option<&WorkspaceServiceSnapshot>) -> HealthLayer {
    match snapshot {
        Some(snapshot) => {
            if snapshot.active_leases == 0 {
                HealthLayer::with_issue(
                    HealthState::Stale,
                    "service_idle",
                    "workspace service bundle is active but idle",
                )
            } else {
                HealthLayer::available()
            }
        }
        None => HealthLayer::with_issue(
            HealthState::Unavailable,
            "service_inactive",
            "workspace service bundle is not activated",
        ),
    }
}

pub fn asset_health_layer(status: &RefreshStatus) -> HealthLayer {
    if status.in_flight {
        return HealthLayer::with_issue(
            HealthState::Contended,
            "asset_refresh_in_flight",
            "runtime-asset refresh is in flight",
        );
    }
    match (status.generation, status.last_outcome) {
        (Some(_), Some(RefreshOutcome::Invalid)) => HealthLayer::with_issue(
            HealthState::Stale,
            "asset_refresh_invalid",
            "runtime-asset refresh was invalid; retained generation is in use",
        ),
        (Some(_), Some(RefreshOutcome::Retained)) => HealthLayer::with_issue(
            HealthState::Stale,
            "asset_refresh_retained",
            "runtime-asset refresh retained the previous generation",
        ),
        (Some(_), _) => HealthLayer::available(),
        (None, Some(RefreshOutcome::Invalid)) => HealthLayer::with_issue(
            HealthState::Unavailable,
            "asset_refresh_invalid",
            "runtime-asset refresh has no usable generation",
        ),
        (None, Some(RefreshOutcome::Cancelled)) => HealthLayer::with_issue(
            HealthState::Unavailable,
            "asset_refresh_cancelled",
            "runtime-asset refresh was cancelled before publication",
        ),
        (None, _) => HealthLayer::with_issue(
            HealthState::Unavailable,
            "asset_refresh_unavailable",
            "runtime-asset publication is not available",
        ),
    }
}

fn bounded_identifier(value: &str, field: &'static str) -> Result<String, ProjectActivationError> {
    if value.is_empty() {
        return Err(ProjectActivationError::InvalidInput {
            field,
            message: "must not be empty".to_string(),
        });
    }
    if value.len() > MAX_ACTIVATION_IDENTIFIER_LENGTH {
        return Err(ProjectActivationError::InvalidInput {
            field,
            message: format!("exceeds {MAX_ACTIVATION_IDENTIFIER_LENGTH} bytes"),
        });
    }
    if value.chars().any(char::is_control) {
        return Err(ProjectActivationError::InvalidInput {
            field,
            message: "contains control characters".to_string(),
        });
    }
    Ok(value.to_string())
}

fn bounded_text(value: &str) -> String {
    let bounded: String = value.chars().take(MAX_HEALTH_TEXT_LENGTH).collect();
    if bounded.contains('/') || bounded.contains('\\') {
        "diagnostic redacted".to_string()
    } else {
        bounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::asset_refresh::RefreshStatus;
    use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
    use codegg_core::workspace_services::{
        ProductionWorkspaceServicesFactory, WorkspaceServicePolicy,
    };

    fn refresh_status(
        generation: Option<u64>,
        outcome: Option<RefreshOutcome>,
        in_flight: bool,
    ) -> RefreshStatus {
        RefreshStatus {
            scope: crate::agent::asset_refresh::AssetScope::new("p", "w"),
            generation,
            fingerprint: None,
            last_success_at: None,
            in_flight,
            last_outcome: outcome,
            last_diagnostics: Vec::new(),
        }
    }

    #[test]
    fn health_precedence_is_bounded_and_diagnostic_free_of_paths() {
        let health = aggregate_health(
            "project",
            "workspace",
            HealthLayer::available(),
            HealthLayer::available(),
            HealthLayer::with_issue(HealthState::Unavailable, "service", "/secret/path"),
            asset_health_layer(&refresh_status(
                Some(1),
                Some(RefreshOutcome::Published),
                false,
            )),
        );
        assert_eq!(health.overall, HealthState::Unavailable);
        assert!(health
            .diagnostics
            .iter()
            .all(|value| !value.contains("/secret/path")));
    }

    #[test]
    fn asset_health_distinguishes_contention_and_retention() {
        assert_eq!(
            asset_health_layer(&refresh_status(None, None, true)).state,
            HealthState::Contended
        );
        assert_eq!(
            asset_health_layer(&refresh_status(
                Some(2),
                Some(RefreshOutcome::Retained),
                false
            ))
            .state,
            HealthState::Stale
        );
    }

    #[test]
    fn catalog_health_maps_contention_to_stale_for_durable_storage() {
        let health = aggregate_health(
            "p",
            "w",
            catalog_health_layer(Some(CatalogHealthStatus::Available)),
            HealthLayer::available(),
            HealthLayer::available(),
            HealthLayer::with_issue(HealthState::Contended, "busy", "refresh busy"),
        );
        assert_eq!(health.catalog_status(), CatalogHealthStatus::Stale);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn expiry_releases_workspace_service_and_invalidates_stale_handle() {
        let root = tempfile::tempdir().unwrap();
        let workspaces = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .unwrap();
        let workspace = workspaces.get_or_register(root.path()).await.unwrap();
        let services = WorkspaceServiceRegistry::new(
            workspaces,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let registry = ProjectActivationRegistry::new(
            services.clone(),
            ProjectActivationPolicy {
                lease_ttl: Duration::from_millis(1),
                max_active_leases: 2,
            },
        );
        let lease = registry
            .acquire("project", workspace.id.as_str(), "owner")
            .await
            .unwrap();
        let report = registry.evict_expired(lease.expires_at() + chrono::Duration::seconds(1));
        assert_eq!(report.evicted_lease_ids, vec![lease.lease_id().to_string()]);
        assert!(!lease.is_active());
        assert_eq!(services.peek(&workspace.id).unwrap().active_leases, 0);
        drop(lease);
        assert_eq!(registry.active_count(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn capacity_is_enforced_across_distinct_concurrent_keys() {
        let workspaces = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .unwrap();
        let mut workspace_ids = Vec::new();
        let mut roots = Vec::new();
        for _ in 0..8 {
            let root = tempfile::tempdir().unwrap();
            let workspace = workspaces.get_or_register(root.path()).await.unwrap();
            roots.push(root);
            workspace_ids.push(workspace.id.to_string());
        }
        let services = WorkspaceServiceRegistry::new(
            workspaces,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let registry = ProjectActivationRegistry::new(
            services,
            ProjectActivationPolicy {
                lease_ttl: Duration::from_secs(60),
                max_active_leases: 2,
            },
        );
        let barrier = Arc::new(tokio::sync::Barrier::new(workspace_ids.len()));
        let mut tasks = Vec::new();
        for (index, workspace_id) in workspace_ids.into_iter().enumerate() {
            let registry = registry.clone();
            let barrier = barrier.clone();
            tasks.push(tokio::spawn(async move {
                match registry
                    .acquire(
                        &format!("project-{index}"),
                        &workspace_id,
                        &format!("owner-{index}"),
                    )
                    .await
                {
                    Ok(lease) => {
                        let lease_id = lease.lease_id().to_string();
                        barrier.wait().await;
                        Ok(lease_id)
                    }
                    Err(error) => {
                        barrier.wait().await;
                        Err(error.to_string())
                    }
                }
            }));
        }
        let mut successes = 0;
        for task in tasks {
            if task.await.unwrap().is_ok() {
                successes += 1;
            }
        }
        assert!(successes <= 2, "capacity admitted {successes} leases");
        assert_eq!(registry.active_count(), 0);
    }
}
