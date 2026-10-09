//! Provider Connections Milestone 3: legacy `provider/model` compatibility
//! adapter.
//!
//! Existing sessions persist a legacy `provider/model` string on the row.
//! This module exposes a deterministic, non-fallbacking resolver that
//! classifies a legacy string against the durable `ProviderConnection`
//! catalog without ever silently choosing a different credentialed endpoint.
//!
//! The resolver is intentionally read-only and never mutates the connection
//! catalog or session state. Callers use the [`LegacyResolution`] outcome
//! to surface actionable diagnostics or trigger an explicit selection via
//! the daemon-owned selection service.
//!
//! ## Invariants
//!
//! - The legacy string is never parsed as an authorization grant.
//! - The resolver never falls back across disabled, credential-missing, or
//!   ambiguous matching connections.
//! - Resolved identities are returned for caller-side diagnostics only;
//!   they are not auto-applied to the session row.

use crate::provider_connections::{
    ProviderConnection, ProviderConnectionState, ProviderConnectionStore, ProviderKind,
};
use crate::session::LegacyResolution;

/// Errors raised by the legacy compatibility resolver. None of these errors
/// silently select a different credentialed endpoint.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LegacyResolutionError {
    #[error("connection store error: {0}")]
    Store(String),
}

/// Resolve a legacy `provider/model` string against the durable connection
/// catalog. The result is a typed [`LegacyResolution`] that the caller can
/// inspect to surface a migration diagnostic or trigger an explicit
/// selection.
///
/// The resolver scans active connections whose `ProviderKind` matches the
/// prefix of `legacy_string`. The legacy string is expected in either
/// `"provider"` form (model omitted) or `"provider/model"` form. The model
/// suffix, when present, is captured for diagnostics only.
pub async fn resolve_legacy_model_string(
    store: &ProviderConnectionStore,
    legacy_string: Option<&str>,
) -> Result<LegacyResolution, LegacyResolutionError> {
    let trimmed = legacy_string.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        return Ok(LegacyResolution::Unset);
    }
    let (provider_kind, model_id) = split_legacy_string(trimmed);

    let candidates = store
        .list()
        .await
        .map_err(|e| LegacyResolutionError::Store(e.to_string()))?
        .into_iter()
        .filter(|c| provider_kind_matches(&c.provider_kind, provider_kind))
        .collect::<Vec<_>>();

    // Ambiguity is only meaningful between connections the caller could
    // actually select. A tombstoned, disabled, credential-missing, errored, or
    // stale row is not selectable, so it must not make a healthy provider look
    // ambiguous — otherwise every provider that had a connection replaced (the
    // `/connect` replacement path tombstones the old row in place) would stay
    // permanently unresolvable via the legacy `provider/model` string.
    let selectable = candidates
        .iter()
        .filter(|c| c.state == ProviderConnectionState::Active)
        .collect::<Vec<_>>();

    match selectable.as_slice() {
        // Exactly one selectable connection wins outright; any remaining rows
        // are non-selectable history for this provider.
        [single] => classify_single(single, model_id),
        // Nothing selectable: preserve the diagnostic that names *why* the
        // matching connections cannot be used.
        [] => match candidates.as_slice() {
            [] => Ok(LegacyResolution::UnresolvedLegacyProvider {
                provider_kind: provider_kind.to_string(),
            }),
            // Several non-selectable rows and no live one: report the first so
            // the caller still gets an actionable state, never a silent pick.
            [only, ..] => classify_single(only, model_id),
        },
        // Two or more live connections genuinely require an explicit choice.
        many => Ok(LegacyResolution::AmbiguousLegacyProvider {
            provider_kind: provider_kind.to_string(),
            candidates: many.iter().map(|c| c.id.as_str().to_string()).collect(),
        }),
    }
}

fn classify_single(
    connection: &ProviderConnection,
    model_id: Option<&str>,
) -> Result<LegacyResolution, LegacyResolutionError> {
    let connection_id = connection.id.as_str().to_string();
    let revision = connection.revision;
    let provider_kind = connection.provider_kind.as_str().to_string();
    let model_id = model_id.map(str::to_string);
    match connection.state {
        ProviderConnectionState::Active => Ok(LegacyResolution::Resolved {
            connection_id,
            revision,
            model_id,
        }),
        ProviderConnectionState::Disabled => Ok(LegacyResolution::DisabledLegacyConnection {
            provider_kind,
            connection_id,
        }),
        ProviderConnectionState::CredentialMissing => {
            Ok(LegacyResolution::MissingCredentialLegacyConnection {
                provider_kind,
                connection_id,
            })
        }
        ProviderConnectionState::ProvisioningRotating
        | ProviderConnectionState::Tombstoned
        | ProviderConnectionState::Error
        | ProviderConnectionState::Stale => Ok(LegacyResolution::DisabledLegacyConnection {
            provider_kind,
            connection_id,
        }),
    }
}

/// Split `"provider/model"` into `(provider, Option<model>)`.
/// When no `/` is present the entire string is the provider and the model
/// is `None`.
fn split_legacy_string(value: &str) -> (&str, Option<&str>) {
    match value.split_once('/') {
        Some((provider, model)) => (provider, Some(model)),
        None => (value, None),
    }
}

/// Match a stored `ProviderKind` against the legacy string's prefix. The
/// legacy prefix is the registered provider implementation ID (e.g.
/// `"openai"`, `"anthropic"`, `"google"`). Built-in kinds map directly;
/// `Eggpool` is reported as `"eggpool"`.
fn provider_kind_matches(kind: &ProviderKind, prefix: &str) -> bool {
    if prefix.is_empty() {
        return false;
    }
    kind.as_str().eq_ignore_ascii_case(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{ProjectId, ProviderConnectionId};
    use crate::provider_connections::{
        Endpoint, NewProviderConnection, ProviderConnectionState, ProviderConnectionStore,
        ProviderConnectionUpdate, ProviderScope, SecretBindingLocator, SecretRef, TlsPolicy,
    };
    use sqlx::sqlite::SqlitePoolOptions;
    use std::time::Duration;

    async fn pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate sqlite");
        pool
    }

    async fn seed_connection(
        store: &ProviderConnectionStore,
        kind: ProviderKind,
        project: ProjectId,
        account: &str,
    ) -> ProviderConnectionId {
        let input = NewProviderConnection {
            provider_kind: kind,
            display_name: format!("Shared {}", account),
            endpoint: Endpoint::new("https://example.com", TlsPolicy::Required).unwrap(),
            tls_policy: TlsPolicy::Required,
            scope: ProviderScope::project(project),
            secret_binding: Some(
                SecretBindingLocator::new(SecretRef::new(), "test", account).unwrap(),
            ),
        };
        store.create(input).await.expect("create connection").id
    }

    #[tokio::test(flavor = "current_thread")]
    async fn empty_legacy_string_resolves_to_unset() {
        let store = ProviderConnectionStore::new(pool().await);
        let result = resolve_legacy_model_string(&store, None).await.unwrap();
        assert_eq!(result, LegacyResolution::Unset);
        let result = resolve_legacy_model_string(&store, Some("")).await.unwrap();
        assert_eq!(result, LegacyResolution::Unset);
        let result = resolve_legacy_model_string(&store, Some("   "))
            .await
            .unwrap();
        assert_eq!(result, LegacyResolution::Unset);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn single_active_connection_is_resolved() {
        let database = pool().await;
        let store = ProviderConnectionStore::new(database.clone());
        let project = ProjectId::new();
        let id = seed_connection(&store, ProviderKind::OpenAi, project, "account-a").await;
        let result = resolve_legacy_model_string(&store, Some("openai/gpt-4o"))
            .await
            .unwrap();
        match result {
            LegacyResolution::Resolved {
                connection_id,
                revision,
                model_id,
            } => {
                assert_eq!(connection_id, id.as_str());
                assert_eq!(revision, 1);
                assert_eq!(model_id.as_deref(), Some("gpt-4o"));
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn provider_only_legacy_string_resolves_with_no_model() {
        let database = pool().await;
        let store = ProviderConnectionStore::new(database.clone());
        let project = ProjectId::new();
        seed_connection(&store, ProviderKind::OpenAi, project, "account-a").await;
        let result = resolve_legacy_model_string(&store, Some("openai"))
            .await
            .unwrap();
        match result {
            LegacyResolution::Resolved { model_id, .. } => {
                assert_eq!(model_id, None);
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disabled_connection_returns_disabled_diagnostic() {
        let database = pool().await;
        let store = ProviderConnectionStore::new(database.clone());
        let project = ProjectId::new();
        let id = seed_connection(&store, ProviderKind::Anthropic, project, "account-a").await;
        let connection = store.get(&id).await.unwrap().unwrap();
        store
            .disable(&id, connection.revision)
            .await
            .expect("disable");
        let result = resolve_legacy_model_string(&store, Some("anthropic/claude-3"))
            .await
            .unwrap();
        match result {
            LegacyResolution::DisabledLegacyConnection {
                provider_kind,
                connection_id,
            } => {
                assert_eq!(provider_kind, "anthropic");
                assert_eq!(connection_id, id.as_str());
            }
            other => panic!("expected DisabledLegacyConnection, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn credential_missing_connection_returns_missing_credential_diagnostic() {
        let database = pool().await;
        let store = ProviderConnectionStore::new(database.clone());
        let project = ProjectId::new();
        let id = seed_connection(&store, ProviderKind::OpenAi, project, "account-a").await;
        let connection = store.get(&id).await.unwrap().unwrap();
        let mut update = ProviderConnectionUpdate::from(&connection);
        update.secret_binding = None;
        let updated = store
            .update(&id, connection.revision, update)
            .await
            .unwrap();
        let _ = updated;
        // transition to CredentialMissing
        store
            .transition(&id, 2, ProviderConnectionState::CredentialMissing)
            .await
            .expect("transition");
        let result = resolve_legacy_model_string(&store, Some("openai/gpt-4o"))
            .await
            .unwrap();
        match result {
            LegacyResolution::MissingCredentialLegacyConnection {
                provider_kind,
                connection_id,
            } => {
                assert_eq!(provider_kind, "openai");
                assert_eq!(connection_id, id.as_str());
            }
            other => panic!("expected MissingCredentialLegacyConnection, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn multiple_matching_connections_returns_ambiguous() {
        let database = pool().await;
        let store = ProviderConnectionStore::new(database.clone());
        let project = ProjectId::new();
        let _id_a =
            seed_connection(&store, ProviderKind::OpenAi, project.clone(), "account-a").await;
        let _id_b = seed_connection(&store, ProviderKind::OpenAi, project, "account-b").await;
        let result = resolve_legacy_model_string(&store, Some("openai/gpt-4o"))
            .await
            .unwrap();
        match result {
            LegacyResolution::AmbiguousLegacyProvider {
                provider_kind,
                candidates,
            } => {
                assert_eq!(provider_kind, "openai");
                assert_eq!(candidates.len(), 2);
            }
            other => panic!("expected AmbiguousLegacyProvider, got {other:?}"),
        }
    }

    /// Regression: the `/connect` replacement path tombstones the superseded row
    /// in place rather than deleting it. A tombstoned row must not make the
    /// provider look ambiguous, otherwise every replaced provider becomes
    /// permanently unresolvable through the legacy `provider/model` string.
    #[tokio::test(flavor = "current_thread")]
    async fn tombstoned_connection_does_not_make_provider_ambiguous() {
        let database = pool().await;
        let store = ProviderConnectionStore::new(database.clone());
        let project = ProjectId::new();
        let old_id =
            seed_connection(&store, ProviderKind::OpenAi, project.clone(), "account-old").await;
        let live_id = seed_connection(&store, ProviderKind::OpenAi, project, "account-new").await;

        // Retire the superseded connection exactly as provisioning does.
        let old = store.get(&old_id).await.unwrap().unwrap();
        store
            .transition(&old_id, old.revision, ProviderConnectionState::Tombstoned)
            .await
            .expect("tombstone superseded connection");

        let result = resolve_legacy_model_string(&store, Some("openai/gpt-4o"))
            .await
            .unwrap();
        match result {
            LegacyResolution::Resolved {
                connection_id,
                model_id,
                ..
            } => {
                assert_eq!(connection_id, live_id.as_str());
                assert_eq!(model_id.as_deref(), Some("gpt-4o"));
            }
            other => panic!("expected Resolved for the live connection, got {other:?}"),
        }
    }

    /// A disabled row alone is still reported as disabled (not silently
    /// resolved), preserving the diagnostic contract.
    #[tokio::test(flavor = "current_thread")]
    async fn disabled_connection_alongside_active_resolves_the_active_one() {
        let database = pool().await;
        let store = ProviderConnectionStore::new(database.clone());
        let project = ProjectId::new();
        let old_id =
            seed_connection(&store, ProviderKind::OpenAi, project.clone(), "account-old").await;
        let live_id = seed_connection(&store, ProviderKind::OpenAi, project, "account-new").await;

        let old = store.get(&old_id).await.unwrap().unwrap();
        store
            .disable(&old_id, old.revision)
            .await
            .expect("disable superseded connection");

        let result = resolve_legacy_model_string(&store, Some("openai/gpt-4o"))
            .await
            .unwrap();
        match result {
            LegacyResolution::Resolved { connection_id, .. } => {
                assert_eq!(connection_id, live_id.as_str());
            }
            other => panic!("expected Resolved for the live connection, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn no_matching_connection_returns_unresolved() {
        let database = pool().await;
        let store = ProviderConnectionStore::new(database);
        let result = resolve_legacy_model_string(&store, Some("openai/gpt-4o"))
            .await
            .unwrap();
        match result {
            LegacyResolution::UnresolvedLegacyProvider { provider_kind } => {
                assert_eq!(provider_kind, "openai");
            }
            other => panic!("expected UnresolvedLegacyProvider, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn different_provider_kind_does_not_match() {
        let database = pool().await;
        let store = ProviderConnectionStore::new(database);
        let project = ProjectId::new();
        seed_connection(&store, ProviderKind::Anthropic, project, "account-a").await;
        let result = resolve_legacy_model_string(&store, Some("openai/gpt-4o"))
            .await
            .unwrap();
        assert!(matches!(
            result,
            LegacyResolution::UnresolvedLegacyProvider { .. }
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unresolved_legacy_provider_handles_store_error() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(1))
            .connect("sqlite::memory:")
            .await
            .unwrap();
        // No migration: a query against a fresh in-memory db should fail.
        let store = ProviderConnectionStore::new(pool);
        let result = resolve_legacy_model_string(&store, Some("openai/gpt-4o")).await;
        assert!(matches!(result, Err(LegacyResolutionError::Store(_))));
    }
}
