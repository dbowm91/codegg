//! Provider Connections Milestone 3 + Execution Reliability M004:
//! daemon-owned session selection service.
//!
//! This module exposes the typed operations that the protocol uses to
//! read, list, and update a session's connection + model selection. The
//! service is owned by the daemon; the TUI and remote clients never
//! construct providers or resolve secrets — they only call into this
//! module.
//!
//! M004 convergence: `CoreRequest::ModelSelect` is a compatibility
//! adapter over [`update_selection`]. The runtime `selected_model`
//! cache is only a projection of the durable row, and the principal's
//! last-used preference is only a convenience default for otherwise
//! unselected sessions.
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
//! - An explicit durable session selection always wins over the
//!   last-used preference; the preference never reroutes an established
//!   binding or silently falls back to another model.

use std::sync::Arc;

use codegg_core::identity::ProviderConnectionId;
use codegg_core::provider_connections::{
    ProviderConnection, ProviderConnectionReferenceKind, ProviderConnectionState,
    ProviderConnectionStore, ProviderScope,
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
#[allow(clippy::large_enum_variant)]
pub enum SelectionUpdateOutcome {
    Updated(SessionSelectionDto),
    /// The supplied `expected_connection_revision` did not match the
    /// stored value. The stored selection is unchanged.
    StaleRevision {
        current_connection_id: String,
        current_revision: u64,
        current_selected_model_id: Option<String>,
    },
    /// The supplied `expected_catalog_revision` did not match the
    /// catalog at the current revision. The stored selection is
    /// unchanged.
    StaleCatalog {
        current_revision: u64,
        current_catalog_revision: Option<String>,
        current_selected_model_id: Option<String>,
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
                    connection: Box::new(summary),
                    model: Box::new(resolved.model),
                    connection_revision: resolved.connection.revision,
                    catalog_revision: resolved.catalog_revision.unwrap_or_else(|| "0".to_string()),
                });
            }
            None => {
                // Preserve the durable selection as an explicit lifecycle
                // diagnostic. Never reinterpret it through the legacy
                // provider/model resolver, which could silently choose a
                // different credentialed endpoint.
                if let Some(connection) = connection_store
                    .get(&connection_id)
                    .await
                    .map_err(|e| SelectionError::ConnectionStore(e.to_string()))?
                {
                    let summary = summary_dto_for(&connection, connection_store).await?;
                    let catalog_revision = session
                        .model_catalog_revision
                        .clone()
                        .unwrap_or_else(|| "0".to_string());
                    return Ok(SessionSelectionDto::Selected {
                        connection: Box::new(summary),
                        model: Box::new(SelectedModelDto {
                            connection_id: connection.id.to_string(),
                            model_id: model_id.clone(),
                            model_name: model_id.clone(),
                            context_window: 0,
                            max_output_tokens: None,
                            supports_tools: false,
                            supports_vision: false,
                            catalog_revision: catalog_revision.clone(),
                        }),
                        connection_revision: connection.revision,
                        catalog_revision,
                    });
                }
                return Ok(SessionSelectionDto::LegacyUnresolved {
                    legacy_provider: "provider_connection".to_string(),
                    legacy_model: Some(model_id.clone()),
                    reason: "selected provider connection is no longer present".to_string(),
                });
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
    let session = session_store
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
        let in_scope = match &connection.scope {
            ProviderScope::Personal { .. } => true,
            ProviderScope::Project { project_id } => project_id.as_str() == session.project_id,
            ProviderScope::Deployment { .. } => false,
        };
        if !in_scope {
            continue;
        }
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
                current_selected_model_id: session.selected_model_id.clone(),
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
                current_selected_model_id: session.selected_model_id.clone(),
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
    if let Some(previous_connection_id) = session.provider_connection_id.as_deref() {
        if previous_connection_id != connection_id.as_str() {
            if let Ok(previous_connection_id) = ProviderConnectionId::parse(previous_connection_id)
            {
                let _ = connection_store
                    .remove_reference(
                        &previous_connection_id,
                        ProviderConnectionReferenceKind::SelectedSession,
                        session.id.as_str(),
                    )
                    .await;
            }
        }
    }
    let _ = connection_store
        .add_reference(
            connection_id,
            ProviderConnectionReferenceKind::SelectedSession,
            session.id.as_str(),
        )
        .await;

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
            connection: Box::new(summary),
            model: Box::new(selected_model),
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

/// M004: `true` when the session row already carries an explicit
/// durable selection. Explicit bindings are authoritative over the
/// last-used preference.
pub fn has_explicit_selection(session: &Session) -> bool {
    session
        .provider_connection_id
        .as_deref()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
        && session
            .selected_model_id
            .as_deref()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
}

/// M004: canonical runtime cache form for a durable selection:
/// `provider_kind/model_id` (e.g. `openai/gpt-4o`). The runtime cache
/// is a projection of this string, never an independent source.
pub fn canonical_runtime_model(provider_kind: &str, model_id: &str) -> String {
    format!("{provider_kind}/{model_id}")
}

/// M004: project the runtime cache value for a durable selection DTO.
/// Returns `None` for unselected/legacy-unresolved states so callers
/// keep the prior cache instead of inventing a model.
pub fn durable_selected_runtime_model(
    selection: &codegg_protocol::provider::SessionSelectionDto,
) -> Option<String> {
    match selection {
        codegg_protocol::provider::SessionSelectionDto::Selected {
            connection, model, ..
        } => Some(canonical_runtime_model(
            &connection.provider_kind,
            &model.model_id,
        )),
        _ => None,
    }
}

/// M004: typed failure resolving a legacy `ModelSelect` model string
/// (`provider/model`) to an explicit connection + model pair. No
/// variant ever selects a different credentialed endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSelectResolveError {
    EmptyModel,
    ModelRequired {
        provider_kind: String,
    },
    UnknownProvider {
        provider_kind: String,
    },
    AmbiguousProvider {
        provider_kind: String,
        count: usize,
    },
    ConnectionNotSelectable {
        connection_id: String,
        state: String,
    },
    Store(String),
}

impl ModelSelectResolveError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyModel => "model_not_specified",
            Self::ModelRequired { .. } => "model_required",
            Self::UnknownProvider { .. } => "unknown_provider",
            Self::AmbiguousProvider { .. } => "ambiguous_provider",
            Self::ConnectionNotSelectable { .. } => "connection_not_selectable",
            Self::Store(_) => "connection_store_error",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::EmptyModel => "Model must be specified as 'provider/model'.".to_string(),
            Self::ModelRequired { provider_kind } => format!(
                "Model is required: '{provider_kind}' names a provider but no model. Use 'provider/model'."
            ),
            Self::UnknownProvider { provider_kind } => format!(
                "No active connection matches provider '{provider_kind}'. Open /connect to create one, or select an existing connection explicitly."
            ),
            Self::AmbiguousProvider {
                provider_kind,
                count,
            } => format!(
                "Multiple connections match provider '{provider_kind}' ({count}). Choose one explicitly via SessionSelectionUpdate."
            ),
            Self::ConnectionNotSelectable {
                connection_id,
                state,
            } => format!(
                "Connection '{connection_id}' is in state '{state}' and cannot be selected."
            ),
            Self::Store(detail) => format!("connection store error: {detail}"),
        }
    }
}

/// M004: resolve a `ModelSelect` model string to its explicit
/// connection + model pair using the read-only legacy resolver. The
/// caller must still validate the model against the bounded catalog
/// via [`update_selection`]; this step only resolves identity.
pub async fn resolve_model_select_target(
    connection_store: &ProviderConnectionStore,
    model: &str,
) -> Result<(ProviderConnectionId, String), ModelSelectResolveError> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return Err(ModelSelectResolveError::EmptyModel);
    }
    let resolution =
        legacy_resolution::resolve_legacy_model_string(connection_store, Some(trimmed))
            .await
            .map_err(|e| ModelSelectResolveError::Store(e.to_string()))?;
    match resolution {
        LegacyResolution::Unset => Err(ModelSelectResolveError::EmptyModel),
        LegacyResolution::Resolved {
            connection_id,
            model_id: Some(model_id),
            ..
        } => {
            let id = ProviderConnectionId::parse(&connection_id)
                .map_err(|_| ModelSelectResolveError::Store("invalid connection id".to_string()))?;
            if model_id.trim().is_empty() {
                return Err(ModelSelectResolveError::ModelRequired {
                    provider_kind: trimmed.to_string(),
                });
            }
            Ok((id, model_id))
        }
        LegacyResolution::Resolved {
            connection_id: _,
            model_id: None,
            ..
        } => {
            let provider_kind = match trimmed.split_once('/') {
                Some((p, _)) => p.to_string(),
                None => trimmed.to_string(),
            };
            Err(ModelSelectResolveError::ModelRequired { provider_kind })
        }
        LegacyResolution::UnresolvedLegacyProvider { provider_kind } => {
            Err(ModelSelectResolveError::UnknownProvider { provider_kind })
        }
        LegacyResolution::AmbiguousLegacyProvider {
            provider_kind,
            candidates,
        } => Err(ModelSelectResolveError::AmbiguousProvider {
            provider_kind,
            count: candidates.len(),
        }),
        LegacyResolution::DisabledLegacyConnection { connection_id, .. } => {
            Err(ModelSelectResolveError::ConnectionNotSelectable {
                connection_id,
                state: "disabled".to_string(),
            })
        }
        LegacyResolution::MissingCredentialLegacyConnection { connection_id, .. } => {
            Err(ModelSelectResolveError::ConnectionNotSelectable {
                connection_id,
                state: "credential_missing".to_string(),
            })
        }
    }
}

/// M004: error applying a last-used preference. Preference-store
/// failures are distinct from selection-store failures so callers can
/// report "preference unavailable" without implying the session is
/// broken.
#[derive(Debug, thiserror::Error)]
pub enum ApplyPreferenceError {
    #[error("selection error: {0}")]
    Selection(#[from] SelectionError),
    #[error("preference error: {0}")]
    Preference(#[from] codegg_core::approval::PreferenceError),
}

pub fn apply_preference_error_code(error: &ApplyPreferenceError) -> &'static str {
    match error {
        ApplyPreferenceError::Selection(inner) => selection_error_code(inner),
        ApplyPreferenceError::Preference(inner) => match inner {
            codegg_core::approval::PreferenceError::Validation(_) => "preference_invalid",
            codegg_core::approval::PreferenceError::Conflict { .. } => "preference_conflict",
            codegg_core::approval::PreferenceError::CeilingExceeded(_) => "preference_ceiling",
            codegg_core::approval::PreferenceError::Storage(_) => "preference_unavailable",
        },
    }
}

/// M004: apply a principal's last-used connection/model preference to
/// an otherwise unselected session.
///
/// Contract (plan §6):
/// 1. load the session; an explicit durable selection wins immediately
///    (`ExplicitSelectionPresent`);
/// 2. load the principal preference; absent/incomplete rows yield
///    `NoPreference`;
/// 3. resolve the exact remembered connection (active + credential
///    state via the selection service path);
/// 4. verify the remembered model exists in the current bounded
///    catalog;
/// 5. apply the durable selection with current revisions (CAS against
///    the just-read connection/catalog revision so a concurrent bump
///    surfaces `StaleCatalog` instead of overwriting);
/// 6. on any invalid state keep the session unselected/legacy and
///    return the bounded diagnostic.
///
/// Project/admin/model restrictions, when present, override the
/// preference (hook point: check before `update_selection`).
pub async fn apply_last_used_preference(
    session_store: &SessionStore,
    connection_store: &ProviderConnectionStore,
    preference_store: &codegg_core::approval::RuntimePreferenceStore,
    session_id: &str,
    principal_id: &str,
) -> Result<codegg_core::approval::PreferenceApplicationOutcome, ApplyPreferenceError> {
    use codegg_core::approval::PreferenceApplicationOutcome;

    let session = session_store
        .get(session_id)
        .await
        .map_err(|e| SelectionError::SessionStore(e.to_string()))?
        .ok_or_else(|| SelectionError::SessionNotFound(session_id.to_string()))?;
    if has_explicit_selection(&session) {
        return Ok(PreferenceApplicationOutcome::ExplicitSelectionPresent);
    }
    let preference = preference_store.get(principal_id).await?;
    let Some(preference) = preference else {
        return Ok(PreferenceApplicationOutcome::NoPreference);
    };
    if !preference.has_model_preference() {
        return Ok(PreferenceApplicationOutcome::NoPreference);
    }
    let connection_id_str = preference
        .last_provider_connection_id
        .clone()
        .unwrap_or_default();
    let model_id = preference.last_model_id.clone().unwrap_or_default();
    let Ok(connection_id) = ProviderConnectionId::parse(&connection_id_str) else {
        return Ok(PreferenceApplicationOutcome::UnavailableConnection {
            reason: "remembered provider connection id is invalid".to_string(),
        });
    };
    let Some(connection) = connection_store
        .get(&connection_id)
        .await
        .map_err(|e| SelectionError::ConnectionStore(e.to_string()))?
    else {
        return Ok(PreferenceApplicationOutcome::UnavailableConnection {
            reason: "remembered provider connection no longer exists".to_string(),
        });
    };
    if connection.state != ProviderConnectionState::Active {
        return Ok(PreferenceApplicationOutcome::UnavailableConnection {
            reason: format!(
                "remembered provider connection {} is {}",
                connection_id.as_str(),
                connection.state.storage_key()
            ),
        });
    }
    let models = list_models(connection_store, &connection_id).await?;
    if !models.iter().any(|m| m.0 == model_id) {
        return Ok(PreferenceApplicationOutcome::UnknownModel {
            connection_id: connection_id.as_str().to_string(),
            model_id,
        });
    }
    let catalog_revision =
        catalog_revision_for(connection_store, &connection_id, connection.revision).await?;
    match update_selection(
        session_store,
        connection_store,
        session_id,
        &connection_id,
        &model_id,
        Some(connection.revision),
        catalog_revision.clone(),
    )
    .await?
    {
        SelectionUpdateOutcome::Updated(_) => Ok(PreferenceApplicationOutcome::Applied {
            connection_id: connection_id.as_str().to_string(),
            model_id,
        }),
        SelectionUpdateOutcome::StaleRevision { .. }
        | SelectionUpdateOutcome::StaleCatalog { .. } => {
            Ok(PreferenceApplicationOutcome::StaleCatalog {
                detail: format!(
                    "connection revision {} catalog {:?} moved during apply",
                    connection.revision, catalog_revision
                ),
            })
        }
        SelectionUpdateOutcome::ConnectionNotSelectable { state, .. } => {
            Ok(PreferenceApplicationOutcome::UnavailableConnection {
                reason: format!("connection became {state} during apply"),
            })
        }
        SelectionUpdateOutcome::UnknownModel { .. } => {
            Ok(PreferenceApplicationOutcome::UnknownModel {
                connection_id: connection_id.as_str().to_string(),
                model_id,
            })
        }
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
            current_selected_model_id,
        } => format!(
            "Connection '{current_connection_id}' is at revision {current_revision} with selected model {:?}; reload and retry.",
            current_selected_model_id
        ),
        SelectionUpdateOutcome::StaleCatalog {
            current_revision,
            current_catalog_revision,
            current_selected_model_id,
        } => format!(
            "Catalog for connection revision {current_revision} is at revision {:?} with selected model {:?}; reload and retry.",
            current_catalog_revision, current_selected_model_id
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
