//! Daemon-owned provider-connection provisioning.
//!
//! This module is intentionally a narrow vertical slice over the existing
//! encrypted credential store and provider-connection metadata. It owns the
//! cross-store sequence, bounded probe, cancellation registry, and redacted
//! protocol projections. Session/model selection remains elsewhere.
//!
//! The canonical service is the provider-neutral
//! [`ProviderConnectionProvisioner`] driven by
//! [`CreateProviderConnectionRequest`]: one pre-credential setup-catalog
//! definition selects the endpoint policy, credential contract, and probe
//! strategy, while Eggpool remains a named compatible-proxy preset.
//! [`EggpoolProvisioner`] is a compatibility alias over the same service.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use dashmap::DashMap;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use codegg_core::identity::{PrincipalId, ProjectId, ProviderConnectionId};
use codegg_core::provider_connections::{
    Endpoint, ProviderConnection, ProviderConnectionState, ProviderScope, SecretRef, TlsPolicy,
};
use codegg_protocol::provider::{
    ConnectionHealthDto, ConnectionProvisioningStatusDto, ConnectionRefreshStatusDto,
    ConnectionRotateChange, ConnectionRotateStatusDto, CreateEggpoolConnectionRequest,
    CreateEggpoolConnectionResult, CreateProviderConnectionRequest, EggpoolConnectionScope,
    EggpoolTlsPolicy, ProviderConnectionScope, ProviderConnectionSummaryDto,
    ProviderCredentialKind, ProviderModelDto, ProviderTlsPolicy, SecretInputRef,
};
use codegg_providers::qualification::{CatalogOutcome, CredentialVerification};
use codegg_providers::setup_catalog::{
    self, SetupEndpointPolicy, SetupProbeStrategy, AZURE_ID, CUSTOM_COMPATIBLE_ID,
    EGGPOOL_PRESET_ID,
};

const WORKFLOW_TIMEOUT: Duration = Duration::from_secs(20);
type RefreshCell = tokio::sync::OnceCell<Result<ConnectionRefreshStatusDto, RefreshError>>;
type RefreshFlights = DashMap<ProviderConnectionId, Arc<RefreshCell>>;

#[derive(Debug, Error)]
pub enum EggpoolError {
    #[error("invalid Eggpool endpoint: {0}")]
    InvalidEndpoint(String),
    #[error("invalid connection scope: {0}")]
    InvalidScope(String),
    #[error("unsupported provider '{0}'")]
    UnsupportedProvider(String),
    #[error("credential kind is not supported by this provider")]
    UnsupportedCredentialKind,
    #[error("credential store unavailable")]
    CredentialStore,
    #[error(
        "no usable master key for existing encrypted material; restore the historical CODEGG_MASTER_KEY"
    )]
    MasterKeyMissing,
    #[error("connection provisioning conflict")]
    Conflict,
    #[error("connection provisioning was cancelled")]
    Cancelled,
    #[error("Eggpool probe failed: {0:?}")]
    Probe(ProbeReason),
    #[error("connection persistence failed")]
    Storage,
    #[error("rotation failed: {0}")]
    Rotation(#[from] RotationError),
    #[error("refresh failed: {0}")]
    Refresh(#[from] RefreshError),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RotationError {
    #[error("connection not found")]
    NotFound,
    #[error("connection revision is stale")]
    StaleRevision,
    #[error("connection lifecycle state does not permit rotation")]
    InvalidState,
    #[error("rotation secret handle is unavailable")]
    SecretUnavailable,
    #[error("endpoint policy rejected")]
    EndpointPolicy,
    #[error("rotation probe failed")]
    ProbeFailed,
    #[error("credential was rejected by the provider")]
    CredentialRejected,
    #[error("rotation cancelled")]
    Cancelled,
    #[error("rotation persistence failed")]
    Storage,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RefreshError {
    #[error("connection not found")]
    NotFound,
    #[error("connection revision is stale")]
    StaleRevision,
    #[error("connection is disabled")]
    Disabled,
    #[error("credential is missing")]
    CredentialMissing,
    #[error("credential was rejected by the provider")]
    CredentialRejected,
    #[error("connection is tombstoned")]
    Tombstoned,
    #[error("refresh probe timed out")]
    Timeout,
    #[error("endpoint policy rejected")]
    EndpointPolicy,
    #[error("bounded provider response rejected")]
    BoundedBody,
    #[error("refresh was cancelled")]
    Cancelled,
    #[error("refresh is backing off after a recent failure")]
    Backoff,
    #[error("refresh persistence failed")]
    Storage,
    #[error("refresh failed")]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeReason {
    AuthenticationFailed,
    Unreachable,
    Timeout,
    TlsFailed,
    RedirectDisallowed,
    UnsupportedApi,
    InvalidJson,
    EmptyCatalog,
    CatalogOversized,
    Cancelled,
}

impl ProbeReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::AuthenticationFailed => "authentication_failed",
            Self::Unreachable => "endpoint_unreachable",
            Self::Timeout => "probe_timeout",
            Self::TlsFailed => "tls_failed",
            Self::RedirectDisallowed => "redirect_disallowed",
            Self::UnsupportedApi => "unsupported_api",
            Self::InvalidJson => "invalid_json",
            Self::EmptyCatalog => "empty_catalog",
            Self::CatalogOversized => "catalog_oversized",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone)]
struct NormalizedSpec {
    provider_id: String,
    provider_storage_key: String,
    endpoint: Endpoint,
    tls_policy: TlsPolicy,
    scope: ProviderScope,
    display_name: String,
    credential_kind: codegg_providers::CredentialKind,
    probe_strategy: SetupProbeStrategy,
}

#[derive(Debug, Clone)]
struct ProbedModel {
    id: String,
    name: String,
    context_window: u64,
    max_output_tokens: Option<u64>,
    supports_tools: bool,
    supports_vision: bool,
}

#[derive(Debug, Clone)]
struct ProbeResult {
    models: Vec<ProbedModel>,
    catalog_revision: String,
    duration_ms: u64,
    /// What this probe proved about the credential — see
    /// [`codegg_providers::qualification`]. Catalog reachability and
    /// credential verification are separate facts; a catalog probe that
    /// returns a usable model list only proves the catalog is reachable.
    credential: CredentialVerification,
}

struct RotationSecret {
    value: String,
    expires_at: Instant,
}

/// Crash-consistency: per-connection maps are ephemeral caches; the SQL
/// store is authoritative. Multi-map updates are not atomic — a crash
/// between them only drops cached status, which is rebuilt on demand.
///
/// This is the canonical provider-neutral provisioning service. The
/// `EggpoolProvisioner` alias below keeps historical call sites compiling;
/// new code must use this name and the generic
/// [`CreateProviderConnectionRequest`].
#[derive(Clone)]
pub struct ProviderConnectionProvisioner {
    pool: sqlx::SqlitePool,
    credential_store: Option<Arc<codegg_providers::CredentialStore>>,
    operations: Arc<DashMap<String, CancellationToken>>,
    refreshes: Arc<RefreshFlights>,
    refresh_statuses: Arc<DashMap<String, ConnectionRefreshStatusDto>>,
    rotation_secrets: Arc<DashMap<String, RotationSecret>>,
    rotation_statuses: Arc<DashMap<String, ConnectionRotateStatusDto>>,
    refresh_cap: Arc<Semaphore>,
    refresh_failures: Arc<DashMap<ProviderConnectionId, u32>>,
    refresh_next_at: Arc<DashMap<ProviderConnectionId, Instant>>,
    background_started: Arc<AtomicBool>,
    reconciled: Arc<AtomicBool>,
}

/// Compatibility alias for the historical Eggpool-specific service name.
/// New code must use [`ProviderConnectionProvisioner`].
pub type EggpoolProvisioner = ProviderConnectionProvisioner;
/// Provider-neutral name for the provisioning failure type.
pub type ProviderProvisionError = EggpoolError;

/// Identity of the connection currently being provisioned.
///
/// These four scalars are always threaded together through the provisioning
/// path and are meaningless apart from each other, so they travel as one
/// value rather than as a long positional argument list.
#[derive(Clone, Copy)]
struct ProvisioningTarget<'a> {
    operation_id: &'a str,
    connection_id: &'a ProviderConnectionId,
    secret_ref: &'a SecretRef,
    account_id: &'a str,
}

impl ProviderConnectionProvisioner {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        let credential_store = codegg_providers::CredentialStore::at_default_location()
            .ok()
            .map(Arc::new);
        Self::with_credential_store(pool, credential_store)
    }

    pub fn with_credential_store(
        pool: sqlx::SqlitePool,
        credential_store: Option<Arc<codegg_providers::CredentialStore>>,
    ) -> Self {
        Self {
            pool,
            credential_store,
            operations: Arc::new(DashMap::new()),
            refreshes: Arc::new(DashMap::new()),
            refresh_statuses: Arc::new(DashMap::new()),
            rotation_secrets: Arc::new(DashMap::new()),
            rotation_statuses: Arc::new(DashMap::new()),
            refresh_cap: Arc::new(Semaphore::new(
                codegg_config::schema::Config::load()
                    .ok()
                    .and_then(|config| config.provider_connections)
                    .map(|config| config.global_refresh_cap.max(1))
                    .unwrap_or(4),
            )),
            refresh_failures: Arc::new(DashMap::new()),
            refresh_next_at: Arc::new(DashMap::new()),
            background_started: Arc::new(AtomicBool::new(false)),
            reconciled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the optional bounded health refresh loop. Background provider
    /// I/O is disabled by default and is never part of daemon startup.
    pub fn start_background_refresh(self: &Arc<Self>) {
        let enabled = codegg_config::schema::Config::load()
            .ok()
            .and_then(|config| config.provider_connections)
            .map(|config| config.background_refresh)
            .unwrap_or(false);
        if !enabled || self.background_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let provisioner = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                let store = codegg_core::provider_connections::ProviderConnectionStore::new(
                    provisioner.pool.clone(),
                );
                let Ok(connections) = store.list().await else {
                    continue;
                };
                for connection in connections {
                    if connection.state != ProviderConnectionState::Active {
                        continue;
                    }
                    if let Err(error) = provisioner
                        .refresh(&connection.id, connection.revision)
                        .await
                    {
                        tracing::warn!(?error, connection_id = %connection.id, "eggpool background refresh failed");
                    }
                }
            }
        });
    }

    /// Compatibility adapter: the legacy Eggpool-named request runs through
    /// the same generic provisioning service as every other provider.
    pub async fn create(
        &self,
        request: CreateEggpoolConnectionRequest,
    ) -> Result<CreateEggpoolConnectionResult, EggpoolError> {
        self.create_connection(CreateProviderConnectionRequest::from(request))
            .await
    }

    /// Provision one durable provider connection for any setup-catalog
    /// provider through the shared sequence:
    ///
    /// validate/normalize → staged journal → operation-owned protected
    /// credential write → bounded probe/model discovery → one final
    /// transaction publishing connection, health, catalog, and committed
    /// provisioning state.
    ///
    /// No network I/O happens inside the final SQLite transaction. Failures
    /// map to redacted stable codes; secrets never reach SQLite, logs, or
    /// protocol responses.
    pub async fn create_connection(
        &self,
        request: CreateProviderConnectionRequest,
    ) -> Result<CreateEggpoolConnectionResult, EggpoolError> {
        self.reconcile_once().await;
        let spec = normalize_generic(&request)?;
        let operation_id = request
            .operation_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("prov-{}", uuid::Uuid::new_v4()));
        let connection_id = ProviderConnectionId::new();
        let account_id = connection_id.as_str().to_owned();
        let secret_ref = SecretRef::new();
        let idempotency_key = idempotency_key(&spec);
        let scope_parts = storage_scope(&spec.scope);

        // Replacement, not refusal.
        //
        // Re-running `/connect` for a provider that is already connected is the
        // normal way to rotate its credential (a new key, a changed plan), so
        // refusing with `Conflict` made the surface unusable: the user had to
        // discover `/connections`, tombstone the connection by hand, and only
        // then could they re-add the very provider they were trying to update.
        //
        // The existing active connection is recorded here and carried through
        // provisioning. It is *not* tombstoned up front: the new credential is
        // probed first, and only a successful probe swaps the connection. A bad
        // key therefore fails without destroying a working configuration.
        let superseded = sqlx::query_as::<_, (String, i64)>(
            "SELECT id, revision FROM provider_connections WHERE provider_kind = ? AND endpoint = ? AND tls_policy = ? AND scope_kind = ? AND scope_ref = ? AND state = 'active'",
        )
        .bind(&spec.provider_storage_key)
        .bind(spec.endpoint.as_str())
        .bind(tls_key(spec.tls_policy))
        .bind(scope_parts.0)
        .bind(scope_parts.1)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| EggpoolError::Storage)?;
        if let Some((existing_id, _)) = superseded.as_ref() {
            tracing::info!(
                %existing_id,
                provider = %spec.provider_id,
                "replacing an existing active connection for this provider"
            );
        }

        // In-flight duplicate guard. This deliberately counts only *non-terminal*
        // states: a 'committed' row records that provisioning succeeded, not
        // that the provider is unavailable. Counting 'committed' here made a
        // permanently-unique `idempotency_key` (see the table constraint) block
        // every future /connect for a provider the user had ever connected, so
        // the second attempt at any provider failed with `connection_conflict`.
        // Duplicate detection for an existing connection is the active-row
        // query above; this guard only rejects a genuinely concurrent attempt.
        if sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM provider_provisioning WHERE idempotency_key = ? AND state IN ('staged', 'probing')",
        )
        .bind(&idempotency_key)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| EggpoolError::Storage)?
            > 0
        {
            return Err(EggpoolError::Conflict);
        }

        // `provider_provisioning.idempotency_key` is UNIQUE for the lifetime of
        // the row, so a terminal row from an earlier attempt (committed, failed,
        // or cancelled) would make this INSERT fail — and that failure was
        // reported as `Conflict`, turning ordinary retry-after-disconnect into a
        // permanent dead end. Retire the superseded terminal rows first so the
        // new operation owns the key.
        if let Err(error) = sqlx::query(
            "DELETE FROM provider_provisioning WHERE idempotency_key = ? AND state IN ('committed', 'failed', 'cancelled')",
        )
        .bind(&idempotency_key)
        .execute(&self.pool)
        .await
        {
            tracing::warn!(?error, "eggpool superseded provisioning cleanup failed");
            return Err(EggpoolError::Storage);
        }

        let now = now_millis();
        sqlx::query(
            "INSERT INTO provider_provisioning (operation_id, connection_id, idempotency_key, provider_kind, display_name, endpoint, tls_policy, scope_kind, scope_ref, secret_ref, secret_provider_ref, secret_account_ref, state, time_created, time_updated) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'staged', ?, ?)",
        )
        .bind(&operation_id)
        .bind(connection_id.as_str())
        .bind(&idempotency_key)
        .bind(&spec.provider_storage_key)
        .bind(&spec.display_name)
        .bind(spec.endpoint.as_str())
        .bind(tls_key(spec.tls_policy))
        .bind(scope_parts.0)
        .bind(scope_parts.1)
        .bind(secret_ref.as_str())
        .bind(&spec.provider_id)
        .bind(&account_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|_| EggpoolError::Conflict)?;

        let cancel = CancellationToken::new();
        let store =
            codegg_core::provider_connections::ProviderConnectionStore::new(self.pool.clone());
        store
            .add_reference(
                &connection_id,
                codegg_core::provider_connections::ProviderConnectionReferenceKind::ProvisioningOperation,
                &operation_id,
            )
            .await
            .map_err(|_| EggpoolError::Storage)?;
        self.operations.insert(operation_id.clone(), cancel.clone());
        let result = self
            .create_inner(
                request.credential.expose(),
                &spec,
                ProvisioningTarget {
                    operation_id: &operation_id,
                    connection_id: &connection_id,
                    secret_ref: &secret_ref,
                    account_id: &account_id,
                },
                superseded,
                cancel,
            )
            .await;
        self.operations.remove(&operation_id);
        if let Err(error) = store
            .remove_reference(
                &connection_id,
                codegg_core::provider_connections::ProviderConnectionReferenceKind::ProvisioningOperation,
                &operation_id,
            )
            .await
        {
            tracing::warn!(?error, %operation_id, "eggpool provisioning reference cleanup failed");
        }
        result
    }

    async fn create_inner(
        &self,
        secret: &str,
        spec: &NormalizedSpec,
        target: ProvisioningTarget<'_>,
        superseded: Option<(String, i64)>,
        cancel: CancellationToken,
    ) -> Result<CreateEggpoolConnectionResult, EggpoolError> {
        let ProvisioningTarget {
            operation_id,
            connection_id,
            secret_ref,
            account_id,
        } = target;
        let store = self
            .credential_store
            .clone()
            .ok_or(EggpoolError::CredentialStore)?;
        // Credential write uses the canonical create-on-write resolver: a
        // fresh local store bootstraps the managed key, while pre-existing
        // encrypted material without a usable key fails closed with
        // `MasterKeyMissing` (never a silent replacement key). The write is
        // operation-owned: compensation below removes exactly this binding.
        if let Err(error) = store.put(
            &spec.provider_id,
            Some(account_id),
            spec.credential_kind,
            secret,
            None,
            Vec::new(),
        ) {
            self.fail(operation_id, ProbeReason::AuthenticationFailed.code())
                .await;
            return Err(match error {
                codegg_providers::AuthError::MasterKeyMissing => EggpoolError::MasterKeyMissing,
                _ => EggpoolError::CredentialStore,
            });
        }

        if cancel.is_cancelled() {
            self.compensate(operation_id, &spec.provider_id, account_id, &store, None)
                .await;
            return Err(EggpoolError::Cancelled);
        }
        if sqlx::query(
            "UPDATE provider_provisioning SET state = 'probing', time_updated = ? WHERE operation_id = ?",
        )
            .bind(now_millis())
            .bind(operation_id)
            .execute(&self.pool)
            .await
            .is_err()
        {
            self.compensate(
                operation_id,
                &spec.provider_id,
                account_id,
                &store,
                Some("connection_storage_error"),
            )
            .await;
            return Err(EggpoolError::Storage);
        }

        // Bounded validation/model discovery selected by the provider
        // definition. Both strategies run outside the final transaction.
        let probe = match spec.probe_strategy {
            SetupProbeStrategy::AuthenticatedCompatibleCatalog => tokio::time::timeout(
                WORKFLOW_TIMEOUT,
                probe(
                    spec.endpoint.as_str(),
                    secret,
                    Some(spec.provider_id.as_str()),
                    cancel.clone(),
                ),
            )
            .await
            .map_err(|_| EggpoolError::Probe(ProbeReason::Timeout))?
            .map_err(EggpoolError::Probe),
            SetupProbeStrategy::ProviderCatalog => {
                probe_provider_catalog(spec, secret, cancel.clone())
                    .await
                    .map_err(EggpoolError::Probe)
            }
        };
        let probe = match probe {
            Ok(value) => value,
            Err(EggpoolError::Probe(ProbeReason::Cancelled)) => {
                self.compensate(operation_id, &spec.provider_id, account_id, &store, None)
                    .await;
                return Err(EggpoolError::Cancelled);
            }
            Err(error) => {
                self.compensate(
                    operation_id,
                    &spec.provider_id,
                    account_id,
                    &store,
                    Some(error_code(&error)),
                )
                .await;
                return Err(error);
            }
        };

        if cancel.is_cancelled() {
            self.compensate(operation_id, &spec.provider_id, account_id, &store, None)
                .await;
            return Err(EggpoolError::Cancelled);
        }

        let result = self
            .finalize(
                operation_id,
                connection_id,
                secret_ref,
                account_id,
                spec,
                superseded.as_ref(),
                &probe,
            )
            .await;
        if result.is_err() {
            self.compensate(
                operation_id,
                &spec.provider_id,
                account_id,
                &store,
                Some("connection_storage_error"),
            )
            .await;
        }
        result
    }

    async fn finalize(
        &self,
        operation_id: &str,
        connection_id: &ProviderConnectionId,
        secret_ref: &SecretRef,
        account_id: &str,
        spec: &NormalizedSpec,
        superseded: Option<&(String, i64)>,
        probe: &ProbeResult,
    ) -> Result<CreateEggpoolConnectionResult, EggpoolError> {
        let mut tx = self.pool.begin().await.map_err(|_| EggpoolError::Storage)?;
        let (scope_kind, scope_ref) = storage_scope(&spec.scope);

        // Retire the connection this one replaces, inside the same transaction
        // that publishes the replacement. Doing it here (rather than before the
        // probe) is what makes replacement safe: if the probe fails we never
        // reach this point, so a bad key leaves the previous connection intact
        // and selectable. Tombstoning preserves the old row's identity, audit
        // history and references for diagnostics, matching `/connections` `d`.
        if let Some((existing_id, _)) = superseded {
            let now = now_millis();
            let retired = sqlx::query(
                "UPDATE provider_connections SET state = 'disabled', revision = revision + 1, time_updated = ? WHERE id = ? AND state = 'active'",
            )
            .bind(now)
            .bind(existing_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| EggpoolError::Storage)?;
            if retired.rows_affected() == 1 {
                let revision: i64 =
                    sqlx::query_scalar("SELECT revision FROM provider_connections WHERE id = ?")
                        .bind(existing_id)
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(|_| EggpoolError::Storage)?;
                sqlx::query(
                    "INSERT INTO provider_connection_lifecycle (connection_id, state, revision, time_updated) VALUES (?, 'tombstoned', ?, ?) ON CONFLICT(connection_id) DO UPDATE SET state = 'tombstoned', revision = excluded.revision, time_updated = excluded.time_updated",
                )
                .bind(existing_id)
                .bind(revision)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|_| EggpoolError::Storage)?;
                let catalog_revision: Option<String> = sqlx::query_scalar(
                    "SELECT catalog_revision FROM provider_connection_health WHERE connection_id = ?",
                )
                .bind(existing_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|_| EggpoolError::Storage)?
                .flatten();
                sqlx::query(
                    "INSERT INTO provider_connection_tombstones (connection_id, tombstoned_at, tombstoned_by_actor, last_known_revision, last_known_catalog_revision, last_known_endpoint_authority) VALUES (?, ?, 'local_operator', ?, ?, ?) ON CONFLICT(connection_id) DO UPDATE SET tombstoned_at = excluded.tombstoned_at, last_known_revision = excluded.last_known_revision, last_known_catalog_revision = excluded.last_known_catalog_revision, last_known_endpoint_authority = excluded.last_known_endpoint_authority",
                )
                .bind(existing_id)
                .bind(now)
                .bind(revision)
                .bind(catalog_revision)
                .bind(spec.endpoint.as_str())
                .execute(&mut *tx)
                .await
                .map_err(|_| EggpoolError::Storage)?;
                sqlx::query(
                    "INSERT INTO provider_connection_audit_events (event_id, connection_id, action, actor_seam, old_revision, new_revision, endpoint_authority, outcome, time_created) VALUES (?, ?, 'replace:superseded', 'local_operator', ?, ?, ?, 'committed', ?)",
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(existing_id)
                .bind(revision - 1)
                .bind(revision)
                .bind(spec.endpoint.as_str())
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|_| EggpoolError::Storage)?;
            }
        }
        sqlx::query(
            "INSERT INTO provider_connections (id, provider_kind, display_name, endpoint, tls_policy, scope_kind, scope_ref, secret_ref, secret_provider_ref, secret_account_ref, state, revision, time_created, time_updated) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', 1, ?, ?)",
        )
        .bind(connection_id.as_str())
        .bind(&spec.provider_storage_key)
        .bind(&spec.display_name)
        .bind(spec.endpoint.as_str())
        .bind(tls_key(spec.tls_policy))
        .bind(scope_kind)
        .bind(scope_ref)
        .bind(secret_ref.as_str())
        .bind(&spec.provider_id)
        .bind(account_id)
        .bind(now_millis())
        .bind(now_millis())
        .execute(&mut *tx)
        .await
        .map_err(|_| EggpoolError::Conflict)?;
        sqlx::query("INSERT INTO provider_connection_health (connection_id, revision, status, credential_status, duration_ms, checked_at, catalog_revision) VALUES (?, 1, 'healthy', ?, ?, ?, ?)")
            .bind(connection_id.as_str())
            .bind(probe.credential.code())
            .bind(probe.duration_ms as i64)
            .bind(now_millis())
            .bind(&probe.catalog_revision)
            .execute(&mut *tx)
            .await
            .map_err(|_| EggpoolError::Storage)?;
        for model in &probe.models {
            sqlx::query("INSERT INTO provider_connection_models (connection_id, revision, model_id, model_name, context_window, max_output_tokens, supports_tools, supports_vision) VALUES (?, 1, ?, ?, ?, ?, ?, ?)")
                .bind(connection_id.as_str())
                .bind(&model.id)
                .bind(&model.name)
                .bind(model.context_window as i64)
                .bind(model.max_output_tokens.map(|v| v as i64))
                .bind(i64::from(model.supports_tools))
                .bind(i64::from(model.supports_vision))
                .execute(&mut *tx)
                .await
                .map_err(|_| EggpoolError::Storage)?;
        }
        sqlx::query("UPDATE provider_provisioning SET state = 'committed', time_updated = ? WHERE operation_id = ? AND state = 'probing'")
            .bind(now_millis())
            .bind(operation_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| EggpoolError::Storage)?;
        tx.commit().await.map_err(|_| EggpoolError::Storage)?;

        // Drop the superseded connection's credential binding now that the
        // replacement is durable. Leaving it behind would leave two records
        // for one provider, and provider resolution picks a single record per
        // provider — the replaced (stale) key could win and silently undo the
        // rotation the user just performed.
        if let Some((existing_id, _)) = superseded {
            let previous_binding: Option<(String, String)> = sqlx::query_as(
                "SELECT secret_provider_ref, secret_account_ref FROM provider_connections WHERE id = ?",
            )
            .bind(existing_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| EggpoolError::Storage)?;
            if let Some((provider_ref, account_ref)) = previous_binding {
                if let Some(store) = self.credential_store.clone() {
                    self.prune_superseded_credential(
                        &store,
                        &provider_ref,
                        &account_ref,
                        Some(existing_id),
                        Some(account_id),
                    )
                    .await;
                }
            }
        }

        let connection =
            codegg_core::provider_connections::ProviderConnectionStore::new(self.pool.clone())
                .get(connection_id)
                .await
                .map_err(|_| EggpoolError::Storage)?
                .ok_or(EggpoolError::Storage)?;
        Ok(CreateEggpoolConnectionResult {
            operation_id: operation_id.to_owned(),
            connection: self
                .summary(&connection, Some(&probe.catalog_revision), Some(probe))
                .await?,
            models: probe.models.iter().map(model_dto).collect(),
            catalog_revision: probe.catalog_revision.clone(),
        })
    }

    async fn compensate(
        &self,
        operation_id: &str,
        provider_ref: &str,
        account_id: &str,
        store: &codegg_providers::CredentialStore,
        failure_code: Option<&str>,
    ) {
        if let Err(error) = store.remove(provider_ref, Some(account_id)) {
            tracing::warn!(?error, %operation_id, "eggpool compensate credential removal failed");
        }
        let (state, code) =
            failure_code.map_or(("cancelled", "cancelled"), |code| ("failed", code));
        if let Err(error) = sqlx::query("UPDATE provider_provisioning SET state = ?, failure_code = ?, time_updated = ? WHERE operation_id = ? AND state <> 'committed'")
            .bind(state)
            .bind(code)
            .bind(now_millis())
            .bind(operation_id)
            .execute(&self.pool)
            .await
        {
            tracing::error!(?error, %operation_id, "eggpool compensate state transition failed");
        }
    }

    async fn fail(&self, operation_id: &str, code: &str) {
        if let Err(error) = sqlx::query("UPDATE provider_provisioning SET state = 'failed', failure_code = ?, time_updated = ? WHERE operation_id = ?")
            .bind(code)
            .bind(now_millis())
            .bind(operation_id)
            .execute(&self.pool)
            .await
        {
            tracing::error!(?error, %operation_id, "eggpool fail state transition failed");
        }
    }

    pub fn cancel(&self, operation_id: &str) -> bool {
        if let Some(token) = self.operations.get(operation_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub async fn status(
        &self,
        operation_id: &str,
    ) -> Result<ConnectionProvisioningStatusDto, EggpoolError> {
        self.reconcile_once().await;
        sqlx::query_as::<_, (String, String, String, Option<String>)>(
            "SELECT operation_id, state, connection_id, failure_code FROM provider_provisioning WHERE operation_id = ?",
        )
        .bind(operation_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| EggpoolError::Storage)?
        .map(|(operation_id, state, connection_id, reason_code)| ConnectionProvisioningStatusDto {
            operation_id,
            state,
            connection_id: Some(connection_id),
            reason_code,
        })
        .ok_or(EggpoolError::Storage)
    }

    pub async fn list(&self) -> Result<Vec<ProviderConnectionSummaryDto>, EggpoolError> {
        let store =
            codegg_core::provider_connections::ProviderConnectionStore::new(self.pool.clone());
        let connections = store.list().await.map_err(|_| EggpoolError::Storage)?;
        let mut result = Vec::with_capacity(connections.len());
        for connection in &connections {
            result.push(self.summary(connection, None, None).await?);
        }
        Ok(result)
    }

    pub async fn models(
        &self,
        connection_id: &ProviderConnectionId,
    ) -> Result<(Option<String>, Vec<ProviderModelDto>), EggpoolError> {
        let revision: i64 =
            sqlx::query_scalar("SELECT revision FROM provider_connections WHERE id = ?")
                .bind(connection_id.as_str())
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| EggpoolError::Storage)?
                .ok_or(EggpoolError::Storage)?;
        let catalog_revision = sqlx::query_scalar::<_, Option<String>>(
            "SELECT catalog_revision FROM provider_connection_health WHERE connection_id = ? AND revision = ?",
        )
        .bind(connection_id.as_str())
        .bind(revision)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| EggpoolError::Storage)?
        .flatten();
        let rows = sqlx::query_as::<_, (String, String, i64, Option<i64>, i64, i64)>(
            "SELECT model_id, model_name, context_window, max_output_tokens, supports_tools, supports_vision FROM provider_connection_models WHERE connection_id = ? AND revision = ? ORDER BY model_id",
        )
        .bind(connection_id.as_str())
        .bind(revision)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| EggpoolError::Storage)?;
        Ok((
            catalog_revision,
            rows.into_iter()
                .map(|(id, name, context, max, tools, vision)| ProviderModelDto {
                    id,
                    name,
                    context_window: context as u64,
                    max_output_tokens: max.map(|v| v as u64),
                    supports_tools: tools != 0,
                    supports_vision: vision != 0,
                })
                .collect(),
        ))
    }

    /// Register a bounded local secret for a rotation request. The handle is
    /// the only value that enters the protocol; the plaintext remains in this
    /// daemon-owned map until the rotation commits or fails.
    pub fn register_rotation_secret(&self, secret: SecretInputRef, value: String) -> bool {
        if value.is_empty()
            || value.len() > codegg_protocol::provider::SecretInput::MAX_LEN
            || value.chars().any(char::is_control)
        {
            return false;
        }
        let now = Instant::now();
        self.rotation_secrets
            .retain(|_, entry| entry.expires_at > now);
        if self.rotation_secrets.len() >= 32 {
            return false;
        }
        self.rotation_secrets.insert(
            secret.handle,
            RotationSecret {
                value,
                expires_at: now + Duration::from_secs(300),
            },
        );
        true
    }

    pub async fn rotate(
        &self,
        request_id: &str,
        connection_id: &ProviderConnectionId,
        expected_revision: u64,
        change: ConnectionRotateChange,
        secret: SecretInputRef,
        delete_previous_on_commit: bool,
    ) -> Result<ConnectionRotateStatusDto, RotationError> {
        let cancel = CancellationToken::new();
        self.operations
            .insert(request_id.to_owned(), cancel.clone());
        self.rotation_statuses.insert(
            request_id.to_owned(),
            ConnectionRotateStatusDto {
                request_id: request_id.to_owned(),
                connection_id: connection_id.to_string(),
                state: "probing".to_owned(),
                new_revision: None,
                catalog_revision: None,
                error_code: None,
            },
        );
        let result = self
            .rotate_inner(
                request_id,
                connection_id,
                expected_revision,
                change,
                secret,
                delete_previous_on_commit,
                cancel,
            )
            .await;
        self.operations.remove(request_id);
        match &result {
            Ok(status) => {
                self.rotation_statuses
                    .insert(request_id.to_owned(), status.clone());
            }
            Err(error) => {
                self.rotation_statuses.insert(
                    request_id.to_owned(),
                    ConnectionRotateStatusDto {
                        request_id: request_id.to_owned(),
                        connection_id: connection_id.to_string(),
                        state: if matches!(error, RotationError::Cancelled) {
                            "cancelled"
                        } else {
                            "failed"
                        }
                        .to_owned(),
                        new_revision: None,
                        catalog_revision: None,
                        error_code: Some(rotation_error_code(error).to_owned()),
                    },
                );
            }
        }
        result
    }

    async fn rotate_inner(
        &self,
        request_id: &str,
        connection_id: &ProviderConnectionId,
        expected_revision: u64,
        change: ConnectionRotateChange,
        secret: SecretInputRef,
        delete_previous_on_commit: bool,
        cancel: CancellationToken,
    ) -> Result<ConnectionRotateStatusDto, RotationError> {
        let result = self
            .rotate_transaction(
                request_id,
                connection_id,
                expected_revision,
                change,
                secret,
                delete_previous_on_commit,
                cancel,
            )
            .await;
        if let Err(error) = sqlx::query(
            "DELETE FROM provider_connection_lifecycle WHERE connection_id = ? AND state = 'provisioning_rotating'",
        )
        .bind(connection_id.as_str())
        .execute(&self.pool)
        .await
        {
            tracing::warn!(?error, connection_id = %connection_id, "eggpool rotate lifecycle cleanup failed");
        }
        result
    }

    async fn rotate_transaction(
        &self,
        request_id: &str,
        connection_id: &ProviderConnectionId,
        expected_revision: u64,
        change: ConnectionRotateChange,
        secret: SecretInputRef,
        delete_previous_on_commit: bool,
        cancel: CancellationToken,
    ) -> Result<ConnectionRotateStatusDto, RotationError> {
        let store =
            codegg_core::provider_connections::ProviderConnectionStore::new(self.pool.clone());
        let current = store
            .get(connection_id)
            .await
            .map_err(|_| RotationError::Storage)?
            .ok_or(RotationError::NotFound)?;
        if current.revision != expected_revision {
            return Err(RotationError::StaleRevision);
        }
        if !matches!(current.state, ProviderConnectionState::Active) {
            return Err(RotationError::InvalidState);
        }
        sqlx::query(
            "INSERT INTO provider_connection_lifecycle (connection_id, state, revision, time_updated) VALUES (?, 'provisioning_rotating', ?, ?) ON CONFLICT(connection_id) DO UPDATE SET state = excluded.state, revision = excluded.revision, time_updated = excluded.time_updated",
        )
        .bind(connection_id.as_str())
        .bind(expected_revision as i64)
        .bind(now_millis())
        .execute(&self.pool)
        .await
        .map_err(|_| RotationError::Storage)?;

        let changes_credential = matches!(
            change,
            ConnectionRotateChange::CredentialOnly
                | ConnectionRotateChange::CredentialAndEndpoint { .. }
        );
        if !changes_credential {
            let _ = self.rotation_secrets.remove(&secret.handle);
        }
        let old_binding = current.secret_binding.clone();
        let staged_secret = if changes_credential {
            Some(
                self.rotation_secrets
                    .remove(&secret.handle)
                    .filter(|(_, value)| value.expires_at > Instant::now())
                    .map(|(_, value)| value.value)
                    .ok_or(RotationError::SecretUnavailable)?,
            )
        } else {
            None
        };
        let (endpoint, tls_policy, display_name) = match &change {
            ConnectionRotateChange::CredentialOnly => (
                current.endpoint.clone(),
                current.tls_policy,
                current.display_name.clone(),
            ),
            ConnectionRotateChange::EndpointOnly {
                endpoint,
                tls_policy,
                display_name,
            }
            | ConnectionRotateChange::CredentialAndEndpoint {
                endpoint,
                tls_policy,
                display_name,
            } => {
                let policy = parse_tls_policy(tls_policy).ok_or(RotationError::EndpointPolicy)?;
                let endpoint =
                    Endpoint::new(endpoint, policy).map_err(|_| RotationError::EndpointPolicy)?;
                (
                    endpoint,
                    policy,
                    display_name
                        .clone()
                        .unwrap_or_else(|| current.display_name.clone()),
                )
            }
        };

        let credential_store = self
            .credential_store
            .clone()
            .ok_or(RotationError::Storage)?;
        // Staged rotation credentials live under the connection's own
        // provider namespace (Eggpool rows keep `eggpool`, so historical
        // behavior is unchanged while catalog rows stay coherent).
        let provider_ref = old_binding
            .as_ref()
            .map(|binding| binding.provider_ref.clone())
            .unwrap_or_else(|| EGGPOOL_PRESET_ID.to_string());
        let (binding, probe_secret) = if let Some(value) = staged_secret.as_deref() {
            let account = format!("rot-{}", uuid::Uuid::new_v4());
            let secret_ref = SecretRef::new();
            credential_store
                .put(
                    &provider_ref,
                    Some(&account),
                    codegg_providers::CredentialKind::ApiKey,
                    value,
                    None,
                    Vec::new(),
                )
                .map_err(|_| RotationError::Storage)?;
            (
                codegg_core::provider_connections::SecretBindingLocator::new(
                    secret_ref,
                    &provider_ref,
                    &account,
                )
                .map_err(|_| RotationError::Storage)?,
                value.to_owned(),
            )
        } else {
            let binding = old_binding
                .clone()
                .ok_or(RotationError::SecretUnavailable)?;
            let value = credential_store
                .get_plaintext(&provider_ref, Some(&binding.account_ref), |_| true)
                .map_err(|_| RotationError::Storage)?
                .ok_or(RotationError::SecretUnavailable)?;
            (binding, value)
        };
        let probe_result = tokio::time::timeout(
            WORKFLOW_TIMEOUT,
            probe(
                endpoint.as_str(),
                &probe_secret,
                Some(current.provider_kind.as_str()),
                cancel,
            ),
        )
        .await
        .map_err(|_| RotationError::ProbeFailed)?
        .map_err(|error| match error {
            ProbeReason::Cancelled => RotationError::Cancelled,
            // A real completion rejected the staged key. Report it as a
            // credential rejection rather than a generic probe failure: the
            // rotation never commits, the staged secret is dropped, and the
            // previously bound credential keeps its own durable verdict.
            ProbeReason::AuthenticationFailed => RotationError::CredentialRejected,
            _ => RotationError::ProbeFailed,
        });
        let probe_result = match probe_result {
            Ok(result) => result,
            Err(error) => {
                if staged_secret.is_some() {
                    if let Err(error) =
                        credential_store.remove(&provider_ref, Some(&binding.account_ref))
                    {
                        tracing::warn!(?error, "eggpool staged credential cleanup failed");
                    }
                }
                return Err(error);
            }
        };

        let new_revision = current.revision.saturating_add(1);
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| RotationError::Storage)?;
        let update = sqlx::query(
            "UPDATE provider_connections SET endpoint = ?, tls_policy = ?, display_name = ?, secret_ref = ?, secret_provider_ref = ?, secret_account_ref = ?, state = 'active', revision = ?, time_updated = ? WHERE id = ? AND revision = ?",
        )
        .bind(endpoint.as_str())
        .bind(tls_key(tls_policy))
        .bind(&display_name)
        .bind(binding.secret_ref.as_str())
        .bind(&binding.provider_ref)
        .bind(&binding.account_ref)
        .bind(new_revision as i64)
        .bind(now_millis())
        .bind(connection_id.as_str())
        .bind(expected_revision as i64)
        .execute(&mut *tx)
        .await
        .map_err(|_| RotationError::Storage)?;
        if update.rows_affected() != 1 {
            drop(tx);
            if staged_secret.is_some() {
                let _ = credential_store.remove(&provider_ref, Some(&binding.account_ref));
            }
            return Err(RotationError::StaleRevision);
        }
        let old_catalog = sqlx::query_scalar::<_, Option<String>>(
            "SELECT catalog_revision FROM provider_connection_health WHERE connection_id = ?",
        )
        .bind(connection_id.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| RotationError::Storage)?
        .flatten();
        // The copy shortcut is only valid when the previous revision actually
        // holds the catalog to copy. Matching digests alone are not enough: if
        // the catalog was ever stranded behind a revision bump, `old_catalog`
        // can still equal the probe digest (the upstream catalog genuinely did
        // not change) while `expected_revision` holds no rows at all. The copy
        // then inserts nothing, the catalog stays empty at the new revision,
        // and no later rotate or refresh can ever rebuild it — `/connections`
        // reports zero models forever. Observed live on a connection at
        // revision 16 whose models were stranded at revisions 1-3, through a
        // rotate that reported success.
        let previous_catalog_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM provider_connection_models WHERE connection_id = ? AND revision = ?",
        )
        .bind(connection_id.as_str())
        .bind(expected_revision as i64)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| RotationError::Storage)?;
        let catalog_unchanged = previous_catalog_rows > 0
            && old_catalog.as_deref() == Some(probe_result.catalog_revision.as_str());
        if catalog_unchanged {
            sqlx::query(
                "INSERT INTO provider_connection_models (connection_id, revision, model_id, model_name, context_window, max_output_tokens, supports_tools, supports_vision) SELECT connection_id, ?, model_id, model_name, context_window, max_output_tokens, supports_tools, supports_vision FROM provider_connection_models WHERE connection_id = ? AND revision = ?",
            )
            .bind(new_revision as i64)
            .bind(connection_id.as_str())
            .bind(expected_revision as i64)
            .execute(&mut *tx)
            .await
            .map_err(|_| RotationError::Storage)?;
        } else {
            for model in &probe_result.models {
                sqlx::query("INSERT INTO provider_connection_models (connection_id, revision, model_id, model_name, context_window, max_output_tokens, supports_tools, supports_vision) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                    .bind(connection_id.as_str())
                    .bind(new_revision as i64)
                    .bind(&model.id)
                    .bind(&model.name)
                    .bind(model.context_window as i64)
                    .bind(model.max_output_tokens.map(|v| v as i64))
                    .bind(i64::from(model.supports_tools))
                    .bind(i64::from(model.supports_vision))
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| RotationError::Storage)?;
            }
        }
        // A rotation/refresh installs a new revision, so the credential verdict is
        // re-derived from *this* probe rather than inherited: the previous
        // revision's verdict described a different credential.
        sqlx::query("UPDATE provider_connection_health SET revision = ?, status = 'healthy', credential_status = ?, reason_code = NULL, duration_ms = ?, checked_at = ?, catalog_revision = ? WHERE connection_id = ?")
            .bind(new_revision as i64)
            .bind(probe_result.credential.code())
            .bind(probe_result.duration_ms as i64)
            .bind(now_millis())
            .bind(&probe_result.catalog_revision)
            .bind(connection_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|_| RotationError::Storage)?;
        sqlx::query(
            "DELETE FROM provider_connection_lifecycle WHERE connection_id = ? AND state = 'provisioning_rotating'",
        )
        .bind(connection_id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|_| RotationError::Storage)?;
        sqlx::query(
            "INSERT INTO provider_connection_audit_events (event_id, connection_id, action, actor_seam, old_revision, new_revision, endpoint_authority, outcome, duration_ms, time_created) VALUES (?, ?, 'rotate', 'local_operator', ?, ?, ?, 'committed', ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(connection_id.as_str())
        .bind(expected_revision as i64)
        .bind(new_revision as i64)
        .bind(endpoint.to_string())
        .bind(probe_result.duration_ms as i64)
        .bind(now_millis())
        .execute(&mut *tx)
        .await
        .map_err(|_| RotationError::Storage)?;
        tx.commit().await.map_err(|_| RotationError::Storage)?;

        if staged_secret.is_some() && delete_previous_on_commit {
            if let Some(old) = old_binding {
                self.prune_superseded_credential(
                    &credential_store,
                    &old.provider_ref,
                    &old.account_ref,
                    Some(connection_id.as_str()),
                    Some(&binding.account_ref),
                )
                .await;
            }
        }
        Ok(ConnectionRotateStatusDto {
            request_id: request_id.to_owned(),
            connection_id: connection_id.to_string(),
            state: "committed".to_owned(),
            new_revision: Some(new_revision),
            catalog_revision: Some(probe_result.catalog_revision),
            error_code: None,
        })
    }

    pub async fn refresh(
        &self,
        connection_id: &ProviderConnectionId,
        expected_revision: u64,
    ) -> Result<ConnectionRefreshStatusDto, RefreshError> {
        let operation_id = format!("refresh-{}", uuid::Uuid::new_v4());
        self.refresh_with_operation(&operation_id, connection_id, expected_revision)
            .await
    }

    pub async fn refresh_with_operation(
        &self,
        operation_id: &str,
        connection_id: &ProviderConnectionId,
        expected_revision: u64,
    ) -> Result<ConnectionRefreshStatusDto, RefreshError> {
        if self
            .refresh_next_at
            .get(connection_id)
            .is_some_and(|next| *next > Instant::now())
        {
            return Err(RefreshError::Backoff);
        }
        let cancel = CancellationToken::new();
        self.operations
            .insert(operation_id.to_owned(), cancel.clone());
        self.refresh_statuses.insert(
            operation_id.to_owned(),
            ConnectionRefreshStatusDto {
                operation_id: operation_id.to_owned(),
                connection_id: connection_id.to_string(),
                state: "running".to_owned(),
                revision: None,
                catalog_revision: None,
                error_code: None,
            },
        );
        let cell = self
            .refreshes
            .entry(connection_id.clone())
            .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
            .clone();
        let result = cell
            .get_or_init(|| async {
                self.refresh_inner(operation_id, connection_id, expected_revision, cancel)
                    .await
            })
            .await
            .clone();
        self.refreshes.remove(connection_id);
        self.operations.remove(operation_id);
        if let Err(error) = &result {
            if matches!(
                error,
                RefreshError::Timeout
                    | RefreshError::EndpointPolicy
                    | RefreshError::BoundedBody
                    | RefreshError::CredentialRejected
                    | RefreshError::Unknown
            ) {
                let attempt = self
                    .refresh_failures
                    .entry(connection_id.clone())
                    .and_modify(|attempt| *attempt = attempt.saturating_add(1))
                    .or_insert(1)
                    .to_owned();
                let config = super::load_config_or_default();
                let base = config
                    .provider_connections
                    .unwrap_or_default()
                    .refresh_backoff_base_ms
                    .max(1);
                let exponential = base.saturating_mul(1u64 << attempt.min(16));
                let mut hasher = DefaultHasher::new();
                connection_id.hash(&mut hasher);
                attempt.hash(&mut hasher);
                let jitter = (exponential / 5).saturating_mul(hasher.finish() % 100) / 100;
                self.refresh_next_at.insert(
                    connection_id.clone(),
                    Instant::now()
                        + Duration::from_millis(exponential.saturating_add(jitter).min(3_600_000)),
                );
            }
            if let Err(db_error) = sqlx::query(
                "UPDATE provider_connection_health SET status = 'unhealthy', reason_code = ?, checked_at = ?, credential_status = CASE WHEN ? THEN ? ELSE credential_status END WHERE connection_id = ? AND revision = ?",
            )
            .bind(refresh_error_code(error))
            .bind(now_millis())
            // Only a genuine typed 401/403 may move the credential axis. Every
            // other refresh failure (timeout, endpoint policy, bounded body,
            // cancellation, storage) leaves `credential_status` untouched — a
            // transient failure must never read as an invalid key, which would
            // erase a valid credential's verified state on a bad network day.
            .bind(i64::from(matches!(error, RefreshError::CredentialRejected)))
            .bind(CredentialVerification::AuthenticationFailed.code())
            .bind(connection_id.as_str())
            .bind(expected_revision as i64)
            .execute(&self.pool)
            .await
            {
                tracing::warn!(?db_error, connection_id = %connection_id, "eggpool health write failed");
            }
        }
        if result.is_ok() {
            self.refresh_failures.remove(connection_id);
            self.refresh_next_at.remove(connection_id);
        }
        self.refresh_statuses.insert(
            operation_id.to_owned(),
            result
                .clone()
                .unwrap_or_else(|error| ConnectionRefreshStatusDto {
                    operation_id: operation_id.to_owned(),
                    connection_id: connection_id.to_string(),
                    state: if matches!(error, RefreshError::Cancelled) {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .to_owned(),
                    revision: None,
                    catalog_revision: None,
                    error_code: Some(refresh_error_code(&error).to_owned()),
                }),
        );
        result
    }

    pub fn refresh_status(&self, operation_id: &str) -> Option<ConnectionRefreshStatusDto> {
        self.refresh_statuses
            .get(operation_id)
            .map(|entry| entry.value().clone())
    }

    pub fn rotation_status(&self, request_id: &str) -> Option<ConnectionRotateStatusDto> {
        self.rotation_statuses
            .get(request_id)
            .map(|entry| entry.value().clone())
    }

    /// Purge durable metadata and, after the metadata transaction commits,
    /// remove the exact credential binding that belonged to that connection.
    /// Credential cleanup is best-effort because the metadata store is the
    /// authoritative purge boundary; reconciliation removes any orphan left
    /// by a credential-store failure.
    pub async fn purge(
        &self,
        connection_id: &ProviderConnectionId,
        expected_revision: u64,
    ) -> Result<
        codegg_core::provider_connections::PurgeOutcome,
        codegg_core::provider_connections::ProviderConnectionError,
    > {
        let store =
            codegg_core::provider_connections::ProviderConnectionStore::new(self.pool.clone());
        self.purge_with_store(&store, connection_id, expected_revision)
            .await
    }

    /// One-step delete for the operator's `d` verb: tombstone, then purge.
    ///
    /// Sharing [`Self::purge`] keeps the two verbs on one removal boundary —
    /// a deleted connection drops its credential binding exactly like a purged
    /// one, and a `Blocked` outcome leaves the tombstone for `p` to finish.
    pub async fn delete(
        &self,
        connection_id: &ProviderConnectionId,
        expected_revision: u64,
    ) -> Result<
        codegg_core::provider_connections::PurgeOutcome,
        codegg_core::provider_connections::ProviderConnectionError,
    > {
        let store =
            codegg_core::provider_connections::ProviderConnectionStore::new(self.pool.clone());
        // Read the binding before the row goes away: purge removes the row,
        // and the credential it referenced has to be dropped with it or it is
        // orphaned with nothing left referencing it.
        let binding = store
            .get(connection_id)
            .await?
            .and_then(|connection| connection.secret_binding);
        let outcome = store
            .delete_permanently(connection_id, expected_revision)
            .await?;
        self.drop_credential_if_purged(binding, &outcome);
        Ok(outcome)
    }

    async fn purge_with_store(
        &self,
        store: &codegg_core::provider_connections::ProviderConnectionStore,
        connection_id: &ProviderConnectionId,
        expected_revision: u64,
    ) -> Result<
        codegg_core::provider_connections::PurgeOutcome,
        codegg_core::provider_connections::ProviderConnectionError,
    > {
        let binding = store
            .get(connection_id)
            .await?
            .and_then(|connection| connection.secret_binding);
        let outcome = store.purge(connection_id, expected_revision).await?;
        self.drop_credential_if_purged(binding, &outcome);
        Ok(outcome)
    }

    /// Drop the credential a removed connection pointed at.
    ///
    /// Only on `Purged`: a `Blocked` outcome leaves the tombstone in place,
    /// and a tombstoned row still needs its binding to be diagnosable and to
    /// be purgeable once the references clear.
    fn drop_credential_if_purged(
        &self,
        binding: Option<codegg_core::provider_connections::SecretBindingLocator>,
        outcome: &codegg_core::provider_connections::PurgeOutcome,
    ) {
        if !matches!(
            outcome,
            codegg_core::provider_connections::PurgeOutcome::Purged
        ) {
            return;
        }
        if let (Some(binding), Some(credentials)) = (binding, self.credential_store.clone()) {
            if let Err(error) =
                credentials.remove(&binding.provider_ref, Some(&binding.account_ref))
            {
                tracing::warn!(
                    ?error,
                    "eggpool purge credential cleanup failed; reconciliation will retry"
                );
            }
        }
    }

    async fn refresh_inner(
        &self,
        operation_id: &str,
        connection_id: &ProviderConnectionId,
        expected_revision: u64,
        cancel: CancellationToken,
    ) -> Result<ConnectionRefreshStatusDto, RefreshError> {
        let _permit = tokio::select! {
            _ = cancel.cancelled() => return Err(RefreshError::Cancelled),
            permit = self.refresh_cap.clone().acquire_owned() => {
                permit.map_err(|_| RefreshError::Cancelled)?
            }
        };
        let store =
            codegg_core::provider_connections::ProviderConnectionStore::new(self.pool.clone());
        let current = store
            .get(connection_id)
            .await
            .map_err(|_| RefreshError::Storage)?
            .ok_or(RefreshError::NotFound)?;
        if current.revision != expected_revision {
            return Err(RefreshError::StaleRevision);
        }
        match current.state {
            ProviderConnectionState::Active => {}
            ProviderConnectionState::Disabled => return Err(RefreshError::Disabled),
            ProviderConnectionState::CredentialMissing => {
                return Err(RefreshError::CredentialMissing)
            }
            ProviderConnectionState::Tombstoned => return Err(RefreshError::Tombstoned),
            _ => return Err(RefreshError::Unknown),
        }
        let binding = current
            .secret_binding
            .ok_or(RefreshError::CredentialMissing)?;
        let store_credentials = self
            .credential_store
            .clone()
            .ok_or(RefreshError::CredentialMissing)?;
        let api_key = store_credentials
            .get_plaintext(&binding.provider_ref, Some(&binding.account_ref), |_| true)
            .map_err(|_| RefreshError::CredentialMissing)?
            .ok_or(RefreshError::CredentialMissing)?;
        let config = super::load_config_or_default();
        let refresh_config = config.provider_connections.unwrap_or_default();
        let probe_options = codegg_providers::EggpoolProbeOptions {
            connect_timeout: Duration::from_millis(refresh_config.refresh_connect_timeout_ms),
            request_timeout: Duration::from_millis(refresh_config.refresh_read_timeout_ms),
            overall_timeout: Duration::from_millis(refresh_config.refresh_overall_timeout_ms),
            ..codegg_providers::EggpoolProbeOptions::default()
        };
        let probe_result = tokio::time::timeout(
            Duration::from_millis(refresh_config.refresh_overall_timeout_ms),
            probe_with_options(
                current.endpoint.as_str(),
                &api_key,
                Some(current.provider_kind.as_str()),
                cancel,
                probe_options,
            ),
        )
        .await
        .map_err(|_| RefreshError::Timeout)?
        .map_err(|error| match error {
            ProbeReason::Timeout => RefreshError::Timeout,
            ProbeReason::CatalogOversized => RefreshError::BoundedBody,
            ProbeReason::Cancelled => RefreshError::Cancelled,
            // A typed 401/403 from an authenticated request is the only
            // refresh failure that says anything about the credential.
            ProbeReason::AuthenticationFailed => RefreshError::CredentialRejected,
            ProbeReason::RedirectDisallowed | ProbeReason::TlsFailed => {
                RefreshError::EndpointPolicy
            }
            _ => RefreshError::Unknown,
        })?;
        let mut tx = self.pool.begin().await.map_err(|_| RefreshError::Storage)?;
        let old_catalog = sqlx::query_scalar::<_, Option<String>>(
            "SELECT catalog_revision FROM provider_connection_health WHERE connection_id = ?",
        )
        .bind(connection_id.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| RefreshError::Storage)?
        .flatten();
        // A matching digest means the upstream catalog did not change, so there is
        // nothing to promote. It does NOT mean this connection already holds
        // its catalog: when the local rows are missing at the current revision
        // — the state a stranded catalog leaves behind — refresh took the
        // no-op branch, wrote no models, and could never repair it no matter
        // how many times it was pressed. An absent catalog counts as changed so
        // refresh rebuilds it.
        let current_catalog_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM provider_connection_models WHERE connection_id = ? AND revision = ?",
        )
        .bind(connection_id.as_str())
        .bind(current.revision as i64)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| RefreshError::Storage)?;
        let catalog_changed = current_catalog_rows == 0
            || old_catalog.as_deref() != Some(probe_result.catalog_revision.as_str());
        let revision = if catalog_changed {
            let next = current.revision.saturating_add(1);
            sqlx::query("UPDATE provider_connections SET revision = ?, time_updated = ? WHERE id = ? AND revision = ?")
                .bind(next as i64)
                .bind(now_millis())
                .bind(connection_id.as_str())
                .bind(expected_revision as i64)
                .execute(&mut *tx)
                .await
                .map_err(|_| RefreshError::StaleRevision)?;
            for model in &probe_result.models {
                sqlx::query("INSERT INTO provider_connection_models (connection_id, revision, model_id, model_name, context_window, max_output_tokens, supports_tools, supports_vision) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                    .bind(connection_id.as_str())
                    .bind(next as i64)
                    .bind(&model.id)
                    .bind(&model.name)
                    .bind(model.context_window as i64)
                    .bind(model.max_output_tokens.map(|v| v as i64))
                    .bind(i64::from(model.supports_tools))
                    .bind(i64::from(model.supports_vision))
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| RefreshError::Storage)?;
            }
            next
        } else {
            current.revision
        };
        // A refresh rediscovers the catalog for the *same* credential, so it
        // only promotes the credential axis when this refresh's own bounded
        // completion accepted the key. `Unverified` (empty catalog, timeout,
        // cancellation, an unresolvable surface, a non-auth status) must leave
        // the stored verdict exactly as it was: a routine catalog refresh, or a
        // bad network day, must never erase a durable `verified` (or a genuine
        // `authentication_failed`) that came from real inference. Rotation is
        // different: it installs a new credential, so the verdict is re-derived
        // from that probe.
        sqlx::query("UPDATE provider_connection_health SET revision = ?, status = 'healthy', credential_status = CASE WHEN ? THEN ? ELSE credential_status END, reason_code = NULL, duration_ms = ?, checked_at = ?, catalog_revision = ? WHERE connection_id = ?")
            .bind(revision as i64)
            .bind(i64::from(probe_result.credential.is_verified()))
            .bind(CredentialVerification::Verified.code())
            .bind(probe_result.duration_ms as i64)
            .bind(now_millis())
            .bind(&probe_result.catalog_revision)
            .bind(connection_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|_| RefreshError::Storage)?;
        sqlx::query(
            "INSERT INTO provider_connection_audit_events (event_id, connection_id, action, actor_seam, old_revision, new_revision, endpoint_authority, outcome, duration_ms, time_created) VALUES (?, ?, 'refresh', 'daemon_refresh', ?, ?, ?, 'committed', ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(connection_id.as_str())
        .bind(current.revision as i64)
        .bind(revision as i64)
        .bind(current.endpoint.to_string())
        .bind(probe_result.duration_ms as i64)
        .bind(now_millis())
        .execute(&mut *tx)
        .await
        .map_err(|_| RefreshError::Storage)?;
        tx.commit().await.map_err(|_| RefreshError::Storage)?;
        Ok(ConnectionRefreshStatusDto {
            operation_id: operation_id.to_owned(),
            connection_id: connection_id.to_string(),
            state: "completed".to_owned(),
            revision: Some(revision),
            catalog_revision: Some(probe_result.catalog_revision),
            error_code: None,
        })
    }

    async fn summary(
        &self,
        connection: &ProviderConnection,
        catalog_revision: Option<&str>,
        probe: Option<&ProbeResult>,
    ) -> Result<ProviderConnectionSummaryDto, EggpoolError> {
        let health_row = sqlx::query_as::<_, (String, Option<String>, i64, i64, Option<String>, String)>(
            "SELECT status, reason_code, checked_at, duration_ms, catalog_revision, credential_status FROM provider_connection_health WHERE connection_id = ?",
        )
        .bind(connection.id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| EggpoolError::Storage)?
        ;
        let catalog_from_health = health_row.as_ref().and_then(|row| row.4.clone());
        let health = health_row.map(
            |(status, reason_code, checked_at, duration_ms, _, credential_status)| {
                ConnectionHealthDto {
                    status,
                    reason_code,
                    checked_at,
                    duration_ms: duration_ms as u64,
                    credential_status: Some(credential_status),
                }
            },
        );
        let model_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_connection_models WHERE connection_id = ? AND revision = ?")
            .bind(connection.id.as_str())
            .bind(connection.revision as i64)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| EggpoolError::Storage)?;
        let credential_hint = self.credential_hint(connection).await;
        Ok(ProviderConnectionSummaryDto {
            id: connection.id.to_string(),
            provider_kind: connection.provider_kind.as_str().to_owned(),
            display_name: connection.display_name.clone(),
            endpoint: connection.endpoint.to_string(),
            tls_policy: tls_key(connection.tls_policy).to_owned(),
            scope: scope_display(&connection.scope),
            state: state_key(connection.state).to_owned(),
            revision: connection.revision,
            model_count: probe
                .map(|p| p.models.len())
                .unwrap_or(model_count as usize),
            catalog_revision: catalog_revision
                .map(ToOwned::to_owned)
                .or(catalog_from_health),
            health,
            credential_hint,
        })
    }

    /// Bounded, non-secret discriminator for the bound credential.
    ///
    /// Decryption happens here, in the root crate, because `codegg-core` must
    /// stay credential-free and `codegg-protocol` must never see plaintext.
    /// Only [`codegg_providers::credential_tail_hint`]'s `…abcd` tail reaches
    /// the wire, and only when a binding exists — the plaintext itself is
    /// dropped immediately and is never logged.
    ///
    /// Remove the credential record a rotation or replacement just superseded.
    ///
    /// Scoping, deliberately narrow:
    ///
    /// - Only the *previous* binding of the connection being rotated or
    ///   replaced is considered. Records that no connection ever bound (a
    ///   key added by hand, or a leftover from an older build) are never
    ///   swept here: nothing proves they are disposable, and deleting them
    ///   would destroy credentials an unbound session still needs.
    /// - The record is removed only when it is **provably unreferenced** by
    ///   any other connection row. A credential two connections share is
    ///   kept and reported; deleting it would silently break the second
    ///   connection.
    /// - `keep_account` is the binding the just-committed write claimed, so a
    ///   rotation can never delete the credential it just installed.
    /// - An unreadable connection store counts as "still referenced": the
    ///   default is to keep the record and say so.
    async fn prune_superseded_credential(
        &self,
        store: &codegg_providers::CredentialStore,
        provider_ref: &str,
        account_ref: &str,
        rotated_connection_id: Option<&str>,
        keep_account: Option<&str>,
    ) {
        if keep_account == Some(account_ref) {
            return;
        }
        let referenced = self
            .credential_account_referenced(provider_ref, account_ref, rotated_connection_id)
            .await;
        if referenced {
            tracing::warn!(
                provider_ref,
                account_ref,
                "superseded credential is still bound by another connection; keeping it"
            );
            return;
        }
        if let Err(error) = store.remove(provider_ref, Some(account_ref)) {
            tracing::warn!(
                ?error,
                provider_ref,
                account_ref,
                "eggpool superseded credential cleanup failed"
            );
        }
    }

    /// `true` when a connection other than `except` still binds this account.
    ///
    /// Raw SQL stays inside this store: pruning a credential is only safe once
    /// no connection row can point at it.
    async fn credential_account_referenced(
        &self,
        provider_ref: &str,
        account_ref: &str,
        except: Option<&str>,
    ) -> bool {
        let count = match except {
            Some(except) => {
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM provider_connections WHERE secret_provider_ref = ? AND secret_account_ref = ? AND id <> ?",
                )
                .bind(provider_ref)
                .bind(account_ref)
                .bind(except)
                .fetch_one(&self.pool)
                .await
            }
            None => {
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM provider_connections WHERE secret_provider_ref = ? AND secret_account_ref = ?",
                )
                .bind(provider_ref)
                .bind(account_ref)
                .fetch_one(&self.pool)
                .await
            }
        };
        match count {
            Ok(count) => count > 0,
            // A read failure must never authorize a deletion.
            Err(error) => {
                tracing::warn!(
                    ?error,
                    provider_ref,
                    account_ref,
                    "credential reference check failed; keeping the superseded credential"
                );
                true
            }
        }
    }

    /// Best-effort by design: an absent hint is a missing discriminator, never
    /// a provisioning error, so an unreadable credential store degrades the
    /// picker instead of failing a connection list.
    async fn credential_hint(&self, connection: &ProviderConnection) -> Option<String> {
        let store = self.credential_store.as_ref()?;
        let binding = connection.secret_binding.as_ref()?;
        let plaintext = store
            .get_plaintext(&binding.provider_ref, Some(&binding.account_ref), |_| true)
            .ok()??;
        codegg_providers::credential_tail_hint(&plaintext)
    }

    async fn reconcile_once(&self) {
        if self.reconciled.load(Ordering::Acquire) {
            return;
        }
        if let Err(error) = sqlx::query(
            "DELETE FROM provider_connection_references WHERE reference_kind = 'provisioning_operation' AND reference_id NOT IN (SELECT operation_id FROM provider_provisioning WHERE state IN ('staged', 'probing'))",
        )
        .execute(&self.pool)
        .await
        {
            tracing::warn!(?error, "eggpool reconcile orphan reference cleanup failed");
        }
        let rows = match sqlx::query_as::<_, (String, String, String)>(
            "SELECT operation_id, secret_provider_ref, secret_account_ref FROM provider_provisioning WHERE state IN ('staged', 'probing')",
        )
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(_) => return,
        };
        if rows.is_empty() {
            self.reconciled.store(true, Ordering::Release);
            return;
        }
        let Some(store) = self.credential_store.clone() else {
            return;
        };
        for (operation_id, provider, account) in rows {
            if let Err(error) = store.remove(&provider, Some(&account)) {
                tracing::warn!(?error, %operation_id, "eggpool reconcile credential cleanup failed");
            }
            if let Err(error) = sqlx::query("UPDATE provider_provisioning SET state = 'failed', failure_code = 'daemon_restarted', time_updated = ? WHERE operation_id = ?")
                .bind(now_millis())
                .bind(operation_id.clone())
                .execute(&self.pool)
                .await
            {
                tracing::error!(?error, %operation_id, "eggpool reconcile state transition failed");
            }
        }
        self.reconciled.store(true, Ordering::Release);
    }
}

/// Provider-neutral normalization driven by the setup catalog.
///
/// Fixed-endpoint providers resolve to their catalog URL and reject
/// caller-supplied endpoint material; endpoint-requiring providers validate a
/// full user URL without imposing the Eggpool preset's default port or `/v1`
/// path; proxy presets reuse the Eggpool host/port/TLS normalization with
/// their own default port.
fn normalize_generic(
    request: &CreateProviderConnectionRequest,
) -> Result<NormalizedSpec, EggpoolError> {
    let provider_id = request.provider_id.trim();
    if provider_id.is_empty() || provider_id.chars().any(char::is_control) {
        return Err(EggpoolError::UnsupportedProvider(
            request.provider_id.clone(),
        ));
    }
    let definition = setup_catalog::setup_definition(provider_id)
        .ok_or_else(|| EggpoolError::UnsupportedProvider(provider_id.to_string()))?;
    if !definition.connectable {
        return Err(EggpoolError::UnsupportedProvider(provider_id.to_string()));
    }
    let credential_kind = match request.credential_kind {
        ProviderCredentialKind::ApiKey => codegg_providers::CredentialKind::ApiKey,
        ProviderCredentialKind::Bearer => codegg_providers::CredentialKind::BearerToken,
    };
    if !definition.credential_capability.accepts(credential_kind) {
        return Err(EggpoolError::UnsupportedCredentialKind);
    }
    let mut display_name =
        display_name_for(request.display_name.as_deref(), definition.display_name)?;
    let mut scope = scope_from_generic(&request.scope)?;
    let (endpoint, tls_policy) = match definition.endpoint_policy {
        SetupEndpointPolicy::Fixed { base_url } => {
            if request
                .endpoint
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
                || request.port.is_some()
                || request.tls_policy.is_some()
            {
                return Err(EggpoolError::InvalidEndpoint(
                    "endpoint is managed by the provider definition".into(),
                ));
            }
            let endpoint = Endpoint::new(base_url, TlsPolicy::Required)
                .map_err(|error| EggpoolError::InvalidEndpoint(error.to_string()))?;
            (endpoint, TlsPolicy::Required)
        }
        SetupEndpointPolicy::OptionalOverride { default_base_url } => {
            if request.port.is_some() {
                return Err(EggpoolError::InvalidEndpoint(
                    "explicit ports are only supported by proxy presets".into(),
                ));
            }
            match request
                .endpoint
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                Some(raw) => endpoint_from_user_url(raw, request.tls_policy)?,
                None => {
                    let base_url = default_base_url.ok_or_else(|| {
                        EggpoolError::InvalidEndpoint("provider has no default endpoint".into())
                    })?;
                    let endpoint = Endpoint::new(base_url, TlsPolicy::Required)
                        .map_err(|error| EggpoolError::InvalidEndpoint(error.to_string()))?;
                    (endpoint, TlsPolicy::Required)
                }
            }
        }
        SetupEndpointPolicy::RequiredEndpoint => {
            if request.port.is_some() {
                return Err(EggpoolError::InvalidEndpoint(
                    "explicit ports are only supported by proxy presets".into(),
                ));
            }
            let raw = request
                .endpoint
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| EggpoolError::InvalidEndpoint("endpoint is required".into()))?;
            endpoint_from_user_url(raw, request.tls_policy)?
        }
        SetupEndpointPolicy::ProxyPreset { default_port } => {
            let compat = CreateEggpoolConnectionRequest {
                host: request.endpoint.clone().unwrap_or_default(),
                port: request.port,
                tls_policy: request
                    .tls_policy
                    .unwrap_or(ProviderTlsPolicy::Required)
                    .into(),
                api_key: request.credential.clone(),
                display_name: Some(display_name.clone()),
                scope: request.scope.clone().into(),
                operation_id: request.operation_id.clone(),
            };
            let normalized = normalize_endpoint(&compat, default_port, definition.display_name)?;
            // Adopt the preset path's scope/display so the host form and the
            // generic form agree exactly (same inputs, same validation).
            display_name = normalized.display_name;
            scope = normalized.scope;
            (normalized.endpoint, normalized.tls_policy)
        }
    };
    Ok(NormalizedSpec {
        provider_id: provider_id.to_string(),
        provider_storage_key: storage_provider_kind(provider_id)?,
        endpoint,
        tls_policy,
        scope,
        display_name,
        credential_kind,
        probe_strategy: definition.probe_strategy,
    })
}

/// Storage `provider_kind` for one setup-catalog provider ID.
///
/// Native transports keep their first-class storage keys; every other
/// catalog provider (specialized or compatible) is stored as
/// `other:{id}` so durable rows preserve the exact implementation identity
/// instead of being coerced to a generic compatible row. Unknown IDs are
/// rejected before any write.
fn storage_provider_kind(provider_id: &str) -> Result<String, EggpoolError> {
    match provider_id {
        EGGPOOL_PRESET_ID => Ok(EGGPOOL_PRESET_ID.to_string()),
        "openai" => Ok("openai".to_string()),
        "anthropic" => Ok("anthropic".to_string()),
        "google" => Ok("google".to_string()),
        AZURE_ID => Ok("azure_openai".to_string()),
        CUSTOM_COMPATIBLE_ID => Ok("openai_compatible".to_string()),
        _ if setup_catalog::setup_definition(provider_id).is_some() => {
            Ok(format!("other:{provider_id}"))
        }
        _ => Err(EggpoolError::UnsupportedProvider(provider_id.to_string())),
    }
}

fn display_name_for(requested: Option<&str>, default: &str) -> Result<String, EggpoolError> {
    let display_name = requested.unwrap_or(default).trim();
    if display_name.is_empty()
        || display_name.len() > 200
        || display_name.chars().any(char::is_control)
    {
        return Err(EggpoolError::InvalidEndpoint(
            "display name is invalid".into(),
        ));
    }
    Ok(display_name.to_owned())
}

fn scope_from_generic(scope: &ProviderConnectionScope) -> Result<ProviderScope, EggpoolError> {
    let scope = match scope {
        ProviderConnectionScope::Personal { owner_id } => ProviderScope::personal(
            PrincipalId::parse(owner_id)
                .map_err(|_| EggpoolError::InvalidScope("owner id is invalid".into()))?,
        ),
        ProviderConnectionScope::Project { project_id } => ProviderScope::project(
            ProjectId::parse(project_id)
                .map_err(|_| EggpoolError::InvalidScope("project id is invalid".into()))?,
        ),
        ProviderConnectionScope::Deployment { deployment_id } => {
            ProviderScope::deployment(deployment_id.clone())
                .map_err(|_| EggpoolError::InvalidScope("deployment id is invalid".into()))?
        }
    };
    Ok(scope)
}

/// Validate a caller-supplied full base URL for endpoint-requiring or
/// override-capable providers. Unlike the proxy-preset normalization this
/// imposes no default port and preserves the caller path; userinfo, query
/// strings, fragments, control characters, and path traversal are rejected
/// before any network request is possible.
fn validate_user_endpoint(raw: &str) -> Result<String, EggpoolError> {
    let invalid = |message: &str| EggpoolError::InvalidEndpoint(message.into());
    let value = raw.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(invalid("endpoint must be a non-empty URL"));
    }
    if has_path_traversal(value) {
        return Err(invalid("path traversal is not permitted"));
    }
    let parsed = url::Url::parse(value).map_err(|_| invalid("endpoint must be a valid URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(invalid("only HTTP(S) endpoints are supported"));
    }
    if parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(invalid("userinfo, query, and fragment are not permitted"));
    }
    if parsed.port() == Some(0) {
        return Err(invalid("port must be in range 1..=65535"));
    }
    if parsed
        .path_segments()
        .map(|mut segments| segments.any(|segment| segment == ".."))
        .unwrap_or(false)
    {
        return Err(invalid("path traversal is not permitted"));
    }
    Ok(value.trim_end_matches('/').to_owned())
}

fn has_path_traversal(value: &str) -> bool {
    let path = value.split(['?', '#']).next().unwrap_or_default();
    path.split('/').any(|segment| {
        segment == ".."
            || segment.eq_ignore_ascii_case("%2e%2e")
            || segment.eq_ignore_ascii_case("%2e.%2e")
            || segment.eq_ignore_ascii_case(".%2e")
    })
}

fn endpoint_from_user_url(
    raw: &str,
    tls_policy: Option<ProviderTlsPolicy>,
) -> Result<(Endpoint, TlsPolicy), EggpoolError> {
    let url = validate_user_endpoint(raw)?;
    let policy = match tls_policy {
        Some(ProviderTlsPolicy::Required) => TlsPolicy::Required,
        Some(ProviderTlsPolicy::Optional) => TlsPolicy::Optional,
        Some(ProviderTlsPolicy::Disabled) => TlsPolicy::Disabled,
        None if url.starts_with("https://") => TlsPolicy::Required,
        None => TlsPolicy::Disabled,
    };
    let endpoint = Endpoint::new(&url, policy)
        .map_err(|error| EggpoolError::InvalidEndpoint(error.to_string()))?;
    Ok((endpoint, policy))
}

struct NormalizedEndpoint {
    endpoint: Endpoint,
    tls_policy: TlsPolicy,
    scope: ProviderScope,
    display_name: String,
}

/// Shared host/port/TLS/path normalization for proxy presets. The Eggpool
/// form keeps calling this with the Eggpool default port; generic presets
/// pass their own catalog default port.
fn normalize_endpoint(
    request: &CreateEggpoolConnectionRequest,
    default_port: u16,
    default_display: &str,
) -> Result<NormalizedEndpoint, EggpoolError> {
    if request.host.chars().any(char::is_control) || request.host.trim().is_empty() {
        return Err(EggpoolError::InvalidEndpoint(
            "host is empty or contains control characters".into(),
        ));
    }
    let policy = match request.tls_policy {
        EggpoolTlsPolicy::Required => TlsPolicy::Required,
        EggpoolTlsPolicy::Optional => TlsPolicy::Optional,
        EggpoolTlsPolicy::Disabled => TlsPolicy::Disabled,
    };
    let raw = request.host.trim();
    let scheme = if raw.contains("://") {
        String::new()
    } else if policy == TlsPolicy::Disabled {
        "http://".to_owned()
    } else {
        "https://".to_owned()
    };
    let parsed = url::Url::parse(&format!("{scheme}{raw}"))
        .map_err(|_| EggpoolError::InvalidEndpoint("host must be a valid HTTP(S) origin".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(EggpoolError::InvalidEndpoint(
            "only HTTP(S) hosts are supported".into(),
        ));
    }
    if parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(EggpoolError::InvalidEndpoint(
            "userinfo, query, and fragment are not permitted".into(),
        ));
    }
    if parsed.path().split('/').any(|segment| {
        segment == ".."
            || segment.eq_ignore_ascii_case("%2e%2e")
            || segment.eq_ignore_ascii_case("%2e.%2e")
            || segment.eq_ignore_ascii_case(".%2e")
    }) {
        return Err(EggpoolError::InvalidEndpoint(
            "path traversal is not permitted".into(),
        ));
    }
    if request.port.is_some() && parsed.port().is_some() && request.port != parsed.port() {
        return Err(EggpoolError::InvalidEndpoint(
            "explicit port conflicts with host port".into(),
        ));
    }
    let port = request.port.or(parsed.port()).unwrap_or(default_port);
    if port == 0 {
        return Err(EggpoolError::InvalidEndpoint(
            "port must be in range 1..=65535".into(),
        ));
    }
    let mut origin = parsed;
    origin
        .set_port(Some(port))
        .map_err(|_| EggpoolError::InvalidEndpoint("invalid port".into()))?;
    match policy {
        TlsPolicy::Required if origin.scheme() != "https" => {
            return Err(EggpoolError::InvalidEndpoint("TLS is required".into()))
        }
        TlsPolicy::Disabled if origin.scheme() != "http" => {
            return Err(EggpoolError::InvalidEndpoint(
                "TLS must be disabled for HTTP".into(),
            ))
        }
        _ => {}
    }
    let mut path = origin.path().trim_end_matches('/').to_owned();
    if path.is_empty() {
        path = "/v1".into();
    } else if !path.ends_with("/v1") {
        path.push_str("/v1");
    }
    origin.set_path(&path);
    let endpoint = Endpoint::new(origin.as_str(), policy)
        .map_err(|e| EggpoolError::InvalidEndpoint(e.to_string()))?;
    let display_name = request
        .display_name
        .as_deref()
        .unwrap_or(default_display)
        .trim();
    if display_name.is_empty()
        || display_name.len() > 200
        || display_name.chars().any(char::is_control)
    {
        return Err(EggpoolError::InvalidEndpoint(
            "display name is invalid".into(),
        ));
    }
    let scope = match &request.scope {
        EggpoolConnectionScope::Personal { owner_id } => ProviderScope::personal(
            PrincipalId::parse(owner_id)
                .map_err(|_| EggpoolError::InvalidScope("owner id is invalid".into()))?,
        ),
        EggpoolConnectionScope::Project { project_id } => ProviderScope::project(
            ProjectId::parse(project_id)
                .map_err(|_| EggpoolError::InvalidScope("project id is invalid".into()))?,
        ),
        EggpoolConnectionScope::Deployment { deployment_id } => {
            ProviderScope::deployment(deployment_id.clone())
                .map_err(|_| EggpoolError::InvalidScope("deployment id is invalid".into()))?
        }
    };
    Ok(NormalizedEndpoint {
        endpoint,
        tls_policy: policy,
        scope,
        display_name: display_name.to_owned(),
    })
}

async fn probe(
    endpoint: &str,
    api_key: &str,
    provider_id: Option<&str>,
    cancel: CancellationToken,
) -> Result<ProbeResult, ProbeReason> {
    probe_with_options(
        endpoint,
        api_key,
        provider_id,
        cancel,
        codegg_providers::EggpoolProbeOptions::default(),
    )
    .await
}

/// Bounded credential-verification budget.
///
/// The catalog half of this probe is a metadata read; this half spends a real
/// (one-token) completion, so it gets a tighter budget than the caller's
/// workflow timeout and never inherits a refresh's long read timeout.
const CREDENTIAL_VERIFY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CREDENTIAL_VERIFY_TIMEOUT: Duration = Duration::from_secs(10);

async fn probe_with_options(
    endpoint: &str,
    api_key: &str,
    provider_id: Option<&str>,
    cancel: CancellationToken,
    options: codegg_providers::EggpoolProbeOptions,
) -> Result<ProbeResult, ProbeReason> {
    let started = Instant::now();
    // Provider-neutral compatible validation: explicit http(s) endpoint,
    // preserved port/path, never injects the Eggpool preset `:11300`.
    // The Eggpool preset's host-shorthand + default-port handling already
    // happened in `NormalizedSpec`; the probe must not re-apply it to
    // generic upstreams.
    let probe = codegg_providers::CompatibleProbe::new(
        endpoint,
        codegg_providers::EggpoolApiKey::from(api_key),
        options,
    )
    .map_err(|error| map_probe_reason(error.reason_code()))?;
    let provider_cancel = codegg_providers::EggpoolCancellationToken::new();
    let summary = tokio::select! {
        _ = cancel.cancelled() => {
            provider_cancel.cancel();
            return Err(ProbeReason::Cancelled);
        }
        result = probe.probe(&provider_cancel) => result.map_err(|error| map_probe_reason(error.reason_code()))?
    };
    let discovered: Vec<ProbedModel> = summary
        .models
        .into_iter()
        .map(|model| ProbedModel {
            id: model.id,
            name: model.name,
            context_window: 128_000,
            max_output_tokens: None,
            supports_tools: true,
            supports_vision: false,
        })
        .collect();
    if discovered.is_empty() {
        return Err(ProbeReason::EmptyCatalog);
    }
    let models = qualify_routable_models(provider_id, discovered)?;
    // A catalog revision identifies the rows this connection actually stores,
    // so it is computed *after* qualification. Provision already digests the
    // qualified list; digesting the raw discovery here would make every
    // refresh report "catalog changed" against a freshly provisioned
    // connection and bump its revision for no change a user can observe.
    let catalog_revision = codegg_providers::eggpool::digest_models(
        &models
            .iter()
            .map(|model| codegg_providers::EggpoolModelSummary {
                id: model.id.clone(),
                name: model.name.clone(),
            })
            .collect::<Vec<_>>(),
    );
    // A `/models` 2xx is a **catalog** fact. It is not credential evidence:
    // several real endpoints answer an unauthenticated reader with 200, and a
    // key with no active entitlement sails through the catalog only to fail
    // every completion. So `Verified` is earned here, by a real completion,
    // and never by the catalog request above.
    let credential = verify_probe_credential(
        endpoint,
        api_key,
        provider_id,
        &models,
        &cancel,
        &provider_cancel,
    )
    .await?;
    Ok(ProbeResult {
        models,
        catalog_revision,
        duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        credential,
    })
}

/// Keep only the models the shared wire profile can actually route.
///
/// Refresh and rotate discover through the provider-neutral `CompatibleProbe`,
/// which reads `/models` and nothing else. That is the right *availability*
/// source and the wrong *selectability* one: the shared profile has a reviewed
/// wire mapping for only some of the ids an upstream advertises, and a model
/// with no mapping fails closed at `wire_unresolved` before a request is ever
/// built. Provision already filters — it goes through `Provider::models()` —
/// but refresh and rotate did not, so the first refresh replaced a correctly
/// qualified catalog with an unqualified one and promoted a never-dispatchable
/// entry to the top of the picker. Measured on OpenCode Go: `cat-jam-free`
/// sorts first, has no reviewed wire mapping, and every turn bound to it fails
/// without reaching the network.
///
/// Providers the shared profile does not adapt are returned untouched: there is
/// no reviewed contract to qualify against, and inventing one here would
/// silently strip models from every generic OpenAI-compatible upstream.
fn qualify_routable_models(
    provider_id: Option<&str>,
    models: Vec<ProbedModel>,
) -> Result<Vec<ProbedModel>, ProbeReason> {
    let Some(provider_id) = provider_id else {
        return Ok(models);
    };
    if codegg_providers::provider_profile::shared_provider_id(provider_id).is_none() {
        return Ok(models);
    }
    let routable: Vec<ProbedModel> = models
        .into_iter()
        .filter(|model| {
            codegg_providers::provider_profile::resolve_route(provider_id, &model.id).is_ok()
        })
        .collect();
    // Qualification emptied a non-empty upstream catalog: every model this
    // provider offers is unroutable. Committing that as an empty catalog would
    // strand the connection at a revision with no rows and leave `/connections`
    // reporting zero models — a silent lie that is harder to diagnose than the
    // failure. Fail the probe and keep the catalog the connection already holds.
    if routable.is_empty() {
        return Err(ProbeReason::EmptyCatalog);
    }
    Ok(routable)
}

/// Run the bounded credential-verification completion for one probed catalog.
///
/// Returns the durable credential verdict:
///
/// * a real completion accepted the key → `Verified`;
/// * a completion 401/403 → the probe fails with
///   [`ProbeReason::AuthenticationFailed`], so a dead key can never be
///   committed by a rotation;
/// * an empty catalog, an unresolvable surface, a transport failure, or a
///   non-authentication status → `Unverified`. Honestly unknown, never a
///   promotion and never a rejection.
async fn verify_probe_credential(
    endpoint: &str,
    api_key: &str,
    provider_id: Option<&str>,
    models: &[ProbedModel],
    cancel: &CancellationToken,
    provider_cancel: &codegg_providers::EggpoolCancellationToken,
) -> Result<CredentialVerification, ProbeReason> {
    // An empty catalog leaves nothing to run a completion against. Reporting
    // that as unverified is the honest answer; claiming `Verified` from a
    // catalog that produced no model would be the exact defect being fixed.
    let Some(model) = pick_verification_model(provider_id, models) else {
        return Ok(CredentialVerification::Unverified);
    };
    let verifier = codegg_providers::CompatibleCredentialProbe::new(
        endpoint,
        codegg_providers::EggpoolApiKey::from(api_key),
        provider_id,
        codegg_providers::CredentialProbeOptions {
            connect_timeout: CREDENTIAL_VERIFY_CONNECT_TIMEOUT,
            request_timeout: CREDENTIAL_VERIFY_TIMEOUT,
            overall_timeout: CREDENTIAL_VERIFY_TIMEOUT,
        },
    )
    .map_err(|error| map_probe_reason(error.reason_code()))?;
    let outcome = tokio::select! {
        _ = cancel.cancelled() => {
            provider_cancel.cancel();
            return Err(ProbeReason::Cancelled);
        }
        result = verifier.verify(model, provider_cancel) => result
            .map_err(|error| map_probe_reason(error.reason_code()))?
    };
    match outcome {
        codegg_providers::CredentialVerificationOutcome::Accepted => {
            Ok(CredentialVerification::Verified)
        }
        // A typed 401/403 from a real completion is the one positive proof of a
        // bad key. It must abort the probe so the rotation fails loudly rather
        // than committing a credential that cannot work.
        codegg_providers::CredentialVerificationOutcome::Rejected => {
            Err(ProbeReason::AuthenticationFailed)
        }
        codegg_providers::CredentialVerificationOutcome::Inconclusive => {
            Ok(CredentialVerification::Unverified)
        }
    }
}

/// Choose the model to spend a verification completion on.
///
/// A model that the shared wire profile cannot route is useless here: the
/// probe resolves a route per model and gives up when none exists, so picking
/// an unroutable entry turns every verification into a silent `Inconclusive`
/// and a credential can never be proven. Measured live: OpenCode Go's catalog
/// lists `cat-jam-free` first, it has no reviewed wire mapping, and choosing
/// it made every rotation report `unverified` regardless of the key.
///
/// Preference order: a tool-capable model that also routes (the same class of
/// request the agent loop issues), any routable model, then a tool-capable
/// model, then anything. Returns `None` only for an empty catalog.
fn pick_verification_model<'a>(
    provider_id: Option<&str>,
    models: &'a [ProbedModel],
) -> Option<&'a str> {
    let routes = |model: &ProbedModel| {
        provider_id
            .and_then(|id| codegg_providers::provider_profile::resolve_route(id, &model.id).ok())
            .is_some()
    };
    models
        .iter()
        .find(|model| model.supports_tools && routes(model))
        .or_else(|| models.iter().find(|model| routes(model)))
        .or_else(|| models.iter().find(|model| model.supports_tools))
        .or_else(|| models.first())
        .map(|model| model.id.as_str())
}

fn map_probe_reason(reason: codegg_providers::EggpoolProbeReasonCode) -> ProbeReason {
    match reason {
        codegg_providers::EggpoolProbeReasonCode::Auth => ProbeReason::AuthenticationFailed,
        codegg_providers::EggpoolProbeReasonCode::Unreachable => ProbeReason::Unreachable,
        codegg_providers::EggpoolProbeReasonCode::Timeout => ProbeReason::Timeout,
        codegg_providers::EggpoolProbeReasonCode::Cancelled => ProbeReason::Cancelled,
        codegg_providers::EggpoolProbeReasonCode::Redirect => ProbeReason::RedirectDisallowed,
        codegg_providers::EggpoolProbeReasonCode::Tls => ProbeReason::TlsFailed,
        codegg_providers::EggpoolProbeReasonCode::InvalidJson => ProbeReason::InvalidJson,
        codegg_providers::EggpoolProbeReasonCode::Unsupported => ProbeReason::UnsupportedApi,
        codegg_providers::EggpoolProbeReasonCode::Empty => ProbeReason::EmptyCatalog,
        codegg_providers::EggpoolProbeReasonCode::Oversized => ProbeReason::CatalogOversized,
        codegg_providers::EggpoolProbeReasonCode::InvalidInput => ProbeReason::UnsupportedApi,
    }
}

/// Catalog discovery for ordinary catalog providers: construct the provider
/// through the canonical definition builder and call `Provider::models()`
/// behind the operation cancellation and overall timeout boundary,
/// normalizing the result into the bounded connection catalog.
/// Provider-specific failures map into the generic provisioning reason
/// taxonomy without response bodies, credentials, or transport detail.
///
/// This is **not** an authentication probe. `Provider::models()` may return a
/// local/static array without any network I/O, generic OpenAI-compatible
/// discovery falls back rather than proving auth, and some provider `/models`
/// endpoints are publicly readable — so success yields
/// [`CredentialVerification::Unverified`] and only real inference feedback can
/// promote a connection. A typed provider auth error is still propagated as
/// [`ProbeReason::AuthenticationFailed`], because that *is* a genuine
/// authenticated rejection.
async fn probe_provider_catalog(
    spec: &NormalizedSpec,
    secret: &str,
    cancel: CancellationToken,
) -> Result<ProbeResult, ProbeReason> {
    use codegg_providers::Provider as _;

    const MODEL_COUNT_LIMIT: usize = 256;
    const MODEL_STRING_LIMIT: usize = 256;

    let started = Instant::now();
    let credential = match spec.credential_kind {
        codegg_providers::CredentialKind::ApiKey => {
            codegg_providers::Credential::api_key(secret.to_owned())
        }
        codegg_providers::CredentialKind::BearerToken => {
            codegg_providers::Credential::bearer(secret.to_owned(), None)
        }
    };
    let provider = setup_catalog::build_durable_provider(
        &spec.provider_id,
        credential,
        Some(spec.endpoint.as_str()),
        &spec.display_name,
    )
    .map_err(|_| ProbeReason::UnsupportedApi)?;
    if cancel.is_cancelled() {
        return Err(ProbeReason::Cancelled);
    }
    let models = tokio::select! {
        _ = cancel.cancelled() => return Err(ProbeReason::Cancelled),
        result = tokio::time::timeout(WORKFLOW_TIMEOUT, provider.models()) => match result {
            Err(_) => return Err(ProbeReason::Timeout),
            Ok(Err(error)) => return Err(map_provider_error_reason(&error)),
            Ok(Ok(models)) => models,
        }
    };
    if models.len() > MODEL_COUNT_LIMIT {
        return Err(ProbeReason::CatalogOversized);
    }
    let mut probed = Vec::with_capacity(models.len());
    for model in &models {
        if model.id.trim().is_empty() {
            continue;
        }
        if model.id.chars().count() > MODEL_STRING_LIMIT
            || model.name.chars().count() > MODEL_STRING_LIMIT
        {
            return Err(ProbeReason::CatalogOversized);
        }
        probed.push(ProbedModel {
            id: model.id.clone(),
            name: if model.name.is_empty() {
                model.id.clone()
            } else {
                model.name.clone()
            },
            context_window: u64::try_from(model.context_window).unwrap_or(u64::MAX),
            max_output_tokens: model
                .max_output_tokens
                .map(|value| u64::try_from(value).unwrap_or(u64::MAX)),
            supports_tools: model.supports_tools,
            supports_vision: model.supports_vision,
        });
    }
    if probed.is_empty() {
        return Err(ProbeReason::EmptyCatalog);
    }
    let catalog_revision = codegg_providers::eggpool::digest_models(
        &probed
            .iter()
            .map(|model| codegg_providers::EggpoolModelSummary {
                id: model.id.clone(),
                name: model.name.clone(),
            })
            .collect::<Vec<_>>(),
    );
    let credential = CredentialVerification::from_probe(
        spec.probe_strategy.credential_evidence(),
        &CatalogOutcome::Available {
            model_count: probed.len(),
            revision: catalog_revision.clone(),
            duration_ms: 0,
        },
        false,
    );
    Ok(ProbeResult {
        models: probed,
        catalog_revision,
        duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        // `Provider::models()` success is a **catalog** fact, never a
        // credential fact. Several providers return a local/static array with
        // no network I/O, generic OpenAI-compatible discovery is
        // deliberately best-effort and falls back rather than proving auth,
        // and some provider `/models` endpoints are publicly readable. The
        // classification therefore always lands on `Unverified` here, and the
        // credential is promoted later by real inference feedback.
        credential,
    })
}

fn map_provider_error_reason(error: &codegg_providers::ProviderError) -> ProbeReason {
    // Typed classification only — never provider-specific string matching.
    // `error_class() == "auth"` covers both the `Auth` variant and API errors
    // carrying a 401/403 code, so a genuine credential rejection is never
    // flattened into a generic catalog failure.
    match error.error_class() {
        "auth" => ProbeReason::AuthenticationFailed,
        "timeout" => ProbeReason::Timeout,
        _ => ProbeReason::UnsupportedApi,
    }
}

fn error_code(error: &EggpoolError) -> &'static str {
    match error {
        EggpoolError::Probe(reason) => reason.code(),
        EggpoolError::Cancelled => "cancelled",
        EggpoolError::CredentialStore => "credential_store_unavailable",
        EggpoolError::MasterKeyMissing => "master_key_missing",
        EggpoolError::InvalidEndpoint(_) => "invalid_endpoint",
        EggpoolError::InvalidScope(_) => "invalid_scope",
        EggpoolError::UnsupportedProvider(_) => "unsupported_provider",
        EggpoolError::UnsupportedCredentialKind => "unsupported_credential_kind",
        EggpoolError::Conflict => "connection_conflict",
        EggpoolError::Storage => "connection_storage_error",
        EggpoolError::Rotation(_) => "connection_rotation_failed",
        EggpoolError::Refresh(_) => "connection_refresh_failed",
    }
}

fn model_dto(model: &ProbedModel) -> ProviderModelDto {
    ProviderModelDto {
        id: model.id.clone(),
        name: model.name.clone(),
        context_window: model.context_window,
        max_output_tokens: model.max_output_tokens,
        supports_tools: model.supports_tools,
        supports_vision: model.supports_vision,
    }
}
fn idempotency_key(spec: &NormalizedSpec) -> String {
    let mut h = Sha256::new();
    h.update(spec.provider_id.as_bytes());
    h.update([0]);
    h.update(spec.endpoint.as_str().as_bytes());
    h.update([0]);
    h.update(format!("{:?}", spec.scope).as_bytes());
    format!("sha256:{}", hex::encode(h.finalize()))
}
fn storage_scope(scope: &ProviderScope) -> (&'static str, &str) {
    match scope {
        ProviderScope::Personal { owner } => ("personal", owner.as_str()),
        ProviderScope::Project { project_id } => ("project", project_id.as_str()),
        ProviderScope::Deployment { deployment_id } => ("deployment", deployment_id.as_str()),
    }
}
fn scope_display(scope: &ProviderScope) -> String {
    let (kind, value) = storage_scope(scope);
    format!("{kind}:{value}")
}
fn tls_key(policy: TlsPolicy) -> &'static str {
    match policy {
        TlsPolicy::Required => "required",
        TlsPolicy::Optional => "optional",
        TlsPolicy::Disabled => "disabled",
    }
}

fn parse_tls_policy(value: &str) -> Option<TlsPolicy> {
    match value {
        "required" => Some(TlsPolicy::Required),
        "optional" => Some(TlsPolicy::Optional),
        "disabled" => Some(TlsPolicy::Disabled),
        _ => None,
    }
}

fn refresh_error_code(error: &RefreshError) -> &'static str {
    match error {
        RefreshError::NotFound => "not_found",
        RefreshError::StaleRevision => "stale_revision",
        RefreshError::Disabled => "disabled",
        RefreshError::CredentialMissing => "credential_missing",
        RefreshError::CredentialRejected => "credential_authentication_failed",
        RefreshError::Tombstoned => "tombstoned",
        RefreshError::Timeout => "timeout",
        RefreshError::EndpointPolicy => "endpoint_policy",
        RefreshError::BoundedBody => "bounded_body",
        RefreshError::Cancelled => "cancelled",
        RefreshError::Backoff => "backoff",
        RefreshError::Storage => "storage",
        RefreshError::Unknown => "unknown",
    }
}

fn rotation_error_code(error: &RotationError) -> &'static str {
    match error {
        RotationError::NotFound => "not_found",
        RotationError::StaleRevision => "stale_revision",
        RotationError::InvalidState => "invalid_state",
        RotationError::SecretUnavailable => "secret_unavailable",
        RotationError::EndpointPolicy => "endpoint_policy",
        RotationError::ProbeFailed => "probe_failed",
        RotationError::CredentialRejected => "credential_authentication_failed",
        RotationError::Cancelled => "cancelled",
        RotationError::Storage => "storage",
    }
}

fn state_key(state: ProviderConnectionState) -> &'static str {
    match state {
        ProviderConnectionState::Active => "active",
        ProviderConnectionState::Disabled => "disabled",
        ProviderConnectionState::CredentialMissing => "credential_missing",
        ProviderConnectionState::ProvisioningRotating => "provisioning_rotating",
        ProviderConnectionState::Tombstoned => "tombstoned",
        ProviderConnectionState::Error => "error",
        ProviderConnectionState::Stale => "stale",
    }
}
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::provider::{
        ConnectionRotateChange, EggpoolConnectionScope, EggpoolTlsPolicy, SecretInput,
        SecretInputRef,
    };
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use tempfile::tempdir;

    struct MasterKeyGuard {
        previous: [Option<String>; 4],
        _env_lock: std::sync::MutexGuard<'static, ()>,
    }

    impl MasterKeyGuard {
        fn new(value: &str) -> Self {
            let env_lock = crate::auth::test_support::lock_env();
            let names = [
                "CODEGG_MASTER_KEY",
                "CODEGG_ENCRYPTION_KEY",
                "OPENCODE_ENCRYPTION_KEY",
                "CODEGG_MASTER_KEY_FILE",
            ];
            let previous = names.map(|name| {
                let previous = std::env::var(name).ok();
                std::env::remove_var(name);
                previous
            });
            std::env::set_var("CODEGG_MASTER_KEY", value);
            Self {
                previous,
                _env_lock: env_lock,
            }
        }
    }

    impl Drop for MasterKeyGuard {
        fn drop(&mut self) {
            for (name, value) in [
                "CODEGG_MASTER_KEY",
                "CODEGG_ENCRYPTION_KEY",
                "OPENCODE_ENCRYPTION_KEY",
                "CODEGG_MASTER_KEY_FILE",
            ]
            .into_iter()
            .zip(self.previous.iter_mut())
            {
                if let Some(value) = value.take() {
                    std::env::set_var(name, value);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }

    /// Number of HTTP requests one successful probe now makes: the catalog
    /// read, then the bounded credential-verification completion.
    ///
    /// A fixture that serves only the first turns the completion into a
    /// transport error, which is indistinguishable from an unreachable provider
    /// — so every "probe succeeds" fixture must serve both.
    const PROBE_REQUEST_COUNT: usize = 2;

    /// Idle window a fake endpoint waits for an *additional* request before
    /// giving up.
    ///
    /// The first request keeps the long readiness barrier; every later one only
    /// has to beat the gap between the catalog response and the immediately
    /// following completion, so a probe that legitimately stops after its first
    /// request does not park the test thread for seconds.
    const EXTRA_REQUEST_IDLE: Duration = Duration::from_millis(750);

    /// Raw HTTP response for one request head.
    fn http_response(status: u16, body: &str) -> String {
        let reason = if status == 200 { "OK" } else { "ERROR" };
        format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// Serve `count` sequential requests, answering each through `responder`.
    ///
    /// `responder` receives the request line (e.g. `GET /v1/models HTTP/1.1`)
    /// so a fixture can serve a catalog and a completion differently — which is
    /// the whole point of the credential-verification tests: a catalog that
    /// answers 200 for a key every completion rejects.
    fn spawn_fake_endpoint<F>(
        label: &str,
        count: usize,
        delay: Duration,
        responder: F,
    ) -> (String, thread::JoinHandle<String>)
    where
        F: Fn(&str) -> String + Send + 'static,
    {
        spawn_fake_endpoint_with_idle(label, count, delay, EXTRA_REQUEST_IDLE, responder)
    }

    fn spawn_fake_endpoint_with_idle<F>(
        label: &str,
        count: usize,
        delay: Duration,
        extra_request_idle: Duration,
        responder: F,
    ) -> (String, thread::JoinHandle<String>)
    where
        F: Fn(&str) -> String + Send + 'static,
    {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fake endpoint");
        listener
            .set_nonblocking(true)
            .expect("configure fake endpoint listener");
        let address = listener.local_addr().expect("fake endpoint address");
        let join = thread::spawn(move || {
            let mut log = String::new();
            for index in 0..count {
                let deadline = std::time::Instant::now()
                    + if index == 0 {
                        Duration::from_secs(5)
                    } else {
                        extra_request_idle
                    };
                let Ok((mut stream, _)) = accept_until(&listener, deadline) else {
                    break;
                };
                if stream.set_nonblocking(false).is_err() {
                    break;
                }
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                loop {
                    let read = stream.read(&mut buffer).expect("read fake request");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                if !delay.is_zero() {
                    thread::sleep(delay);
                }
                let text = String::from_utf8_lossy(&request).into_owned();
                let request_line = text.lines().next().unwrap_or_default();
                let _ = stream.write_all(responder(request_line).as_bytes());
                log.push_str(&text);
                log.push('\n');
            }
            log
        });
        let _ = label;
        (format!("http://{address}"), join)
    }

    /// Accept one connection until `deadline`.
    fn accept_until(
        listener: &TcpListener,
        deadline: std::time::Instant,
    ) -> std::io::Result<(std::net::TcpStream, std::net::SocketAddr)> {
        loop {
            match listener.accept() {
                Ok(pair) => return Ok(pair),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "fake endpoint accept timed out",
                        ));
                    }
                    // Sleep rather than spin: several fixtures deliberately sit
                    // out their second-request window, and a pure yield loop
                    // would burn a core for the whole window doing it.
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Catalog body the fake endpoints serve for `/models`.
    const FAKE_CATALOG_BODY: &str = r#"{"data":[{"id":"eggpool-model","name":"Eggpool Model"}]}"#;

    /// Fake Eggpool endpoint that serves `count` requests, so a test can
    /// exercise replacement (a second `/connect` against the same identity)
    /// without the second probe hitting a dead socket.
    ///
    /// `count` is a *request* count, not a probe count: one probe is
    /// [`PROBE_REQUEST_COUNT`] requests. Returns the address; the join handle
    /// completes once every response has been written, yielding the full
    /// request log.
    fn fake_eggpool_repeating(
        count: usize,
        delay: Duration,
    ) -> (String, thread::JoinHandle<String>) {
        // The replacement case performs SQLite assertions between probes, so
        // its fake server must outlive that legitimate gap after serving the
        // first catalog and verification requests.
        spawn_fake_endpoint_with_idle("eggpool", count, delay, Duration::from_secs(5), |_| {
            http_response(200, FAKE_CATALOG_BODY)
        })
    }

    /// Fake endpoint that answers every request with `200` plus a one-model
    /// catalog, for both the catalog read and the verification completion.
    fn fake_eggpool(delay: Duration) -> (String, thread::JoinHandle<String>) {
        fake_eggpool_repeating(PROBE_REQUEST_COUNT, delay)
    }

    async fn migrated_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("migrate test pool");
        pool
    }

    fn normalize_preset(
        request: &CreateEggpoolConnectionRequest,
    ) -> Result<NormalizedEndpoint, EggpoolError> {
        normalize_endpoint(
            request,
            codegg_providers::setup_catalog::EGGPOOL_PRESET_DEFAULT_PORT,
            "Eggpool",
        )
    }

    fn request(host: &str) -> CreateEggpoolConnectionRequest {
        CreateEggpoolConnectionRequest {
            host: host.into(),
            port: None,
            tls_policy: EggpoolTlsPolicy::Disabled,
            api_key: SecretInput::new("test-key").unwrap(),
            display_name: None,
            scope: EggpoolConnectionScope::Personal {
                owner_id: "local-user".into(),
            },
            operation_id: None,
        }
    }

    fn generic_request(provider_id: &str) -> CreateProviderConnectionRequest {
        CreateProviderConnectionRequest {
            provider_id: provider_id.into(),
            endpoint: None,
            port: None,
            tls_policy: None,
            credential: SecretInput::new("generic-test-key").unwrap(),
            credential_kind: ProviderCredentialKind::ApiKey,
            display_name: None,
            scope: ProviderConnectionScope::Personal {
                owner_id: "local-user".into(),
            },
            operation_id: None,
        }
    }

    /// Fake endpoint with a configurable status/body. Mirrors `fake_eggpool`
    /// so error-shape cases (auth, oversized) run against the same generic
    /// service without live providers.
    ///
    /// Serves [`PROBE_REQUEST_COUNT`] requests, but a probe that legitimately
    /// stops at the catalog simply leaves the fixture idle rather than failing.
    fn fake_status_server(
        status: u16,
        body: String,
        delay: Duration,
    ) -> (String, thread::JoinHandle<String>) {
        spawn_fake_endpoint("status", PROBE_REQUEST_COUNT, delay, move |_| {
            http_response(status, &body)
        })
    }

    /// The defect this whole fixture exists for: a catalog endpoint that serves
    /// `200` to *any* key — including a bogus one — while every completion
    /// rejects it.
    ///
    /// This is the real OpenCode Go shape, and it is why a catalog 2xx is not
    /// credential evidence.
    fn fake_catalog_ok_completion_forbidden(
        catalog_body: String,
    ) -> (String, thread::JoinHandle<String>) {
        spawn_fake_endpoint(
            "catalog-ok-completion-forbidden",
            PROBE_REQUEST_COUNT,
            Duration::ZERO,
            move |request_line| {
                if request_line.ends_with("/models HTTP/1.1") {
                    http_response(200, &catalog_body)
                } else {
                    http_response(
                        403,
                        r#"{"error":"An active OpenCode Go subscription is required"}"#,
                    )
                }
            },
        )
    }

    fn closed_port_url() -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind closed-port probe");
        let address = listener.local_addr().expect("closed-port address");
        drop(listener);
        format!("http://{address}")
    }

    fn oversized_catalog_body() -> String {
        let mut body = String::from(r#"{"data":["#);
        for index in 0..300 {
            if index > 0 {
                body.push(',');
            }
            body.push_str(&format!(r#"{{"id":"oversized-model-{index:03}"}}"#));
        }
        body.push_str("]}");
        body
    }

    async fn storage_rows_contain(pool: &sqlx::SqlitePool, needle: &str) -> bool {
        let tables = [
            "provider_connections",
            "provider_provisioning",
            "provider_connection_health",
            "provider_connection_models",
        ];
        for table in tables {
            let count: i64 = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM {table} WHERE {}",
                match table {
                    "provider_connections" => {
                        "id LIKE '%' || ? || '%' OR display_name LIKE '%' || ? || '%' OR endpoint LIKE '%' || ? || '%'"
                    }
                    "provider_provisioning" => {
                        "operation_id LIKE '%' || ? || '%' OR display_name LIKE '%' || ? || '%' OR endpoint LIKE '%' || ? || '%'"
                    }
                    "provider_connection_health" => {
                        "connection_id LIKE '%' || ? || '%' OR status LIKE '%' || ? || '%' OR reason_code LIKE '%' || ? || '%'"
                    }
                    _ => {
                        "connection_id LIKE '%' || ? || '%' OR model_id LIKE '%' || ? || '%' OR model_name LIKE '%' || ? || '%'"
                    }
                }
            ))
            .bind(needle)
            .bind(needle)
            .bind(needle)
            .fetch_one(pool)
            .await
            .expect("secret scan");
            if count > 0 {
                return true;
            }
        }
        false
    }

    #[test]
    fn omitted_port_uses_eggpool_default_and_v1_path() {
        let spec = normalize_preset(&request("127.0.0.1")).unwrap();
        assert_eq!(spec.endpoint.as_str(), "http://127.0.0.1:11300/v1");
    }

    #[test]
    fn conflicting_ports_are_rejected() {
        let mut req = request("http://127.0.0.1:9000");
        req.port = Some(9001);
        assert!(matches!(
            normalize_preset(&req),
            Err(EggpoolError::InvalidEndpoint(_))
        ));
    }

    #[test]
    fn tls_and_explicit_port_matrix_is_deterministic() {
        let mut req = request("https://eggpool.example:9443/v1/");
        req.port = Some(9443);
        req.tls_policy = EggpoolTlsPolicy::Required;
        assert_eq!(
            normalize_preset(&req).unwrap().endpoint.as_str(),
            "https://eggpool.example:9443/v1"
        );

        let mut optional = request("eggpool.example");
        optional.tls_policy = EggpoolTlsPolicy::Optional;
        assert_eq!(
            normalize_preset(&optional).unwrap().endpoint.as_str(),
            "https://eggpool.example:11300/v1"
        );

        let mut required_http = request("http://eggpool.example");
        required_http.tls_policy = EggpoolTlsPolicy::Required;
        assert!(matches!(
            normalize_preset(&required_http),
            Err(EggpoolError::InvalidEndpoint(_))
        ));

        let disabled_https = request("https://eggpool.example");
        assert!(matches!(
            normalize_preset(&disabled_https),
            Err(EggpoolError::InvalidEndpoint(_))
        ));
    }

    #[test]
    fn endpoint_rejects_encoded_traversal_and_secret_material() {
        for host in [
            "https://eggpool.example/%2e%2e/private",
            "https://user:secret@eggpool.example",
            "https://eggpool.example?api_key=secret",
            "https://eggpool.example/#secret",
        ] {
            assert!(matches!(
                normalize_preset(&request(host)),
                Err(EggpoolError::InvalidEndpoint(_))
            ));
        }
    }

    #[test]
    fn probe_reason_mapping_is_bounded_and_stable() {
        let cases = [
            (
                codegg_providers::EggpoolProbeReasonCode::Auth,
                ProbeReason::AuthenticationFailed,
                "authentication_failed",
            ),
            (
                codegg_providers::EggpoolProbeReasonCode::Unreachable,
                ProbeReason::Unreachable,
                "endpoint_unreachable",
            ),
            (
                codegg_providers::EggpoolProbeReasonCode::Timeout,
                ProbeReason::Timeout,
                "probe_timeout",
            ),
            (
                codegg_providers::EggpoolProbeReasonCode::Tls,
                ProbeReason::TlsFailed,
                "tls_failed",
            ),
            (
                codegg_providers::EggpoolProbeReasonCode::Redirect,
                ProbeReason::RedirectDisallowed,
                "redirect_disallowed",
            ),
            (
                codegg_providers::EggpoolProbeReasonCode::InvalidJson,
                ProbeReason::InvalidJson,
                "invalid_json",
            ),
            (
                codegg_providers::EggpoolProbeReasonCode::Unsupported,
                ProbeReason::UnsupportedApi,
                "unsupported_api",
            ),
            (
                codegg_providers::EggpoolProbeReasonCode::Empty,
                ProbeReason::EmptyCatalog,
                "empty_catalog",
            ),
            (
                codegg_providers::EggpoolProbeReasonCode::Oversized,
                ProbeReason::CatalogOversized,
                "catalog_oversized",
            ),
            (
                codegg_providers::EggpoolProbeReasonCode::Cancelled,
                ProbeReason::Cancelled,
                "cancelled",
            ),
        ];
        for (provider_reason, expected, code) in cases {
            let mapped = map_probe_reason(provider_reason);
            assert_eq!(mapped, expected);
            assert_eq!(mapped.code(), code);
        }
    }

    #[test]
    fn secret_input_debug_is_redacted() {
        let input = SecretInput::new("test-key").unwrap();
        assert!(!format!("{input:?}").contains("test-key"));
        assert!(!serde_json::to_string(&input).unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn successful_provision_persists_redacted_connection_and_catalog() {
        let _master = MasterKeyGuard::new("eggpool-workflow-test-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        // Two probes (the initial connect and the replacement below), each of
        // which is a catalog request plus a verification completion.
        let (host, server) = fake_eggpool_repeating(2 * PROBE_REQUEST_COUNT, Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));
        let mut create_request = request(&host);
        create_request.operation_id = Some("prov-success".to_string());
        let result = provisioner
            .create(create_request)
            .await
            .expect("provision succeeds");

        assert_eq!(result.connection.endpoint, format!("{host}/v1"));
        assert_eq!(result.models.len(), 1);
        assert_eq!(result.connection.model_count, 1);
        assert_eq!(credential_store.list().len(), 1);
        assert_ne!(credential_store.list()[0].encrypted_secret, "test-key");

        let active: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM provider_connections WHERE state = 'active'")
                .fetch_one(&pool)
                .await
                .expect("active count");
        let models: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_connection_models")
            .fetch_one(&pool)
            .await
            .expect("model count");
        assert_eq!(active, 1);
        assert_eq!(models, 1);

        // Re-adding the same provider replaces the existing connection rather
        // than refusing: re-running /connect is how a credential is rotated.
        let first_id = result.connection.id.clone();
        let replacement = provisioner
            .create(request(&host))
            .await
            .expect("an equivalent connection must replace, not conflict");
        assert_ne!(
            replacement.connection.id, first_id,
            "replacement must publish a new connection id"
        );
        let active: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM provider_connections WHERE state = 'active'")
                .fetch_one(&pool)
                .await
                .expect("active count");
        assert_eq!(active, 1, "replacement must leave exactly one active row");
        let superseded_state: String =
            sqlx::query_scalar("SELECT state FROM provider_connections WHERE id = ?")
                .bind(&first_id)
                .fetch_one(&pool)
                .await
                .expect("superseded state");
        // The legacy `state` column only admits
        // active/disabled/credential_missing; the authoritative tombstone lives
        // in provider_connection_lifecycle, exactly as
        // ProviderConnectionStore::delete records it.
        assert_eq!(
            superseded_state, "disabled",
            "the replaced connection must leave the active set"
        );
        let lifecycle_state: Option<String> = sqlx::query_scalar(
            "SELECT state FROM provider_connection_lifecycle WHERE connection_id = ?",
        )
        .bind(&first_id)
        .fetch_optional(&pool)
        .await
        .expect("lifecycle state");
        assert_eq!(
            lifecycle_state.as_deref(),
            Some("tombstoned"),
            "the replaced connection must be tombstoned in the authoritative lifecycle table"
        );

        let raw_requests = server.join().expect("fake server joins");
        let served = raw_requests
            .lines()
            .filter(|line| line.starts_with("GET ") || line.starts_with("POST "))
            .count();
        assert_eq!(
            served,
            2 * PROBE_REQUEST_COUNT,
            "both probes must have been served, catalog plus verification each"
        );
        assert!(raw_requests.contains("/v1/models"));
        assert!(raw_requests.contains("authorization: Bearer test-key"));
        assert!(
            raw_requests.contains("/v1/chat/completions"),
            "a credential verdict must come from a real completion, not the catalog"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn committed_provisioning_row_does_not_block_reconnecting_same_provider() {
        // Regression: `provider_provisioning` carries UNIQUE(idempotency_key)
        // permanently, and the pre-flight conflict check counted rows in state
        // 'committed'. A provider that was ever connected therefore could never
        // be connected again — the user saw every subsequent /connect attempt
        // fail with `connection_conflict`, even for a provider they had just
        // connected. A committed journal row must not block a *new* attempt;
        // the active-connection check already owns duplicate detection.
        let _master = MasterKeyGuard::new("eggpool-readd-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store));

        // First connect commits and leaves a 'committed' journal row.
        let (host_a, server_a) = fake_eggpool(Duration::ZERO);
        let mut first = request(&host_a);
        first.operation_id = Some("prov-readd-1".to_string());
        provisioner
            .create(first)
            .await
            .expect("first connect succeeds");
        server_a.join().expect("first fake server joins");

        let committed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM provider_provisioning WHERE state = 'committed'",
        )
        .fetch_one(&pool)
        .await
        .expect("committed count");
        assert_eq!(
            committed, 1,
            "first connect must leave a committed journal row"
        );

        // The user removes the connection and connects again to the *same*
        // provider endpoint — the exact /connect retry. The stale 'committed'
        // journal row must not reject the new attempt. The probe itself will
        // fail because the one-shot fake server is gone, but that failure must
        // be a probe error, never a pre-flight `Conflict`.
        sqlx::query("DELETE FROM provider_connections")
            .execute(&pool)
            .await
            .expect("remove prior connection");
        let mut second = request(&host_a);
        second.operation_id = Some("prov-readd-2".to_string());
        let outcome = provisioner.create(second).await;
        assert!(
            !matches!(outcome, Err(EggpoolError::Conflict)),
            "reconnecting the same provider endpoint must not be rejected as a \
             conflict; got {outcome:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_equivalent_provisioning_still_conflicts() {
        // The duplicate guard must survive the fix above: while one attempt is
        // genuinely in flight, a second equivalent one is still rejected.
        let _master = MasterKeyGuard::new("eggpool-readd-guard-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, _server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store));
        // Warm reconciliation first. `reconcile_once` deliberately fails every
        // 'staged'/'probing' row left behind by a crashed daemon, so a row
        // staged before the first call models a crash, not concurrency.
        provisioner.reconcile_once().await;
        let submitted = request(&host);
        let spec = normalize_generic(&CreateProviderConnectionRequest::from(submitted.clone()))
            .expect("normalize");
        let key = idempotency_key(&spec);
        sqlx::query(
            "INSERT INTO provider_provisioning (operation_id, connection_id, idempotency_key, provider_kind, display_name, endpoint, tls_policy, scope_kind, scope_ref, secret_ref, secret_provider_ref, secret_account_ref, state, time_created, time_updated) VALUES ('op-inflight', 'conn-inflight', ?, ?, ?, ?, ?, ?, ?, 'sr', ?, 'acct', 'probing', 0, 0)",
        )
        .bind(&key)
        .bind(&spec.provider_storage_key)
        .bind(&spec.display_name)
        .bind(spec.endpoint.as_str())
        .bind(tls_key(spec.tls_policy))
        .bind("personal")
        .bind("local-user")
        .bind(&spec.provider_id)
        .execute(&pool)
        .await
        .expect("stage in-flight row");

        let error = provisioner
            .create(submitted)
            .await
            .expect_err("an in-flight equivalent operation must still conflict");
        assert!(matches!(error, EggpoolError::Conflict));
    }

    struct CleanProfileGuard {
        previous: [Option<String>; 4],
        _env_lock: std::sync::MutexGuard<'static, ()>,
    }

    impl CleanProfileGuard {
        fn new() -> Self {
            let env_lock = crate::auth::test_support::lock_env();
            let names = [
                "CODEGG_MASTER_KEY",
                "CODEGG_ENCRYPTION_KEY",
                "OPENCODE_ENCRYPTION_KEY",
                "CODEGG_MASTER_KEY_FILE",
            ];
            let previous = names.map(|name| {
                let previous = std::env::var(name).ok();
                std::env::remove_var(name);
                previous
            });
            Self {
                previous,
                _env_lock: env_lock,
            }
        }
    }

    impl Drop for CleanProfileGuard {
        fn drop(&mut self) {
            for (name, value) in [
                "CODEGG_MASTER_KEY",
                "CODEGG_ENCRYPTION_KEY",
                "OPENCODE_ENCRYPTION_KEY",
                "CODEGG_MASTER_KEY_FILE",
            ]
            .into_iter()
            .zip(self.previous.iter_mut())
            {
                if let Some(value) = value.take() {
                    std::env::set_var(name, value);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn clean_profile_provision_bootstraps_managed_key_without_env() {
        // M001 regression fixture: the M002 suite always used a synthetic
        // master-key guard, so it never exercised a clean environment. A
        // fresh local profile with all three legacy master-key variables
        // absent must provision through the fake server by bootstrapping
        // one managed key.
        let _clean = CleanProfileGuard::new();
        let directory = tempdir().expect("credential tempdir");
        std::env::set_var(
            "CODEGG_MASTER_KEY_FILE",
            directory.path().join("master.key"),
        );
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));
        let result = provisioner
            .create(request(&host))
            .await
            .expect("clean-profile provision bootstraps managed key");
        server.join().expect("fake server joins");

        assert_eq!(result.connection.endpoint, format!("{host}/v1"));
        assert_eq!(result.models.len(), 1);
        assert!(directory.path().join("master.key").exists());
        // Restart-equivalent: the stored credential decrypts with no env.
        let account = credential_store.list()[0].account_id.clone();
        let credential = credential_store
            .get_credential("eggpool", account.as_deref())
            .expect("decrypt");
        assert!(credential.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rotation_commits_new_revision_and_removes_only_previous_credential() {
        let _master = MasterKeyGuard::new("eggpool-workflow-rotation-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));
        let result = provisioner
            .create(request(&host))
            .await
            .expect("initial provision succeeds");
        server.join().expect("initial fake server joins");
        let connection_id = ProviderConnectionId::parse(&result.connection.id).unwrap();

        let failed = provisioner
            .rotate(
                "rotation-invalid-endpoint",
                &connection_id,
                result.connection.revision,
                ConnectionRotateChange::EndpointOnly {
                    endpoint: "http://user:secret@example.invalid".to_owned(),
                    tls_policy: "disabled".to_owned(),
                    display_name: None,
                },
                SecretInputRef::new("unused-rotation-handle").unwrap(),
                false,
            )
            .await;
        assert!(matches!(failed, Err(RotationError::EndpointPolicy)));
        assert_eq!(
            codegg_core::provider_connections::ProviderConnectionStore::new(pool.clone())
                .get(&connection_id)
                .await
                .unwrap()
                .unwrap()
                .revision,
            result.connection.revision
        );

        let (rotated_host, rotated_server) = fake_eggpool(Duration::ZERO);
        let secret_ref = SecretInputRef::new("rotation-secret-handle").unwrap();
        assert!(provisioner.register_rotation_secret(secret_ref.clone(), "rotated-key".to_owned()));
        let status = provisioner
            .rotate(
                "rotation-success",
                &connection_id,
                result.connection.revision,
                ConnectionRotateChange::CredentialAndEndpoint {
                    endpoint: format!("{rotated_host}/v1"),
                    tls_policy: "disabled".to_owned(),
                    display_name: Some("Rotated Eggpool".to_owned()),
                },
                secret_ref,
                true,
            )
            .await
            .expect("rotation succeeds");
        rotated_server.join().expect("rotation fake server joins");

        assert_eq!(status.state, "committed");
        assert_eq!(status.new_revision, Some(result.connection.revision + 1));
        assert_eq!(credential_store.list().len(), 1);
        let stored = codegg_core::provider_connections::ProviderConnectionStore::new(pool)
            .get(&connection_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.revision, result.connection.revision + 1);
        assert_eq!(stored.endpoint.as_str(), format!("{rotated_host}/v1"));
        assert_eq!(stored.display_name, "Rotated Eggpool");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rotation_never_prunes_a_credential_another_connection_still_binds() {
        // Pruning exists so a rotation cannot leave an orphan behind, but a
        // credential two connections share is not an orphan: deleting it
        // would silently break the second connection. Fixture the shared
        // binding by cloning the connection row onto a second id, which is
        // what two connections provisioned against one account look like.
        let _master = MasterKeyGuard::new("eggpool-rotation-shared-credential-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));
        let result = provisioner
            .create(request(&host))
            .await
            .expect("initial provision succeeds");
        server.join().expect("initial fake server joins");
        let connection_id = ProviderConnectionId::parse(&result.connection.id).unwrap();
        let shared_account = credential_store.list()[0]
            .account_id
            .clone()
            .expect("provisioned connection has an account");
        let shared_provider = credential_store.list()[0].provider_id.clone();

        // A second connection shares the account the rotation is about to
        // supersede. Two connections provisioned against one account look
        // exactly like this.
        let other_id = ProviderConnectionId::new();
        sqlx::query(
            "INSERT INTO provider_connections (id, provider_kind, display_name, endpoint, tls_policy, scope_kind, scope_ref, secret_ref, secret_provider_ref, secret_account_ref, state, revision, time_created, time_updated) SELECT ?, provider_kind, 'Shared', endpoint, tls_policy, 'personal', 'shared-scope', 'shared-secret', ?, ?, 'active', 1, ?, ? FROM provider_connections WHERE id = ?",
        )
        .bind(other_id.as_str())
        .bind(&shared_provider)
        .bind(shared_account.as_str())
        .bind(now_millis())
        .bind(now_millis())
        .bind(connection_id.as_str())
        .execute(&pool)
        .await
        .expect("shared-connection row inserts");

        let (rotated_host, rotated_server) = fake_eggpool(Duration::ZERO);
        let secret_ref = SecretInputRef::new("shared-rotation-handle").unwrap();
        assert!(provisioner.register_rotation_secret(secret_ref.clone(), "rotated-key".to_owned()));
        let status = provisioner
            .rotate(
                "rotation-shared-credential",
                &connection_id,
                result.connection.revision,
                ConnectionRotateChange::CredentialAndEndpoint {
                    endpoint: format!("{rotated_host}/v1"),
                    tls_policy: "disabled".to_owned(),
                    display_name: None,
                },
                secret_ref,
                true,
            )
            .await
            .expect("rotation succeeds");
        rotated_server.join().expect("rotation fake server joins");
        assert_eq!(status.state, "committed");

        // The superseded account is still bound by `other_id`, so it survived
        // and still decrypts.
        assert!(
            credential_store
                .find_record(&shared_provider, Some(&shared_account))
                .is_some(),
            "a credential another connection binds must never be pruned"
        );
        assert!(credential_store
            .get_credential(&shared_provider, Some(&shared_account))
            .expect("decrypt")
            .is_some());

        // The rotation installed exactly one new record and pruned nothing
        // else: the shared account plus the record it just claimed.
        let accounts: Vec<String> = credential_store
            .list()
            .into_iter()
            .map(|record| record.account_id.unwrap_or_default())
            .collect();
        assert_eq!(
            accounts.len(),
            2,
            "the rotation must add one record and keep the shared one: {accounts:?}"
        );
        assert!(
            accounts.contains(&shared_account),
            "the shared record must remain in {accounts:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rotation_refuses_a_credential_the_endpoint_catalog_but_not_completion_rejects() {
        // The live failure this exists for: a rotation committed, health read
        // `verified`, and the next turn 403'd. A key that passes catalog
        // discovery and fails a real completion must not be installable.
        let _master = MasterKeyGuard::new("eggpool-rotation-credential-rejected-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));
        let result = provisioner
            .create(request(&host))
            .await
            .expect("initial provision succeeds");
        server.join().expect("initial fake server joins");
        let connection_id = ProviderConnectionId::parse(&result.connection.id).unwrap();
        assert_eq!(
            credential_status_of(&pool, &result.connection.id, result.connection.revision).await,
            Some("verified".to_string()),
            "precondition: the working credential is verified"
        );

        let (rotated_host, rotated_server) = fake_catalog_ok_completion_forbidden(
            r#"{"data":[{"id":"eggpool-model","name":"Eggpool Model"}]}"#.to_string(),
        );
        let secret_ref = SecretInputRef::new("rotation-dead-key").unwrap();
        assert!(provisioner.register_rotation_secret(secret_ref.clone(), "dead-key".to_owned()));
        let error = provisioner
            .rotate(
                "rotation-dead-credential",
                &connection_id,
                result.connection.revision,
                // The endpoint moves to the fixture that rejects completions;
                // `CredentialOnly` would keep pointing at the original host,
                // whose fake has already exited.
                ConnectionRotateChange::CredentialAndEndpoint {
                    endpoint: format!("{rotated_host}/v1"),
                    tls_policy: "disabled".to_owned(),
                    display_name: None,
                },
                secret_ref,
                true,
            )
            .await
            .expect_err("a completion-rejected key must not rotate");
        rotated_server.join().expect("rotation fake server joins");

        assert_eq!(error, RotationError::CredentialRejected);
        assert_eq!(
            rotation_error_code(&RotationError::CredentialRejected),
            "credential_authentication_failed",
            "the operator needs a reason code that names the credential"
        );
        assert_eq!(
            provisioner
                .rotation_status("rotation-dead-credential")
                .map(|status| status.error_code),
            Some(Some("credential_authentication_failed".to_owned()))
        );
        assert_eq!(
            codegg_core::provider_connections::ProviderConnectionStore::new(pool.clone())
                .get(&connection_id)
                .await
                .unwrap()
                .unwrap()
                .revision,
            result.connection.revision,
            "a rejected rotation must not commit a revision"
        );
        assert_eq!(
            credential_store.list().len(),
            1,
            "the staged secret must be dropped, leaving the previous binding"
        );
        assert_eq!(
            credential_status_of(&pool, &result.connection.id, result.connection.revision).await,
            Some("verified".to_string()),
            "the previously installed credential keeps its own verdict"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_refresh_leaves_a_verified_credential_untouched() {
        // A cancellation is not evidence about the credential. Erasing a
        // durable `verified` because the user hit ctrl-c — or because the
        // network blipped — is exactly the false negative this axis must not
        // produce.
        let _master = MasterKeyGuard::new("eggpool-refresh-cancel-verdict-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store));
        let result = provisioner
            .create(request(&host))
            .await
            .expect("initial provision succeeds");
        server.join().expect("initial fake server joins");
        let connection_id = ProviderConnectionId::parse(&result.connection.id).unwrap();
        let revision = result.connection.revision;

        let (refresh_host, refresh_server) = fake_eggpool(Duration::from_millis(400));
        sqlx::query("UPDATE provider_connections SET endpoint = ? WHERE id = ?")
            .bind(format!("{refresh_host}/v1"))
            .bind(connection_id.as_str())
            .execute(&pool)
            .await
            .unwrap();

        let running = provisioner.clone();
        let running_id = connection_id.clone();
        let task = tokio::spawn(async move {
            running
                .refresh_with_operation("refresh-cancelled", &running_id, revision)
                .await
        });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            if provisioner
                .refresh_status("refresh-cancelled")
                .is_some_and(|status| status.state == "running")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            provisioner.cancel("refresh-cancelled"),
            "the refresh must be cancellable while it runs"
        );
        let outcome = task.await.unwrap();
        assert!(
            matches!(outcome, Err(RefreshError::Cancelled)),
            "a cancelled refresh reports cancellation, got {outcome:?}"
        );
        let _ = refresh_server.join();

        assert_eq!(
            credential_status_of(&pool, &result.connection.id, revision).await,
            Some("verified".to_string()),
            "cancellation must leave the credential axis exactly as it was"
        );
    }

    fn probed(ids: &[&str]) -> Vec<ProbedModel> {
        ids.iter()
            .map(|id| ProbedModel {
                id: (*id).to_string(),
                name: (*id).to_string(),
                context_window: 128_000,
                max_output_tokens: None,
                supports_tools: true,
                supports_vision: false,
            })
            .collect()
    }

    fn kept(result: Result<Vec<ProbedModel>, ProbeReason>) -> Vec<String> {
        result
            .expect("qualification keeps at least one model")
            .into_iter()
            .map(|model| model.id)
            .collect()
    }

    /// A refresh must not advertise a model the agent loop cannot dispatch.
    ///
    /// Provision discovers through `Provider::models()`, which drops every id
    /// the shared wire profile has no reviewed mapping for. Refresh and rotate
    /// discovered through the provider-neutral `CompatibleProbe`, which kept
    /// them all — so the first refresh a connection ever ran replaced a correct
    /// 30-model catalog with an unqualified 37-model one. Observed live on
    /// OpenCode Go: `/connections` then offered 9 models that can only ever
    /// fail closed at `wire_unresolved`, with `cat-jam-free` — alphabetically
    /// first, and therefore the row a user lands on first — among them.
    #[test]
    fn refresh_qualification_drops_models_with_no_reviewed_wire_mapping() {
        let qualified = kept(qualify_routable_models(
            Some("opencode_go"),
            probed(&["cat-jam-free", "mimo-v2.6-flash", "omen-alpha"]),
        ));
        assert_eq!(qualified, vec!["mimo-v2.6-flash".to_string()]);
    }

    /// Qualification is scoped to providers the shared profile adapts.
    ///
    /// For every other upstream there is no reviewed contract to qualify
    /// against. Filtering there would strip models from generic
    /// OpenAI-compatible providers that dispatch perfectly well.
    #[test]
    fn unadapted_providers_keep_every_discovered_model() {
        let models = probed(&["some-model", "another-model"]);
        assert_eq!(
            kept(qualify_routable_models(
                Some("generalcompute"),
                models.clone()
            )),
            vec!["some-model".to_string(), "another-model".to_string()]
        );
        assert_eq!(
            kept(qualify_routable_models(None, models)),
            vec!["some-model".to_string(), "another-model".to_string()]
        );
    }

    /// A provider whose entire catalog is unroutable fails instead of stranding.
    ///
    /// Committing an empty catalog would bump the revision, write zero rows,
    /// and leave `/connections` reporting zero models — indistinguishable from
    /// the stranded-catalog defect. Refusing keeps the rows already held.
    #[test]
    fn a_catalog_with_nothing_routable_is_refused_not_committed_as_empty() {
        assert!(matches!(
            qualify_routable_models(Some("opencode_go"), probed(&["cat-jam-free"])),
            Err(ProbeReason::EmptyCatalog)
        ));
    }

    /// Exactly one active connection is the authority for its provider.
    ///
    /// This is the rule that lets `codegg exec`, `codegg models`, and `/models`
    /// find a provider at all. They have no session and no turn, so without a
    /// connection they fall back to counting credential-store records — and a
    /// rotation leaves the live record beside the orphan it replaced, the count
    /// reaches two, and the provider disappears with `Provider not found` while
    /// a healthy `active` connection holds its working key.
    #[tokio::test(flavor = "current_thread")]
    async fn one_active_connection_is_the_authority_for_its_provider() {
        let _master = MasterKeyGuard::new("eggpool-connection-authority-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));
        let result = provisioner
            .create(request(&host))
            .await
            .expect("initial provision succeeds");
        server.join().expect("initial fake server joins");
        let connection_id = ProviderConnectionId::parse(&result.connection.id).unwrap();
        let store = codegg_core::provider_connections::ProviderConnectionStore::new(pool.clone());
        let provider_kind = store
            .get(&connection_id)
            .await
            .unwrap()
            .unwrap()
            .provider_kind
            .as_str()
            .to_string();

        let locators = crate::core::session_selection::connection_credential_locators(&store).await;
        assert_eq!(locators.len(), 1, "one active connection is authority");
        assert_eq!(locators[0].0, provider_kind);
        assert_eq!(
            locators[0].1.account_ref,
            credential_store.list()[0]
                .account_id
                .clone()
                .expect("provisioned connection has an account")
        );

        // A second *active* connection for the same provider is a real
        // ambiguity. Picking one would be a coin flip, so the rule yields
        // nothing and the existing refusal stands.
        let second_id = ProviderConnectionId::new();
        sqlx::query(
            "INSERT INTO provider_connections (id, provider_kind, display_name, endpoint, tls_policy, scope_kind, scope_ref, secret_ref, secret_provider_ref, secret_account_ref, state, revision, time_created, time_updated) SELECT ?, provider_kind, 'Second', endpoint, tls_policy, 'personal', 'second-scope', 'second-secret', secret_provider_ref, secret_account_ref, 'active', 1, ?, ? FROM provider_connections WHERE id = ?",
        )
        .bind(second_id.as_str())
        .bind(now_millis())
        .bind(now_millis())
        .bind(connection_id.as_str())
        .execute(&pool)
        .await
        .expect("second-connection row inserts");
        assert!(
            crate::core::session_selection::connection_credential_locators(&store)
                .await
                .is_empty(),
            "two active connections for one provider must not be guessed between"
        );

        // Only `active` speaks. Disabling one restores a single answer, which
        // is what the user reaches for when they disable a duplicate.
        sqlx::query("UPDATE provider_connections SET state = 'disabled' WHERE id = ?")
            .bind(second_id.as_str())
            .execute(&pool)
            .await
            .expect("disable second connection");
        assert_eq!(
            crate::core::session_selection::connection_credential_locators(&store)
                .await
                .len(),
            1,
            "a disabled connection is not authority"
        );

        // And with none active there is nothing to say.
        sqlx::query("UPDATE provider_connections SET state = 'disabled' WHERE id = ?")
            .bind(connection_id.as_str())
            .execute(&pool)
            .await
            .expect("disable first connection");
        assert!(
            crate::core::session_selection::connection_credential_locators(&store)
                .await
                .is_empty()
        );
    }

    /// A catalog stranded behind a revision bump must be rebuildable.
    ///
    /// Both the rotate copy-shortcut and the refresh no-op branch used to trust
    /// a matching upstream digest as proof that this connection already held its
    /// models. When the rows are absent at the current revision that assumption
    /// is false and both branches do nothing, so the catalog stays empty through
    /// any number of rotations and refreshes and `/connections` reports zero
    /// models forever. Observed live: revision 16, rows stranded at 1-3, a
    /// rotate that reported success.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn refresh_rebuilds_a_catalog_stranded_behind_a_revision_bump() {
        let _master = MasterKeyGuard::new("eggpool-stranded-catalog-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store));
        let result = provisioner
            .create(request(&host))
            .await
            .expect("initial provision succeeds");
        server.join().expect("initial fake server joins");
        let connection_id = ProviderConnectionId::parse(&result.connection.id).unwrap();
        let seeded = result.connection.revision;

        let models_at = |revision: u64| {
            let pool = pool.clone();
            let id = connection_id.clone();
            async move {
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM provider_connection_models WHERE connection_id = ? AND revision = ?",
                )
                .bind(id.as_str())
                .bind(revision as i64)
                .fetch_one(&pool)
                .await
                .unwrap()
            }
        };
        assert!(
            models_at(seeded).await > 0,
            "provision seeds the catalog at the connection revision"
        );

        // Stranding: rows survive only at older revisions while the connection
        // has moved on -- the state a revision bump that forgot to carry them
        // leaves behind.
        sqlx::query(
            "DELETE FROM provider_connection_models WHERE connection_id = ? AND revision >= ?",
        )
        .bind(connection_id.as_str())
        .bind(seeded as i64)
        .execute(&pool)
        .await
        .unwrap();
        let stranded = seeded + 3;
        sqlx::query("UPDATE provider_connections SET revision = ? WHERE id = ?")
            .bind(stranded as i64)
            .bind(connection_id.as_str())
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            models_at(stranded).await,
            0,
            "the catalog is now absent at the current revision"
        );

        // The upstream serves the same model list, so the catalog digest is
        // unchanged. That alone previously sent refresh down its no-op branch.
        let (refresh_host, refresh_server) = fake_eggpool(Duration::ZERO);
        sqlx::query("UPDATE provider_connections SET endpoint = ? WHERE id = ?")
            .bind(format!("{refresh_host}/v1"))
            .bind(connection_id.as_str())
            .execute(&pool)
            .await
            .unwrap();

        let outcome = provisioner
            .refresh_with_operation("refresh-stranded", &connection_id, stranded)
            .await
            .expect("refresh succeeds");
        refresh_server.join().expect("refresh fake server joins");

        let refreshed_to = outcome.revision.expect("refresh reports a revision");
        let rebuilt = models_at(refreshed_to).await;
        assert!(
            rebuilt > 0,
            "refresh must rebuild a missing catalog even when the upstream digest is unchanged; revision {refreshed_to} holds no models"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn refresh_coalesces_and_preserves_revision_for_unchanged_catalog() {
        let _master = MasterKeyGuard::new("eggpool-workflow-refresh-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store));
        let result = provisioner
            .create(request(&host))
            .await
            .expect("initial provision succeeds");
        server.join().expect("initial fake server joins");
        let connection_id = ProviderConnectionId::parse(&result.connection.id).unwrap();

        let (refresh_host, refresh_server) = fake_eggpool(Duration::from_millis(100));
        sqlx::query("UPDATE provider_connections SET endpoint = ? WHERE id = ?")
            .bind(format!("{refresh_host}/v1"))
            .bind(connection_id.as_str())
            .execute(&pool)
            .await
            .unwrap();

        let first = provisioner.clone();
        let first_id = connection_id.clone();
        let revision = result.connection.revision;
        let first_task = tokio::spawn(async move {
            first
                .refresh_with_operation("refresh-a", &first_id, revision)
                .await
        });
        let mut running_seen = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            if provisioner
                .refresh_status("refresh-a")
                .is_some_and(|status| status.state == "running")
            {
                running_seen = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            running_seen,
            "first refresh did not enter the running state"
        );
        let second = provisioner.clone();
        let second_id = connection_id.clone();
        let second_task = tokio::spawn(async move {
            second
                .refresh_with_operation("refresh-b", &second_id, revision)
                .await
        });
        let first_result = first_task.await.unwrap().unwrap();
        let second_result = second_task.await.unwrap().unwrap();
        refresh_server.join().expect("refresh fake server joins");

        assert_eq!(first_result.state, "completed");
        assert_eq!(second_result.state, "completed");
        assert_eq!(first_result.revision, Some(revision));
        assert_eq!(second_result.revision, Some(revision));
        let stored_revision: i64 =
            sqlx::query_scalar("SELECT revision FROM provider_connections WHERE id = ?")
                .bind(connection_id.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored_revision, revision as i64);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancellation_compensates_operation_owned_credential() {
        let _master = MasterKeyGuard::new("eggpool-workflow-cancel-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_eggpool(Duration::from_millis(250));
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));
        let mut request = request(&host);
        request.operation_id = Some("prov-cancel".to_string());
        let running = provisioner.clone();
        let task = tokio::spawn(async move { running.create(request).await });
        tokio::time::sleep(Duration::from_millis(25)).await;
        let mut cancelled = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            if provisioner.cancel("prov-cancel") {
                cancelled = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(cancelled, "provisioning operation was not registered");
        let result = task.await.expect("provision task joins");
        assert!(matches!(result, Err(EggpoolError::Cancelled)));
        // Wake the bounded fixture if cancellation happened before the
        // request reached the listener, then join it deterministically.
        let wake_address = host.strip_prefix("http://").unwrap_or(&host);
        let _ = std::net::TcpStream::connect(wake_address);
        let _ = server.join();

        let active: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM provider_connections WHERE state = 'active'")
                .fetch_one(&pool)
                .await
                .expect("active count");
        assert_eq!(active, 0);
        assert!(credential_store.list().is_empty());
    }

    // ── M002 generic provisioning ────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn generic_direct_provider_provision_succeeds_without_network() {
        let _master = MasterKeyGuard::new("generic-direct-provision-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        // The openai definition requires live `/models` discovery, so a bare
        // default-endpoint provision would dial the real upstream. Point the
        // request at a local fake instead: no outside network, same probe path.
        let body = r#"{"data":[{"id":"model-a","name":"Model A"},{"id":"model-b","name":"Model B"},{"id":"model-c","name":"Model C"}]}"#.to_string();
        let (host, server) = fake_status_server(200, body, Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));

        let mut request = generic_request("openai");
        request.endpoint = Some(host.clone());
        request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        assert!(!format!("{request:?}").contains("generic-test-key"));
        let result = provisioner
            .create_connection(request)
            .await
            .expect("ordinary direct provision succeeds");
        server.join().expect("fake server joins");

        assert_eq!(result.connection.provider_kind, "openai");
        assert_eq!(result.connection.endpoint, format!("{host}/"));
        assert_eq!(result.models.len(), 3);
        assert_eq!(result.connection.model_count, 3);
        assert!(!result.catalog_revision.is_empty());
        // The credential is encrypted at rest and the secret never reaches
        // SQLite rows or protocol snapshots.
        assert_eq!(credential_store.list().len(), 1);
        assert_ne!(
            credential_store.list()[0].encrypted_secret,
            "generic-test-key"
        );
        assert!(!storage_rows_contain(&pool, "generic-test-key").await);
        let snapshot = serde_json::to_value(&result).expect("result serializes");
        assert!(!snapshot.to_string().contains("generic-test-key"));

        // Equivalent resubmission replaces the durable connection rather than
        // refusing. Replacement is decided *before* the probe (the superseded
        // row is only tombstoned once the replacement succeeds), so assert the
        // durable outcome here without depending on a second live probe of a
        // real provider endpoint.
        let superseded: Option<(String, i64)> = sqlx::query_as(
            "SELECT id, revision FROM provider_connections WHERE provider_kind = ? AND endpoint = ? AND state = 'active'",
        )
        .bind(&result.connection.provider_kind)
        .bind(&result.connection.endpoint)
        .fetch_optional(&pool)
        .await
        .expect("existing connection is discoverable for replacement");
        assert!(
            superseded.is_some(),
            "an equivalent active connection must be treated as replaceable, \
             not as a refusal"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn generic_eggpool_preset_provision_succeeds_through_generic_service() {
        let _master = MasterKeyGuard::new("generic-eggpool-provision-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));

        let mut create_request = generic_request("eggpool");
        create_request.endpoint = Some(host.clone());
        create_request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        create_request.operation_id = Some("prov-generic-eggpool".to_string());
        let result = provisioner
            .create_connection(create_request)
            .await
            .expect("eggpool preset provision succeeds");
        let raw_request = server.join().expect("fake server joins");

        assert_eq!(result.connection.provider_kind, "eggpool");
        assert_eq!(result.connection.endpoint, format!("{host}/v1"));
        assert_eq!(result.models.len(), 1);
        assert!(raw_request.contains("/v1/models"));
        assert!(raw_request.contains("authorization: Bearer generic-test-key"));
        assert!(!storage_rows_contain(&pool, "generic-test-key").await);

        // The same host through the legacy compat adapter reaches the same row
        // shape, proving one shared service. It is recognised as the same
        // identity, so it is replaceable rather than refused; the single-shot
        // fake server is already consumed by the first probe, so assert the
        // durable precondition instead of a second live probe. Full
        // replacement (including the tombstone swap) is covered by
        // `reconnecting_replaces_the_active_connection` and
        // `successful_provision_persists_redacted_connection_and_catalog`.
        let superseded: Option<(String, i64)> = sqlx::query_as(
            "SELECT id, revision FROM provider_connections WHERE provider_kind = ? AND endpoint = ? AND state = 'active'",
        )
        .bind(&result.connection.provider_kind)
        .bind(&result.connection.endpoint)
        .fetch_optional(&pool)
        .await
        .expect("generic row is discoverable for the compat adapter");
        assert!(
            superseded.is_some(),
            "the compat adapter must observe the generic row as replaceable"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn generic_custom_compatible_provision_succeeds() {
        let _master = MasterKeyGuard::new("generic-custom-provision-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let body = r#"{"data":[{"id":"custom-model","name":"Custom Model"}]}"#.to_string();
        let (host, server) = fake_status_server(200, body, Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));

        let mut request = generic_request("custom");
        request.endpoint = Some(host.clone());
        request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        request.credential_kind = ProviderCredentialKind::Bearer;
        let result = provisioner
            .create_connection(request)
            .await
            .expect("custom compatible provision succeeds");
        server.join().expect("fake server joins");

        assert_eq!(result.connection.provider_kind, "openai_compatible");
        assert_eq!(result.connection.endpoint, format!("{host}/"));
        assert_eq!(result.models.len(), 1);
        assert_eq!(result.models[0].id, "custom-model");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn generic_invalid_credential_maps_to_bounded_reason() {
        let _master = MasterKeyGuard::new("generic-invalid-credential-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_status_server(
            401,
            r#"{"error":"bad-key-must-not-leak"}"#.to_string(),
            Duration::ZERO,
        );
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));

        let mut request = generic_request("eggpool");
        request.endpoint = Some(host);
        request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        let error = provisioner
            .create_connection(request)
            .await
            .expect_err("invalid credential must fail");
        server.join().expect("fake server joins");

        assert!(matches!(
            error,
            EggpoolError::Probe(ProbeReason::AuthenticationFailed)
        ));
        assert_eq!(error_code(&error), "authentication_failed");
        assert!(!error.to_string().contains("bad-key-must-not-leak"));
        // Ownership-aware compensation removed the staged credential and left
        // no durable connection.
        assert!(credential_store.list().is_empty());
        let active: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM provider_connections WHERE state = 'active'")
                .fetch_one(&pool)
                .await
                .expect("active count");
        assert_eq!(active, 0);
        let failed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM provider_provisioning WHERE state = 'failed' AND failure_code = 'authentication_failed'",
        )
        .fetch_one(&pool)
        .await
        .expect("failed count");
        assert_eq!(failed, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn generic_unavailable_endpoint_maps_to_unreachable() {
        let _master = MasterKeyGuard::new("generic-unreachable-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));

        let mut request = generic_request("eggpool");
        request.endpoint = Some(closed_port_url());
        request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        let error = provisioner
            .create_connection(request)
            .await
            .expect_err("unreachable endpoint must fail");
        assert!(matches!(
            error,
            EggpoolError::Probe(ProbeReason::Unreachable)
        ));
        assert!(credential_store.list().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn generic_oversized_catalog_is_rejected() {
        let _master = MasterKeyGuard::new("generic-oversized-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_status_server(200, oversized_catalog_body(), Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));

        let mut request = generic_request("eggpool");
        request.endpoint = Some(host);
        request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        let error = provisioner
            .create_connection(request)
            .await
            .expect_err("oversized catalog must fail");
        server.join().expect("fake server joins");
        assert!(matches!(
            error,
            EggpoolError::Probe(ProbeReason::CatalogOversized)
        ));
        assert!(credential_store.list().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn generic_cancellation_compensates_operation_owned_credential() {
        let _master = MasterKeyGuard::new("generic-cancel-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_status_server(
            200,
            r#"{"data":[{"id":"slow-model"}]}"#.to_string(),
            Duration::from_millis(250),
        );
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));
        let mut request = generic_request("eggpool");
        request.endpoint = Some(host.clone());
        request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        request.operation_id = Some("prov-generic-cancel".to_string());
        let running = provisioner.clone();
        let task = tokio::spawn(async move { running.create_connection(request).await });
        tokio::time::sleep(Duration::from_millis(25)).await;
        let mut cancelled = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            if provisioner.cancel("prov-generic-cancel") {
                cancelled = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(cancelled, "provisioning operation was not registered");
        let result = task.await.expect("provision task joins");
        assert!(matches!(result, Err(EggpoolError::Cancelled)));
        let wake_address = host.strip_prefix("http://").unwrap_or(&host);
        let _ = std::net::TcpStream::connect(wake_address);
        let _ = server.join();
        assert!(credential_store.list().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn generic_restart_reconciliation_fails_staged_and_cleans_credential() {
        let _master = MasterKeyGuard::new("generic-reconcile-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        // Simulate a crash between the staged journal write and the final
        // transaction: a staged row plus its operation-owned credential.
        credential_store
            .put(
                "openai",
                Some("stale-account"),
                codegg_providers::CredentialKind::ApiKey,
                "stale-secret",
                None,
                Vec::new(),
            )
            .expect("stage stale credential");
        sqlx::query(
            "INSERT INTO provider_provisioning (operation_id, connection_id, idempotency_key, provider_kind, display_name, endpoint, tls_policy, scope_kind, scope_ref, secret_ref, secret_provider_ref, secret_account_ref, state, time_created, time_updated) VALUES ('op-stale', 'conn-stale', 'key-stale', 'openai', 'Stale', 'https://api.openai.com/v1', 'required', 'personal', 'local-user', 'secret-ref', 'openai', 'stale-account', 'staged', 0, 0)",
        )
        .execute(&pool)
        .await
        .expect("stage stale row");

        // A fresh provisioner (restart equivalent) reconciles before its own
        // create: the staged row fails closed and its credential is removed,
        // while the new ordinary provision succeeds. The new provision uses a
        // local fake for `/models` discovery so it never dials the real
        // upstream.
        let body = r#"{"data":[{"id":"model-a","name":"Model A"}]}"#.to_string();
        let (host, server) = fake_status_server(200, body, Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));
        let mut request = generic_request("openai");
        request.endpoint = Some(host);
        request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        let result = provisioner
            .create_connection(request)
            .await
            .expect("post-restart provision succeeds");
        server.join().expect("fake server joins");
        assert_eq!(result.connection.provider_kind, "openai");

        let (state, code): (String, Option<String>) = sqlx::query_as(
            "SELECT state, failure_code FROM provider_provisioning WHERE operation_id = 'op-stale'",
        )
        .fetch_one(&pool)
        .await
        .expect("stale row");
        assert_eq!(state, "failed");
        assert_eq!(code.as_deref(), Some("daemon_restarted"));
        assert!(
            credential_store
                .list()
                .iter()
                .all(|record| record.account_id.as_deref() != Some("stale-account")),
            "stale operation-owned credential must be cleaned"
        );
    }

    #[test]
    fn generic_unsupported_provider_and_credential_kind_are_typed() {
        let unknown = normalize_generic(&generic_request("no_such_provider"))
            .expect_err("unknown provider must be rejected");
        assert!(matches!(unknown, EggpoolError::UnsupportedProvider(_)));

        let mut bearer_for_api_key_only = generic_request("openai");
        bearer_for_api_key_only.credential_kind = ProviderCredentialKind::Bearer;
        let kind_error = normalize_generic(&bearer_for_api_key_only)
            .expect_err("bearer must be rejected for an API-key-only provider");
        assert!(matches!(
            kind_error,
            EggpoolError::UnsupportedCredentialKind
        ));

        let mut endpoint_for_fixed = generic_request("mistral");
        endpoint_for_fixed.endpoint = Some("https://mistral.example/v1".to_string());
        assert!(matches!(
            normalize_generic(&endpoint_for_fixed),
            Err(EggpoolError::InvalidEndpoint(_))
        ));

        let mut missing_endpoint = generic_request("custom");
        missing_endpoint.tls_policy = Some(ProviderTlsPolicy::Disabled);
        assert!(matches!(
            normalize_generic(&missing_endpoint),
            Err(EggpoolError::InvalidEndpoint(_))
        ));

        let secret_leak = normalize_generic(&generic_request("azure"))
            .expect_err("azure without an endpoint must fail");
        assert!(!format!("{secret_leak:?}").contains("generic-test-key"));
    }
    #[test]
    fn direct_probe_error_mapping_is_bounded() {
        use codegg_providers::ProviderError;
        assert_eq!(
            map_provider_error_reason(&ProviderError::Auth("x".to_string())),
            ProbeReason::AuthenticationFailed
        );
        assert_eq!(
            map_provider_error_reason(&ProviderError::Timeout("x".to_string())),
            ProbeReason::Timeout
        );
        assert_eq!(
            map_provider_error_reason(&ProviderError::RateLimit),
            ProbeReason::UnsupportedApi
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rotation_uses_the_connection_credential_namespace() {
        use codegg_core::identity::ProviderConnectionId;

        let _master = MasterKeyGuard::new("generic-rotation-namespace-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));

        // Provision a non-Eggpool row through the generic service.
        let body = r#"{"data":[{"id":"rot-model"}]}"#.to_string();
        let (host, server) = fake_status_server(200, body, Duration::ZERO);
        let mut create_request = generic_request("custom");
        create_request.endpoint = Some(host);
        create_request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        create_request.credential_kind = ProviderCredentialKind::Bearer;
        let created = provisioner
            .create_connection(create_request)
            .await
            .expect("custom provision succeeds");
        server.join().expect("fake server joins");
        assert_eq!(created.connection.provider_kind, "openai_compatible");
        let connection_id = ProviderConnectionId::parse(&created.connection.id).unwrap();

        // Endpoint-only rotation must resolve the stored credential through
        // the connection's own namespace (`custom`, not `eggpool`) and keep
        // exactly one credential record there.
        let (rotated_host, rotated_server) = fake_status_server(
            200,
            r#"{"data":[{"id":"rot-model"}]}"#.to_string(),
            Duration::ZERO,
        );
        let status = provisioner
            .rotate(
                "rotation-namespace",
                &connection_id,
                created.connection.revision,
                ConnectionRotateChange::EndpointOnly {
                    endpoint: rotated_host,
                    tls_policy: "disabled".to_owned(),
                    display_name: None,
                },
                SecretInputRef::new("unused-rotation-handle").unwrap(),
                false,
            )
            .await
            .expect("endpoint-only rotation succeeds");
        rotated_server.join().expect("rotation fake server joins");

        assert_eq!(status.state, "committed");
        let records = credential_store.list();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].provider_id, "custom");
    }

    // ── M010 provider connection qualification semantics ────────────────────
    //
    // Catalog reachability and credential verification are separate facts.
    // These tests pin the separation against the real migration, the real
    // provisioning path, and the real inference-feedback writer.

    /// Read the durable credential verdict for a connection revision.
    async fn credential_status_of(
        pool: &sqlx::SqlitePool,
        connection_id: &str,
        revision: u64,
    ) -> Option<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT credential_status FROM provider_connection_health \
             WHERE connection_id = ? AND revision = ?",
        )
        .bind(connection_id)
        .bind(i64::try_from(revision).unwrap_or(i64::MAX))
        .fetch_optional(pool)
        .await
        .expect("credential status row")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn connection_summary_carries_a_bounded_masked_credential_tail() {
        // Two connections can share a display name and differ only by bound
        // key, so the picker needs a discriminator. It must be a hint, not the
        // key: exactly four trailing characters, computed daemon-side, never
        // crossing the wire in full and never logged.
        let _master = MasterKeyGuard::new("summary-credential-hint-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store));
        let result = provisioner
            .create(request(&host))
            .await
            .expect("provision succeeds");
        server.join().expect("fake server joins");

        // `request()` stages "test-key"; only its last four characters hint.
        let secret = "test-key";
        let summary = provisioner
            .list()
            .await
            .expect("connection list renders")
            .into_iter()
            .find(|connection| connection.id == result.connection.id)
            .expect("the provisioned connection is listed");

        let hint = summary
            .credential_hint
            .clone()
            .expect("a bound credential hints");
        assert_eq!(hint, "…-key");
        assert_eq!(
            hint.trim_start_matches('\u{2026}').chars().count(),
            codegg_providers::CREDENTIAL_HINT_TAIL_LEN
        );
        assert!(
            !storage_rows_contain(&pool, secret).await,
            "the plaintext must never reach a durable row"
        );
        let wire = serde_json::to_string(&summary).expect("summary serializes");
        assert!(wire.contains("credential_hint"));
        assert!(
            !wire.contains(secret),
            "only the bounded tail may cross the wire, never the key"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn connection_summary_omits_the_hint_without_a_credential_binding() {
        let _master = MasterKeyGuard::new("summary-no-hint-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store));
        let result = provisioner
            .create(request(&host))
            .await
            .expect("provision succeeds");
        server.join().expect("fake server joins");

        // `secret_ref` is NOT NULL, so "no usable binding" is modelled as a
        // binding that resolves to nothing — the same state a purged credential
        // leaves behind.
        sqlx::query("UPDATE provider_connections SET secret_ref = 'ref-absent', secret_account_ref = 'account-absent' WHERE id = ?")
            .bind(result.connection.id.as_str())
            .execute(&pool)
            .await
            .unwrap();
        let summary = provisioner
            .list()
            .await
            .expect("connection list renders without a binding")
            .into_iter()
            .find(|connection| connection.id == result.connection.id)
            .expect("the provisioned connection is listed");

        assert_eq!(summary.credential_hint, None, "no binding means no hint");
        let wire = serde_json::to_string(&summary).expect("summary serializes");
        assert!(
            !wire.contains("credential_hint"),
            "an absent hint must be omitted from the wire, not sent as null or empty"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn static_catalog_connection_is_never_reported_as_verified() {
        // `openai` catalog discovery proves only that the catalog is usable.
        // The M010 defect was that this also claimed the credential was valid.
        // Discovery requires a live `/models` endpoint, so serve it from a
        // local fake: no outside network, same `ProviderCatalog` probe path.
        let _master = MasterKeyGuard::new("m010-static-catalog-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let body = r#"{"data":[{"id":"model-a","name":"Model A"}]}"#.to_string();
        let (host, server) = fake_status_server(200, body, Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store));

        let mut request = generic_request("openai");
        request.endpoint = Some(host);
        request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        let result = provisioner
            .create_connection(request)
            .await
            .expect("static-catalog provision succeeds");
        server.join().expect("fake server joins");
        let health = result
            .connection
            .health
            .as_ref()
            .expect("provisioning records health");

        // Catalog axis: reachable.
        assert_eq!(health.status, "healthy");
        // Credential axis: explicitly unknown, never "verified".
        assert_eq!(
            health.credential_status.as_deref(),
            Some("unverified"),
            "catalog discovery must not verify a credential"
        );
        assert_ne!(health.credential_status.as_deref(), Some("verified"));
        assert_eq!(
            credential_status_of(&pool, &result.connection.id, result.connection.revision).await,
            Some("unverified".to_string())
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn accepted_completion_is_the_only_thing_that_verifies_the_credential() {
        // `custom` uses the strict compatible `/models` probe plus a bounded
        // verification completion. Only the completion proves the key: the
        // catalog 2xx on its own is worth nothing (see the sibling test).
        let _master = MasterKeyGuard::new("m010-verified-completion-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_status_server(
            200,
            r#"{"data":[{"id":"custom-model"}]}"#.to_string(),
            Duration::ZERO,
        );
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store));

        let mut request = generic_request("custom");
        request.endpoint = Some(host.clone());
        request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        request.credential_kind = ProviderCredentialKind::Bearer;
        let result = provisioner
            .create_connection(request)
            .await
            .expect("compatible provision succeeds");
        let raw = server.join().expect("fake server joins");

        assert!(
            raw.contains("/models"),
            "the catalog request must still be made"
        );
        assert!(
            raw.contains("/chat/completions"),
            "verification must spend a real completion, not a metadata read"
        );
        assert_eq!(
            result
                .connection
                .health
                .as_ref()
                .and_then(|health| health.credential_status.as_deref()),
            Some("verified"),
            "an accepted completion is genuine positive credential evidence"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_catalog_that_answers_any_key_never_verifies_the_credential() {
        // The measured defect: the real OpenCode Go gateway answers `200` to
        // `GET /models` even with no `Authorization` header, so a key with no
        // active entitlement sails through catalog discovery and then fails
        // every completion with 403. Provisioning must refuse it rather than
        // record `verified` and let the user find out nineteen seconds later.
        let _master = MasterKeyGuard::new("m010-catalog-is-not-credential-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_catalog_ok_completion_forbidden(
            r#"{"data":[{"id":"opencode-go-model"}]}"#.to_string(),
        );
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store));

        let mut request = generic_request("custom");
        request.endpoint = Some(host);
        request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        request.credential_kind = ProviderCredentialKind::Bearer;
        let error = provisioner
            .create_connection(request)
            .await
            .expect_err("a completion-rejected credential must not provision");
        let raw = server.join().expect("fake server joins");

        assert!(
            raw.contains("/models"),
            "the catalog endpoint really did answer 200"
        );
        assert!(
            raw.contains("/chat/completions"),
            "the rejection has to come from a real completion"
        );
        assert!(
            matches!(
                error,
                EggpoolError::Probe(ProbeReason::AuthenticationFailed)
            ),
            "a completion-level 401/403 must surface as a credential failure, got {error:?}"
        );
        let verified: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM provider_connection_health WHERE credential_status = 'verified'",
        )
        .fetch_one(&pool)
        .await
        .expect("verified count");
        assert_eq!(
            verified, 0,
            "a catalog 2xx must never persist as a verified credential"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_empty_catalog_never_claims_a_verified_credential() {
        // With no model there is nothing to run a verification completion on.
        // The honest verdict is "no evidence", never `Verified`.
        let _master = MasterKeyGuard::new("m010-empty-catalog-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) = fake_status_server(200, r#"{"data":[]}"#.to_string(), Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store));

        let mut request = generic_request("custom");
        request.endpoint = Some(host);
        request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        request.credential_kind = ProviderCredentialKind::Bearer;
        let error = provisioner
            .create_connection(request)
            .await
            .expect_err("an empty catalog must not provision");
        server.join().expect("fake server joins");

        assert!(
            matches!(error, EggpoolError::Probe(ProbeReason::EmptyCatalog)),
            "an empty catalog is a catalog fact, got {error:?}"
        );
        assert_eq!(
            pick_verification_model(Some("openai_compatible"), &[]),
            None
        );
        let verified: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM provider_connection_health WHERE credential_status = 'verified'",
        )
        .fetch_one(&pool)
        .await
        .expect("verified count");
        assert_eq!(verified, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejected_credential_fails_provisioning_without_a_health_row() {
        // A typed 401 from the authenticated probe is a real rejection: it
        // must fail, and it must not leave a durable "verified" verdict.
        let _master = MasterKeyGuard::new("m010-rejected-credential-master");
        let directory = tempdir().expect("credential tempdir");
        let credential_store = Arc::new(
            codegg_providers::CredentialStore::at_path(directory.path().join("credentials.json"))
                .expect("credential store"),
        );
        let pool = migrated_pool().await;
        let (host, server) =
            fake_status_server(403, r#"{"error":"forbidden"}"#.to_string(), Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store));

        let mut request = generic_request("custom");
        request.endpoint = Some(host);
        request.tls_policy = Some(ProviderTlsPolicy::Disabled);
        request.credential_kind = ProviderCredentialKind::Bearer;
        let error = provisioner
            .create_connection(request)
            .await
            .expect_err("rejected credential must fail provisioning");
        server.join().expect("fake server joins");

        assert!(matches!(
            error,
            EggpoolError::Probe(ProbeReason::AuthenticationFailed)
        ));
        let verified: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM provider_connection_health WHERE credential_status = 'verified'",
        )
        .fetch_one(&pool)
        .await
        .expect("verified count");
        assert_eq!(
            verified, 0,
            "a rejected credential must never persist as verified"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inference_outcome_promotes_and_demotes_within_one_revision() {
        // Real inference is the first authenticated signal a catalog-only
        // provider connection produces.
        let _master = MasterKeyGuard::new("m010-inference-feedback-master");
        let pool = migrated_pool().await;
        sqlx::query(
            "INSERT INTO provider_connections (id, provider_kind, display_name, endpoint, tls_policy, scope_kind, scope_ref, secret_ref, secret_provider_ref, secret_account_ref, state, revision, time_created, time_updated) \
             VALUES ('conn-1', 'openai', 'OpenAI', 'https://api.openai.com/v1', 'system', 'personal', 'local-user', 'sr', 'openai', 'acct', 'active', 1, 0, 0)",
        )
        .execute(&pool)
        .await
        .expect("connection row");
        sqlx::query(
            "INSERT INTO provider_connection_health (connection_id, revision, status, credential_status, duration_ms, checked_at) \
             VALUES ('conn-1', 1, 'healthy', 'unverified', 5, 0)",
        )
        .execute(&pool)
        .await
        .expect("health row");

        let reporter =
            super::super::provider_qualification::ProviderConnectionCredentialReporter::new(
                pool.clone(),
                Arc::from("conn-1"),
                1,
            );

        // 1. An inconclusive (transient) outcome writes nothing at all.
        assert!(
            !reporter
                .record(
                    super::super::provider_qualification::InferenceCredentialOutcome::Inconclusive
                )
                .await
        );
        assert_eq!(
            credential_status_of(&pool, "conn-1", 1).await,
            Some("unverified".to_string()),
            "a transient failure must not move the credential axis"
        );

        // 2. An authenticated success establishes verified.
        assert!(
            reporter
                .record(
                    super::super::provider_qualification::InferenceCredentialOutcome::Authenticated
                )
                .await
        );
        assert_eq!(
            credential_status_of(&pool, "conn-1", 1).await,
            Some("verified".to_string())
        );

        // 3. A later genuine 401/403 for the same revision marks it rejected.
        assert!(reporter
            .record(
                super::super::provider_qualification::InferenceCredentialOutcome::AuthenticationFailed
            )
            .await);
        assert_eq!(
            credential_status_of(&pool, "conn-1", 1).await,
            Some("authentication_failed".to_string())
        );
        let reason: Option<String> = sqlx::query_scalar(
            "SELECT reason_code FROM provider_connection_health WHERE connection_id = 'conn-1'",
        )
        .fetch_one(&pool)
        .await
        .expect("reason code");
        assert_eq!(reason.as_deref(), Some("credential_authentication_failed"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inference_outcome_for_a_stale_revision_is_discarded() {
        // A verdict describes one credential. Once the connection rotates, a
        // late-arriving turn must not mark the *new* credential as bad.
        let _master = MasterKeyGuard::new("m010-stale-revision-master");
        let pool = migrated_pool().await;
        sqlx::query(
            "INSERT INTO provider_connection_health (connection_id, revision, status, credential_status, duration_ms, checked_at) \
             VALUES ('conn-2', 2, 'healthy', 'unverified', 5, 0)",
        )
        .execute(&pool)
        .await
        .expect("health row");

        let stale = super::super::provider_qualification::ProviderConnectionCredentialReporter::new(
            pool.clone(),
            Arc::from("conn-2"),
            1,
        );
        assert!(
            !stale
                .record(
                    super::super::provider_qualification::InferenceCredentialOutcome::AuthenticationFailed
                )
                .await,
            "a verdict for revision 1 must not match revision 2"
        );
        assert_eq!(
            credential_status_of(&pool, "conn-2", 2).await,
            Some("unverified".to_string())
        );

        let current =
            super::super::provider_qualification::ProviderConnectionCredentialReporter::new(
                pool.clone(),
                Arc::from("conn-2"),
                2,
            );
        assert!(
            current
                .record(
                    super::super::provider_qualification::InferenceCredentialOutcome::Authenticated
                )
                .await
        );
        assert_eq!(
            credential_status_of(&pool, "conn-2", 2).await,
            Some("verified".to_string())
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn legacy_health_rows_migrate_to_the_conservative_unverified_axis() {
        // Additive-migration safety: a row written before the credential axis
        // existed has no authenticated evidence, so it must land on
        // 'unverified' — never a promotion to 'verified'.
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        // Build the pre-v69 shape: the original table without credential_status.
        sqlx::query(
            "CREATE TABLE provider_connection_health (
                connection_id TEXT PRIMARY KEY,
                revision INTEGER NOT NULL CHECK (revision > 0),
                status TEXT NOT NULL CHECK (status IN ('healthy', 'unhealthy')),
                reason_code TEXT,
                duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
                checked_at INTEGER NOT NULL,
                catalog_revision TEXT
            )",
        )
        .execute(&pool)
        .await
        .expect("legacy health table");
        sqlx::query(
            "INSERT INTO provider_connection_health (connection_id, revision, status, duration_ms, checked_at, catalog_revision) \
             VALUES ('legacy-1', 1, 'healthy', 12, 0, 'digest')",
        )
        .execute(&pool)
        .await
        .expect("legacy row");

        sqlx::query(
            "ALTER TABLE provider_connection_health ADD COLUMN credential_status TEXT NOT NULL \
             DEFAULT 'unverified' CHECK (credential_status IN \
             ('verified', 'unverified', 'authentication_failed', 'no_credential_required'))",
        )
        .execute(&pool)
        .await
        .expect("v69 add column");

        let migrated: String =
            sqlx::query_scalar("SELECT credential_status FROM provider_connection_health WHERE connection_id = 'legacy-1'")
                .fetch_one(&pool)
                .await
                .expect("migrated legacy row");
        assert_eq!(migrated, "unverified");

        // The CHECK constraint rejects a value outside the typed contract.
        let rejected = sqlx::query(
            "UPDATE provider_connection_health SET credential_status = 'healthy' WHERE connection_id = 'legacy-1'",
        )
        .execute(&pool)
        .await;
        assert!(
            rejected.is_err(),
            "credential_status must reject values outside the typed contract"
        );
    }
}
