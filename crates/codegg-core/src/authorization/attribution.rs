//! Durable originating-principal attribution and its concrete store.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::{AuthorizationDecision, PolicyKind};
use crate::error::StorageError;
use crate::identity::PrincipalId;
use crate::team::PrincipalKind;
use crate::transport_auth::{AuthMethod, AuthenticatedPrincipal, TransportClass};

/// Immutable originating-principal attribution for durable work.
///
/// Built by the daemon boundary from the transport-bound principal plus
/// the captured [`AuthorizationDecision`]. The origin never changes for
/// the lifetime of the attributed scope; continuation and cancellation
/// rules consume the captured policy/revision instead of re-resolving
/// authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OriginAttribution {
    pub origin_principal: PrincipalId,
    pub origin_kind: PrincipalKind,
    pub auth_method: AuthMethod,
    pub transport_class: TransportClass,
    pub policy: PolicyKind,
    pub membership_revision: Option<u64>,
    pub decision_id: String,
    pub correlation_id: String,
    pub created_at_ms: i64,
}

impl OriginAttribution {
    /// Build attribution from transport authority plus its decision.
    ///
    /// The principal comes from the bound [`AuthenticatedPrincipal`];
    /// `decision` supplies the captured policy context. Callers cannot
    /// substitute a payload-supplied identity.
    pub fn from_authority(
        principal: &AuthenticatedPrincipal,
        decision: &AuthorizationDecision,
    ) -> Self {
        Self {
            origin_principal: principal.principal_id().clone(),
            origin_kind: principal.kind(),
            auth_method: principal.auth_method(),
            transport_class: principal.transport_class(),
            policy: decision.policy,
            membership_revision: decision.membership_revision,
            decision_id: decision.decision_id.clone(),
            correlation_id: decision.correlation_id.clone(),
            created_at_ms: now_millis(),
        }
    }

    /// Explicit legacy provenance for pre-M003 records that carry no
    /// canonical principal.
    ///
    /// The provenance marker is the literal `"legacy-local"` principal
    /// namespace: it records that the work predates attribution without
    /// fabricating a team identity for it.
    pub fn legacy_local(correlation_id: impl Into<String>) -> Self {
        Self {
            origin_principal: PrincipalId::parse("legacy-local")
                .expect("legacy-local satisfies the identity lexical contract"),
            origin_kind: PrincipalKind::LocalOwner,
            auth_method: AuthMethod::LocalOwner,
            transport_class: TransportClass::Local,
            policy: PolicyKind::LocalOwnerBroad,
            membership_revision: None,
            decision_id: "legacy-local".to_owned(),
            correlation_id: correlation_id.into(),
            created_at_ms: now_millis(),
        }
    }

    /// `true` for the explicit legacy provenance marker (not a team grant).
    pub fn is_legacy(&self) -> bool {
        self.origin_principal.as_str() == "legacy-local"
    }
}

/// Durable store for originating-principal attribution.
///
/// One row per attributed scope `(scope_kind, scope_id)`. The first
/// attribution wins (`INSERT ... ON CONFLICT DO NOTHING`): origin is
/// immutable, and a concurrent second writer cannot rewrite it. Scopes
/// name sessions, turns, runs, jobs, worktrees, and provider selections.
#[derive(Clone)]
pub struct OriginAttributionStore {
    pool: SqlitePool,
}

impl OriginAttributionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Record the origin for `scope`. The first write wins; later calls
    /// for the same scope return the stored row unchanged.
    pub async fn record(
        &self,
        scope_kind: &str,
        scope_id: &str,
        attribution: &OriginAttribution,
    ) -> Result<OriginAttribution, StorageError> {
        validate_attribution_scope(scope_kind, scope_id)?;
        let json = serde_json::to_string(attribution)
            .map_err(|e| StorageError::Database(e.to_string()))?;
        sqlx::query(
            "INSERT INTO origin_attribution (scope_kind, scope_id, origin_principal, \
             origin_kind, auth_method, transport_class, policy, membership_revision, \
             decision_id, correlation_id, time_created, attribution_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(scope_kind, scope_id) DO NOTHING",
        )
        .bind(scope_kind)
        .bind(scope_id)
        .bind(attribution.origin_principal.as_str())
        .bind(origin_kind_str(attribution.origin_kind))
        .bind(auth_method_str(attribution.auth_method))
        .bind(transport_class_str(attribution.transport_class))
        .bind(attribution.policy.as_str())
        .bind(attribution.membership_revision.map(|r| r as i64))
        .bind(&attribution.decision_id)
        .bind(&attribution.correlation_id)
        .bind(attribution.created_at_ms)
        .bind(&json)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        self.get(scope_kind, scope_id)
            .await?
            .ok_or_else(|| StorageError::Database("origin attribution write lost".to_owned()))
    }

    /// Fetch the stored origin for `scope`, if any.
    pub async fn get(
        &self,
        scope_kind: &str,
        scope_id: &str,
    ) -> Result<Option<OriginAttribution>, StorageError> {
        validate_attribution_scope(scope_kind, scope_id)?;
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT attribution_json FROM origin_attribution WHERE scope_kind = ? AND scope_id = ?",
        )
        .bind(scope_kind)
        .bind(scope_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        row.map(|(json,)| {
            serde_json::from_str(&json).map_err(|e| StorageError::Database(e.to_string()))
        })
        .transpose()
    }
}

fn validate_attribution_scope(scope_kind: &str, scope_id: &str) -> Result<(), StorageError> {
    const ALLOWED_KINDS: [&str; 6] = ["session", "turn", "run", "job", "worktree", "provider"];
    if !ALLOWED_KINDS.contains(&scope_kind) {
        return Err(StorageError::Database(format!(
            "unknown attribution scope kind {scope_kind:?}"
        )));
    }
    if scope_id.is_empty() || scope_id.len() > 128 {
        return Err(StorageError::Database(
            "attribution scope id must be non-empty and bounded".to_owned(),
        ));
    }
    if scope_id.contains('/') || scope_id.contains('\\') || scope_id.contains('\0') {
        return Err(StorageError::Database(
            "attribution scope id must not be path-like".to_owned(),
        ));
    }
    Ok(())
}

fn origin_kind_str(kind: PrincipalKind) -> &'static str {
    kind.as_str()
}

fn auth_method_str(method: AuthMethod) -> &'static str {
    method.as_str()
}

fn transport_class_str(class: TransportClass) -> &'static str {
    class.as_str()
}

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(i64::MAX))
        .unwrap_or(0)
}
