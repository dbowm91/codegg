mod common;

use codegg::core::session_selection::{
    get_selection, list_selection, list_selection_models, update_selection, SelectionError,
    SelectionUpdateOutcome,
};
use codegg_core::identity::{PrincipalId, ProviderConnectionId};
use codegg_core::provider_connections::{
    Endpoint, NewProviderConnection, ProviderConnectionStore, ProviderKind, ProviderScope,
    SecretBindingLocator, SecretRef, TlsPolicy,
};
use codegg_core::session::{CreateSession, SessionStore};
use codegg_protocol::provider::SessionSelectionDto;

async fn migrated_pool() -> sqlx::SqlitePool {
    let pool = common::pool::isolated_pool().await;
    // Seed a project row so session creation passes the FK constraint.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    sqlx::query(
        r#"INSERT OR IGNORE INTO project (id, name, time_created, time_updated, sandboxes)
           VALUES (?, ?, ?, ?, ?)"#,
    )
    .bind("test-proj")
    .bind("test-proj")
    .bind(now)
    .bind(now)
    .bind("[]")
    .execute(&pool)
    .await
    .expect("seed project");
    pool
}

fn personal_scope() -> ProviderScope {
    ProviderScope::personal(PrincipalId::parse("test-user").unwrap())
}

/// Seed a provider connection via the store and return its ID.
async fn seed_connection(
    store: &ProviderConnectionStore,
    kind: ProviderKind,
    display_name: &str,
    endpoint: &str,
    secret_account: &str,
) -> ProviderConnectionId {
    let input = NewProviderConnection {
        provider_kind: kind,
        display_name: display_name.to_string(),
        endpoint: Endpoint::new(endpoint, TlsPolicy::Disabled).unwrap(),
        tls_policy: TlsPolicy::Disabled,
        scope: personal_scope(),
        secret_binding: Some(
            SecretBindingLocator::new(SecretRef::new(), "test-provider", secret_account).unwrap(),
        ),
    };
    store.create(input).await.expect("create connection").id
}

/// Insert catalog models and health row for a connection at revision 1.
async fn seed_models(
    pool: &sqlx::SqlitePool,
    connection_id: &ProviderConnectionId,
    models: &[(&str, &str, u64, Option<u64>, bool, bool)],
    catalog_revision: &str,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    sqlx::query(
        "INSERT INTO provider_connection_health \
         (connection_id, revision, status, duration_ms, checked_at, catalog_revision) \
         VALUES (?, 1, 'healthy', 10, ?, ?)",
    )
    .bind(connection_id.as_str())
    .bind(now)
    .bind(catalog_revision)
    .execute(pool)
    .await
    .expect("seed health");

    for (model_id, model_name, ctx, max, tools, vision) in models {
        sqlx::query(
            "INSERT INTO provider_connection_models \
             (connection_id, revision, model_id, model_name, context_window, \
              max_output_tokens, supports_tools, supports_vision) \
             VALUES (?, 1, ?, ?, ?, ?, ?, ?)",
        )
        .bind(connection_id.as_str())
        .bind(model_id)
        .bind(model_name)
        .bind(*ctx as i64)
        .bind(max.map(|v| v as i64))
        .bind(i64::from(*tools))
        .bind(i64::from(*vision))
        .execute(pool)
        .await
        .expect("seed model");
    }
}

/// Create a session and return its ID.
async fn seed_session(session_store: &SessionStore, model: Option<&str>) -> String {
    let session = session_store
        .create(CreateSession {
            project_id: "test-proj".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Test".to_string()),
            parent_id: None,
            workspace_id: None,
            agent: None,
            model: model.map(|s| s.to_string()),
            tags: None,
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
        })
        .await
        .expect("create session");
    session.id
}

// ── Tests ──────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn session_selection_round_trip() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    // Create connection + catalog.
    let conn_id = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "My OpenAI",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    seed_models(
        &pool,
        &conn_id,
        &[("gpt-4o", "GPT-4o", 128000, Some(16384), true, true)],
        "cat-v1",
    )
    .await;

    // Session with no selection.
    let session_id = seed_session(&session_store, None).await;
    let dto = get_selection(&session_store, &conn_store, &session_id)
        .await
        .unwrap();
    assert!(matches!(dto, SessionSelectionDto::Unselected {}));

    // Set selection via update_selection.
    let outcome = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_id,
        "gpt-4o",
        None,
        None,
    )
    .await
    .unwrap();
    let updated_dto = match outcome {
        SelectionUpdateOutcome::Updated(d) => d,
        other => panic!("expected Updated, got {other:?}"),
    };
    assert!(matches!(updated_dto, SessionSelectionDto::Selected { .. }));

    // Verify get_selection returns Selected.
    let dto = get_selection(&session_store, &conn_store, &session_id)
        .await
        .unwrap();
    match dto {
        SessionSelectionDto::Selected {
            connection,
            model,
            connection_revision,
            catalog_revision,
        } => {
            assert_eq!(connection.id, conn_id.as_str());
            assert_eq!(model.model_id, "gpt-4o");
            assert_eq!(connection_revision, 1);
            assert_eq!(catalog_revision, "cat-v1");
        }
        other => panic!("expected Selected, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn session_selection_update_stale_revision() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    let conn_id = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    seed_models(
        &pool,
        &conn_id,
        &[("gpt-4o", "GPT-4o", 128000, Some(16384), true, true)],
        "cat-v1",
    )
    .await;
    let session_id = seed_session(&session_store, None).await;

    // First update succeeds.
    let outcome = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_id,
        "gpt-4o",
        Some(1),
        Some("cat-v1".to_string()),
    )
    .await
    .unwrap();
    assert!(matches!(outcome, SelectionUpdateOutcome::Updated(_)));

    // Bump connection revision by updating the connection.
    let conn = conn_store.get(&conn_id).await.unwrap().unwrap();
    let mut update = codegg_core::provider_connections::ProviderConnectionUpdate::from(&conn);
    update.secret_binding =
        Some(SecretBindingLocator::new(SecretRef::new(), "test-provider", "new-account").unwrap());
    conn_store
        .update(&conn_id, conn.revision, update)
        .await
        .unwrap();

    // Now attempt update with old revision.
    let outcome = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_id,
        "gpt-4o",
        Some(1), // stale
        Some("cat-v1".to_string()),
    )
    .await
    .unwrap();
    match outcome {
        SelectionUpdateOutcome::StaleRevision {
            current_connection_id,
            current_revision,
        } => {
            assert_eq!(current_connection_id, conn_id.as_str());
            assert_eq!(current_revision, 2);
        }
        other => panic!("expected StaleRevision, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn session_selection_update_stale_catalog() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    let conn_id = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    seed_models(
        &pool,
        &conn_id,
        &[("gpt-4o", "GPT-4o", 128000, Some(16384), true, true)],
        "cat-v1",
    )
    .await;
    let session_id = seed_session(&session_store, None).await;

    // First update succeeds.
    let outcome = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_id,
        "gpt-4o",
        Some(1),
        Some("cat-v1".to_string()),
    )
    .await
    .unwrap();
    assert!(matches!(outcome, SelectionUpdateOutcome::Updated(_)));

    // Mutate catalog revision directly in the health table.
    sqlx::query(
        "UPDATE provider_connection_health SET catalog_revision = ? WHERE connection_id = ?",
    )
    .bind("cat-v2")
    .bind(conn_id.as_str())
    .execute(&pool)
    .await
    .unwrap();

    // Attempt update with old catalog revision.
    let outcome = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_id,
        "gpt-4o",
        Some(1),
        Some("cat-v1".to_string()), // stale
    )
    .await
    .unwrap();
    match outcome {
        SelectionUpdateOutcome::StaleCatalog {
            current_revision,
            current_catalog_revision,
        } => {
            assert_eq!(current_revision, 1);
            assert_eq!(current_catalog_revision.as_deref(), Some("cat-v2"));
        }
        other => panic!("expected StaleCatalog, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn session_selection_update_disabled_connection() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    let conn_id = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    seed_models(
        &pool,
        &conn_id,
        &[("gpt-4o", "GPT-4o", 128000, Some(16384), true, true)],
        "cat-v1",
    )
    .await;
    let session_id = seed_session(&session_store, None).await;

    // Disable the connection.
    let conn = conn_store.get(&conn_id).await.unwrap().unwrap();
    conn_store.disable(&conn_id, conn.revision).await.unwrap();

    // Attempt update.
    let outcome = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_id,
        "gpt-4o",
        None,
        None,
    )
    .await
    .unwrap();
    match outcome {
        SelectionUpdateOutcome::ConnectionNotSelectable {
            connection_id,
            state,
        } => {
            assert_eq!(connection_id, conn_id.as_str());
            assert_eq!(state, "disabled");
        }
        other => panic!("expected ConnectionNotSelectable, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn session_selection_update_unknown_model() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    let conn_id = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    seed_models(
        &pool,
        &conn_id,
        &[("gpt-4o", "GPT-4o", 128000, Some(16384), true, true)],
        "cat-v1",
    )
    .await;
    let session_id = seed_session(&session_store, None).await;

    let outcome = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_id,
        "nonexistent-model",
        None,
        None,
    )
    .await
    .unwrap();
    match outcome {
        SelectionUpdateOutcome::UnknownModel {
            connection_id,
            model_id,
        } => {
            assert_eq!(connection_id, conn_id.as_str());
            assert_eq!(model_id, "nonexistent-model");
        }
        other => panic!("expected UnknownModel, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn session_selection_list_excludes_other_connections() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    let _id_a = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI A",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    let _id_b = seed_connection(
        &conn_store,
        ProviderKind::Anthropic,
        "Anthropic B",
        "http://b.example.com",
        "acct-b",
    )
    .await;
    let session_id = seed_session(&session_store, None).await;

    let summaries = list_selection(&session_store, &conn_store, &session_id)
        .await
        .unwrap();
    assert_eq!(summaries.len(), 2);
    let kinds: Vec<&str> = summaries.iter().map(|s| s.provider_kind.as_str()).collect();
    assert!(kinds.contains(&"openai"));
    assert!(kinds.contains(&"anthropic"));
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_unresolved_returns_diagnostic() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    // Session with legacy model string but no matching connection.
    let session_id = seed_session(&session_store, Some("openai/gpt-4o")).await;
    let dto = get_selection(&session_store, &conn_store, &session_id)
        .await
        .unwrap();
    match dto {
        SessionSelectionDto::LegacyUnresolved {
            legacy_provider,
            reason,
            ..
        } => {
            assert_eq!(legacy_provider, "openai");
            assert!(reason.contains("No active connection"));
        }
        other => panic!("expected LegacyUnresolved, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_resolution_does_not_auto_select() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    // Create a single active OpenAI connection.
    let _conn_id = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI",
        "http://a.example.com",
        "acct-a",
    )
    .await;

    // Session with legacy model string that resolves to the connection.
    let session_id = seed_session(&session_store, Some("openai/gpt-4o")).await;
    let dto = get_selection(&session_store, &conn_store, &session_id)
        .await
        .unwrap();
    // Even though the legacy string resolves, it maps to Unselected (no auto-promotion).
    assert!(matches!(dto, SessionSelectionDto::Unselected {}));
}

#[tokio::test(flavor = "current_thread")]
async fn two_sessions_can_share_one_connection() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    let conn_id = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    seed_models(
        &pool,
        &conn_id,
        &[
            ("gpt-4o", "GPT-4o", 128000, Some(16384), true, true),
            (
                "gpt-4o-mini",
                "GPT-4o Mini",
                128000,
                Some(16384),
                true,
                true,
            ),
        ],
        "cat-v1",
    )
    .await;

    let session_a = seed_session(&session_store, None).await;
    let session_b = seed_session(&session_store, None).await;

    // Session A selects gpt-4o.
    let outcome_a = update_selection(
        &session_store,
        &conn_store,
        &session_a,
        &conn_id,
        "gpt-4o",
        None,
        None,
    )
    .await
    .unwrap();
    assert!(matches!(outcome_a, SelectionUpdateOutcome::Updated(_)));

    // Session B selects gpt-4o-mini.
    let outcome_b = update_selection(
        &session_store,
        &conn_store,
        &session_b,
        &conn_id,
        "gpt-4o-mini",
        None,
        None,
    )
    .await
    .unwrap();
    assert!(matches!(outcome_b, SelectionUpdateOutcome::Updated(_)));

    // Verify each session has its own model.
    let dto_a = get_selection(&session_store, &conn_store, &session_a)
        .await
        .unwrap();
    match dto_a {
        SessionSelectionDto::Selected { model, .. } => assert_eq!(model.model_id, "gpt-4o"),
        other => panic!("expected Selected for A, got {other:?}"),
    }
    let dto_b = get_selection(&session_store, &conn_store, &session_b)
        .await
        .unwrap();
    match dto_b {
        SessionSelectionDto::Selected { model, .. } => assert_eq!(model.model_id, "gpt-4o-mini"),
        other => panic!("expected Selected for B, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn no_silent_fallback_to_another_connection() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    // Two OpenAI connections.
    let _id_a = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI A",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    let _id_b = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI B",
        "http://b.example.com",
        "acct-b",
    )
    .await;

    // Session with legacy model string matches both → ambiguous.
    let session_id = seed_session(&session_store, Some("openai/gpt-4o")).await;
    let dto = get_selection(&session_store, &conn_store, &session_id)
        .await
        .unwrap();
    match dto {
        SessionSelectionDto::LegacyUnresolved {
            legacy_provider,
            reason,
            ..
        } => {
            assert_eq!(legacy_provider, "openai");
            assert!(reason.contains("Multiple connections"));
        }
        other => panic!("expected LegacyUnresolved with multiple, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn update_with_null_expected_revisions_replaces_selection() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    let conn_id = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    seed_models(
        &pool,
        &conn_id,
        &[("gpt-4o", "GPT-4o", 128000, Some(16384), true, true)],
        "cat-v1",
    )
    .await;
    let session_id = seed_session(&session_store, None).await;

    // First update.
    let outcome = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_id,
        "gpt-4o",
        None,
        None,
    )
    .await
    .unwrap();
    assert!(matches!(outcome, SelectionUpdateOutcome::Updated(_)));

    // Second update with null expected revisions replaces without conflict.
    let outcome = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_id,
        "gpt-4o",
        None,
        None,
    )
    .await
    .unwrap();
    match outcome {
        SelectionUpdateOutcome::Updated(d) => {
            assert!(matches!(d, SessionSelectionDto::Selected { .. }));
        }
        other => panic!("expected Updated, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn connection_store_error_propagates_as_selection_error() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    // Non-existent session returns SessionNotFound.
    let result = get_selection(&session_store, &conn_store, "nonexistent-id").await;
    assert!(matches!(result, Err(SelectionError::SessionNotFound(_))));

    // Non-existent connection via update.
    let session_id = seed_session(&session_store, None).await;
    let bad_id = ProviderConnectionId::parse("00000000-0000-0000-0000-000000000000").unwrap();
    let result = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &bad_id,
        "gpt-4o",
        None,
        None,
    )
    .await;
    match result {
        Err(SelectionError::ConnectionStore(_)) => {}
        other => panic!("expected ConnectionStore error, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn models_list_is_revision_safe() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    let conn_id = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    seed_models(
        &pool,
        &conn_id,
        &[("gpt-4o", "GPT-4o", 128000, Some(16384), true, true)],
        "cat-v1",
    )
    .await;
    let session_id = seed_session(&session_store, None).await;

    // List models at initial revision.
    let (catalog_rev, models) =
        list_selection_models(&session_store, &conn_store, &session_id, &conn_id)
            .await
            .unwrap();
    assert_eq!(catalog_rev.as_deref(), Some("cat-v1"));
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "gpt-4o");

    // Update connection to bump revision (change secret_binding).
    let conn = conn_store.get(&conn_id).await.unwrap().unwrap();
    let mut update = codegg_core::provider_connections::ProviderConnectionUpdate::from(&conn);
    update.secret_binding =
        Some(SecretBindingLocator::new(SecretRef::new(), "test-provider", "new-account").unwrap());
    conn_store
        .update(&conn_id, conn.revision, update)
        .await
        .unwrap();

    // Catalog models are pinned to the old revision; new revision has no models.
    let conn = conn_store.get(&conn_id).await.unwrap().unwrap();
    assert_eq!(conn.revision, 2);

    let (catalog_rev, models) =
        list_selection_models(&session_store, &conn_store, &session_id, &conn_id)
            .await
            .unwrap();
    // No health row at revision 2, so catalog_revision is None.
    assert!(catalog_rev.is_none());
    // No model rows at revision 2.
    assert!(models.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn update_preserves_connection_revision() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    let conn_id = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    seed_models(
        &pool,
        &conn_id,
        &[("gpt-4o", "GPT-4o", 128000, Some(16384), true, true)],
        "cat-v1",
    )
    .await;
    let session_id = seed_session(&session_store, None).await;

    let conn_before = conn_store.get(&conn_id).await.unwrap().unwrap();
    let rev_before = conn_before.revision;

    let outcome = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_id,
        "gpt-4o",
        None,
        None,
    )
    .await
    .unwrap();

    match outcome {
        SelectionUpdateOutcome::Updated(SessionSelectionDto::Selected {
            connection_revision,
            ..
        }) => {
            assert_eq!(connection_revision, rev_before);
        }
        other => panic!("expected Updated with matching revision, got {other:?}"),
    }

    // The connection itself was not modified by the selection update.
    let conn_after = conn_store.get(&conn_id).await.unwrap().unwrap();
    assert_eq!(conn_after.revision, rev_before);
}

#[tokio::test(flavor = "current_thread")]
async fn update_with_concurrent_revision_conflict() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    let conn_id = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    seed_models(
        &pool,
        &conn_id,
        &[
            ("gpt-4o", "GPT-4o", 128000, Some(16384), true, true),
            (
                "gpt-4o-mini",
                "GPT-4o Mini",
                128000,
                Some(16384),
                true,
                true,
            ),
        ],
        "cat-v1",
    )
    .await;
    let session_id = seed_session(&session_store, None).await;

    // First update succeeds.
    let outcome1 = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_id,
        "gpt-4o",
        Some(1),
        Some("cat-v1".to_string()),
    )
    .await
    .unwrap();
    assert!(matches!(outcome1, SelectionUpdateOutcome::Updated(_)));

    // Simulate another session updating the same connection's revision.
    let conn = conn_store.get(&conn_id).await.unwrap().unwrap();
    let mut update = codegg_core::provider_connections::ProviderConnectionUpdate::from(&conn);
    update.secret_binding =
        Some(SecretBindingLocator::new(SecretRef::new(), "test-provider", "bumped").unwrap());
    conn_store
        .update(&conn_id, conn.revision, update)
        .await
        .unwrap();

    // Second update with stale revision → StaleRevision.
    let outcome2 = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_id,
        "gpt-4o-mini",
        Some(1),
        Some("cat-v1".to_string()),
    )
    .await
    .unwrap();
    match outcome2 {
        SelectionUpdateOutcome::StaleRevision {
            current_connection_id,
            current_revision,
        } => {
            assert_eq!(current_connection_id, conn_id.as_str());
            assert_eq!(current_revision, 2);
        }
        other => panic!("expected StaleRevision, got {other:?}"),
    }

    // The session's stored selection is unchanged (StaleRevision did not
    // mutate it), but get_selection returns Unselected because the stored
    // connection revision (1) no longer matches the connection's current
    // revision (2), so the resolver falls through to legacy resolution.
    let dto = get_selection(&session_store, &conn_store, &session_id)
        .await
        .unwrap();
    assert!(matches!(dto, SessionSelectionDto::Unselected {}));
}

#[tokio::test(flavor = "current_thread")]
async fn list_models_returns_catalog_models() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    let conn_id = seed_connection(
        &conn_store,
        ProviderKind::OpenAi,
        "OpenAI",
        "http://a.example.com",
        "acct-a",
    )
    .await;
    seed_models(
        &pool,
        &conn_id,
        &[
            ("gpt-4o", "GPT-4o", 128000, Some(16384), true, true),
            ("gpt-4o-mini", "GPT-4o Mini", 128000, Some(8192), true, true),
        ],
        "cat-v1",
    )
    .await;
    let session_id = seed_session(&session_store, None).await;

    let (_catalog_rev, models) =
        list_selection_models(&session_store, &conn_store, &session_id, &conn_id)
            .await
            .unwrap();
    assert_eq!(models.len(), 2);
    // Sorted by model_id.
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["gpt-4o", "gpt-4o-mini"]);
    assert_eq!(models[0].name, "GPT-4o");
    assert_eq!(models[0].context_window, 128000);
    assert_eq!(models[0].max_output_tokens, Some(16384));
    assert!(models[0].supports_tools);
    assert!(models[0].supports_vision);
}
