//! E2E: adding a provider through the real `ProviderConnectionProvisioner`
//! stays possible after a previous attempt, and re-adding replaces.
//!
//! This is the reported failure, reproduced at the level where it actually
//! happened. A user's own database showed exactly this shape:
//!
//! ```text
//! provider_provisioning: prov-… | committed | other:opencode_go
//! provider_connections : …      | active   | other:opencode_go
//! ```
//!
//! Because `provider_provisioning.idempotency_key` is `UNIQUE` for the
//! lifetime of the row *and* the pre-flight duplicate check counted the
//! terminal `'committed'` state, that single successful connect permanently
//! blocked every later attempt to connect the same provider — the user saw
//! `connection_conflict: an equivalent connection or provisioning operation
//! already exists` and could add no providers at all.
//!
//! Re-adding a provider is also the credential-rotation path, so an equivalent
//! *active* connection is now replaced rather than refused. Replacement is
//! published only after the new credential probes successfully, so a rejected
//! key leaves the previous connection intact.
//!
//! The scenarios below drive the real provisioner (`create_connection`, the
//! exact path `/connect` uses) against a real migrated SQLite pool with a
//! local fake provider endpoint. No live provider network and no real
//! credential is involved.

use codegg::core::eggpool::ProviderConnectionProvisioner;
use codegg::protocol::provider::{
    CreateProviderConnectionRequest, ProviderConnectionScope, ProviderCredentialKind,
    ProviderTlsPolicy, SecretInput,
};
use sqlx::SqlitePool;
use std::io::{Read, Write as _};
use std::sync::Arc;

/// The generic (setup-catalog) create request the TUI `/connect` dialog sends.
/// `base_url` must be the *full* URL of the fake endpoint, including the port
/// the listener actually bound (the catalog treats `eggpool` as an
/// endpoint-supplied provider, so nothing is defaulted for us).
fn create_request(base_url: &str, operation_id: &str) -> CreateProviderConnectionRequest {
    CreateProviderConnectionRequest {
        provider_id: "eggpool".to_string(),
        endpoint: Some(base_url.to_string()),
        port: None,
        tls_policy: Some(ProviderTlsPolicy::Disabled),
        credential: SecretInput::new("sk-e2e-readd-secret").expect("secret"),
        credential_kind: ProviderCredentialKind::ApiKey,
        display_name: None,
        scope: ProviderConnectionScope::Personal {
            owner_id: "local-user".into(),
        },
        operation_id: Some(operation_id.to_string()),
    }
}

/// Fake endpoint that serves the given responses in order, one per connection,
/// so a test can reconnect to the *same* endpoint. Reusing the endpoint is
/// essential: the idempotency key hashes (provider_id, endpoint, scope), so a
/// fresh random port would produce a different key and the test would pass
/// without ever exercising the collision it exists to cover.
fn fake_eggpool_serving(responses: &[(u16, String)]) -> (String, std::thread::JoinHandle<()>) {
    let responses = responses.to_vec();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fake eggpool");
    let address = listener.local_addr().expect("local addr");
    let handle = std::thread::spawn(move || {
        for (status, body) in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => read,
                    Err(_) => return,
                };
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (format!("http://{address}"), handle)
}

fn ok_model_body() -> (u16, String) {
    (
        200,
        r#"{"data":[{"id":"eggpool-model","name":"Eggpool Model"}]}"#.to_string(),
    )
}

async fn migrated_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

fn tempdir() -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!("codegg-readd-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&base).expect("tempdir");
    base
}

fn provisioner(pool: SqlitePool, dir: &std::path::Path) -> ProviderConnectionProvisioner {
    let store = Arc::new(
        codegg_providers::CredentialStore::at_path(dir.join("credentials.json"))
            .expect("credential store"),
    );
    ProviderConnectionProvisioner::with_credential_store(pool, Some(store))
}

async fn count(pool: &SqlitePool, state: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM provider_provisioning WHERE state = ?")
        .bind(state)
        .fetch_one(pool)
        .await
        .expect("count provisioning rows")
}

/// The headline user scenario: connect, disconnect, connect again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnecting_a_provider_after_removal_succeeds() {
    let dir = tempdir();
    let pool = migrated_pool().await;
    let provisioner = provisioner(pool.clone(), &dir);

    // One endpoint serving both probes, so both attempts share an idempotency key.
    let (endpoint, server) = fake_eggpool_serving(&[ok_model_body(), ok_model_body()]);

    // 1. First connect succeeds.
    let first = provisioner
        .create_connection(create_request(&endpoint, "prov-e2e-readd-1"))
        .await
        .expect("first connect must succeed");
    assert_eq!(first.models.len(), 1);
    assert_eq!(count(&pool, "committed").await, 1, "leaves a committed row");

    // 2. The user removes that connection (what `/connections` offers).
    sqlx::query("DELETE FROM provider_connections")
        .execute(&pool)
        .await
        .expect("remove connection");

    // 3. They connect the same provider again, to the same endpoint. Before
    //    the fix this returned `connection_conflict`, because the committed
    //    journal row still held the permanently-unique idempotency key.
    let second = provisioner
        .create_connection(create_request(&endpoint, "prov-e2e-readd-2"))
        .await
        .unwrap_or_else(|error| panic!("reconnecting must not fail, got {error:?}"));
    server.join().expect("fake endpoint joins");
    assert_eq!(second.models.len(), 1);
}

/// A failed attempt (rejected credential) must not permanently poison the
/// provider: the user retries with a good key and it has to work.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_after_a_rejected_attempt_succeeds() {
    let dir = tempdir();
    let pool = migrated_pool().await;
    let provisioner = provisioner(pool.clone(), &dir);

    // 403 first, then a well-formed response on the same endpoint, so the retry
    // collides with the 'failed' journal row's idempotency key.
    let (endpoint, server) = fake_eggpool_serving(&[
        (403, r#"{"error":"forbidden"}"#.to_string()),
        ok_model_body(),
    ]);

    let rejected = provisioner
        .create_connection(create_request(&endpoint, "prov-e2e-retry-1"))
        .await;
    assert!(
        rejected.is_err(),
        "a rejected attempt must not succeed, got {rejected:?}"
    );
    assert_eq!(
        count(&pool, "failed").await,
        1,
        "a rejected attempt leaves a failed journal row"
    );

    // The user corrects the credential and retries the same endpoint.
    let retried = provisioner
        .create_connection(create_request(&endpoint, "prov-e2e-retry-2"))
        .await
        .unwrap_or_else(|error| panic!("retry after failure must succeed, got {error:?}"));
    server.join().expect("fake endpoint joins");
    assert_eq!(retried.models.len(), 1);
}

/// A failed replacement must NOT destroy the working connection.
///
/// This is the reason the superseded connection is tombstoned inside the
/// finalize transaction rather than before the probe: a user who typos their
/// new key must still have the old, working connection afterwards.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_replacement_leaves_the_previous_connection_intact() {
    let dir = tempdir();
    let pool = migrated_pool().await;
    let provisioner = provisioner(pool.clone(), &dir);

    // First connect succeeds; the second (replacement) is rejected.
    let (endpoint, server) = fake_eggpool_serving(&[
        ok_model_body(),
        (403, r#"{"error":"forbidden"}"#.to_string()),
    ]);

    let first = provisioner
        .create_connection(create_request(&endpoint, "prov-e2e-safe-1"))
        .await
        .expect("first connect succeeds");
    let first_id = first.connection.id.clone();

    let rejected = provisioner
        .create_connection(create_request(&endpoint, "prov-e2e-safe-2"))
        .await;
    server.join().expect("fake endpoint joins");
    assert!(
        rejected.is_err(),
        "a rejected replacement must fail, got {rejected:?}"
    );

    let state: String = sqlx::query_scalar("SELECT state FROM provider_connections WHERE id = ?")
        .bind(&first_id)
        .fetch_one(&pool)
        .await
        .expect("previous connection state");
    assert_eq!(
        state, "active",
        "a failed replacement must leave the working connection active"
    );
    let active: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM provider_connections WHERE state = 'active'")
            .fetch_one(&pool)
            .await
            .expect("active count");
    assert_eq!(active, 1);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnecting_replaces_the_active_connection() {
    let dir = tempdir();
    let pool = migrated_pool().await;
    let provisioner = provisioner(pool.clone(), &dir);

    // One endpoint serving both probes so both attempts share an idempotency key.
    let (endpoint, server) = fake_eggpool_serving(&[ok_model_body(), ok_model_body()]);

    let first = provisioner
        .create_connection(create_request(&endpoint, "prov-e2e-replace-1"))
        .await
        .expect("first connect succeeds");
    let first_id = first.connection.id.clone();

    // Re-adding the same provider replaces it rather than refusing. This is
    // the credential-rotation path: the user runs /connect again with a new
    // key and expects the new key to win.
    let second = provisioner
        .create_connection(create_request(&endpoint, "prov-e2e-replace-2"))
        .await
        .unwrap_or_else(|error| panic!("a duplicate must replace, not fail: {error:?}"));
    server.join().expect("fake endpoint joins");

    assert_ne!(
        second.connection.id, first_id,
        "replacement must publish a new connection"
    );
    let active: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM provider_connections WHERE state = 'active'")
            .fetch_one(&pool)
            .await
            .expect("active count");
    assert_eq!(active, 1, "exactly one active connection must remain");

    let old_state: String =
        sqlx::query_scalar("SELECT state FROM provider_connections WHERE id = ?")
            .bind(&first_id)
            .fetch_one(&pool)
            .await
            .expect("superseded state");
    assert_ne!(
        old_state, "active",
        "the replaced row must leave the active set"
    );
    let lifecycle_state: Option<String> = sqlx::query_scalar(
        "SELECT state FROM provider_connection_lifecycle WHERE connection_id = ?",
    )
    .bind(&first_id)
    .fetch_optional(&pool)
    .await
    .expect("lifecycle state");
    assert_eq!(
        lifecycle_state.as_deref(),
        Some("tombstoned"),
        "the replaced row must be tombstoned in the authoritative lifecycle table"
    );
}
