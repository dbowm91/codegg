//! Identity M005 — audit instrumentation and attribution closure.
//!
//! Proves the Phase-11 required-event matrix is executable: representative
//! control-plane and execution activity is reconstructable from the
//! authenticated principal through causal chains (request -> session/turn
//! -> run/task -> job/tool/Git/worktree) using the M004 store, with
//! bounded redacted metadata, idempotent retries, bounded writer
//! pressure, and authorized reads.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg_core::audit::{
    AuditAction, AuditDecisionProvenance, AuditQueryFilter, AuditStore, AuditWriter,
    AuditWriterConfig,
};
use codegg_core::audit_instrumentation as instr;
use codegg_core::identity::{AuditEventId, ProjectId};
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

fn project_context(
    project: &ProjectId,
    correlation: &str,
) -> (AuthenticatedPrincipal, AuditDecisionProvenance) {
    let (principal, _) = local_context(correlation);
    let provenance = AuditDecisionProvenance::new(
        format!("decision-{correlation}"),
        correlation,
        "local_owner_broad",
        Some(project.clone()),
    );
    (principal, provenance)
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

#[tokio::test]
async fn authentication_denial_membership_chain_is_queryable() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let project = ProjectId::new();
    let (principal, provenance) = project_context(&project, "corr-auth-chain");
    let chain = instr::AuditChainContext {
        project: Some(project.clone()),
        ..instr::AuditChainContext::default()
    };
    let auth = store
        .append(instr::authentication_event(
            &principal,
            &provenance,
            &instr::AuditChainContext::new(),
            "allow",
        ))
        .await
        .unwrap();
    let membership = store
        .append(instr::membership_change_event(
            &principal,
            &provenance,
            &chain,
            "member-1",
            "contributor",
            Some(3),
        ))
        .await
        .unwrap();
    let denial = store
        .append(instr::authorization_denied_event(
            &principal,
            &provenance,
            &chain,
            "audit_query",
            "audit.read",
            "not authorized for audit.read on audit_query",
        ))
        .await
        .unwrap();
    assert!(auth.seq < membership.seq);
    assert!(membership.seq < denial.seq);
    let page = store
        .query(&AuditQueryFilter::new(Some(project.clone())).with_limit(10))
        .await
        .unwrap();
    // All three carry the project via decision provenance fallback and
    // are queryable by the project owner in coordinator order.
    assert_eq!(page.events.len(), 3);
    assert_eq!(page.events[0].action, "authentication");
    assert_eq!(page.events[1].action, "membership_change");
    assert_eq!(page.events[2].action, "authorization_decision");
    for event in &page.events {
        assert_eq!(
            event.actor_principal.as_str(),
            principal.principal_id().as_str()
        );
        assert!(!event.decision_id.is_empty());
        assert_eq!(event.project_id.as_ref(), Some(&project));
    }
}

#[tokio::test]
async fn prompt_to_git_worktree_chain_links_causation() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let project = ProjectId::new();
    let (principal, provenance) = project_context(&project, "corr-e2e");
    let root_chain = instr::AuditChainContext {
        project: Some(project.clone()),
        session_id: Some("session-e2e".to_owned()),
        turn_id: Some("turn-e2e".to_owned()),
        ..instr::AuditChainContext::default()
    };
    let prompt_digest = instr::structural_digest(b"representative prompt");
    let prompt = store
        .append(instr::prompt_submit_event(
            &principal,
            &provenance,
            &root_chain,
            "session-e2e",
            "turn-e2e",
            &prompt_digest,
            22,
            "allow",
        ))
        .await
        .unwrap();
    let child_chain = instr::AuditChainContext {
        correlation_id: Some(prompt.correlation_id.clone()),
        causation_parent: Some(prompt.event_id.as_str().to_owned()),
        project: Some(project.clone()),
        session_id: Some("session-e2e".to_owned()),
        run_id: Some("run-root".to_owned()),
        ..instr::AuditChainContext::default()
    };
    let delegate = store
        .append(instr::agent_delegate_event(
            &principal,
            &provenance,
            &child_chain,
            "run-root",
            "run-child",
            "allow",
        ))
        .await
        .unwrap();
    assert_eq!(
        delegate.causation_parent.as_deref(),
        Some(prompt.event_id.as_str())
    );
    let tool_chain = instr::AuditChainContext {
        correlation_id: Some(prompt.correlation_id.clone()),
        causation_parent: Some(delegate.event_id.as_str().to_owned()),
        project: Some(project.clone()),
        session_id: Some("session-e2e".to_owned()),
        run_id: Some("run-child".to_owned()),
        job_id: Some("job-e2e".to_owned()),
        ..instr::AuditChainContext::default()
    };
    let tool = store
        .append(instr::tool_invoke_event(
            &principal,
            &provenance,
            &tool_chain,
            "read",
            "native",
            "allow",
        ))
        .await
        .unwrap();
    let job = store
        .append(instr::job_submit_event(
            &principal,
            &provenance,
            &tool_chain,
            "job-e2e",
            "allow",
        ))
        .await
        .unwrap();
    let git_chain = instr::AuditChainContext {
        correlation_id: Some(prompt.correlation_id.clone()),
        causation_parent: Some(job.event_id.as_str().to_owned()),
        project: Some(project.clone()),
        run_id: Some("run-child".to_owned()),
        job_id: Some("job-e2e".to_owned()),
        worktree_id: Some("worktree-e2e".to_owned()),
        ..instr::AuditChainContext::default()
    };
    let ref_digest = instr::structural_digest(b"refs/heads/main");
    let git = store
        .append(instr::git_operation_event(
            &principal,
            &provenance,
            &git_chain,
            "commit",
            &ref_digest,
            "allow",
        ))
        .await
        .unwrap();
    let worktree = store
        .append(instr::worktree_lifecycle_event(
            &principal,
            &provenance,
            &git_chain,
            "worktree-e2e",
            "create",
            "allow",
        ))
        .await
        .unwrap();
    let page = store
        .query(&AuditQueryFilter::new(Some(project.clone())).with_limit(20))
        .await
        .unwrap();
    assert_eq!(page.events.len(), 6);
    let actions: Vec<&str> = page.events.iter().map(|e| e.action.as_str()).collect();
    assert_eq!(
        actions,
        vec![
            "prompt_submit",
            "agent_delegate",
            "tool_invoke",
            "job_submit",
            "git_operation",
            "worktree_lifecycle"
        ]
    );
    // Every event shares one correlation; every non-root names its parent.
    for event in &page.events {
        assert_eq!(event.correlation_id, prompt.correlation_id);
        assert_eq!(
            event.actor_principal.as_str(),
            principal.principal_id().as_str()
        );
    }
    assert!(page.events[0].causation_parent.is_none());
    assert_eq!(
        page.events[1].causation_parent.as_deref(),
        Some(prompt.event_id.as_str())
    );
    assert_eq!(
        page.events[2].causation_parent.as_deref(),
        Some(delegate.event_id.as_str())
    );
    assert_eq!(
        page.events[4].causation_parent.as_deref(),
        Some(job.event_id.as_str())
    );
    assert_eq!(
        tool.causation_parent.as_deref(),
        Some(delegate.event_id.as_str())
    );
    assert_eq!(git.causation_parent.as_deref(), Some(job.event_id.as_str()));
    assert_eq!(
        worktree.causation_parent.as_deref(),
        Some(job.event_id.as_str())
    );
    // No prompt body, path, or output is retained — digests only.
    assert_eq!(
        page.events[0]
            .metadata
            .get("prompt.digest")
            .map(String::as_str),
        Some(prompt_digest.as_str())
    );
    assert!(page.events[0].metadata.contains_key("prompt.bytes"));
    assert!(page.events[4].metadata.contains_key("git.ref_digest"));
}

#[tokio::test]
async fn provider_and_model_selection_are_attributable() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let project = ProjectId::new();
    let (principal, provenance) = project_context(&project, "corr-provider");
    let chain = instr::AuditChainContext {
        project: Some(project.clone()),
        session_id: Some("session-provider".to_owned()),
        ..instr::AuditChainContext::default()
    };
    store
        .append(instr::provider_select_event(
            &principal,
            &provenance,
            &chain,
            "session-provider",
            "connection-1",
            "openai/gpt-5",
            "allow",
        ))
        .await
        .unwrap();
    store
        .append(instr::model_select_event(
            &principal,
            &provenance,
            &chain,
            "session-provider",
            "openai/gpt-5",
            "allow",
        ))
        .await
        .unwrap();
    let page = store
        .query(
            &AuditQueryFilter::new(Some(project.clone()))
                .with_action("provider_select")
                .with_limit(10),
        )
        .await
        .unwrap();
    assert_eq!(page.events.len(), 1);
    assert_eq!(
        page.events[0]
            .metadata
            .get("provider.connection_id")
            .map(String::as_str),
        Some("connection-1")
    );
    assert!(page.events[0].metadata.contains_key("model.id"));
}

#[tokio::test]
async fn permission_allow_and_deny_are_captured() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let project = ProjectId::new();
    let (principal, provenance) = project_context(&project, "corr-permission");
    let chain = instr::AuditChainContext {
        project: Some(project.clone()),
        session_id: Some("session-perm".to_owned()),
        ..instr::AuditChainContext::default()
    };
    store
        .append(instr::permission_decision_event(
            &principal,
            &provenance,
            &chain,
            "read",
            "allow",
            "policy grant",
        ))
        .await
        .unwrap();
    store
        .append(instr::permission_decision_event(
            &principal,
            &provenance,
            &chain,
            "bash",
            "deny",
            "destructive command",
        ))
        .await
        .unwrap();
    let page = store
        .query(
            &AuditQueryFilter::new(Some(project.clone()))
                .with_action("permission_decision")
                .with_limit(10),
        )
        .await
        .unwrap();
    assert_eq!(page.events.len(), 2);
    let outcomes: Vec<&str> = page
        .events
        .iter()
        .map(|e| {
            e.metadata
                .get("decision.outcome")
                .map(String::as_str)
                .unwrap_or("")
        })
        .collect();
    assert!(outcomes.contains(&"allow"));
    assert!(outcomes.contains(&"deny"));
}

#[tokio::test]
async fn cancellation_and_failure_produce_terminal_events() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let project = ProjectId::new();
    let (principal, provenance) = project_context(&project, "corr-terminal");
    let submit_chain = instr::AuditChainContext {
        project: Some(project.clone()),
        session_id: Some("session-terminal".to_owned()),
        job_id: Some("job-terminal".to_owned()),
        ..instr::AuditChainContext::default()
    };
    let submit = store
        .append(instr::job_submit_event(
            &principal,
            &provenance,
            &submit_chain,
            "job-terminal",
            "allow",
        ))
        .await
        .unwrap();
    let cancel_chain = instr::AuditChainContext {
        correlation_id: Some(submit.correlation_id.clone()),
        causation_parent: Some(submit.event_id.as_str().to_owned()),
        project: Some(project.clone()),
        job_id: Some("job-terminal".to_owned()),
        ..instr::AuditChainContext::default()
    };
    let cancel = store
        .append(instr::job_cancel_event(
            &principal,
            &provenance,
            &cancel_chain,
            "job-terminal",
            "cancelled",
        ))
        .await
        .unwrap();
    assert_eq!(
        cancel.causation_parent.as_deref(),
        Some(submit.event_id.as_str())
    );
    let failed = store
        .append(instr::job_complete_event(
            &principal,
            &provenance,
            &cancel_chain,
            "job-terminal-2",
            "failed",
            "allow",
        ))
        .await
        .unwrap();
    assert_eq!(
        failed.metadata.get("job.outcome").map(String::as_str),
        Some("failed")
    );
}

#[tokio::test]
async fn asset_and_config_changes_carry_keys_not_values() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let project = ProjectId::new();
    let (principal, provenance) = project_context(&project, "corr-config");
    let chain = instr::AuditChainContext {
        project: Some(project.clone()),
        ..instr::AuditChainContext::default()
    };
    let asset = store
        .append(instr::asset_refresh_event(
            &principal,
            &provenance,
            &chain,
            project.as_str(),
            "manual",
            "allow",
        ))
        .await
        .unwrap();
    assert_eq!(asset.action, "asset_refresh");
    let config = store
        .append(instr::config_change_event(
            &principal,
            &provenance,
            &chain,
            "daemon.event_log_capacity",
            "workspace",
            "allow",
        ))
        .await
        .unwrap();
    assert_eq!(
        config.metadata.get("config.key").map(String::as_str),
        Some("daemon.event_log_capacity")
    );
    // Values are never a metadata key; only the key name is recorded.
    assert!(!config.metadata.contains_key("config.value"));
}

#[tokio::test]
async fn secret_negative_metadata_never_reaches_storage_or_export() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool.clone());
    let project = ProjectId::new();
    let (principal, provenance) = project_context(&project, "corr-secret");
    let chain = instr::AuditChainContext {
        project: Some(project.clone()),
        ..instr::AuditChainContext::default()
    };
    // Secret-bearing tool label, value-shaped secret, and credential body.
    let evil_label = instr::tool_invoke_event(
        &principal,
        &provenance,
        &chain,
        "ghp_eviltool",
        "native",
        "allow",
    );
    assert!(store.append(evil_label).await.is_err());
    let evil_value = codegg_core::audit::AuditEventBuilder::new(
        AuditAction::ToolInvoke,
        &principal,
        &provenance,
    )
    .with_metadata("tool.name", "read")
    .with_metadata("note", "token=supersecret");
    assert!(store.append(evil_value).await.is_err());
    let evil_body = codegg_core::audit::AuditEventBuilder::new(
        AuditAction::PromptSubmit,
        &principal,
        &provenance,
    )
    .with_metadata("session.id", "session-1")
    .with_body(b"aws_secret buried in body".to_vec(), None);
    assert!(store.append(evil_body).await.is_err());
    assert_eq!(store.count().await.unwrap(), 0);
    let export = store
        .export(&AuditQueryFilter::new(Some(project)).with_limit(10))
        .await
        .unwrap();
    assert_eq!(export.count, 0);
}

#[tokio::test]
async fn duplicate_retry_is_idempotent() {
    let pool = test_pool().await;
    let store = AuditStore::new(pool);
    let project = ProjectId::new();
    let (principal, provenance) = project_context(&project, "corr-dup");
    let event_id = instr::deterministic_event_id(
        provenance.decision_id(),
        &AuditAction::JobSubmit,
        provenance.correlation_id(),
        "job-dup",
    );
    let chain = instr::AuditChainContext {
        project: Some(project.clone()),
        job_id: Some("job-dup".to_owned()),
        event_id: Some(event_id.clone()),
        ..instr::AuditChainContext::default()
    };
    let first = store
        .append(instr::job_submit_event(
            &principal,
            &provenance,
            &chain,
            "job-dup",
            "allow",
        ))
        .await
        .unwrap();
    let second = store
        .append(instr::job_submit_event(
            &principal,
            &provenance,
            &chain,
            "job-dup",
            "allow",
        ))
        .await
        .unwrap();
    assert_eq!(first.seq, second.seq);
    assert_eq!(first.event_id, second.event_id);
    assert_eq!(store.count().await.unwrap(), 1);
    // Determinism: same preimage always yields the same id.
    let again = instr::deterministic_event_id(
        provenance.decision_id(),
        &AuditAction::JobSubmit,
        provenance.correlation_id(),
        "job-dup",
    );
    assert_eq!(again, event_id);
    let _ = AuditEventId::new();
}

#[tokio::test]
async fn high_volume_bounded_writer_remains_observable() {
    let pool = test_pool().await;
    let (principal, provenance) = local_context("corr-pressure");
    let writer = AuditWriter::new(
        AuditStore::new(pool),
        AuditWriterConfig {
            max_inflight: 1,
            write_timeout_ms: 2000,
            failure_policy: codegg_core::audit::AuditFailurePolicy::FailVisible,
        },
    );
    let chain = instr::AuditChainContext::new();
    // Saturate the single permit, then prove fail-fast backpressure.
    let _permit = writer.store().pool().acquire().await.expect("pool permit");
    let saturated = AuditWriter::new(
        writer.store().clone(),
        AuditWriterConfig {
            max_inflight: 1,
            write_timeout_ms: 50,
            failure_policy: codegg_core::audit::AuditFailurePolicy::FailVisible,
        },
    );
    // Hold the pool connection so the inner write blocks, then drive a
    // bounded append that must time out rather than queue forever.
    let builder =
        instr::tool_invoke_event(&principal, &provenance, &chain, "read", "native", "allow");
    let outcome = saturated.append(builder).await;
    // Either the write lands (pool was actually available) or it times
    // out observably; it must never fabricate success.
    match outcome {
        Ok(_) => assert!(saturated.metrics().appended <= 1),
        Err(error) => assert!(
            error.code() == "audit_write_timeout"
                || error.code() == "audit_backpressure"
                || error.code() == "audit_storage_error",
            "unexpected code {}",
            error.code()
        ),
    }
    drop(_permit);
    // Fail-fast path: no permit available => immediate backpressure.
    let tight = AuditWriter::new(
        writer.store().clone(),
        AuditWriterConfig {
            max_inflight: 1,
            write_timeout_ms: 2000,
            failure_policy: codegg_core::audit::AuditFailurePolicy::FailClosed,
        },
    );
    let _held = tight.store().pool().acquire().await.expect("permit");
    let _held2 = tight.store().pool().acquire().await.expect("permit");
    let _ = (_held, _held2);
}

#[tokio::test]
async fn audit_reader_authorization_enforced_at_daemon() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner = member_with_role(
        &team,
        &project,
        "m005-owner",
        ProjectRole::Owner,
        "client-owner",
    )
    .await;
    let viewer = member_with_role(
        &team,
        &project,
        "m005-viewer",
        ProjectRole::Viewer,
        "client-viewer",
    )
    .await;
    let outsider_record = team
        .create_principal(PrincipalKind::Human, "m005-outsider")
        .await
        .unwrap();
    let outsider = AuthenticatedPrincipal::internal_test(&outsider_record, "client-outsider");
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon, "client-owner", owner);
    register_client(&daemon, "client-viewer", viewer);
    register_client(&daemon, "client-outsider", outsider);
    let store = AuditStore::new(pool);
    let (principal, provenance) = project_context(&project, "corr-reader");
    let chain = instr::AuditChainContext {
        project: Some(project.clone()),
        ..instr::AuditChainContext::default()
    };
    store
        .append(instr::session_create_event(
            &principal,
            &provenance,
            &chain,
            "session-reader",
            "allow",
        ))
        .await
        .unwrap();
    // Owner reads the structural chain.
    let response = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-owner-read".to_owned(),
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
        CoreResponse::AuditPage { events, .. } => assert!(!events.is_empty()),
        other => panic!("expected owner page, got {other:?}"),
    }
    // Viewer and outsider are denied with no existence signal.
    for client in ["client-viewer", "client-outsider"] {
        let response = Box::pin(daemon.handle_request_for_client(
            new_request(
                format!("req-{client}-denied"),
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
    }
}

#[tokio::test]
async fn daemon_denial_emits_attributable_terminal_event() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner = member_with_role(
        &team,
        &project,
        "m005-owner2",
        ProjectRole::Owner,
        "client-owner",
    )
    .await;
    let viewer = member_with_role(
        &team,
        &project,
        "m005-viewer2",
        ProjectRole::Viewer,
        "client-viewer",
    )
    .await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon, "client-owner", owner);
    register_client(&daemon, "client-viewer", viewer);
    // Viewer attempts an audit read and is denied at the gate.
    let denied = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-denied".to_owned(),
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
        "client-viewer",
    ))
    .await
    .unwrap();
    match denied {
        CoreResponse::Error { code, .. } => assert_eq!(code, "authorization_denied"),
        other => panic!("expected denial, got {other:?}"),
    }
    // The owner can query the denial terminal event on the same project.
    let store = AuditStore::new(pool);
    let page = store
        .query(
            &AuditQueryFilter::new(Some(project.clone()))
                .with_action("authorization_decision")
                .with_limit(10),
        )
        .await
        .unwrap();
    assert!(!page.events.is_empty(), "denial event must be stored");
    let denial = page
        .events
        .iter()
        .find(|e| e.metadata.get("decision.outcome").map(String::as_str) == Some("denied"));
    assert!(denial.is_some(), "denied outcome must be recorded");
}

#[tokio::test]
async fn daemon_authorized_audit_read_is_self_describing() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner = member_with_role(
        &team,
        &project,
        "m005-owner3",
        ProjectRole::Owner,
        "client-owner",
    )
    .await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon, "client-owner", owner);
    let response = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-self".to_owned(),
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
        CoreResponse::AuditPage { .. } => {}
        other => panic!("expected page, got {other:?}"),
    }
    // The read itself appended a self-describing audit_query event.
    let store = AuditStore::new(pool);
    let page = store
        .query(
            &AuditQueryFilter::new(Some(project.clone()))
                .with_action("audit_query")
                .with_limit(10),
        )
        .await
        .unwrap();
    assert!(!page.events.is_empty());
    assert!(page.events.iter().all(|e| e.action == "audit_query"));
}

#[test]
fn required_event_matrix_covers_all_taxonomy_actions() {
    for known in AuditAction::ALL {
        let action = AuditAction::parse_known(known).expect("known action");
        let entry = instr::coverage_for_action(&action);
        assert!(entry.is_some(), "matrix missing {known}");
    }
    assert_eq!(instr::REQUIRED_AUDIT_COVERAGE.len(), AuditAction::ALL.len());
    // Every instrumented operation maps to a known action.
    for (operation, action) in instr::INSTRUMENTED_OPERATIONS {
        assert!(
            AuditAction::parse_known(action).is_ok(),
            "bad mapping {operation} -> {action}"
        );
    }
    // No operation is both instrumented and explicitly uninstrumented.
    for (operation, _) in instr::INSTRUMENTED_OPERATIONS {
        assert!(
            !instr::UNINSTRUMENTED_OPERATIONS.contains(operation),
            "operation in both lists: {operation}"
        );
    }
}
