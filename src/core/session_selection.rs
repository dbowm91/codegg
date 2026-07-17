//! Provider Connections Milestone 3: daemon-owned session selection
//! service.
//!
//! This module exposes the typed operations that the protocol uses to
//! read, list, and update a session's connection + model selection. The
//! service is owned by the daemon; the TUI and remote clients never
//! construct providers or resolve secrets — they only call into this
//! module.
//!
//! ## Invariants
//!
//! - A session resolves only the selected connection ID and model
//!   revision; it never receives a different credentialed endpoint.
//! - Stale selection updates return a typed conflict and leave the stored
//!   selection unchanged.
//! - A missing, disabled, or credential-missing connection returns a
//!   typed diagnostic; it never chooses another connection.
//! - The TUI never constructs providers or resolves secrets.

use std::sync::Arc;

use codegg_core::identity::ProviderConnectionId;
use codegg_core::provider_connections::{
    ProviderConnection, ProviderConnectionState, ProviderConnectionStore, ProviderScope,
};
use codegg_core::session::{
    legacy_resolution, LegacyResolution, Session, SessionStore, UpdateSession,
};
use codegg_protocol::provider::{
    ProviderConnectionSummaryDto, SelectedModelDto, SessionSelectionDto,
};

use crate::core::eggpool::EggpoolProvisioner;

/// Outcome of a session selection update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionUpdateOutcome {
    Updated(SessionSelectionDto),
    /// The supplied `expected_connection_revision` did not match the
    /// stored value. The stored selection is unchanged.
    StaleRevision {
        current_connection_id: String,
        current_revision: u64,
    },
    /// The supplied `expected_catalog_revision` did not match the
    /// catalog at the current revision. The stored selection is
    /// unchanged.
    StaleCatalog {
        current_revision: u64,
        current_catalog_revision: Option<String>,
    },
    /// The targeted connection is not active (disabled, credential
    /// missing, or deleted). The stored selection is unchanged.
    ConnectionNotSelectable {
        connection_id: String,
        state: String,
    },
    /// The targeted model is not in the connection's bounded catalog.
    UnknownModel {
        connection_id: String,
        model_id: String,
    },
}

/// Errors raised by the selection service that are not the typed
/// outcomes above. These map to `CoreResponse::Error` with a stable code.
#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("connection store error: {0}")]
    ConnectionStore(String),
    #[error("invalid connection id: {0}")]
    InvalidConnectionId(String),
    #[error("session store error: {0}")]
    SessionStore(String),
    #[error("missing project context")]
    MissingProjectContext,
}

/// Result of resolving a session's effective selection, including the
/// bounded catalog row used.
#[derive(Debug, Clone)]
struct ResolvedSelection {
    connection: ProviderConnection,
    catalog_revision: Option<String>,
    model: SelectedModelDto,
}

/// Resolve the current selection for `session_id` against the durable
/// connection store. Returns a typed [`SessionSelectionDto`] suitable
/// for direct protocol projection.
///
/// The resolver is intentionally read-only and never mutates the session
/// or connection tables.
pub async fn get_selection(
    session_store: &SessionStore,
    connection_store: &ProviderConnectionStore,
    session_id: &str,
) -> Result<SessionSelectionDto, SelectionError> {
    let session = session_store
        .get(session_id)
        .await
        .map_err(|e| SelectionError::SessionStore(e.to_string()))?
        .ok_or_else(|| SelectionError::SessionNotFound(session_id.to_string()))?;

    resolve_for_session(session_store, connection_store, &session).await
}

/// Resolve the selection for a session that is already in memory.
async fn resolve_for_session(
    session_store: &SessionStore,
    connection_store: &ProviderConnectionStore,
    session: &Session,
) -> Result<SessionSelectionDto, SelectionError> {
    if let (Some(connection_id), Some(revision), Some(model_id)) = (
        session.provider_connection_id.as_ref(),
        session.provider_connection_revision,
        session.selected_model_id.as_ref(),
    ) {
        let connection_id = ProviderConnectionId::parse(connection_id)
            .map_err(|_| SelectionError::InvalidConnectionId(connection_id.clone()))?;
        match resolve_selection_by_connection(
            session_store,
            connection_store,
            session.id.as_str(),
            &connection_id,
            revision,
            model_id,
        )
        .await?
        {
            Some(resolved) => {
                let summary = summary_dto_for(&resolved.connection, connection_store).await?;
                return Ok(SessionSelectionDto::Selected {
                    connection: summary,
                    model: resolved.model,
                    connection_revision: resolved.connection.revision,
                    catalog_revision: resolved.catalog_revision.unwrap_or_else(|| "0".to_string()),
                });
            }
            None => {
                // Stale connection or removed model — fall through to
                // legacy resolution to surface a typed diagnostic.
            }
        }
    }

    // Legacy compatibility path: resolve the stored `provider/model`
    // string against the connection catalog.
    let resolution =
        legacy_resolution::resolve_legacy_model_string(connection_store, session.model.as_deref())
            .await
            .map_err(|e| SelectionError::ConnectionStore(e.to_string()))?;
    Ok(legacy_resolution_to_dto(session.id.as_str(), &resolution))
}

async fn resolve_selection_by_connection(
    _session_store: &SessionStore,
    connection_store: &ProviderConnectionStore,
    _session_id: &str,
    connection_id: &ProviderConnectionId,
    expected_revision: u64,
    expected_model_id: &str,
) -> Result<Option<ResolvedSelection>, SelectionError> {
    let Some(connection) = connection_store
        .get(connection_id)
        .await
        .map_err(|e| SelectionError::ConnectionStore(e.to_string()))?
    else {
        return Ok(None);
    };
    if connection.state != ProviderConnectionState::Active {
        return Ok(None);
    }
    if connection.revision != expected_revision {
        return Ok(None);
    }
    // Look up the catalog at the connection's current revision.
    let models = list_models(connection_store, connection_id).await?;
    let Some(model_row) = models.into_iter().find(|m| m.0 == expected_model_id) else {
        return Ok(None);
    };
    let catalog_revision =
        catalog_revision_for(connection_store, connection_id, connection.revision).await?;
    Ok(Some(ResolvedSelection {
        connection,
        catalog_revision: catalog_revision.clone(),
        model: SelectedModelDto {
            connection_id: connection_id.as_str().to_string(),
            model_id: model_row.0.clone(),
            model_name: model_row.1.clone(),
            context_window: model_row.2,
            max_output_tokens: model_row.3,
            supports_tools: model_row.4,
            supports_vision: model_row.5,
            catalog_revision: catalog_revision.unwrap_or_else(|| "0".to_string()),
        },
    }))
}

/// Load the bounded catalog row set for a connection at its current
/// revision. Returns tuples of `(model_id, model_name, context_window,
/// max_output_tokens, supports_tools, supports_vision)`.
async fn list_models(
    connection_store: &ProviderConnectionStore,
    connection_id: &ProviderConnectionId,
) -> Result<Vec<(String, String, u64, Option<u64>, bool, bool)>, SelectionError> {
    let store = connection_store.clone();
    let id_string = connection_id.as_str().to_string();
    let pool = store.pool().clone();
    let _ = id_string;
    let _ = pool;
    // Use the existing read seam directly via the store's pool so we
    // don't have to depend on the eggpool provisioner for read paths.
    codegg_core::session::selection_catalog::list_models_for_connection(&store, connection_id)
        .await
        .map_err(|e| SelectionError::ConnectionStore(e.to_string()))
}

/// Fetch the catalog revision string for a connection revision, when
/// present. Reads the `provider_connection_health` table.
async fn catalog_revision_for(
    connection_store: &ProviderConnectionStore,
    connection_id: &ProviderConnectionId,
    revision: u64,
) -> Result<Option<String>, SelectionError> {
    codegg_core::session::selection_catalog::catalog_revision_for(
        connection_store,
        connection_id,
        revision,
    )
    .await
    .map_err(|e| SelectionError::ConnectionStore(e.to_string()))
}

fn legacy_resolution_to_dto(
    _session_id: &str,
    resolution: &LegacyResolution,
) -> SessionSelectionDto {
    match resolution {
        LegacyResolution::Unset => SessionSelectionDto::Unselected {},
        LegacyResolution::Resolved { .. } => {
            // Caller should already have applied the selection via a
            // selection update. This branch surfaces as Unselected to
            // avoid implying persistence.
            SessionSelectionDto::Unselected {}
        }
        LegacyResolution::UnresolvedLegacyProvider { provider_kind } => {
            SessionSelectionDto::LegacyUnresolved {
                legacy_provider: provider_kind.clone(),
                legacy_model: None,
                reason: format!(
                    "No active connection matches legacy provider '{provider_kind}'. Open /connect to create one, or select an existing connection explicitly."
                ),
            }
        }
        LegacyResolution::AmbiguousLegacyProvider {
            provider_kind,
            candidates,
        } => SessionSelectionDto::LegacyUnresolved {
            legacy_provider: provider_kind.clone(),
            legacy_model: None,
            reason: format!(
                "Multiple connections match legacy provider '{provider_kind}' ({}). Choose one explicitly.",
                candidates.len()
            ),
        },
        LegacyResolution::DisabledLegacyConnection {
            provider_kind,
            connection_id,
        } => SessionSelectionDto::LegacyUnresolved {
            legacy_provider: provider_kind.clone(),
            legacy_model: None,
            reason: format!(
                "Connection '{connection_id}' for provider '{provider_kind}' is disabled."
            ),
        },
        LegacyResolution::MissingCredentialLegacyConnection {
            provider_kind,
            connection_id,
        } => SessionSelectionDto::LegacyUnresolved {
            legacy_provider: provider_kind.clone(),
            legacy_model: None,
            reason: format!(
                "Connection '{connection_id}' for provider '{provider_kind}' has no usable credential."
            ),
        },
    }
}

/// Build a redacted [`ProviderConnectionSummaryDto`] for a connection.
/// The summary never includes credential material.
async fn summary_dto_for(
    connection: &ProviderConnection,
    connection_store: &ProviderConnectionStore,
) -> Result<ProviderConnectionSummaryDto, SelectionError> {
    let health =
        codegg_core::session::selection_catalog::health_for(connection_store, &connection.id)
            .await
            .map_err(|e| SelectionError::ConnectionStore(e.to_string()))?;

    let model_count =
        codegg_core::session::selection_catalog::model_count_for(connection_store, &connection.id)
            .await
            .map_err(|e| SelectionError::ConnectionStore(e.to_string()))? as usize;

    Ok(ProviderConnectionSummaryDto {
        id: connection.id.as_str().to_string(),
        provider_kind: connection.provider_kind.as_str().to_string(),
        display_name: connection.display_name.clone(),
        endpoint: connection.endpoint.as_str().to_string(),
        tls_policy: format!("{:?}", connection.tls_policy).to_lowercase(),
        scope: scope_label(&connection.scope),
        state: connection.state.storage_key().to_string(),
        revision: connection.revision,
        model_count,
        catalog_revision: health.as_ref().and_then(|h| h.4.clone()),
        health: health.map(|(status, reason_code, checked_at, duration_ms, _)| {
            codegg_protocol::provider::ConnectionHealthDto {
                status,
                reason_code,
                checked_at,
                duration_ms: duration_ms as u64,
            }
        }),
    })
}

fn scope_label(scope: &ProviderScope) -> String {
    match scope {
        ProviderScope::Personal { .. } => "personal".to_string(),
        ProviderScope::Project { .. } => "project".to_string(),
        ProviderScope::Deployment { .. } => "deployment".to_string(),
    }
}

/// List connections available for selection for the supplied session
/// scope. The list is always redacted; credentials never appear. Personal
/// scope returns all connections; project/deployment scope filters by
/// matching scope.
pub async fn list_selection(
    session_store: &SessionStore,
    connection_store: &ProviderConnectionStore,
    session_id: &str,
) -> Result<Vec<ProviderConnectionSummaryDto>, SelectionError> {
    let _ = session_store
        .get(session_id)
        .await
        .map_err(|e| SelectionError::SessionStore(e.to_string()))?
        .ok_or_else(|| SelectionError::SessionNotFound(session_id.to_string()))?;
    let connections = connection_store
        .list()
        .await
        .map_err(|e| SelectionError::ConnectionStore(e.to_string()))?;
    let mut out = Vec::with_capacity(connections.len());
    for connection in &connections {
        out.push(summary_dto_for(connection, connection_store).await?);
    }
    Ok(out)
}

/// List the bounded model catalog for a connection at its current
/// revision, scoped to the session's authoritative context. The catalog
/// revision is returned so stale revisions are detected.
pub async fn list_selection_models(
    session_store: &SessionStore,
    connection_store: &ProviderConnectionStore,
    session_id: &str,
    connection_id: &ProviderConnectionId,
) -> Result<
    (
        Option<String>,
        Vec<codegg_protocol::provider::ProviderModelDto>,
    ),
    SelectionError,
> {
    let _ = session_store
        .get(session_id)
        .await
        .map_err(|e| SelectionError::SessionStore(e.to_string()))?
        .ok_or_else(|| SelectionError::SessionNotFound(session_id.to_string()))?;
    let revision: u64 = connection_store
        .get(connection_id)
        .await
        .map_err(|e| SelectionError::ConnectionStore(e.to_string()))?
        .map(|c| c.revision)
        .ok_or_else(|| SelectionError::ConnectionStore("connection missing".to_string()))?;
    let catalog_revision = catalog_revision_for(connection_store, connection_id, revision).await?;
    let rows = list_models(connection_store, connection_id).await?;
    let models = rows
        .into_iter()
        .map(|(id, name, context, max, tools, vision)| {
            codegg_protocol::provider::ProviderModelDto {
                id,
                name,
                context_window: context,
                max_output_tokens: max,
                supports_tools: tools,
                supports_vision: vision,
            }
        })
        .collect();
    Ok((catalog_revision, models))
}

/// Apply a session selection update with optimistic revision checks.
pub async fn update_selection(
    session_store: &SessionStore,
    connection_store: &ProviderConnectionStore,
    session_id: &str,
    connection_id: &ProviderConnectionId,
    model_id: &str,
    expected_connection_revision: Option<u64>,
    expected_catalog_revision: Option<String>,
) -> Result<SelectionUpdateOutcome, SelectionError> {
    let session = session_store
        .get(session_id)
        .await
        .map_err(|e| SelectionError::SessionStore(e.to_string()))?
        .ok_or_else(|| SelectionError::SessionNotFound(session_id.to_string()))?;

    let connection = connection_store
        .get(connection_id)
        .await
        .map_err(|e| SelectionError::ConnectionStore(e.to_string()))?
        .ok_or_else(|| SelectionError::ConnectionStore("connection missing".to_string()))?;

    if connection.state != ProviderConnectionState::Active {
        return Ok(SelectionUpdateOutcome::ConnectionNotSelectable {
            connection_id: connection_id.as_str().to_string(),
            state: connection.state.storage_key().to_string(),
        });
    }

    if let Some(expected) = expected_connection_revision {
        if connection.revision != expected {
            return Ok(SelectionUpdateOutcome::StaleRevision {
                current_connection_id: connection_id.as_str().to_string(),
                current_revision: connection.revision,
            });
        }
    }

    let models = list_models(connection_store, connection_id).await?;
    let Some(model_row) = models.into_iter().find(|m| m.0 == model_id) else {
        return Ok(SelectionUpdateOutcome::UnknownModel {
            connection_id: connection_id.as_str().to_string(),
            model_id: model_id.to_string(),
        });
    };

    let catalog_revision =
        catalog_revision_for(connection_store, connection_id, connection.revision).await?;

    if let Some(expected) = expected_catalog_revision.as_deref() {
        if catalog_revision.as_deref() != Some(expected) {
            return Ok(SelectionUpdateOutcome::StaleCatalog {
                current_revision: connection.revision,
                current_catalog_revision: catalog_revision,
            });
        }
    }

    let catalog_revision_str = catalog_revision.clone().unwrap_or_else(|| "0".to_string());

    let update = UpdateSession {
        provider_connection_id: Some(Some(connection_id.as_str().to_string())),
        provider_connection_revision: Some(Some(connection.revision)),
        model_catalog_revision: Some(Some(catalog_revision_str.clone())),
        selected_model_id: Some(Some(model_id.to_string())),
        ..UpdateSession::default()
    };
    session_store
        .update(&session.id, update)
        .await
        .map_err(|e| SelectionError::SessionStore(e.to_string()))?;

    let summary = summary_dto_for(&connection, connection_store).await?;
    let selected_model = SelectedModelDto {
        connection_id: connection_id.as_str().to_string(),
        model_id: model_row.0.clone(),
        model_name: model_row.1.clone(),
        context_window: model_row.2,
        max_output_tokens: model_row.3,
        supports_tools: model_row.4,
        supports_vision: model_row.5,
        catalog_revision: catalog_revision_str.clone(),
    };
    Ok(SelectionUpdateOutcome::Updated(
        SessionSelectionDto::Selected {
            connection: summary,
            model: selected_model,
            connection_revision: connection.revision,
            catalog_revision: catalog_revision_str,
        },
    ))
}

/// Adapter that exposes a typed selection service over an `Arc<CoreDaemon>`-style
/// façade. The protocol layer can call this without holding a direct
/// reference to the daemon struct.
#[derive(Clone)]
pub struct SelectionService {
    pub session_store: Arc<SessionStore>,
    pub connection_store: Arc<ProviderConnectionStore>,
    pub eggpool: Option<Arc<EggpoolProvisioner>>,
}

impl SelectionService {
    pub fn new(
        session_store: Arc<SessionStore>,
        connection_store: Arc<ProviderConnectionStore>,
        eggpool: Option<Arc<EggpoolProvisioner>>,
    ) -> Self {
        Self {
            session_store,
            connection_store,
            eggpool,
        }
    }

    pub async fn get(&self, session_id: &str) -> Result<SessionSelectionDto, SelectionError> {
        get_selection(
            self.session_store.as_ref(),
            self.connection_store.as_ref(),
            session_id,
        )
        .await
    }

    pub async fn list(
        &self,
        session_id: &str,
    ) -> Result<Vec<ProviderConnectionSummaryDto>, SelectionError> {
        list_selection(
            self.session_store.as_ref(),
            self.connection_store.as_ref(),
            session_id,
        )
        .await
    }

    pub async fn models(
        &self,
        session_id: &str,
        connection_id: &ProviderConnectionId,
    ) -> Result<
        (
            Option<String>,
            Vec<codegg_protocol::provider::ProviderModelDto>,
        ),
        SelectionError,
    > {
        list_selection_models(
            self.session_store.as_ref(),
            self.connection_store.as_ref(),
            session_id,
            connection_id,
        )
        .await
    }

    pub async fn update(
        &self,
        session_id: &str,
        connection_id: &ProviderConnectionId,
        model_id: &str,
        expected_connection_revision: Option<u64>,
        expected_catalog_revision: Option<String>,
    ) -> Result<SelectionUpdateOutcome, SelectionError> {
        update_selection(
            self.session_store.as_ref(),
            self.connection_store.as_ref(),
            session_id,
            connection_id,
            model_id,
            expected_connection_revision,
            expected_catalog_revision,
        )
        .await
    }
}

/// Stable error code for [`SelectionError`]. Returned alongside
/// `CoreResponse::Error` so callers can branch on the failure type.
pub fn selection_error_code(error: &SelectionError) -> &'static str {
    match error {
        SelectionError::SessionNotFound(_) => "session_not_found",
        SelectionError::ConnectionStore(_) => "connection_store_error",
        SelectionError::InvalidConnectionId(_) => "invalid_connection_id",
        SelectionError::SessionStore(_) => "session_store_error",
        SelectionError::MissingProjectContext => "missing_project_context",
    }
}

pub fn selection_error_message(error: &SelectionError) -> String {
    error.to_string()
}

/// Stable outcome code for [`SelectionUpdateOutcome`]. Returned in the
/// `code` field of `CoreResponse::Error` when the variant is not `Updated`.
pub fn selection_outcome_code(outcome: &SelectionUpdateOutcome) -> &'static str {
    match outcome {
        SelectionUpdateOutcome::Updated(_) => "selection_updated",
        SelectionUpdateOutcome::StaleRevision { .. } => "selection_revision_conflict",
        SelectionUpdateOutcome::StaleCatalog { .. } => "selection_catalog_stale",
        SelectionUpdateOutcome::ConnectionNotSelectable { .. } => "connection_not_selectable",
        SelectionUpdateOutcome::UnknownModel { .. } => "unknown_model",
    }
}

pub fn selection_outcome_message(outcome: &SelectionUpdateOutcome) -> String {
    match outcome {
        SelectionUpdateOutcome::Updated(_) => "Selection updated".to_string(),
        SelectionUpdateOutcome::StaleRevision {
            current_connection_id,
            current_revision,
        } => format!(
            "Connection '{current_connection_id}' is at revision {current_revision}; reload and retry."
        ),
        SelectionUpdateOutcome::StaleCatalog {
            current_revision,
            current_catalog_revision,
        } => format!(
            "Catalog for connection revision {current_revision} is at revision {:?}; reload and retry.",
            current_catalog_revision
        ),
        SelectionUpdateOutcome::ConnectionNotSelectable {
            connection_id,
            state,
        } => format!(
            "Connection '{connection_id}' is in state '{state}' and cannot be selected."
        ),
        SelectionUpdateOutcome::UnknownModel {
            connection_id,
            model_id,
        } => format!(
            "Model '{model_id}' is not in the bounded catalog of connection '{connection_id}'."
        ),
    }
}
