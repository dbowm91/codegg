//! C001: persisted OpenCode Go connection + model selection reaches the correct
//! multi-surface runtime, without live credentials.
//!
//! This is the cross-layer trajectory the M011 closure record could not prove.
//! It crosses, in order and with no layer bypassed:
//!
//! 1. a durable `other:opencode_go` connection row whose secret reference
//!    resolves through the real encrypted [`CredentialStore`];
//! 2. a persisted bounded model catalog containing wire-resolved models;
//! 3. a durable session selection (connection + revision + catalog revision +
//!    model) written through [`update_selection`];
//! 4. the runtime model projection ([`durable_selected_runtime_model`]) that a
//!    turn actually consumes;
//! 5. the daemon's real provider runtime manager ([`ConnectionManager`]) with
//!    the real [`ProviderConnectionFactory`], resolving the durable connection
//!    at the revision the selection pinned;
//! 6. one inference request on the runtime provider, captured on a loopback
//!    socket to prove the selected model reached the profile-owned surface;
//! 7. the M010 revision-scoped credential writer reacting to that outcome; and
//! 8. the secret-absence invariant across storage, protocol, and the request.
//!
//! Layer (6) needs the origin redirected, because the shared profile pins the
//! production origin. That redirection is a test-only seam
//! (`ProviderConnectionFactory::with_test_capture_base`), compiled only under
//! `cfg(test)` or the opt-in `capture-test-support` feature and scoped to one
//! factory instance. It replaces the **origin only**: path, auth shape, and
//! session header still come from the shared profile, so a captured path is the
//! path production would request.

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use codegg::agent::provider_qualification::InferenceCredentialOutcome;
use codegg::core::provider_connections::ConnectionManager;
use codegg::core::provider_qualification::ProviderConnectionCredentialReporter;
use codegg::core::session_selection::{
    durable_selected_runtime_model, get_selection, update_selection, SelectionUpdateOutcome,
};
use codegg_core::identity::{PrincipalId, ProviderConnectionId};
use codegg_core::provider_connections::{
    Endpoint, NewProviderConnection, ProviderConnectionStore, ProviderKind, ProviderScope,
    SecretBindingLocator, SecretRef, TlsPolicy,
};
use codegg_core::session::{CreateSession, SessionStore};
use codegg_protocol::provider::SessionSelectionDto;
use codegg_providers::{
    ChatRequest, ContentPart, CredentialKind, CredentialStore, Message, Provider,
    ProviderConnectionFactory, ProviderRequestContext,
};
use sqlx::{Column, Row};

/// The secret must never appear in storage, in the protocol, or in a request.
const SECRET: &str = "sk-c001-trajectory-secret-value";

const SESSION_ID: &str = "c001-trajectory-session";
const CATALOG_REVISION: &str = "cat-c001-1";

/// Persisted exactly as a successful catalog probe would persist them: bounded,
/// one row per advertised model id. The second row is a Messages-surface model,
/// so the trajectory proves the *selected* model decides the surface rather
/// than the connection.
const CATALOG: &[(&str, &str)] = &[
    ("gpt-5.6-luna", "GPT-5.6 Luna"),
    ("minimax-m3", "minimax-m3"),
];

/// Localhost mirror of the shared profile origin, recorded as a durable
/// endpoint the way provisioning records it, without weakening TLS.
const PROFILE_ORIGIN: &str = "https://opencode.ai/zen/go";

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis() as i64
}

async fn migrated_pool() -> sqlx::SqlitePool {
    let pool = common::pool::isolated_pool().await;
    let now = now_millis();
    sqlx::query(
        r#"INSERT OR IGNORE INTO project (id, name, time_created, time_updated, sandboxes)
           VALUES ('c001-proj', 'c001-proj', ?, ?, '[]')"#,
    )
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .expect("seed project");
    pool
}

/// Persist the durable connection with the secret-binding semantics
/// provisioning establishes: a real [`SecretRef`] bound to the
/// `(provider, account, kind)` namespace in the encrypted credential store.
async fn seed_durable_connection(
    store: &ProviderConnectionStore,
    credential_store: &CredentialStore,
) -> ProviderConnectionId {
    let account = "c001-account";
    credential_store
        .put(
            "opencode_go",
            Some(account),
            CredentialKind::BearerToken,
            SECRET,
            None,
            Vec::new(),
        )
        .expect("store credential");

    store
        .create(NewProviderConnection {
            provider_kind: ProviderKind::Other("opencode_go".to_string()),
            display_name: "OpenCode Go".to_string(),
            endpoint: Endpoint::new(PROFILE_ORIGIN, TlsPolicy::Required).expect("endpoint"),
            tls_policy: TlsPolicy::Required,
            scope: ProviderScope::personal(PrincipalId::parse("c001-user").expect("principal")),
            secret_binding: Some(
                SecretBindingLocator::new(SecretRef::new(), "opencode_go", account)
                    .expect("secret binding"),
            ),
        })
        .await
        .expect("create durable connection")
        .id
}

/// Persist the bounded catalog plus a health row that starts `unverified`.
/// Catalog reachability is not credential verification (M010), so the
/// trajectory has to be able to *move* this row afterwards.
async fn seed_catalog_and_health(pool: &sqlx::SqlitePool, connection_id: &ProviderConnectionId) {
    sqlx::query(
        "INSERT INTO provider_connection_health \
         (connection_id, revision, status, credential_status, duration_ms, checked_at, \
          catalog_revision) \
         VALUES (?, 1, 'healthy', 'unverified', 12, ?, ?)",
    )
    .bind(connection_id.as_str())
    .bind(now_millis())
    .bind(CATALOG_REVISION)
    .execute(pool)
    .await
    .expect("seed health");

    for (model_id, model_name) in CATALOG {
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
        .expect("seed model row");
    }
}

async fn seed_session(session_store: &SessionStore) -> String {
    session_store
        .create(CreateSession {
            project_id: "c001-proj".to_string(),
            directory: "/tmp".to_string(),
            title: Some("C001 trajectory".to_string()),
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

async fn credential_status(
    pool: &sqlx::SqlitePool,
    connection_id: &ProviderConnectionId,
) -> String {
    sqlx::query_scalar(
        "SELECT credential_status FROM provider_connection_health \
         WHERE connection_id = ? AND revision = 1",
    )
    .bind(connection_id.as_str())
    .fetch_one(pool)
    .await
    .expect("credential_status row")
}

// ── loopback capture ─────────────────────────────────────────────────

#[derive(Debug)]
struct Capture {
    request_line: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Capture {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn has(&self, name: &str) -> bool {
        self.header(name).is_some()
    }

    fn body_field(&self, name: &str) -> Option<String> {
        serde_json::from_str::<serde_json::Value>(&self.body)
            .ok()?
            .get(name)?
            .as_str()
            .map(str::to_string)
    }
}

fn read_request(stream: &mut TcpStream) -> Capture {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        match stream.read(&mut chunk) {
            Ok(0) => panic!("client closed before sending headers"),
            Ok(read) => {
                buffer.extend_from_slice(&chunk[..read]);
                if let Some(position) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                    break position;
                }
            }
            Err(error) => panic!("read request: {error}"),
        }
    };
    let header_text = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = header_text.lines();
    let request_line = lines.next().unwrap_or_default().to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| {
            line.split_once(':')
                .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect();
    let content_length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < content_length {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => body.extend_from_slice(&chunk[..read]),
            Err(error) => panic!("read body: {error}"),
        }
    }
    Capture {
        request_line,
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    }
}

const RESPONSES_SSE: &str = concat!(
    "event: response.output_text.delta\n",
    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
    "event: response.completed\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r\",\"usage\":",
    "{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
);

const MESSAGES_SSE: &str = concat!(
    "event: message_start\n",
    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"model\":\"minimax-m3\",",
    "\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
    "event: message_stop\n",
    "data: {\"type\":\"message_stop\"}\n\n",
);

/// Single-shot capture server: accepts one request, records it, replies with
/// the canned SSE stream for the expected surface.
fn spawn_capture(response_body: &'static str) -> (String, std::thread::JoinHandle<Capture>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
    let addr = listener.local_addr().expect("capture address");
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let capture = read_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response_body.len()
        )
        .expect("write headers");
        stream
            .write_all(response_body.as_bytes())
            .expect("write body");
        stream.flush().ok();
        capture
    });
    (format!("http://{addr}"), handle)
}

/// Capture server returning a bare 401, for the typed auth-rejection case.
fn spawn_rejection() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind rejection server");
    let addr = listener.local_addr().expect("rejection address");
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let _capture = read_request(&mut stream);
        let body = r#"{"error":{"message":"invalid key"}}"#;
        let _ = write!(
            stream,
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.flush();
    });
    format!("http://{addr}")
}

fn request(model: &str) -> ChatRequest {
    ChatRequest {
        messages: vec![Message::User {
            content: vec![ContentPart::Text {
                text: "hello".to_string().into(),
            }],
        }],
        model: model.to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
        context: ProviderRequestContext {
            session_id: Some(Arc::from(SESSION_ID)),
            ..Default::default()
        },
    }
}

async fn drain(stream: codegg_providers::EventStream) {
    use futures_util::StreamExt;
    let mut events = stream;
    while let Some(event) = events.next().await {
        event.expect("stream event");
    }
}

// ── the trajectory ───────────────────────────────────────────────────

struct Trajectory {
    pool: sqlx::SqlitePool,
    connection_id: ProviderConnectionId,
    /// Kept so a re-selection can be written from a test that already resolved.
    session_id: String,
    connection_store: ProviderConnectionStore,
    credential_store: Arc<CredentialStore>,
    /// Kept alive so the encrypted credential file survives the assertions.
    _credential_dir: tempfile::TempDir,
    credential_file: std::path::PathBuf,
    runtime_model: String,
    selection: SessionSelectionDto,
}

/// Drive layers 1-4: durable connection + bounded catalog + durable selection +
/// the runtime projection a turn actually reads.
async fn durable_half(model_id: &str) -> Trajectory {
    let pool = migrated_pool().await;
    let credential_dir = tempfile::tempdir().expect("temp dir");
    let credential_file = credential_dir.path().join("credentials.json");
    let credential_store = Arc::new(
        CredentialStore::at_path(credential_file.clone()).expect("encrypted credential store"),
    );

    let connection_store = ProviderConnectionStore::new(pool.clone());
    let session_store = SessionStore::new(pool.clone());

    let connection_id = seed_durable_connection(&connection_store, &credential_store).await;
    seed_catalog_and_health(&pool, &connection_id).await;
    let session_id = seed_session(&session_store).await;

    let outcome = update_selection(
        &session_store,
        &connection_store,
        &session_id,
        &connection_id,
        model_id,
        Some(1),
        Some(CATALOG_REVISION.to_string()),
    )
    .await
    .expect("update selection");
    assert!(
        matches!(outcome, SelectionUpdateOutcome::Updated(_)),
        "durable selection must commit, got {outcome:?}"
    );

    let selection = get_selection(&session_store, &connection_store, &session_id)
        .await
        .expect("read selection");
    let runtime_model = durable_selected_runtime_model(&selection)
        .expect("a committed selection always projects a runtime model");

    Trajectory {
        pool,
        connection_id,
        session_id,
        connection_store,
        credential_store,
        _credential_dir: credential_dir,
        credential_file,
        runtime_model,
        selection,
    }
}

fn pinned_revision(selection: &SessionSelectionDto) -> u64 {
    match selection {
        SessionSelectionDto::Selected {
            connection_revision,
            ..
        } => *connection_revision,
        other => panic!("expected a committed selection, got {other:?}"),
    }
}

/// Layers 5: split the projected runtime model the way a turn does, then resolve
/// the durable connection at the revision the selection pinned.
///
/// `capture_base` is the C001 test-only origin override, installed on *this*
/// factory instance. Construction policy, credential resolution, lifecycle
/// gating, and revision pinning are all the production code paths.
async fn resolve_runtime_provider(
    trajectory: &Trajectory,
    expected_model: &str,
    capture_base: &str,
) -> (Arc<dyn Provider>, u64) {
    let (provider_kind, model_id) = trajectory
        .runtime_model
        .split_once('/')
        .expect("projected runtime model carries provider/model");
    assert_eq!(
        provider_kind, "opencode_go",
        "durable provider_kind must project the shared-profile id, not a storage key"
    );
    assert_eq!(model_id, expected_model);

    let pinned = pinned_revision(&trajectory.selection);
    let factory = ProviderConnectionFactory::from_store(trajectory.credential_store.clone())
        .with_test_capture_base(capture_base);
    let manager = ConnectionManager::new(
        Arc::new(trajectory.connection_store.clone()),
        Arc::new(factory),
    );
    let (provider, _lease) = manager
        .resolve_with_runtime_reference(&trajectory.connection_id, Some(pinned))
        .await
        .expect("resolve durable connection through the runtime manager");
    assert_eq!(provider.id(), "opencode_go");
    (provider, pinned)
}

/// The durable secret must stay out of the database, out of the protocol
/// projection, and out of the encrypted credential file on disk.
async fn assert_secret_absent(trajectory: &Trajectory, capture: &Capture) {
    for table in [
        "provider_connections",
        "provider_connection_health",
        "provider_connection_models",
        "session",
    ] {
        let rows = sqlx::query(&format!("SELECT * FROM {table}"))
            .fetch_all(&trajectory.pool)
            .await
            .expect("dump table rows");
        let mut rendered = String::new();
        for row in rows {
            for column in row.columns() {
                if let Ok(Some(value)) = row.try_get::<Option<String>, _>(column.name()) {
                    rendered.push_str(&value);
                    rendered.push('\n');
                } else if let Ok(Some(bytes)) = row.try_get::<Option<Vec<u8>>, _>(column.name()) {
                    rendered.push_str(&hex_encode(&bytes));
                    rendered.push('\n');
                }
            }
        }
        assert!(
            !rendered.contains(SECRET),
            "{table} leaked the durable secret"
        );
    }

    let dto = serde_json::to_string(&trajectory.selection).expect("serialize selection DTO");
    assert!(
        !dto.contains(SECRET),
        "selection DTO leaked the durable secret"
    );

    assert!(
        !capture.body.contains(SECRET),
        "request body echoed the credential"
    );

    // The credential store holds the secret encrypted at rest, never in clear.
    let on_disk = std::fs::read(&trajectory.credential_file).expect("read credential file");
    assert!(
        !String::from_utf8_lossy(&on_disk).contains(SECRET),
        "credential store persisted the secret in clear"
    );
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ── tests ────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn durable_opencode_go_selection_reaches_the_profile_owned_responses_surface() {
    let trajectory = durable_half("gpt-5.6-luna").await;

    // Catalog discovery is not credential verification (M010).
    assert_eq!(
        credential_status(&trajectory.pool, &trajectory.connection_id).await,
        "unverified",
        "a persisted catalog must not read as a verified credential"
    );
    assert_eq!(
        trajectory.runtime_model, "opencode_go/gpt-5.6-luna",
        "durable selection must project the runtime model a turn consumes"
    );

    let (base, handle) = spawn_capture(RESPONSES_SSE);
    let (provider, pinned) = resolve_runtime_provider(&trajectory, "gpt-5.6-luna", &base).await;

    drain(
        provider
            .stream(&request("gpt-5.6-luna"))
            .await
            .expect("inference request through the resolved runtime provider"),
    )
    .await;
    let capture = handle.join().expect("capture thread");

    // The durable-selected model reached the Responses surface...
    assert!(
        capture.request_line.contains("POST /responses "),
        "unexpected path: {}",
        capture.request_line
    );
    // ...with the profile-owned Bearer auth shape and no Messages header...
    let expected_auth = format!("Bearer {SECRET}");
    assert_eq!(
        capture.header("authorization"),
        Some(expected_auth.as_str())
    );
    assert!(
        !capture.has("x-api-key"),
        "Responses must not carry the Messages-surface credential header"
    );
    // ...with the provider-owned stable session header...
    assert_eq!(capture.header("x-opencode-session"), Some(SESSION_ID));
    // ...and the selected model in the request grammar.
    assert_eq!(capture.body_field("model").as_deref(), Some("gpt-5.6-luna"));

    // M010: a completed authenticated request is what promotes qualification.
    let reporter = ProviderConnectionCredentialReporter::new(
        trajectory.pool.clone(),
        Arc::from(trajectory.connection_id.as_str()),
        pinned,
    );
    assert!(
        reporter
            .record(InferenceCredentialOutcome::Authenticated)
            .await
    );
    assert_eq!(
        credential_status(&trajectory.pool, &trajectory.connection_id).await,
        "verified"
    );

    assert_secret_absent(&trajectory, &capture).await;
}

#[tokio::test(flavor = "current_thread")]
async fn the_durable_selection_not_the_connection_picks_the_surface() {
    // One connection, one credential, one revision. Only the durable selection
    // changes, and the surface has to follow it.
    let trajectory = durable_half("gpt-5.6-luna").await;
    assert_eq!(trajectory.runtime_model, "opencode_go/gpt-5.6-luna");

    let (responses_base, responses_handle) = spawn_capture(RESPONSES_SSE);
    let (responses_provider, pinned) =
        resolve_runtime_provider(&trajectory, "gpt-5.6-luna", &responses_base).await;
    drain(
        responses_provider
            .stream(&request("gpt-5.6-luna"))
            .await
            .expect("responses request"),
    )
    .await;
    let responses_capture = responses_handle.join().expect("capture thread");
    assert!(
        responses_capture.request_line.contains("POST /responses "),
        "unexpected path: {}",
        responses_capture.request_line
    );

    // Re-select the Messages-surface model on the same durable connection.
    let session_store = SessionStore::new(trajectory.pool.clone());
    let outcome = update_selection(
        &session_store,
        &trajectory.connection_store,
        &trajectory.session_id,
        &trajectory.connection_id,
        "minimax-m3",
        Some(pinned),
        Some(CATALOG_REVISION.to_string()),
    )
    .await
    .expect("re-select on the same connection");
    assert!(
        matches!(outcome, SelectionUpdateOutcome::Updated(_)),
        "re-selection must commit, got {outcome:?}"
    );

    let reselected = get_selection(
        &session_store,
        &trajectory.connection_store,
        &trajectory.session_id,
    )
    .await
    .expect("read re-selection");
    assert_eq!(
        durable_selected_runtime_model(&reselected).as_deref(),
        Some("opencode_go/minimax-m3"),
        "the runtime projection must follow the durable selection"
    );
    let selected_connection_id = |dto: &SessionSelectionDto| match dto {
        SessionSelectionDto::Selected { connection, .. } => connection.id.clone(),
        other => panic!("expected a committed selection, got {other:?}"),
    };
    assert_eq!(
        selected_connection_id(&reselected),
        selected_connection_id(&trajectory.selection),
        "a model re-selection must not change the connection identity"
    );

    let (messages_base, messages_handle) = spawn_capture(MESSAGES_SSE);
    let factory = ProviderConnectionFactory::from_store(trajectory.credential_store.clone())
        .with_test_capture_base(&messages_base);
    let manager = ConnectionManager::new(
        Arc::new(trajectory.connection_store.clone()),
        Arc::new(factory),
    );
    let (messages_provider, _lease) = manager
        .resolve_with_runtime_reference(&trajectory.connection_id, Some(pinned))
        .await
        .expect("resolve the same durable connection after re-selection");
    drain(
        messages_provider
            .stream(&request("minimax-m3"))
            .await
            .expect("messages request"),
    )
    .await;
    let messages_capture = messages_handle.join().expect("capture thread");

    assert!(
        messages_capture.request_line.contains("POST /messages "),
        "unexpected path: {}",
        messages_capture.request_line
    );
    // Messages uses the profile's x-api-key shape, not Bearer.
    assert_eq!(messages_capture.header("x-api-key"), Some(SECRET));
    assert!(
        !messages_capture.has("authorization"),
        "Messages must not carry the Responses-surface credential header"
    );
    assert_eq!(
        messages_capture.header("x-opencode-session"),
        Some(SESSION_ID)
    );
    assert_eq!(
        messages_capture.body_field("model").as_deref(),
        Some("minimax-m3")
    );

    // One authenticated request on the connection promotes it once.
    let reporter = ProviderConnectionCredentialReporter::new(
        trajectory.pool.clone(),
        Arc::from(trajectory.connection_id.as_str()),
        pinned,
    );
    assert!(
        reporter
            .record(InferenceCredentialOutcome::Authenticated)
            .await
    );
    assert_eq!(
        credential_status(&trajectory.pool, &trajectory.connection_id).await,
        "verified"
    );

    assert_secret_absent(&trajectory, &messages_capture).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_typed_auth_rejection_writes_a_revision_scoped_verdict() {
    let trajectory = durable_half("gpt-5.6-luna").await;

    let rejection_base = spawn_rejection();
    let (provider, pinned) =
        resolve_runtime_provider(&trajectory, "gpt-5.6-luna", &rejection_base).await;

    let error = match provider.stream(&request("gpt-5.6-luna")).await {
        Ok(_) => panic!("a 401 must not yield a successful stream"),
        Err(error) => error,
    };
    // Typed classification only: the 401 is the auth class, so it is a verdict.
    assert_eq!(
        InferenceCredentialOutcome::from_provider_error(&error),
        InferenceCredentialOutcome::AuthenticationFailed
    );

    let reporter = ProviderConnectionCredentialReporter::new(
        trajectory.pool.clone(),
        Arc::from(trajectory.connection_id.as_str()),
        pinned,
    );
    assert!(
        reporter
            .record(InferenceCredentialOutcome::AuthenticationFailed)
            .await
    );
    assert_eq!(
        credential_status(&trajectory.pool, &trajectory.connection_id).await,
        "authentication_failed"
    );

    // A verdict for a superseded revision must match no row, so a late turn can
    // never overwrite a newer credential's state.
    let stale = ProviderConnectionCredentialReporter::new(
        trajectory.pool.clone(),
        Arc::from(trajectory.connection_id.as_str()),
        pinned + 7,
    );
    assert!(
        !stale
            .record(InferenceCredentialOutcome::Authenticated)
            .await,
        "a stale revision must not rewrite the current verdict"
    );
    assert_eq!(
        credential_status(&trajectory.pool, &trajectory.connection_id).await,
        "authentication_failed"
    );
}
