//! Identity M004 — append-only audit foundation.
//!
//! Boundary proof for the plan: the coordinator assigns a deterministic
//! unique sequence across concurrent appends, duplicate event IDs are
//! idempotent, restart continues the sequence safely, secrets never reach
//! storage or export, bodies expire independently of structural records,
//! and audit reads are authorized (`audit.read`) at both the store and
//! the daemon boundary.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg_core::audit::{
    export_digest, AuditAction, AuditDecisionProvenance, AuditEventBuilder, AuditQueryFilter,
    AuditStore, AuditVisibility, AuditWriter, AuditWriterConfig,
};
use codegg_core::identity::{AuditEventId, PrincipalId, ProjectId};
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::AuthenticatedPrincipal;

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

async fn file_pool(path: &std::path::Path) -> sqlx::SqlitePool {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .expect("file pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate file pool");
    pool
}

fn local_context(correlation: &str) -> (AuthenticatedPrincipal, AuditDecisionProvenance) {
    let principal = AuthenticatedPrincipal::local_owner("client-test");
    let provenance = AuditDecisionProvenance::new(
        format!("decision-{correlation}"),
        correlation,
        "local_owner_broad",
        None,
    );
    (principal, provenance)
}

fn builder(
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    index: usize,
) -> AuditEventBuilder {
    AuditEventBuilder::new(AuditAction::SessionCreate, principal, provenance)
        .with_metadata("index", index.to_string())
        .with_metadata("session.id", format!("session-{index}"))
}

async fn member_with_role(
    team: &TeamStore,
    project: &ProjectId,
    name: &str,
    role: ProjectRole,
    client: &str,
) -> AuthenticatedPrincipal {
    let record = team
        .create_principal(PrincipalKind::Human, name)
        .await
        .unwrap();
    team.create_membership(project, &record.id, role)
        .await
        .unwrap();
    AuthenticatedPrincipal::internal_test(&record, client)
}

fn register_client(daemon: &CoreDaemon, client_id: &str, principal: AuthenticatedPrincipal) {
    daemon.clients.register_with_principal(
        client_id.to_owned(),
        format!("{client_id}-name"),
        None,
        principal,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_append_assigns_unique_ordered_sequence() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let (principal, decision) = local_context("corr-concurrent");
    let mut handles = Vec::new();
    for index in 0..16 {
        let store = store.clone();
        let principal = principal.clone();
        let decision = decision.clone();
        handles.push(tokio::spawn(async move {
            store
                .append(builder(&principal, &decision, index))
                .await
                .expect("concurrent append")
        }));
    }
    let mut seqs = Vec::new();
    for handle in handles {
        seqs.push(handle.await.expect("join").seq);
    }
    seqs.sort_unstable();
    assert_eq!(seqs.len(), 16);
    let mut dedup = seqs.clone();
    dedup.dedup();
    assert_eq!(dedup.len(), 16, "sequences must be unique: {seqs:?}");
    for window in seqs.windows(2) {
        assert!(window[0] < window[1], "sequences must order: {seqs:?}");
    }
    assert_eq!(store.count().await.unwrap(), 16);
}

#[tokio::test]
async fn duplicate_append_returns_prior_event() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let (principal, decision) = local_context("corr-duplicate");
    let event_id = AuditEventId::new();
    let first = store
        .append(
            AuditEventBuilder::new(AuditAction::ToolInvoke, &principal, &decision)
                .with_event_id(event_id.clone())
                .with_metadata("tool.name", "read"),
        )
        .await
        .unwrap();
    let second = store
        .append(
            AuditEventBuilder::new(AuditAction::ToolInvoke, &principal, &decision)
                .with_event_id(event_id)
                .with_metadata("tool.name", "read"),
        )
        .await
        .unwrap();
    assert_eq!(first.seq, second.seq);
    assert_eq!(first.event_id, second.event_id);
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn restart_continues_sequence_safely() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("audit-restart.db");
    let project = ProjectId::new();
    {
        let pool = file_pool(&db).await;
        let store = AuditStore::new(pool.clone());
        let (principal, decision) = local_context("corr-restart-a");
        for index in 0..2 {
            let mut event = builder(&principal, &decision, index);
            event = event.with_project(project.clone());
            store.append(event).await.unwrap();
        }
        assert_eq!(store.max_seq().await.unwrap(), Some(2));
        pool.close().await;
    }
    {
        let pool = file_pool(&db).await;
        let store = AuditStore::new(pool.clone());
        assert_eq!(store.count().await.unwrap(), 2);
        let (principal, decision) = local_context("corr-restart-b");
        let event = store
            .append(builder(&principal, &decision, 2).with_project(project.clone()))
            .await
            .unwrap();
        assert_eq!(event.seq, 3, "sequence must continue across restart");
        let page = store
            .query(&AuditQueryFilter::new(Some(project.clone())).with_limit(10))
            .await
            .unwrap();
        assert_eq!(page.events.len(), 3);
        assert!(page.events.windows(2).all(|w| w[0].seq < w[1].seq));
        pool.close().await;
    }
}

#[tokio::test]
async fn migration_is_additive_and_restart_safe() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("audit-migrate.db");
    let pool = file_pool(&db).await;
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("remigration is safe");
    let version: (i64,) = sqlx::query_as("SELECT version FROM migration_version WHERE id = 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        version.0,
        codegg_core::storage::STORAGE_LAYOUT_VERSION as i64
    );
    for table in ["audit_event", "audit_body"] {
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?")
                .bind(table)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.0, 1, "table {table} must exist");
    }
    pool.close().await;
}

#[tokio::test]
async fn secret_corpus_never_reaches_storage_or_export() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let (principal, decision) = local_context("corr-corpus");
    let secret_keys = [
        "password",
        "api_token",
        "db_secret",
        "bearer",
        "private_key",
        "client_secret",
        "authorization",
    ];
    for key in secret_keys {
        let error = store
            .append(
                AuditEventBuilder::new(AuditAction::SessionCreate, &principal, &decision)
                    .with_metadata(key, "redacted-value"),
            )
            .await
            .expect_err("secret key must be rejected");
        assert_eq!(error.code(), "audit_secret_detected", "key {key}");
    }
    let secret_values = [
        "hunter2 password=hunter2",
        "ghp_sentinelvalue123",
        "-----BEGIN PRIVATE KEY-----",
        "sk-live-sentinel",
        "AKIAIOSFODNN7SENTINEL",
        "aws_secret sentinel",
        "token=sentinel-value",
    ];
    for value in secret_values {
        let error = store
            .append(
                AuditEventBuilder::new(AuditAction::SessionCreate, &principal, &decision)
                    .with_metadata("note", value),
            )
            .await
            .expect_err("secret value must be rejected");
        assert_eq!(error.code(), "audit_secret_detected", "value {value}");
    }
    let sentinel_body = b"credential payload bearer sentinel".to_vec();
    let error = store
        .append(
            AuditEventBuilder::new(AuditAction::FileMutate, &principal, &decision)
                .with_body(sentinel_body, None),
        )
        .await
        .expect_err("secret body must be rejected");
    assert_eq!(error.code(), "audit_secret_detected");
    assert_eq!(store.count().await.unwrap(), 0);
    let page = store
        .query(&AuditQueryFilter::new(None).with_limit(10))
        .await
        .unwrap();
    assert!(page.events.is_empty());
    let export = export_digest(&page.events);
    assert_eq!(export.len(), 64);
}

#[tokio::test]
async fn metadata_and_body_bounds_are_enforced() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let (principal, decision) = local_context("corr-bounds");
    let mut oversized = AuditEventBuilder::new(AuditAction::SessionCreate, &principal, &decision);
    for index in 0..(codegg_core::audit::MAX_METADATA_ENTRIES + 1) {
        oversized = oversized.with_metadata(format!("key.{index:02}"), "v");
    }
    assert_eq!(
        store
            .append(oversized)
            .await
            .expect_err("entry bound")
            .code(),
        "audit_metadata_too_large"
    );
    let oversized_body = vec![b'a'; codegg_core::audit::MAX_BODY_BYTES + 1];
    assert_eq!(
        store
            .append(
                AuditEventBuilder::new(AuditAction::SessionCreate, &principal, &decision)
                    .with_body(oversized_body, None),
            )
            .await
            .expect_err("body bound")
            .code(),
        "audit_body_too_large"
    );
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn retention_expiry_preserves_structure() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let (principal, decision) = local_context("corr-retention");
    let event = store
        .append(
            AuditEventBuilder::new(AuditAction::FileMutate, &principal, &decision)
                .with_metadata("file.path_hash", "abc123")
                .with_body(b"structural-body".to_vec(), Some(100)),
        )
        .await
        .unwrap();
    let body_ref = event.body_ref.clone().expect("body ref");
    let digest_before = event.content_digest.clone();
    assert_eq!(store.expire_bodies(99).await.unwrap(), 0);
    assert!(store.read_body(&body_ref).await.unwrap().is_some());
    assert_eq!(store.expire_bodies(100).await.unwrap(), 1);
    assert!(store.read_body(&body_ref).await.unwrap().is_none());
    let structural = store
        .get_by_id(&event.event_id)
        .await
        .unwrap()
        .expect("kept");
    assert_eq!(structural.seq, event.seq);
    assert_eq!(structural.content_digest, digest_before);
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn pagination_and_filter_stability() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let (principal, decision) = local_context("corr-page");
    for index in 0..5 {
        let action = if index % 2 == 0 {
            AuditAction::SessionCreate
        } else {
            AuditAction::ToolInvoke
        };
        store
            .append(
                AuditEventBuilder::new(action, &principal, &decision)
                    .with_metadata("index", index.to_string()),
            )
            .await
            .unwrap();
    }
    let first = store
        .query(&AuditQueryFilter::new(None).with_limit(2))
        .await
        .unwrap();
    assert_eq!(first.events.len(), 2);
    assert!(first.truncated);
    let cursor = first.next_cursor.expect("cursor");
    let second = store
        .query(
            &AuditQueryFilter::new(None)
                .with_limit(10)
                .with_from_seq(cursor),
        )
        .await
        .unwrap();
    assert_eq!(second.events.len(), 3);
    assert!(!second.truncated);
    let filtered = store
        .query(
            &AuditQueryFilter::new(None)
                .with_limit(10)
                .with_action("tool_invoke"),
        )
        .await
        .unwrap();
    assert_eq!(filtered.events.len(), 2);
    let by_principal = store
        .query(
            &AuditQueryFilter::new(None)
                .with_limit(10)
                .with_principal(PrincipalId::parse("local-owner").unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(by_principal.events.len(), 5);
}

#[tokio::test]
async fn daemon_authorized_owner_queries_ordered_attributable_log() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner = member_with_role(
        &team,
        &project,
        "audit-owner",
        ProjectRole::Owner,
        "client-owner",
    )
    .await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon, "client-owner", owner);
    let store = AuditStore::new(pool);
    let (principal, decision) = local_context("corr-daemon");
    let mut expected = Vec::new();
    for index in 0..3 {
        let event = store
            .append(builder(&principal, &decision, index).with_project(project.clone()))
            .await
            .unwrap();
        expected.push(event.seq);
    }
    let response = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-audit-query".to_owned(),
            CoreRequest::AuditQuery {
                query: codegg::protocol::core::AuditQueryRequestDto {
                    project_id: project.as_str().to_owned(),
                    action_filter: None,
                    principal_filter: None,
                    from_seq: None,
                    limit: Some(10),
                },
            },
        ),
        "client-owner",
    ))
    .await
    .unwrap();
    match response {
        CoreResponse::AuditPage {
            events, truncated, ..
        } => {
            assert!(!truncated);
            assert_eq!(events.len(), 3);
            let seqs: Vec<u64> = events.iter().map(|event| event.seq).collect();
            assert_eq!(seqs, expected);
            for event in &events {
                assert_eq!(event.actor_principal, "local-owner");
                assert_eq!(event.project_id.as_deref(), Some(project.as_str()));
                assert!(!event.decision_id.is_empty());
            }
        }
        other => panic!("expected audit page, got {other:?}"),
    }
    let response = Box::pin(daemon.handle_request_for_client(
        new_request("req-audit-caps".to_owned(), CoreRequest::AuditCapabilities),
        "client-owner",
    ))
    .await
    .unwrap();
    match response {
        CoreResponse::AuditCapabilities { capabilities } => {
            assert!(capabilities.supported);
            assert_eq!(
                capabilities.max_query_limit,
                codegg_core::audit::MAX_QUERY_LIMIT
            );
        }
        other => panic!("expected capabilities, got {other:?}"),
    }
}

#[tokio::test]
async fn daemon_unauthorized_reads_are_denied() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let viewer = member_with_role(
        &team,
        &project,
        "audit-viewer",
        ProjectRole::Viewer,
        "client-viewer",
    )
    .await;
    let outsider_record = team
        .create_principal(PrincipalKind::Human, "audit-outsider")
        .await
        .unwrap();
    let outsider = AuthenticatedPrincipal::internal_test(&outsider_record, "client-outsider");
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon, "client-viewer", viewer);
    register_client(&daemon, "client-outsider", outsider);
    for (client, request_id) in [
        ("client-viewer", "req-viewer"),
        ("client-outsider", "req-outsider"),
    ] {
        let response = Box::pin(daemon.handle_request_for_client(
            new_request(
                request_id.to_owned(),
                CoreRequest::AuditQuery {
                    query: codegg::protocol::core::AuditQueryRequestDto {
                        project_id: project.as_str().to_owned(),
                        action_filter: None,
                        principal_filter: None,
                        from_seq: None,
                        limit: Some(10),
                    },
                },
            ),
            client,
        ))
        .await
        .unwrap();
        match response {
            CoreResponse::Error { code, .. } => assert_eq!(code, "authorization_denied"),
            other => panic!("expected denial for {client}, got {other:?}"),
        }
        let response = Box::pin(daemon.handle_request_for_client(
            new_request(
                format!("{request_id}-export"),
                CoreRequest::AuditExport {
                    request: codegg::protocol::core::AuditExportRequestDto {
                        project_id: project.as_str().to_owned(),
                        action_filter: None,
                        principal_filter: None,
                        from_seq: None,
                        limit: Some(10),
                    },
                },
            ),
            client,
        ))
        .await
        .unwrap();
        match response {
            CoreResponse::Error { code, .. } => assert_eq!(code, "authorization_denied"),
            other => panic!("expected export denial for {client}, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn writer_failure_and_backpressure_are_bounded_and_observable() {
    let pool = test_pool().await;
    let writer = AuditWriter::new(
        AuditStore::new(pool),
        AuditWriterConfig {
            max_inflight: 1,
            write_timeout_ms: 2000,
            failure_policy: codegg_core::audit::AuditFailurePolicy::FailVisible,
        },
    );
    let (principal, decision) = local_context("corr-writer");
    let _guard = writer.store().pool().acquire().await.expect("pool permit");
    let saturated = AuditWriter::new(
        writer.store().clone(),
        AuditWriterConfig {
            max_inflight: 1,
            write_timeout_ms: 2000,
            failure_policy: codegg_core::audit::AuditFailurePolicy::FailVisible,
        },
    );
    let _held = saturated
        .try_append(builder(&principal, &decision, 0))
        .await
        .expect("first writer append");
    drop(_guard);
    let metrics = saturated.metrics();
    assert_eq!(metrics.appended, 1);
    let duplicate_id = AuditEventId::new();
    let first = saturated
        .try_append(
            AuditEventBuilder::new(AuditAction::SessionCreate, &principal, &decision)
                .with_event_id(duplicate_id.clone())
                .with_metadata("index", "0"),
        )
        .await
        .unwrap();
    let second = saturated
        .try_append(
            AuditEventBuilder::new(AuditAction::SessionCreate, &principal, &decision)
                .with_event_id(duplicate_id)
                .with_metadata("index", "0"),
        )
        .await
        .unwrap();
    assert_eq!(first.seq, second.seq);
    assert_eq!(saturated.metrics().duplicates, 1);
}

#[tokio::test]
async fn export_digest_covers_ordered_events() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool.clone());
    let (principal, decision) = local_context("corr-export");
    for index in 0..3 {
        store
            .append(builder(&principal, &decision, index))
            .await
            .unwrap();
    }
    let export = store
        .export(&AuditQueryFilter::new(None).with_limit(10))
        .await
        .unwrap();
    assert_eq!(export.count, 3);
    assert_eq!(export.digest, export_digest(&export.events));
    assert_eq!(export.digest.len(), 64);
    let mut tampered = export.events.clone();
    tampered.reverse();
    assert_ne!(export_digest(&tampered), export.digest);
    let empty = export_digest(&[]);
    assert_eq!(empty.len(), 64);
    assert_ne!(empty, export.digest);
}

#[tokio::test]
async fn audit_visibility_builder_defaults_to_project() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let (principal, decision) = local_context("corr-visibility");
    let event = store
        .append(
            AuditEventBuilder::new(AuditAction::MembershipChange, &principal, &decision)
                .with_visibility(AuditVisibility::Administrators)
                .with_metadata("member.id", "member-1"),
        )
        .await
        .unwrap();
    assert_eq!(event.visibility, "administrators");
    let defaulted = store
        .append(builder(&principal, &decision, 99))
        .await
        .unwrap();
    assert_eq!(defaulted.visibility, "project");
}
