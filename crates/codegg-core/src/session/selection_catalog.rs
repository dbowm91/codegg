//! Provider Connections Milestone 3: read-only catalog helpers used by the
//! session selection service.
//!
//! This module exposes a thin, typed seam over the catalog/health tables
//! created by Milestone 2. It does not mutate the database and never
//! resolves credentials.

use crate::error::StorageError;
use crate::identity::ProviderConnectionId;
use crate::provider_connections::ProviderConnectionStore;
use codegg_protocol::provider::ConnectionHealthDto;

/// Read-side row type for the bounded model catalog.
pub type ModelRow = (String, String, u64, Option<u64>, bool, bool);

/// Read-side row type for the connection health table.
pub type HealthRow = (String, Option<String>, i64, i64, Option<String>);

/// Load the bounded model catalog at the connection's current revision.
/// Rows are returned in `model_id` order to match the protocol contract.
pub async fn list_models_for_connection(
    store: &ProviderConnectionStore,
    connection_id: &ProviderConnectionId,
) -> Result<Vec<ModelRow>, StorageError> {
    let pool = store.pool().clone();
    let revision: Option<i64> =
        sqlx::query_scalar("SELECT revision FROM provider_connections WHERE id = ?")
            .bind(connection_id.as_str())
            .fetch_optional(&pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
    let Some(revision) = revision else {
        return Ok(Vec::new());
    };
    let rows: Vec<(String, String, i64, Option<i64>, i64, i64)> = sqlx::query_as(
        "SELECT model_id, model_name, context_window, max_output_tokens, supports_tools, supports_vision \
         FROM provider_connection_models WHERE connection_id = ? AND revision = ? ORDER BY model_id",
    )
    .bind(connection_id.as_str())
    .bind(revision)
    .fetch_all(&pool)
    .await
    .map_err(|e| StorageError::Database(e.to_string()))?;
    Ok(rows
        .into_iter()
        .map(|(id, name, context, max, tools, vision)| {
            (
                id,
                name,
                context as u64,
                max.map(|v| v as u64),
                tools != 0,
                vision != 0,
            )
        })
        .collect())
}

/// Return the bounded catalog row count for a connection at its current
/// revision. Used by summary DTOs.
pub async fn model_count_for(
    store: &ProviderConnectionStore,
    connection_id: &ProviderConnectionId,
) -> Result<i64, StorageError> {
    let pool = store.pool().clone();
    let revision: Option<i64> =
        sqlx::query_scalar("SELECT revision FROM provider_connections WHERE id = ?")
            .bind(connection_id.as_str())
            .fetch_optional(&pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
    let Some(revision) = revision else {
        return Ok(0);
    };
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM provider_connection_models WHERE connection_id = ? AND revision = ?",
    )
    .bind(connection_id.as_str())
    .bind(revision)
    .fetch_one(&pool)
    .await
    .map_err(|e| StorageError::Database(e.to_string()))?;
    Ok(count)
}

/// Fetch the catalog revision string for the supplied connection
/// revision. Reads the `provider_connection_health` row whose
/// `revision` matches. Returns `None` when no health row exists yet.
pub async fn catalog_revision_for(
    store: &ProviderConnectionStore,
    connection_id: &ProviderConnectionId,
    revision: u64,
) -> Result<Option<String>, StorageError> {
    let pool = store.pool().clone();
    let value: Option<Option<String>> = sqlx::query_scalar(
        "SELECT catalog_revision FROM provider_connection_health WHERE connection_id = ? AND revision = ?",
    )
    .bind(connection_id.as_str())
    .bind(revision as i64)
    .fetch_optional(&pool)
    .await
    .map_err(|e| StorageError::Database(e.to_string()))?;
    Ok(value.flatten())
}

/// Fetch the latest health row for a connection. Returns `None` when no
/// health has been recorded yet. Used to populate redacted health DTOs.
pub async fn health_for(
    store: &ProviderConnectionStore,
    connection_id: &ProviderConnectionId,
) -> Result<Option<HealthRow>, StorageError> {
    let pool = store.pool().clone();
    let row: Option<HealthRow> = sqlx::query_as(
        "SELECT status, reason_code, checked_at, duration_ms, catalog_revision \
         FROM provider_connection_health WHERE connection_id = ?",
    )
    .bind(connection_id.as_str())
    .fetch_optional(&pool)
    .await
    .map_err(|e| StorageError::Database(e.to_string()))?;
    Ok(row)
}

/// Convert a [`HealthRow`] into the protocol [`ConnectionHealthDto`].
/// `catalog_revision` is dropped here; callers surface it through
/// `ProviderConnectionSummaryDto` separately.
pub fn health_row_to_dto(row: HealthRow) -> ConnectionHealthDto {
    let (status, reason_code, checked_at, duration_ms, _catalog_revision) = row;
    ConnectionHealthDto {
        status,
        reason_code,
        checked_at,
        duration_ms: duration_ms as u64,
    }
}
