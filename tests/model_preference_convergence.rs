//! M004: selected-model / runtime-preference convergence.
//!
//! Focused coverage for plan §10 at the narrowest meaningful scope:
//! preference resolution states, explicit precedence, removed
//! model/disabled connection, ModelSelect adapter resolution, runtime
//! projection helpers, restart durability, CAS contention, and
//! secret-free/cross-principal negative cases.

mod common;

use codegg::core::session_selection::{
    apply_last_used_preference, apply_preference_error_code, canonical_runtime_model,
    durable_selected_runtime_model, has_explicit_selection, resolve_model_select_target,
    update_selection, ModelSelectResolveError, SelectionUpdateOutcome,
};
use codegg_core::approval::{PreferenceApplicationOutcome, RuntimePreferenceStore};
use codegg_core::identity::{PrincipalId, ProviderConnectionId};
use codegg_core::provider_connections::{
    Endpoint, NewProviderConnection, ProviderConnectionStore, ProviderKind, ProviderScope,
    SecretBindingLocator, SecretRef, TlsPolicy,
};
use codegg_core::session::{CreateSession, SessionStore};
use codegg_protocol::provider::SessionSelectionDto;

async fn migrated_pool() -> sqlx::SqlitePool {
    let pool = common::pool::isolated_pool().await;
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

async fn seed_connection(
    store: &ProviderConnectionStore,
    kind: ProviderKind,
    display_name: &str,
    secret_account: &str,
) -> ProviderConnectionId {
    let input = NewProviderConnection {
        provider_kind: kind,
        display_name: display_name.to_string(),
        endpoint: Endpoint::new("http://a.example.com", TlsPolicy::Disabled).unwrap(),
        tls_policy: TlsPolicy::Disabled,
        scope: personal_scope(),
        secret_binding: Some(
            SecretBindingLocator::new(SecretRef::new(), "test-provider", secret_account).unwrap(),
        ),
    };
    store.create(input).await.expect("create connection").id
}

async fn seed_models(
    pool: &sqlx::SqlitePool,
    connection_id: &ProviderConnectionId,
    models: &[(&str, &str)],
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
    for (model_id, model_name) in models {
        sqlx::query(
            "INSERT INTO provider_connection_models \
             (connection_id, revision, model_id, model_name, context_window, \
              max_output_tokens, supports_tools, supports_vision) \
             VALUES (?, 1, ?, ?, 128000, 16384, 1, 1)",
        )
        .bind(connection_id.as_str())
        .bind(model_id)
        .bind(model_name)
        .execute(pool)
        .await
        .expect("seed model");
    }
}

async fn seed_session(session_store: &SessionStore) -> String {
    session_store
        .create(CreateSession {
            project_id: "test-proj".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Test".to_string()),
            parent_id: None,
            workspace_id: None,
            agent: None,
            model: None,
            tags: None,
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
        })
        .await
        .expect("create session")
        .id
}

// ── Preference resolution states ─────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn preference_applies_to_unselected_session_with_current_revisions() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());
    let pref_store = RuntimePreferenceStore::new(pool.clone());

    let conn_id = seed_connection(&conn_store, ProviderKind::OpenAi, "OpenAI", "acct-a").await;
    seed_models(&pool, &conn_id, &[("gpt-4o", "GPT-4o")], "cat-v1").await;
    let session_id = seed_session(&session_store).await;
    pref_store
        .set_model_preference("alice", Some(conn_id.as_str()), Some("gpt-4o"), None)
        .await
        .unwrap();

    let outcome = apply_last_used_preference(
        &session_store,
        &conn_store,
        &pref_store,
        &session_id,
        "alice",
    )
    .await
    .unwrap();
    assert!(
        matches!(outcome, PreferenceApplicationOutcome::Applied { .. }),
        "expected Applied, got {outcome:?}"
    );
    // Durable row now carries the explicit selection.
    let session = session_store.get(&session_id).await.unwrap().unwrap();
    assert!(has_explicit_selection(&session));
    assert_eq!(session.selected_model_id.as_deref(), Some("gpt-4o"));
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_selection_wins_over_preference() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());
    let pref_store = RuntimePreferenceStore::new(pool.clone());

    let conn_a = seed_connection(&conn_store, ProviderKind::OpenAi, "A", "acct-a").await;
    seed_models(&pool, &conn_a, &[("gpt-4o", "GPT-4o")], "cat-v1").await;
    let conn_b = seed_connection(&conn_store, ProviderKind::Anthropic, "B", "acct-b").await;
    seed_models(&pool, &conn_b, &[("claude-3", "Claude 3")], "cat-v1").await;

    let session_id = seed_session(&session_store).await;
    // Explicit binding to A/gpt-4o.
    let outcome = update_selection(
        &session_store,
        &conn_store,
        &session_id,
        &conn_a,
        "gpt-4o",
        None,
        None,
    )
    .await
    .unwrap();
    assert!(matches!(outcome, SelectionUpdateOutcome::Updated(_)));
    // Preference remembers B/claude-3 but must not overwrite.
    pref_store
        .set_model_preference("alice", Some(conn_b.as_str()), Some("claude-3"), None)
        .await
        .unwrap();
    let outcome = apply_last_used_preference(
        &session_store,
        &conn_store,
        &pref_store,
        &session_id,
        "alice",
    )
    .await
    .unwrap();
    assert_eq!(
        outcome,
        PreferenceApplicationOutcome::ExplicitSelectionPresent
    );
    let session = session_store.get(&session_id).await.unwrap().unwrap();
    assert_eq!(session.selected_model_id.as_deref(), Some("gpt-4o"));
}

#[tokio::test(flavor = "current_thread")]
async fn missing_preference_yields_no_preference() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());
    let pref_store = RuntimePreferenceStore::new(pool.clone());
    let session_id = seed_session(&session_store).await;
    let outcome = apply_last_used_preference(
        &session_store,
        &conn_store,
        &pref_store,
        &session_id,
        "nobody",
    )
    .await
    .unwrap();
    assert_eq!(outcome, PreferenceApplicationOutcome::NoPreference);
}

#[tokio::test(flavor = "current_thread")]
async fn removed_model_leaves_session_unselected() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());
    let pref_store = RuntimePreferenceStore::new(pool.clone());

    let conn_id = seed_connection(&conn_store, ProviderKind::OpenAi, "OpenAI", "acct-a").await;
    seed_models(&pool, &conn_id, &[("gpt-4o", "GPT-4o")], "cat-v1").await;
    let session_id = seed_session(&session_store).await;
    pref_store
        .set_model_preference("alice", Some(conn_id.as_str()), Some("gone-model"), None)
        .await
        .unwrap();

    let outcome = apply_last_used_preference(
        &session_store,
        &conn_store,
        &pref_store,
        &session_id,
        "alice",
    )
    .await
    .unwrap();
    assert!(
        matches!(outcome, PreferenceApplicationOutcome::UnknownModel { .. }),
        "expected UnknownModel, got {outcome:?}"
    );
    // No silent fallback: the session stays unselected.
    let session = session_store.get(&session_id).await.unwrap().unwrap();
    assert!(!has_explicit_selection(&session));
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_connection_leaves_session_unselected() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());
    let pref_store = RuntimePreferenceStore::new(pool.clone());

    let conn_id = seed_connection(&conn_store, ProviderKind::OpenAi, "OpenAI", "acct-a").await;
    seed_models(&pool, &conn_id, &[("gpt-4o", "GPT-4o")], "cat-v1").await;
    let conn = conn_store.get(&conn_id).await.unwrap().unwrap();
    conn_store.disable(&conn_id, conn.revision).await.unwrap();

    let session_id = seed_session(&session_store).await;
    pref_store
        .set_model_preference("alice", Some(conn_id.as_str()), Some("gpt-4o"), None)
        .await
        .unwrap();
    let outcome = apply_last_used_preference(
        &session_store,
        &conn_store,
        &pref_store,
        &session_id,
        "alice",
    )
    .await
    .unwrap();
    assert!(
        matches!(
            outcome,
            PreferenceApplicationOutcome::UnavailableConnection { .. }
        ),
        "expected UnavailableConnection, got {outcome:?}"
    );
    let session = session_store.get(&session_id).await.unwrap().unwrap();
    assert!(!has_explicit_selection(&session));
}

// ── ModelSelect adapter resolution ───────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn model_select_resolves_single_active_connection() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let conn_id = seed_connection(&conn_store, ProviderKind::OpenAi, "OpenAI", "acct-a").await;
    seed_models(&pool, &conn_id, &[("gpt-4o", "GPT-4o")], "cat-v1").await;

    let (resolved_conn, resolved_model) = resolve_model_select_target(&conn_store, "openai/gpt-4o")
        .await
        .unwrap();
    assert_eq!(resolved_conn.as_str(), conn_id.as_str());
    assert_eq!(resolved_model, "gpt-4o");
}

#[tokio::test(flavor = "current_thread")]
async fn model_select_rejects_empty_and_provider_only_strings() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let err = resolve_model_select_target(&conn_store, "   ")
        .await
        .unwrap_err();
    assert_eq!(err, ModelSelectResolveError::EmptyModel);
    assert_eq!(err.code(), "model_not_specified");

    let conn_id = seed_connection(&conn_store, ProviderKind::OpenAi, "OpenAI", "acct-a").await;
    seed_models(&pool, &conn_id, &[("gpt-4o", "GPT-4o")], "cat-v1").await;
    let err = resolve_model_select_target(&conn_store, "openai")
        .await
        .unwrap_err();
    assert!(matches!(err, ModelSelectResolveError::ModelRequired { .. }));
}

#[tokio::test(flavor = "current_thread")]
async fn model_select_rejects_unknown_and_ambiguous_providers() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let err = resolve_model_select_target(&conn_store, "openai/gpt-4o")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        ModelSelectResolveError::UnknownProvider { .. }
    ));

    let _a = seed_connection(&conn_store, ProviderKind::OpenAi, "A", "acct-a").await;
    let _b = seed_connection(&conn_store, ProviderKind::OpenAi, "B", "acct-b").await;
    let err = resolve_model_select_target(&conn_store, "openai/gpt-4o")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        ModelSelectResolveError::AmbiguousProvider { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn model_select_rejects_disabled_connection_without_fallback() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let conn_id = seed_connection(&conn_store, ProviderKind::OpenAi, "OpenAI", "acct-a").await;
    seed_models(&pool, &conn_id, &[("gpt-4o", "GPT-4o")], "cat-v1").await;
    let conn = conn_store.get(&conn_id).await.unwrap().unwrap();
    conn_store.disable(&conn_id, conn.revision).await.unwrap();
    let err = resolve_model_select_target(&conn_store, "openai/gpt-4o")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        ModelSelectResolveError::ConnectionNotSelectable { .. }
    ));
}

// ── Runtime projection helpers ───────────────────────────────────────

#[test]
fn canonical_runtime_model_matches_turn_format() {
    assert_eq!(canonical_runtime_model("openai", "gpt-4o"), "openai/gpt-4o");
}

#[test]
fn durable_selection_projects_to_canonical_runtime_string() {
    let selection = SessionSelectionDto::Selected {
        connection: Box::new(codegg_protocol::provider::ProviderConnectionSummaryDto {
            id: "conn-1".into(),
            provider_kind: "openai".into(),
            display_name: "OpenAI".into(),
            endpoint: "http://a.example.com".into(),
            tls_policy: "disabled".into(),
            scope: "personal".into(),
            state: "active".into(),
            revision: 1,
            model_count: 1,
            catalog_revision: Some("cat-v1".into()),
            health: None,
        }),
        model: Box::new(codegg_protocol::provider::SelectedModelDto {
            connection_id: "conn-1".into(),
            model_id: "gpt-4o".into(),
            model_name: "GPT-4o".into(),
            context_window: 128000,
            max_output_tokens: Some(16384),
            supports_tools: true,
            supports_vision: true,
            catalog_revision: "cat-v1".into(),
        }),
        connection_revision: 1,
        catalog_revision: "cat-v1".into(),
    };
    assert_eq!(
        durable_selected_runtime_model(&selection).as_deref(),
        Some("openai/gpt-4o")
    );
    assert_eq!(
        durable_selected_runtime_model(&SessionSelectionDto::Unselected {}),
        None
    );
}

// ── Restart / contention / security / migration ──────────────────────

#[tokio::test(flavor = "current_thread")]
async fn selection_and_preference_survive_daemon_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("m004.db");
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    codegg_core::session::schema::migrate(&pool).await.unwrap();
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
    .unwrap();
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());
    let pref_store = RuntimePreferenceStore::new(pool.clone());
    let conn_id = seed_connection(&conn_store, ProviderKind::OpenAi, "OpenAI", "acct-a").await;
    seed_models(&pool, &conn_id, &[("gpt-4o", "GPT-4o")], "cat-v1").await;
    let session_id = seed_session(&session_store).await;
    update_selection(
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
    pref_store
        .set_model_preference("alice", Some(conn_id.as_str()), Some("gpt-4o"), None)
        .await
        .unwrap();
    pool.close().await;

    // Restart: reopen the same file; both rows must still be present.
    let pool2 = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    codegg_core::session::schema::migrate(&pool2).await.unwrap();
    let session_store2 = SessionStore::new(pool2.clone());
    let pref_store2 = RuntimePreferenceStore::new(pool2);
    let session = session_store2.get(&session_id).await.unwrap().unwrap();
    assert!(has_explicit_selection(&session));
    let pref = pref_store2.get("alice").await.unwrap().unwrap();
    assert_eq!(
        pref.last_provider_connection_id.as_deref(),
        Some(conn_id.as_str())
    );
    assert_eq!(pref.last_model_id.as_deref(), Some("gpt-4o"));
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_selection_updates_conflict_on_catalog_revision() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());
    let conn_id = seed_connection(&conn_store, ProviderKind::OpenAi, "OpenAI", "acct-a").await;
    seed_models(&pool, &conn_id, &[("gpt-4o", "GPT-4o")], "cat-v1").await;
    let session_id = seed_session(&session_store).await;
    let first = update_selection(
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
    assert!(matches!(first, SelectionUpdateOutcome::Updated(_)));
    // Catalog moves underneath the second writer.
    sqlx::query(
        "UPDATE provider_connection_health SET catalog_revision = ? WHERE connection_id = ?",
    )
    .bind("cat-v2")
    .bind(conn_id.as_str())
    .execute(&pool)
    .await
    .unwrap();
    let second = update_selection(
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
    assert!(
        matches!(second, SelectionUpdateOutcome::StaleCatalog { .. }),
        "expected StaleCatalog, got {second:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn crafted_connection_id_is_rejected_without_cross_scope_access() {
    let pool = migrated_pool().await;
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());
    let pref_store = RuntimePreferenceStore::new(pool.clone());
    let session_id = seed_session(&session_store).await;
    // Crafted non-UUID connection identity: preference apply must not
    // touch another scope; it stays unselected with a diagnostic.
    pref_store
        .set_model_preference("alice", Some("not-a-uuid"), Some("gpt-4o"), None)
        .await
        .unwrap();
    let outcome = apply_last_used_preference(
        &session_store,
        &conn_store,
        &pref_store,
        &session_id,
        "alice",
    )
    .await
    .unwrap();
    assert!(
        matches!(
            outcome,
            PreferenceApplicationOutcome::UnavailableConnection { .. }
        ),
        "expected UnavailableConnection, got {outcome:?}"
    );
    // Preference-store errors keep a stable secret-free code.
    let err = pref_store.get(&"p".repeat(300)).await.unwrap_err();
    assert_eq!(
        apply_preference_error_code(
            &codegg::core::session_selection::ApplyPreferenceError::Preference(err)
        ),
        "preference_invalid"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn pre_m004_preference_rows_default_safely() {
    let pool = migrated_pool().await;
    sqlx::query(
        "INSERT INTO runtime_preferences
         (principal_id, approval_mode, sandbox_profile, revision, updated_at)
         VALUES ('legacy', 'interactive', 'workspace_write', 1, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let store = RuntimePreferenceStore::new(pool.clone());
    let pref = store.get("legacy").await.unwrap().unwrap();
    assert!(!pref.has_model_preference());
    // Applying with no model preference is a clean NoPreference.
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());
    let session_id = seed_session(&session_store).await;
    let outcome =
        apply_last_used_preference(&session_store, &conn_store, &store, &session_id, "legacy")
            .await
            .unwrap();
    assert_eq!(outcome, PreferenceApplicationOutcome::NoPreference);
}
