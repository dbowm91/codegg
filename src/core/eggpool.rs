//! Daemon-owned Eggpool connection provisioning.
//!
//! This module is intentionally a narrow vertical slice over the existing
//! encrypted credential store and provider-connection metadata. It owns the
//! cross-store sequence, bounded probe, cancellation registry, and redacted
//! protocol projections. Session/model selection remains elsewhere.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use codegg_core::identity::{PrincipalId, ProjectId, ProviderConnectionId};
use codegg_core::provider_connections::{
    Endpoint, ProviderConnection, ProviderConnectionState, ProviderScope, SecretRef, TlsPolicy,
};
use codegg_protocol::provider::{
    ConnectionHealthDto, ConnectionProvisioningStatusDto, CreateEggpoolConnectionRequest,
    CreateEggpoolConnectionResult, EggpoolConnectionScope, EggpoolTlsPolicy,
    ProviderConnectionSummaryDto, ProviderModelDto,
};

const DEFAULT_PORT: u16 = 11_300;
const WORKFLOW_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Error)]
pub enum EggpoolError {
    #[error("invalid Eggpool endpoint: {0}")]
    InvalidEndpoint(String),
    #[error("invalid connection scope: {0}")]
    InvalidScope(String),
    #[error("credential store unavailable")]
    CredentialStore,
    #[error("master key is not configured")]
    MasterKeyMissing,
    #[error("connection provisioning conflict")]
    Conflict,
    #[error("connection provisioning was cancelled")]
    Cancelled,
    #[error("Eggpool probe failed: {0:?}")]
    Probe(ProbeReason),
    #[error("connection persistence failed")]
    Storage,
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
    endpoint: Endpoint,
    tls_policy: TlsPolicy,
    scope: ProviderScope,
    display_name: String,
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
}

#[derive(Clone)]
pub struct EggpoolProvisioner {
    pool: sqlx::SqlitePool,
    credential_store: Option<Arc<codegg_providers::CredentialStore>>,
    operations: Arc<DashMap<String, CancellationToken>>,
    reconciled: Arc<AtomicBool>,
}

impl EggpoolProvisioner {
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
            reconciled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn create(
        &self,
        request: CreateEggpoolConnectionRequest,
    ) -> Result<CreateEggpoolConnectionResult, EggpoolError> {
        self.reconcile_once().await;
        let spec = normalize(&request)?;
        let operation_id = request
            .operation_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("prov-{}", uuid::Uuid::new_v4()));
        let connection_id = ProviderConnectionId::new();
        let account_id = connection_id.as_str().to_owned();
        let secret_ref = SecretRef::new();
        let provider_ref = "eggpool";
        let idempotency_key = idempotency_key(&spec, &request.scope);
        let scope_parts = storage_scope(&spec.scope);

        if sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM provider_connections WHERE provider_kind = 'eggpool' AND endpoint = ? AND tls_policy = ? AND scope_kind = ? AND scope_ref = ? AND state = 'active'",
        )
        .bind(spec.endpoint.as_str())
        .bind(tls_key(spec.tls_policy))
        .bind(scope_parts.0)
        .bind(scope_parts.1)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| EggpoolError::Storage)?
            > 0
        {
            return Err(EggpoolError::Conflict);
        }

        if sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM provider_provisioning WHERE idempotency_key = ? AND state IN ('staged', 'probing', 'committed')",
        )
        .bind(&idempotency_key)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| EggpoolError::Storage)?
            > 0
        {
            return Err(EggpoolError::Conflict);
        }

        let now = now_millis();
        sqlx::query(
            "INSERT INTO provider_provisioning (operation_id, connection_id, idempotency_key, provider_kind, display_name, endpoint, tls_policy, scope_kind, scope_ref, secret_ref, secret_provider_ref, secret_account_ref, state, time_created, time_updated) VALUES (?, ?, ?, 'eggpool', ?, ?, ?, ?, ?, ?, ?, ?, 'staged', ?, ?)",
        )
        .bind(&operation_id)
        .bind(connection_id.as_str())
        .bind(&idempotency_key)
        .bind(&spec.display_name)
        .bind(spec.endpoint.as_str())
        .bind(tls_key(spec.tls_policy))
        .bind(scope_parts.0)
        .bind(scope_parts.1)
        .bind(secret_ref.as_str())
        .bind(provider_ref)
        .bind(&account_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|_| EggpoolError::Conflict)?;

        let cancel = CancellationToken::new();
        self.operations.insert(operation_id.clone(), cancel.clone());
        let result = self
            .create_inner(
                &request,
                &spec,
                &operation_id,
                &connection_id,
                &secret_ref,
                &account_id,
                cancel,
            )
            .await;
        self.operations.remove(&operation_id);
        result
    }

    async fn create_inner(
        &self,
        request: &CreateEggpoolConnectionRequest,
        spec: &NormalizedSpec,
        operation_id: &str,
        connection_id: &ProviderConnectionId,
        secret_ref: &SecretRef,
        account_id: &str,
        cancel: CancellationToken,
    ) -> Result<CreateEggpoolConnectionResult, EggpoolError> {
        let store = self
            .credential_store
            .clone()
            .ok_or(EggpoolError::CredentialStore)?;
        if codegg_config::encryption::get_master_key().is_none() {
            self.fail(operation_id, ProbeReason::AuthenticationFailed.code())
                .await;
            return Err(EggpoolError::MasterKeyMissing);
        }
        if store
            .put(
                "eggpool",
                Some(account_id),
                codegg_providers::CredentialKind::ApiKey,
                request.api_key.expose(),
                None,
                Vec::new(),
            )
            .is_err()
        {
            self.fail(operation_id, "credential_store_unavailable")
                .await;
            return Err(EggpoolError::CredentialStore);
        }

        if cancel.is_cancelled() {
            self.compensate(operation_id, account_id, &store, None)
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
                account_id,
                &store,
                Some("connection_storage_error"),
            )
            .await;
            return Err(EggpoolError::Storage);
        }

        let probe = tokio::time::timeout(
            WORKFLOW_TIMEOUT,
            probe(
                spec.endpoint.as_str(),
                request.api_key.expose(),
                cancel.clone(),
            ),
        )
        .await
        .map_err(|_| EggpoolError::Probe(ProbeReason::Timeout))?
        .map_err(EggpoolError::Probe);
        let probe = match probe {
            Ok(value) => value,
            Err(EggpoolError::Probe(ProbeReason::Cancelled)) => {
                self.compensate(operation_id, account_id, &store, None)
                    .await;
                return Err(EggpoolError::Cancelled);
            }
            Err(error) => {
                self.compensate(operation_id, account_id, &store, Some(error_code(&error)))
                    .await;
                return Err(error);
            }
        };

        if cancel.is_cancelled() {
            self.compensate(operation_id, account_id, &store, None)
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
                &probe,
            )
            .await;
        if result.is_err() {
            self.compensate(
                operation_id,
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
        probe: &ProbeResult,
    ) -> Result<CreateEggpoolConnectionResult, EggpoolError> {
        let mut tx = self.pool.begin().await.map_err(|_| EggpoolError::Storage)?;
        let (scope_kind, scope_ref) = storage_scope(&spec.scope);
        sqlx::query(
            "INSERT INTO provider_connections (id, provider_kind, display_name, endpoint, tls_policy, scope_kind, scope_ref, secret_ref, secret_provider_ref, secret_account_ref, state, revision, time_created, time_updated) VALUES (?, 'eggpool', ?, ?, ?, ?, ?, ?, 'eggpool', ?, 'active', 1, ?, ?)",
        )
        .bind(connection_id.as_str())
        .bind(&spec.display_name)
        .bind(spec.endpoint.as_str())
        .bind(tls_key(spec.tls_policy))
        .bind(scope_kind)
        .bind(scope_ref)
        .bind(secret_ref.as_str())
        .bind(account_id)
        .bind(now_millis())
        .bind(now_millis())
        .execute(&mut *tx)
        .await
        .map_err(|_| EggpoolError::Conflict)?;
        sqlx::query("INSERT INTO provider_connection_health (connection_id, revision, status, duration_ms, checked_at, catalog_revision) VALUES (?, 1, 'healthy', ?, ?, ?)")
            .bind(connection_id.as_str())
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
        account_id: &str,
        store: &codegg_providers::CredentialStore,
        failure_code: Option<&str>,
    ) {
        let _ = store.remove("eggpool", Some(account_id));
        let (state, code) =
            failure_code.map_or(("cancelled", "cancelled"), |code| ("failed", code));
        let _ = sqlx::query("UPDATE provider_provisioning SET state = ?, failure_code = ?, time_updated = ? WHERE operation_id = ? AND state <> 'committed'")
            .bind(state)
            .bind(code)
            .bind(now_millis())
            .bind(operation_id)
            .execute(&self.pool)
            .await;
    }

    async fn fail(&self, operation_id: &str, code: &str) {
        let _ = sqlx::query("UPDATE provider_provisioning SET state = 'failed', failure_code = ?, time_updated = ? WHERE operation_id = ?")
            .bind(code)
            .bind(now_millis())
            .bind(operation_id)
            .execute(&self.pool)
            .await;
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

    async fn summary(
        &self,
        connection: &ProviderConnection,
        catalog_revision: Option<&str>,
        probe: Option<&ProbeResult>,
    ) -> Result<ProviderConnectionSummaryDto, EggpoolError> {
        let health_row = sqlx::query_as::<_, (String, Option<String>, i64, i64, Option<String>)>(
            "SELECT status, reason_code, checked_at, duration_ms, catalog_revision FROM provider_connection_health WHERE connection_id = ?",
        )
        .bind(connection.id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| EggpoolError::Storage)?
        ;
        let catalog_from_health = health_row.as_ref().and_then(|row| row.4.clone());
        let health = health_row.map(|(status, reason_code, checked_at, duration_ms, _)| {
            ConnectionHealthDto {
                status,
                reason_code,
                checked_at,
                duration_ms: duration_ms as u64,
            }
        });
        let model_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_connection_models WHERE connection_id = ? AND revision = ?")
            .bind(connection.id.as_str())
            .bind(connection.revision as i64)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| EggpoolError::Storage)?;
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
        })
    }

    async fn reconcile_once(&self) {
        if self.reconciled.load(Ordering::Acquire) {
            return;
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
            let _ = store.remove(&provider, Some(&account));
            let _ = sqlx::query("UPDATE provider_provisioning SET state = 'failed', failure_code = 'daemon_restarted', time_updated = ? WHERE operation_id = ?")
                .bind(now_millis())
                .bind(operation_id)
                .execute(&self.pool)
                .await;
        }
        self.reconciled.store(true, Ordering::Release);
    }
}

fn normalize(request: &CreateEggpoolConnectionRequest) -> Result<NormalizedSpec, EggpoolError> {
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
    let parsed = reqwest::Url::parse(&format!("{scheme}{raw}"))
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
    let port = request.port.or(parsed.port()).unwrap_or(DEFAULT_PORT);
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
    let display_name = request.display_name.as_deref().unwrap_or("Eggpool").trim();
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
    Ok(NormalizedSpec {
        endpoint,
        tls_policy: policy,
        scope,
        display_name: display_name.to_owned(),
    })
}

async fn probe(
    endpoint: &str,
    api_key: &str,
    cancel: CancellationToken,
) -> Result<ProbeResult, ProbeReason> {
    let started = Instant::now();
    let probe = codegg_providers::EggpoolProbe::new(
        endpoint,
        codegg_providers::EggpoolApiKey::from(api_key),
        codegg_providers::EggpoolProbeOptions::default(),
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
    Ok(ProbeResult {
        models: summary
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
            .collect(),
        catalog_revision: summary.digest,
        duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
    })
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

fn error_code(error: &EggpoolError) -> &'static str {
    match error {
        EggpoolError::Probe(reason) => reason.code(),
        EggpoolError::Cancelled => "cancelled",
        EggpoolError::CredentialStore => "credential_store_unavailable",
        EggpoolError::MasterKeyMissing => "master_key_missing",
        EggpoolError::InvalidEndpoint(_) => "invalid_endpoint",
        EggpoolError::InvalidScope(_) => "invalid_scope",
        EggpoolError::Conflict => "connection_conflict",
        EggpoolError::Storage => "connection_storage_error",
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
fn idempotency_key(spec: &NormalizedSpec, scope: &EggpoolConnectionScope) -> String {
    let mut h = Sha256::new();
    h.update(spec.endpoint.as_str().as_bytes());
    h.update([0]);
    h.update(format!("{scope:?}").as_bytes());
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
fn state_key(state: ProviderConnectionState) -> &'static str {
    match state {
        ProviderConnectionState::Active => "active",
        ProviderConnectionState::Disabled => "disabled",
        ProviderConnectionState::CredentialMissing => "credential_missing",
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
    use codegg_protocol::provider::{EggpoolConnectionScope, EggpoolTlsPolicy, SecretInput};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use tempfile::tempdir;

    struct MasterKeyGuard {
        previous: [Option<String>; 3],
        _env_lock: std::sync::MutexGuard<'static, ()>,
    }

    impl MasterKeyGuard {
        fn new(value: &str) -> Self {
            let env_lock = crate::auth::test_support::lock_env();
            let names = [
                "CODEGG_MASTER_KEY",
                "CODEGG_ENCRYPTION_KEY",
                "OPENCODE_ENCRYPTION_KEY",
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

    fn fake_eggpool(delay: Duration) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fake Eggpool");
        listener
            .set_nonblocking(true)
            .expect("configure fake Eggpool listener");
        let address = listener.local_addr().expect("fake Eggpool address");
        let join = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() >= deadline {
                            return "no-request".to_string();
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("accept fake Eggpool request: {error}"),
                }
            };
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
            let body = r#"{"data":[{"id":"eggpool-model","name":"Eggpool Model"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            String::from_utf8_lossy(&request).into_owned()
        });
        (format!("http://{address}"), join)
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

    #[test]
    fn omitted_port_uses_eggpool_default_and_v1_path() {
        let spec = normalize(&request("127.0.0.1")).unwrap();
        assert_eq!(spec.endpoint.as_str(), "http://127.0.0.1:11300/v1");
    }

    #[test]
    fn conflicting_ports_are_rejected() {
        let mut req = request("http://127.0.0.1:9000");
        req.port = Some(9001);
        assert!(matches!(
            normalize(&req),
            Err(EggpoolError::InvalidEndpoint(_))
        ));
    }

    #[test]
    fn tls_and_explicit_port_matrix_is_deterministic() {
        let mut req = request("https://eggpool.example:9443/v1/");
        req.port = Some(9443);
        req.tls_policy = EggpoolTlsPolicy::Required;
        assert_eq!(
            normalize(&req).unwrap().endpoint.as_str(),
            "https://eggpool.example:9443/v1"
        );

        let mut optional = request("eggpool.example");
        optional.tls_policy = EggpoolTlsPolicy::Optional;
        assert_eq!(
            normalize(&optional).unwrap().endpoint.as_str(),
            "https://eggpool.example:11300/v1"
        );

        let mut required_http = request("http://eggpool.example");
        required_http.tls_policy = EggpoolTlsPolicy::Required;
        assert!(matches!(
            normalize(&required_http),
            Err(EggpoolError::InvalidEndpoint(_))
        ));

        let disabled_https = request("https://eggpool.example");
        assert!(matches!(
            normalize(&disabled_https),
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
                normalize(&request(host)),
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
        let (host, server) = fake_eggpool(Duration::ZERO);
        let provisioner =
            EggpoolProvisioner::with_credential_store(pool.clone(), Some(credential_store.clone()));
        let mut create_request = request(&host);
        create_request.operation_id = Some("prov-success".to_string());
        let result = provisioner
            .create(create_request)
            .await
            .expect("provision succeeds");
        let raw_request = server.join().expect("fake server joins");

        assert_eq!(result.connection.endpoint, format!("{host}/v1"));
        assert_eq!(result.models.len(), 1);
        assert_eq!(result.connection.model_count, 1);
        assert!(raw_request.contains("/v1/models"));
        assert!(raw_request.contains("authorization: Bearer test-key"));
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

        let duplicate = provisioner
            .create(request(&host))
            .await
            .expect_err("equivalent connection must conflict");
        assert!(matches!(duplicate, EggpoolError::Conflict));
    }

    #[tokio::test(flavor = "current_thread")]
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
        for _ in 0..100 {
            if provisioner.cancel("prov-cancel") {
                cancelled = true;
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(cancelled, "provisioning operation was not registered");
        let result = task.await.expect("provision task joins");
        assert!(matches!(result, Err(EggpoolError::Cancelled)));
        // The client may be cancelled before the HTTP request reaches the
        // listener. Dropping the handle intentionally detaches this bounded
        // test fixture rather than making cleanup depend on socket timing.
        drop(server);

        let active: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM provider_connections WHERE state = 'active'")
                .fetch_one(&pool)
                .await
                .expect("active count");
        assert_eq!(active, 0);
        assert!(credential_store.list().is_empty());
    }
}
