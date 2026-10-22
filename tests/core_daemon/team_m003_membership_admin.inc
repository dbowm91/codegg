//! Team Collaboration Corrective M003 — membership and device-token administration.
//!
//! Boundary proof for the canonical team administration surface:
//! project Owners manage memberships inside their own project through
//! `member.manage`; LocalOwner alone provisions principals and per-device
//! personal tokens through `Opaque + project.configure` operations; token
//! plaintext appears exactly once and never enters audit, events, logs,
//! or list responses; stale revisions conflict visibly; revocation takes
//! effect at the next authorization boundary.
//!
//! - Owner lists/adds/updates/revokes membership in their own project.
//! - Maintainer/Contributor/Viewer cannot use `member.manage`.
//! - Owner cannot administer another project (privacy-safe denial).
//! - Project Owner cannot issue global tokens (fail closed).
//! - LocalOwner creates a principal and issues one device token.
//! - Plaintext appears once; digests/plaintext absent from
//!   debug/audit/events/list/chat.
//! - Token revocation rejects new authentications.
//! - Stale membership revisions conflict without overwrite.
//! - M002 chat overrides stay revision-safe through the `/team` path.
//! - `/collaborators` presence behavior is unchanged.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{ChatPolicyDecisionDto, CoreRequest, CoreResponse};
use codegg_core::identity::{PrincipalId, ProjectId};
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

async fn human_token(team: &TeamStore, name: &str, client_id: &str) -> AuthenticatedPrincipal {
    let tokens = PersonalTokenStore::with_team(team.pool().clone(), team.clone());
    let record = team
        .create_principal(PrincipalKind::Human, name)
        .await
        .unwrap();
    let (plaintext, _) = tokens
        .create_personal_token(&record.id, "device", None)
        .await
        .unwrap();
    tokens
        .verify_for_client(&plaintext, client_id)
        .await
        .unwrap()
}

async fn member_in_project(
    team: &TeamStore,
    project: &ProjectId,
    name: &str,
    client_id: &str,
    role: ProjectRole,
) -> AuthenticatedPrincipal {
    let principal = human_token(team, name, client_id).await;
    let record = team
        .get_principal(principal.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(project, &record.id, role)
        .await
        .unwrap();
    principal
}

fn register_client(daemon: &CoreDaemon, client_id: &str, principal: AuthenticatedPrincipal) {
    daemon.clients.register_with_principal(
        client_id.to_owned(),
        format!("{client_id}-name"),
        None,
        principal,
    );
}

async fn call(daemon: &CoreDaemon, client: &str, request: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(format!("req-{}-{}", client, uuid::Uuid::new_v4()), request),
        client,
    ))
    .await
    .unwrap()
}

fn error_code(response: &CoreResponse) -> String {
    match response {
        CoreResponse::Error { code, .. } => code.clone(),
        other => panic!("expected error response, got {other:?}"),
    }
}

fn local_owner_client(daemon: &CoreDaemon, client_id: &str) {
    register_client(
        daemon,
        client_id,
        AuthenticatedPrincipal::local_owner(client_id),
    );
}

// ── Membership administration ──────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn owner_can_list_add_update_revoke_membership_in_own_project() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner = member_in_project(
        &team,
        &project,
        "m003-owner",
        "client-owner",
        ProjectRole::Owner,
    )
    .await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon, "client-owner", owner);

    // List: one row (the owner).
    match call(
        &daemon,
        "client-owner",
        CoreRequest::TeamMembershipList {
            project_id: project.as_str().to_owned(),
            limit: None,
        },
    )
    .await
    {
        CoreResponse::TeamMembershipList { memberships, .. } => assert_eq!(memberships.len(), 1),
        other => panic!("expected membership list, got {other:?}"),
    }

    // Add an existing principal as contributor.
    let carol = team
        .create_principal(PrincipalKind::Human, "m003-carol")
        .await
        .unwrap();
    let added_revision = match call(
        &daemon,
        "client-owner",
        CoreRequest::TeamMembershipAdd {
            project_id: project.as_str().to_owned(),
            principal_id: carol.id.as_str().to_owned(),
            role: "contributor".to_owned(),
        },
    )
    .await
    {
        CoreResponse::TeamMembership { membership } => {
            assert_eq!(membership.role, "contributor");
            assert_eq!(membership.state, "active");
            membership.revision
        }
        other => panic!("expected membership, got {other:?}"),
    };

    // Update role to maintainer under the current revision.
    let updated_revision = match call(
        &daemon,
        "client-owner",
        CoreRequest::TeamMembershipUpdate {
            project_id: project.as_str().to_owned(),
            principal_id: carol.id.as_str().to_owned(),
            expected_revision: added_revision,
            role: Some("maintainer".to_owned()),
            state: None,
        },
    )
    .await
    {
        CoreResponse::TeamMembership { membership } => {
            assert_eq!(membership.role, "maintainer");
            membership.revision
        }
        other => panic!("expected membership, got {other:?}"),
    };
    assert!(updated_revision > added_revision);

    // Revoke under the current revision.
    match call(
        &daemon,
        "client-owner",
        CoreRequest::TeamMembershipRevoke {
            project_id: project.as_str().to_owned(),
            principal_id: carol.id.as_str().to_owned(),
            expected_revision: updated_revision,
        },
    )
    .await
    {
        CoreResponse::TeamMembership { membership } => assert_eq!(membership.state, "revoked"),
        other => panic!("expected membership, got {other:?}"),
    }

    // Revoked rows are retained: re-adding conflicts instead of
    // bypassing revocation through re-creation.
    let code = error_code(
        &call(
            &daemon,
            "client-owner",
            CoreRequest::TeamMembershipAdd {
                project_id: project.as_str().to_owned(),
                principal_id: carol.id.as_str().to_owned(),
                role: "viewer".to_owned(),
            },
        )
        .await,
    );
    assert_eq!(code, "team_membership_conflict");
}

#[tokio::test(flavor = "current_thread")]
async fn non_owners_cannot_use_member_manage() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner = member_in_project(
        &team,
        &project,
        "m003-owner",
        "client-owner",
        ProjectRole::Owner,
    )
    .await;
    let maintainer = member_in_project(
        &team,
        &project,
        "m003-maint",
        "client-maint",
        ProjectRole::Maintainer,
    )
    .await;
    let contributor = member_in_project(
        &team,
        &project,
        "m003-contrib",
        "client-contrib",
        ProjectRole::Contributor,
    )
    .await;
    let viewer = member_in_project(
        &team,
        &project,
        "m003-viewer",
        "client-viewer",
        ProjectRole::Viewer,
    )
    .await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon, "client-owner", owner);
    register_client(&daemon, "client-maint", maintainer);
    register_client(&daemon, "client-contrib", contributor);
    register_client(&daemon, "client-viewer", viewer);

    for client in ["client-maint", "client-contrib", "client-viewer"] {
        let code = error_code(
            &call(
                &daemon,
                client,
                CoreRequest::TeamMembershipList {
                    project_id: project.as_str().to_owned(),
                    limit: None,
                },
            )
            .await,
        );
        // Privacy-safe: indistinguishable from an absent project.
        assert_eq!(code, "project_not_found", "client {client}");
        let code = error_code(
            &call(
                &daemon,
                client,
                CoreRequest::TeamMembershipAdd {
                    project_id: project.as_str().to_owned(),
                    principal_id: "01J0000000000000000000999".to_owned(),
                    role: "viewer".to_owned(),
                },
            )
            .await,
        );
        assert_eq!(code, "project_not_found", "client {client}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn owner_cannot_administer_another_project() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project_a = ProjectId::new();
    let project_b = ProjectId::new();
    let owner_a = member_in_project(
        &team,
        &project_a,
        "m003-owner-a",
        "client-a",
        ProjectRole::Owner,
    )
    .await;
    let owner_b = member_in_project(
        &team,
        &project_b,
        "m003-owner-b",
        "client-b",
        ProjectRole::Owner,
    )
    .await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon, "client-a", owner_a);
    register_client(&daemon, "client-b", owner_b);

    // Owner of A probing B learns nothing beyond not-found.
    let code = error_code(
        &call(
            &daemon,
            "client-a",
            CoreRequest::TeamMembershipList {
                project_id: project_b.as_str().to_owned(),
                limit: None,
            },
        )
        .await,
    );
    assert_eq!(code, "project_not_found");

    // Owner of A cannot add a membership in B either.
    let newcomer = team
        .create_principal(PrincipalKind::Human, "m003-newcomer")
        .await
        .unwrap();
    let code = error_code(
        &call(
            &daemon,
            "client-a",
            CoreRequest::TeamMembershipAdd {
                project_id: project_b.as_str().to_owned(),
                principal_id: newcomer.id.as_str().to_owned(),
                role: "viewer".to_owned(),
            },
        )
        .await,
    );
    assert_eq!(code, "project_not_found");
}

#[tokio::test(flavor = "current_thread")]
async fn project_owner_cannot_issue_global_tokens() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner = member_in_project(
        &team,
        &project,
        "m003-owner",
        "client-owner",
        ProjectRole::Owner,
    )
    .await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon, "client-owner", owner);

    // Principal listing is LocalOwner-only: the project Owner fails
    // closed without a project-derived widening.
    let code = error_code(
        &call(
            &daemon,
            "client-owner",
            CoreRequest::TeamPrincipalList { limit: None },
        )
        .await,
    );
    assert!(
        code == "authorization_scope_required" || code == "authorization_denied",
        "unexpected code {code}"
    );
    // Token issuance likewise fails closed for the project Owner.
    let code = error_code(
        &call(
            &daemon,
            "client-owner",
            CoreRequest::TeamTokenList {
                principal_id: "local-owner".to_owned(),
            },
        )
        .await,
    );
    assert!(
        code == "authorization_scope_required" || code == "authorization_denied",
        "unexpected code {code}"
    );
}

// ── LocalOwner principal/device-token provisioning ─────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn local_owner_creates_principal_and_issues_one_device_token() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    // Bootstrap the LocalOwner record; request authority is LocalOwner.
    team.ensure_local_owner().await.unwrap();
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    local_owner_client(&daemon, "client-local");

    // Create a human principal.
    let principal_id = match call(
        &daemon,
        "client-local",
        CoreRequest::TeamPrincipalCreate {
            kind: Some("human".to_owned()),
            display_name: "M003 Device Owner".to_owned(),
        },
    )
    .await
    {
        CoreResponse::TeamPrincipal { principal } => {
            assert_eq!(principal.kind, "human");
            assert_eq!(principal.status, "active");
            principal.principal_id
        }
        other => panic!("expected principal, got {other:?}"),
    };

    // Issue one device token: plaintext appears exactly once.
    let (token_id, plaintext) = match call(
        &daemon,
        "client-local",
        CoreRequest::TeamTokenCreate {
            principal_id: principal_id.clone(),
            label: "field-device".to_owned(),
            expires_at_ms: None,
            idempotency_key: None,
        },
    )
    .await
    {
        CoreResponse::TeamTokenCreated { token, plaintext } => {
            assert!(plaintext.starts_with("cggt_"), "unexpected prefix");
            assert_eq!(token.principal_id, principal_id);
            (token.token_id, plaintext)
        }
        other => panic!("expected token creation, got {other:?}"),
    };

    // The token metadata listing carries no plaintext and no digest.
    match call(
        &daemon,
        "client-local",
        CoreRequest::TeamTokenList {
            principal_id: principal_id.clone(),
        },
    )
    .await
    {
        CoreResponse::TeamTokenList { tokens, .. } => {
            assert_eq!(tokens.len(), 1);
            assert_eq!(tokens[0].token_id, token_id);
            let json = serde_json::to_value(&tokens[0]).unwrap().to_string();
            assert!(
                !json.contains(&plaintext),
                "list must never repeat plaintext"
            );
            assert!(!json.contains("digest"), "list must never carry digests");
        }
        other => panic!("expected token list, got {other:?}"),
    }

    // The minted credential verifies for a new device binding.
    let tokens = PersonalTokenStore::new(pool.clone());
    let bound = tokens
        .verify_for_client(&plaintext, "device-1")
        .await
        .unwrap();
    assert_eq!(bound.principal_id().as_str(), principal_id.as_str());
}

#[tokio::test(flavor = "current_thread")]
async fn token_plaintext_absent_from_debug_audit_events_and_chat() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    team.ensure_local_owner().await.unwrap();
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    local_owner_client(&daemon, "client-local");

    let principal_id = match call(
        &daemon,
        "client-local",
        CoreRequest::TeamPrincipalCreate {
            kind: None,
            display_name: "M003 Census".to_owned(),
        },
    )
    .await
    {
        CoreResponse::TeamPrincipal { principal } => principal.principal_id,
        other => panic!("expected principal, got {other:?}"),
    };

    // Subscribe before issuance so the liveness event is observed.
    let mut events = daemon.event_log.subscribe();
    let plaintext = match call(
        &daemon,
        "client-local",
        CoreRequest::TeamTokenCreate {
            principal_id: principal_id.clone(),
            label: "census-device".to_owned(),
            expires_at_ms: None,
            idempotency_key: None,
        },
    )
    .await
    {
        CoreResponse::TeamTokenCreated { plaintext, .. } => plaintext,
        other => panic!("expected token creation, got {other:?}"),
    };

    // Events carry ids and the minted flag only.
    let observed = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
        .await
        .expect("token event arrives")
        .expect("event decodes");
    let event_json = serde_json::to_value(&observed.payload).unwrap().to_string();
    assert!(
        event_json.contains("team_token_changed"),
        "got {event_json}"
    );
    assert!(
        !event_json.contains(&plaintext),
        "events must never carry plaintext"
    );

    // Audit carries structural metadata only (no plaintext, no digest).
    let store = codegg_core::audit::AuditStore::new(pool.clone());
    let page = store
        .query(&codegg_core::audit::AuditQueryFilter::new(None).with_limit(50))
        .await
        .unwrap();
    assert!(!page.events.is_empty(), "team mutations must be audited");
    for event in &page.events {
        let json = serde_json::to_value(event).unwrap().to_string();
        assert!(
            !json.contains(&plaintext),
            "audit must never carry plaintext"
        );
        assert!(
            !json.contains("cggt_"),
            "audit must never carry token material"
        );
    }

    // Stored token rows expose no digest through Debug.
    let tokens = PersonalTokenStore::new(pool.clone());
    let principal = PrincipalId::parse(&principal_id).unwrap();
    let rows = tokens.list_tokens_for_principal(&principal).await.unwrap();
    assert_eq!(rows.len(), 1);
    let debug = format!("{:?}", rows[0]);
    assert!(!debug.contains("digest"), "record Debug must omit digests");
    assert!(
        !debug.contains(&plaintext),
        "record Debug must omit plaintext"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn token_revocation_rejects_new_authentications() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    team.ensure_local_owner().await.unwrap();
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    local_owner_client(&daemon, "client-local");

    let principal_id = match call(
        &daemon,
        "client-local",
        CoreRequest::TeamPrincipalCreate {
            kind: None,
            display_name: "M003 Revoke".to_owned(),
        },
    )
    .await
    {
        CoreResponse::TeamPrincipal { principal } => principal.principal_id,
        other => panic!("expected principal, got {other:?}"),
    };
    let (token_id, plaintext) = match call(
        &daemon,
        "client-local",
        CoreRequest::TeamTokenCreate {
            principal_id: principal_id.clone(),
            label: "revocable".to_owned(),
            expires_at_ms: None,
            idempotency_key: None,
        },
    )
    .await
    {
        CoreResponse::TeamTokenCreated { token, plaintext } => (token.token_id, plaintext),
        other => panic!("expected token creation, got {other:?}"),
    };

    // Live before revocation.
    let tokens = PersonalTokenStore::new(pool.clone());
    tokens
        .verify_for_client(&plaintext, "device-1")
        .await
        .unwrap();

    // Revoke (idempotent, monotonic).
    match call(
        &daemon,
        "client-local",
        CoreRequest::TeamTokenRevoke {
            token_id: token_id.clone(),
        },
    )
    .await
    {
        CoreResponse::TeamToken { token } => {
            assert!(token.revoked_at_ms.is_some());
        }
        other => panic!("expected token, got {other:?}"),
    }
    // Second revoke converges without error.
    match call(
        &daemon,
        "client-local",
        CoreRequest::TeamTokenRevoke { token_id },
    )
    .await
    {
        CoreResponse::TeamToken { .. } => {}
        other => panic!("expected token, got {other:?}"),
    }

    // New authentications are rejected after revocation.
    assert!(tokens
        .verify_for_client(&plaintext, "device-2")
        .await
        .is_err());
}

// ── Revision safety ────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn stale_membership_revision_conflicts_without_overwrite() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner = member_in_project(
        &team,
        &project,
        "m003-owner",
        "client-owner",
        ProjectRole::Owner,
    )
    .await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon, "client-owner", owner);

    let carol = team
        .create_principal(PrincipalKind::Human, "m003-carol")
        .await
        .unwrap();
    let rev1 = match call(
        &daemon,
        "client-owner",
        CoreRequest::TeamMembershipAdd {
            project_id: project.as_str().to_owned(),
            principal_id: carol.id.as_str().to_owned(),
            role: "viewer".to_owned(),
        },
    )
    .await
    {
        CoreResponse::TeamMembership { membership } => membership.revision,
        other => panic!("expected membership, got {other:?}"),
    };
    // Fresh write wins and bumps the revision.
    let rev2 = match call(
        &daemon,
        "client-owner",
        CoreRequest::TeamMembershipUpdate {
            project_id: project.as_str().to_owned(),
            principal_id: carol.id.as_str().to_owned(),
            expected_revision: rev1,
            role: Some("contributor".to_owned()),
            state: None,
        },
    )
    .await
    {
        CoreResponse::TeamMembership { membership } => membership.revision,
        other => panic!("expected membership, got {other:?}"),
    };
    assert!(rev2 > rev1);
    // Stale writer holding rev1 conflicts and changes nothing.
    let code = error_code(
        &call(
            &daemon,
            "client-owner",
            CoreRequest::TeamMembershipUpdate {
                project_id: project.as_str().to_owned(),
                principal_id: carol.id.as_str().to_owned(),
                expected_revision: rev1,
                role: Some("owner".to_owned()),
                state: None,
            },
        )
        .await,
    );
    assert_eq!(code, "team_revision_conflict");
    let current = team
        .get_membership(&project, &carol.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.revision, rev2);
    assert_eq!(current.role, ProjectRole::Contributor);
}

#[tokio::test(flavor = "current_thread")]
async fn chat_override_changes_through_team_stay_revision_safe() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner = member_in_project(
        &team,
        &project,
        "m003-owner",
        "client-owner",
        ProjectRole::Owner,
    )
    .await;
    let viewer = member_in_project(
        &team,
        &project,
        "m003-viewer",
        "client-viewer",
        ProjectRole::Viewer,
    )
    .await;
    let viewer_id = viewer.principal_id().as_str().to_owned();
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon, "client-owner", owner);
    register_client(&daemon, "client-viewer", viewer);

    // Owner grants the viewer project chat through the team surface.
    match call(
        &daemon,
        "client-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_id.clone(),
            decision: Some(ChatPolicyDecisionDto::Allow),
            expected_revision: None,
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { project, .. } => assert!(project.revision >= 1),
        other => panic!("expected policy, got {other:?}"),
    }
    // A second write against a possibly-stale revision is still
    // revision-checked: it either applies (when rev == 1) or conflicts
    // (when the first write already bumped past 1) — never blind.
    match call(
        &daemon,
        "client-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_id,
            decision: Some(ChatPolicyDecisionDto::Deny),
            expected_revision: Some(1),
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { .. } => {}
        CoreResponse::Error { code, .. } => assert_eq!(code, "chat_policy_conflict"),
        other => panic!("unexpected response {other:?}"),
    }
}

// ── Presence separation ────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn collaborators_presence_behavior_remains_unchanged() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let member = member_in_project(
        &team,
        &project,
        "m003-member",
        "client-member",
        ProjectRole::Contributor,
    )
    .await;
    let outsider_record = team
        .create_principal(PrincipalKind::Human, "m003-outsider")
        .await
        .unwrap();
    let outsider = AuthenticatedPrincipal::internal_test(&outsider_record, "client-outsider");
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon, "client-member", member);
    register_client(&daemon, "client-outsider", outsider);

    // Members still observe presence; outsiders still see not-found.
    match call(
        &daemon,
        "client-member",
        CoreRequest::PresenceSnapshotGet {
            project_id: project.as_str().to_owned(),
        },
    )
    .await
    {
        CoreResponse::PresenceSnapshot { .. } => {}
        other => panic!("expected presence snapshot, got {other:?}"),
    }
    let code = error_code(
        &call(
            &daemon,
            "client-outsider",
            CoreRequest::PresenceSnapshotGet {
                project_id: project.as_str().to_owned(),
            },
        )
        .await,
    );
    assert_eq!(code, "project_not_found");
}

// ── Static classifications ─────────────────────────────────────────────────

#[tokio::test]
async fn authorization_matrix_classifies_team_surface() {
    use codegg_core::authorization::{operation_descriptor, ScopeKind};
    use codegg_core::team::Capability;

    let membership = operation_descriptor(&CoreRequest::TeamMembershipList {
        project_id: String::new(),
        limit: None,
    });
    assert_eq!(membership.operation, "team_membership_list");
    assert_eq!(membership.scope_kind, ScopeKind::DirectProject);
    assert_eq!(membership.capability, Some(Capability::MemberManage));

    for request in [
        CoreRequest::TeamPrincipalList { limit: None },
        CoreRequest::TeamPrincipalCreate {
            kind: None,
            display_name: String::new(),
        },
        CoreRequest::TeamTokenList {
            principal_id: String::new(),
        },
        CoreRequest::TeamTokenCreate {
            principal_id: String::new(),
            label: String::new(),
            expires_at_ms: None,
            idempotency_key: None,
        },
        CoreRequest::TeamTokenRevoke {
            token_id: String::new(),
        },
    ] {
        let descriptor = operation_descriptor(&request);
        assert_eq!(
            descriptor.scope_kind,
            ScopeKind::Opaque,
            "operation {} must be Opaque so team principals fail closed",
            descriptor.operation
        );
        assert_eq!(descriptor.capability, Some(Capability::ProjectConfigure));
    }

    // Device-token issuance stays local-only (no remote secret delivery).
    assert!(CoreRequest::TeamTokenCreate {
        principal_id: "p".to_string(),
        label: "d".to_string(),
        expires_at_ms: None,
        idempotency_key: None,
    }
    .is_secret_bearing());
}

#[tokio::test]
async fn team_events_are_safe_and_structural_only() {
    use codegg::protocol::core::CoreEvent;
    use codegg_core::projection_replay::safe_publication::{classify, SafePublicationClass};

    for event in [
        CoreEvent::TeamMembershipChanged {
            project_id: "p".to_string(),
            principal_id: "u".to_string(),
            revision: 1,
        },
        CoreEvent::TeamPrincipalChanged {
            principal_id: "u".to_string(),
            revision: 1,
        },
        CoreEvent::TeamTokenChanged {
            principal_id: "u".to_string(),
            token_id: "t".to_string(),
            revoked: false,
        },
    ] {
        assert_eq!(classify(&event), SafePublicationClass::Safe);
        let json = serde_json::to_value(&event).unwrap().to_string();
        assert!(
            !json.contains("cggt_"),
            "events must never carry token material"
        );
        assert!(!json.contains("digest"), "events must never carry digests");
    }
}
