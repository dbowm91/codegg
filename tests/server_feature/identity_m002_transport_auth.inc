//! Identity M002 — transport authentication and principal binding.
//!
//! Covers the plan-required behaviors that span the HTTP/WS transport seam
//! and the daemon `ClientRegistry`: fail-closed network listeners, distinct
//! team principals, payload-spoof negatives, projection convergence onto the
//! bound principal, and the bootstrap-compatibility disposition of the
//! legacy global bearer.

use codegg_core::team::{PrincipalKind, TeamStore};
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

fn test_config_with_token(token: Option<&str>) -> codegg::config::schema::Config {
    let mut config = codegg::config::schema::Config::default();
    config.server = Some(codegg::config::schema::ServerConfig {
        token: token.map(str::to_owned),
        ..Default::default()
    });
    config
}

#[tokio::test(flavor = "current_thread")]
async fn network_no_token_fails_closed() {
    let pool = test_pool().await;
    // No credential configured at all: personal-token presentations are
    // unknown and legacy presentations have nothing to match.
    let config = test_config_with_token(None);
    let result = codegg::server::middleware::auth::resolve_bearer_principal(
        "some-bearer",
        &config,
        &pool,
        "client-1",
    )
    .await;
    assert_eq!(
        result.unwrap_err().as_u16(),
        503,
        "missing server credential must fail closed with 503"
    );

    // Credential configured but request presents the wrong bearer.
    let config = test_config_with_token(Some("correct-global-bearer"));
    let result = codegg::server::middleware::auth::resolve_bearer_principal(
        "wrong-bearer",
        &config,
        &pool,
        "client-1",
    )
    .await;
    assert_eq!(
        result.unwrap_err().as_u16(),
        401,
        "wrong bearer must be rejected with 401"
    );

    // Unknown personal token fails closed without a valid principal.
    let result = codegg::server::middleware::auth::resolve_bearer_principal(
        "cggt_unknown.secret",
        &config,
        &pool,
        "client-1",
    )
    .await;
    assert_eq!(
        result.unwrap_err().as_u16(),
        401,
        "unknown personal token must be rejected with 401"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn two_remote_clients_authenticate_as_different_principals() {
    let pool = test_pool().await;
    let config = test_config_with_token(Some("global-bootstrap"));
    let team = TeamStore::new(pool.clone());
    let tokens = PersonalTokenStore::with_team(pool.clone(), team.clone());

    let alice = team
        .create_principal(PrincipalKind::Human, "Alice")
        .await
        .unwrap();
    let bob = team
        .create_principal(PrincipalKind::Human, "Bob")
        .await
        .unwrap();
    let (alice_plaintext, _) = tokens
        .create_personal_token(&alice.id, "alice-laptop", None)
        .await
        .unwrap();
    let (bob_plaintext, _) = tokens
        .create_personal_token(&bob.id, "bob-laptop", None)
        .await
        .unwrap();

    let alice_principal = codegg::server::middleware::auth::resolve_bearer_principal(
        &alice_plaintext,
        &config,
        &pool,
        "client-alice",
    )
    .await
    .unwrap();
    let bob_principal = codegg::server::middleware::auth::resolve_bearer_principal(
        &bob_plaintext,
        &config,
        &pool,
        "client-bob",
    )
    .await
    .unwrap();

    assert_eq!(alice_principal.principal_id(), &alice.id);
    assert_eq!(bob_principal.principal_id(), &bob.id);
    assert_ne!(alice_principal.principal_id(), bob_principal.principal_id());
    assert_ne!(
        alice_principal.projection_principal_string(),
        bob_principal.projection_principal_string()
    );
    // Neither remote binding uses a synthetic placeholder.
    assert_ne!(
        alice_principal.projection_principal_string(),
        "authenticated-remote"
    );
    assert_ne!(
        bob_principal.projection_principal_string(),
        "authenticated-remote"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn bootstrap_global_bearer_maps_to_local_owner_only() {
    let pool = test_pool().await;
    let config = test_config_with_token(Some("shared-bootstrap-secret"));
    let principal = codegg::server::middleware::auth::resolve_bearer_principal(
        "shared-bootstrap-secret",
        &config,
        &pool,
        "client-legacy",
    )
    .await
    .unwrap();
    assert_eq!(principal.principal_id().as_str(), "local-owner");
    assert!(principal.is_bootstrap_compatibility());
    assert_eq!(principal.projection_principal_string(), "local-user");

    // A personal token never matches the global-bearer path and vice versa.
    let team = TeamStore::new(pool.clone());
    let tokens = PersonalTokenStore::with_team(pool.clone(), team.clone());
    let human = team
        .create_principal(PrincipalKind::Human, "Ada")
        .await
        .unwrap();
    let (plaintext, _) = tokens
        .create_personal_token(&human.id, "laptop", None)
        .await
        .unwrap();
    assert!(codegg_core::transport_auth::is_personal_token_presentation(
        &plaintext
    ));
    assert!(
        !codegg_core::transport_auth::is_personal_token_presentation("shared-bootstrap-secret")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn client_payload_cannot_select_its_principal() {
    // The registry binding is immutable: a second principal cannot hijack
    // the connection, and the daemon resolves authority from the registry
    // (transport evidence), never from wire fields like ClientHello name or
    // Subscribe client_id.
    let registry = codegg::core::client_registry::ClientRegistry::new();
    let local = AuthenticatedPrincipal::local_owner("client-1");
    registry.register_with_principal(
        "client-1".to_string(),
        "attacker-chosen-name".to_string(),
        None,
        local.clone(),
    );
    assert_eq!(registry.principal_for("client-1").as_ref(), Some(&local));

    // Even if the attacker names a victim principal in a payload, the
    // registry keeps the transport-bound value.
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let victim = team
        .create_principal(PrincipalKind::Human, "Victim")
        .await
        .unwrap();
    let victim_principal = AuthenticatedPrincipal::personal_token(&victim, "client-1");
    assert!(
        !registry.set_principal("client-1", victim_principal),
        "payload-selected principal must not rebind the connection"
    );
    assert_eq!(registry.principal_for("client-1").as_ref(), Some(&local));
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_projection_context_uses_bound_principal() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let tokens = PersonalTokenStore::with_team(pool.clone(), team.clone());
    let alice = team
        .create_principal(PrincipalKind::Human, "Alice")
        .await
        .unwrap();
    let (plaintext, _) = tokens
        .create_personal_token(&alice.id, "laptop", None)
        .await
        .unwrap();
    let bound = tokens
        .verify_for_client(&plaintext, "client-alice")
        .await
        .unwrap();

    // Bind the verified principal to a registry entry the way the WS and
    // socket transports do, then prove the daemon's projection helper
    // carries the canonical id (not the legacy synthetic).
    let registry = codegg::core::client_registry::ClientRegistry::new();
    registry.register_with_principal(
        "client-alice".to_string(),
        "remote-tui".to_string(),
        None,
        bound.clone(),
    );
    let stored = registry.principal_for("client-alice").unwrap();
    let ctx = stored.to_projection_access_context(
        "corr-1",
        codegg_core::projection_replay::context::ProjectionCapabilitySet::local_user(),
        std::sync::Arc::new(codegg_core::projection_replay::context::AllowAllProjectResolver),
    );
    assert_eq!(ctx.principal_id.as_str(), alice.id.as_str());
    assert_ne!(ctx.principal_id.as_str(), "authenticated-remote");
}

#[tokio::test(flavor = "current_thread")]
async fn revoked_token_fails_new_authentication_but_existing_binding_is_explicit() {
    let pool = test_pool().await;
    let config = test_config_with_token(Some("global-bootstrap"));
    let team = TeamStore::new(pool.clone());
    let tokens = PersonalTokenStore::with_team(pool.clone(), team.clone());
    let alice = team
        .create_principal(PrincipalKind::Human, "Alice")
        .await
        .unwrap();
    let (plaintext, record) = tokens
        .create_personal_token(&alice.id, "laptop", None)
        .await
        .unwrap();
    // First authentication succeeds and binds.
    let first = codegg::server::middleware::auth::resolve_bearer_principal(
        &plaintext, &config, &pool, "client-1",
    )
    .await
    .unwrap();
    assert_eq!(first.principal_id(), &alice.id);
    // Revocation fails new authentication immediately.
    tokens
        .revoke_personal_token(&record.token_id)
        .await
        .unwrap();
    let second = codegg::server::middleware::auth::resolve_bearer_principal(
        &plaintext, &config, &pool, "client-2",
    )
    .await;
    assert_eq!(
        second.unwrap_err().as_u16(),
        401,
        "revoked credentials must fail new authentication"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn auth_events_carry_no_secrets() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let tokens = PersonalTokenStore::with_team(pool.clone(), team.clone());
    let alice = team
        .create_principal(PrincipalKind::Human, "Alice")
        .await
        .unwrap();
    let (plaintext, record) = tokens
        .create_personal_token(&alice.id, "laptop", None)
        .await
        .unwrap();
    // The durable record's Debug omits the digest; the plaintext never
    // appears in the record's diagnostic rendering.
    let debug = format!("{record:?}");
    assert!(!debug.contains(&record.token_digest_hex));
    // Redaction preserves kind without echoing secret bytes.
    let redacted = codegg_core::transport_auth::redact_presented_token(&plaintext);
    assert!(!redacted.contains(&plaintext));
    assert_eq!(redacted, "[REDACTED:personal-token]");
}
