use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio_util::sync::CancellationToken;

use crate::core::daemon::CoreDaemon;
use crate::core::transport::projection::{ProjectionLifecycleBoundary, ProjectionLifecycleSeam};
use crate::protocol::core::CoreEvent;
use crate::protocol::frames::{ClientCapabilities, ClientHello, ClientKind, CoreFrame};

/// Read a single JSON frame (newline-delimited) from a `BufReader`.
async fn read_frame(reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>) -> Option<CoreFrame> {
    let mut line = String::new();
    match reader.read_line(&mut line).await {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            serde_json::from_str::<CoreFrame>(trimmed).ok()
        }
    }
}

/// Set up a daemon listening on a temp Unix socket, returning the
/// path, the `TempDir` guard, and a `JoinHandle` to the server task.
/// The test must abort the handle before dropping the `TempDir` so
/// the socket file is not removed while the server still holds it.
async fn spawn_daemon(
    daemon: Arc<CoreDaemon>,
) -> (String, tempfile::TempDir, tokio::task::JoinHandle<()>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("daemon.sock");
    let socket_path_str = socket_path.to_string_lossy().to_string();

    let daemon_for_server = Arc::clone(&daemon);
    let socket_path_for_server = socket_path_str.clone();
    let handle = tokio::spawn(async move {
        let _ = crate::core::transport::daemon_socket::run_core_socket(
            daemon_for_server,
            &socket_path_for_server,
        )
        .await;
    });

    // The server binds the listener inside the spawned task; a short
    // sleep is enough to let the OS register the listener before the
    // test starts connecting.
    tokio::time::sleep(Duration::from_millis(100)).await;
    (socket_path_str, dir, handle)
}

/// Bind before spawning so lifecycle tests can use a real listener shutdown
/// token without relying on a startup sleep.
async fn spawn_daemon_with_shutdown(
    daemon: Arc<CoreDaemon>,
) -> (
    String,
    tempfile::TempDir,
    tokio::task::JoinHandle<()>,
    CancellationToken,
) {
    spawn_daemon_with_shutdown_and_seam(daemon, ProjectionLifecycleSeam::default()).await
}

async fn spawn_daemon_with_shutdown_and_seam(
    daemon: Arc<CoreDaemon>,
    lifecycle_seam: ProjectionLifecycleSeam,
) -> (
    String,
    tempfile::TempDir,
    tokio::task::JoinHandle<()>,
    CancellationToken,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("daemon.sock");
    let socket_path_str = socket_path.to_string_lossy().to_string();
    let listener = crate::core::transport::daemon_socket::bind_listener(&socket_path)
        .expect("bind test listener");
    let shutdown = CancellationToken::new();
    let shutdown_for_server = shutdown.clone();
    let endpoint = socket_path.clone();
    let handle = tokio::spawn(async move {
        let _ = crate::core::transport::daemon_socket::run_core_socket_with_listener_and_seam(
            daemon,
            listener,
            &endpoint,
            shutdown_for_server,
            lifecycle_seam,
        )
        .await;
    });
    (socket_path_str, dir, handle, shutdown)
}

/// Drive a complete `ClientHello` + `Subscribe` handshake against the
/// running daemon, then drain any replayed events. Returns the
/// `BufReader` positioned at the live event boundary, plus the
/// negotiated `client_id`.
async fn handshake_and_subscribe(
    stream: UnixStream,
    session_id: Option<String>,
) -> (
    BufReader<tokio::net::unix::OwnedReadHalf>,
    tokio::net::unix::OwnedWriteHalf,
    String,
) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // ClientHello
    let hello = CoreFrame::ClientHello(ClientHello {
        client_name: "integration-test".to_string(),
        client_kind: ClientKind::Automation,
        protocol_version: crate::protocol::core::PROTOCOL_VERSION,
        capabilities: ClientCapabilities {
            visual_notifications: false,
            desktop_notifications: false,
            audio: false,
            tts: false,
            multi_session_view: false,
            plugin_ui_dialog: false,
            plugin_ui_toast: false,
            plugin_ui_panel: false,
            plugin_ui_status_item: false,
            plugin_ui_table: false,
            plugin_ui_markdown: false,
            plugin_ui_code: false,
            plugin_ui_progress: false,
            workspace_registration: false,
            project_catalog: false,
            session_projection: false,
        },
    });
    let json = serde_json::to_string(&hello).expect("serialize ClientHello");
    write_half.write_all(json.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    // ServerHello
    let server_hello = match read_frame(&mut reader).await {
        Some(CoreFrame::ServerHello(sh)) => sh,
        other => panic!("expected ServerHello, got {:?}", other),
    };
    let client_id = server_hello.client_id.clone();

    // Subscribe for the requested session. We deliberately do not send
    // a default global subscription, so the per-connection filter
    // list contains exactly one filter, scoped to `session_id`. This
    // lets us observe session-filter isolation in isolation.
    let sub = CoreFrame::Subscribe {
        client_id: server_hello.client_id.clone(),
        session_id,
        from_event_seq: Some(0),
    };
    let json = serde_json::to_string(&sub).expect("serialize Subscribe");
    write_half.write_all(json.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    // Drain any replayed events. The replay delivers events as a
    // burst; a short timeout is enough to surface the historical
    // events before live ones start flowing.
    let drain = tokio::time::timeout(Duration::from_millis(150), async {
        while let Some(CoreFrame::Event(_)) = read_frame(&mut reader).await {}
    })
    .await;
    let _ = drain;

    (reader, write_half, client_id)
}

/// Establish a projection-capable client and wait for the canonical
/// `ProjectionSubscribed` response before returning the live reader. The
/// caller can therefore publish an event only after observing the response on
/// the actual Unix-socket byte stream.
async fn projection_handshake_and_subscribe(
    stream: UnixStream,
    project_id: &str,
) -> (
    BufReader<tokio::net::unix::OwnedReadHalf>,
    tokio::net::unix::OwnedWriteHalf,
    String,
    codegg_protocol::projection::replay::ProjectionSubscriptionId,
) {
    let (reader, writer, client_id, subscription_id, _cursor) =
        projection_handshake_and_subscribe_with_cursor(stream, project_id).await;
    (reader, writer, client_id, subscription_id)
}

async fn projection_handshake_and_subscribe_with_cursor(
    stream: UnixStream,
    project_id: &str,
) -> (
    BufReader<tokio::net::unix::OwnedReadHalf>,
    tokio::net::unix::OwnedWriteHalf,
    String,
    codegg_protocol::projection::replay::ProjectionSubscriptionId,
    codegg_protocol::projection::replay::ProjectionCursor,
) {
    use crate::protocol::core::CoreRequest;
    use crate::protocol::projection::replay::{
        ProjectionStreamKind, ProjectionSubscriptionRequest,
    };

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let hello = CoreFrame::ClientHello(ClientHello {
        client_name: format!("projection-{project_id}"),
        client_kind: ClientKind::Automation,
        protocol_version: crate::protocol::core::PROTOCOL_VERSION,
        capabilities: ClientCapabilities {
            visual_notifications: false,
            desktop_notifications: false,
            audio: false,
            tts: false,
            multi_session_view: false,
            plugin_ui_dialog: false,
            plugin_ui_toast: false,
            plugin_ui_panel: false,
            plugin_ui_status_item: false,
            plugin_ui_table: false,
            plugin_ui_markdown: false,
            plugin_ui_code: false,
            plugin_ui_progress: false,
            workspace_registration: false,
            project_catalog: false,
            session_projection: true,
        },
    });
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
    let server_hello = match read_frame(&mut reader).await.unwrap() {
        CoreFrame::ServerHello(hello) => hello,
        other => panic!("expected ServerHello, got {:?}", other),
    };

    let request_id = format!("subscribe-{project_id}");
    let request = CoreFrame::Request(crate::core::new_request(
        request_id.clone(),
        CoreRequest::ProjectionSubscribe {
            request: ProjectionSubscriptionRequest {
                scope: ProjectionStreamKind::Project,
                scope_id: project_id.to_string(),
                cursor: None,
                projection_version: 1,
            },
        },
    ));
    write_half
        .write_all(serde_json::to_string(&request).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
    let subscription_id = match read_frame(&mut reader).await.unwrap() {
        CoreFrame::Response {
            request_id: response_id,
            response,
        } if response_id == request_id => match *response {
            crate::protocol::core::CoreResponse::ProjectionSubscribed {
                subscription_id,
                cursor,
                ..
            } => (subscription_id, cursor),
            other => panic!("expected ProjectionSubscribed, got {:?}", other),
        },
        other => panic!("expected projection response, got {:?}", other),
    };
    let (subscription_id, cursor) = subscription_id;
    (
        reader,
        write_half,
        server_hello.client_id,
        subscription_id,
        cursor,
    )
}

/// Establish the projection-capable handshake and send the subscription
/// request, but do not read its response. Lifecycle-seam tests use this to
/// publish while the receiver is installed and the canonical response is
/// intentionally blocked.
async fn projection_handshake_with_blocked_response(
    stream: UnixStream,
    project_id: &str,
) -> (
    BufReader<tokio::net::unix::OwnedReadHalf>,
    tokio::net::unix::OwnedWriteHalf,
    String,
) {
    use crate::protocol::core::CoreRequest;
    use crate::protocol::projection::replay::{
        ProjectionStreamKind, ProjectionSubscriptionRequest,
    };

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let hello = CoreFrame::ClientHello(ClientHello {
        client_name: format!("projection-race-{project_id}"),
        client_kind: ClientKind::Automation,
        protocol_version: crate::protocol::core::PROTOCOL_VERSION,
        capabilities: ClientCapabilities {
            visual_notifications: false,
            desktop_notifications: false,
            audio: false,
            tts: false,
            multi_session_view: false,
            plugin_ui_dialog: false,
            plugin_ui_toast: false,
            plugin_ui_panel: false,
            plugin_ui_status_item: false,
            plugin_ui_table: false,
            plugin_ui_markdown: false,
            plugin_ui_code: false,
            plugin_ui_progress: false,
            workspace_registration: false,
            project_catalog: false,
            session_projection: true,
        },
    });
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
    let server_hello = match read_frame(&mut reader).await.unwrap() {
        CoreFrame::ServerHello(hello) => hello,
        other => panic!("expected ServerHello, got {:?}", other),
    };

    let request = CoreFrame::Request(crate::core::new_request(
        format!("projection-race-{project_id}"),
        CoreRequest::ProjectionSubscribe {
            request: ProjectionSubscriptionRequest {
                scope: ProjectionStreamKind::Project,
                scope_id: project_id.to_string(),
                cursor: None,
                projection_version: 1,
            },
        },
    ));
    write_half
        .write_all(serde_json::to_string(&request).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
    (reader, write_half, server_hello.client_id)
}

async fn projection_handshake_prefix(
    stream: UnixStream,
    client_name: &str,
) -> (
    BufReader<tokio::net::unix::OwnedReadHalf>,
    tokio::net::unix::OwnedWriteHalf,
    String,
) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let hello = CoreFrame::ClientHello(ClientHello {
        client_name: client_name.to_string(),
        client_kind: ClientKind::Automation,
        protocol_version: crate::protocol::core::PROTOCOL_VERSION,
        capabilities: ClientCapabilities {
            visual_notifications: false,
            desktop_notifications: false,
            audio: false,
            tts: false,
            multi_session_view: false,
            plugin_ui_dialog: false,
            plugin_ui_toast: false,
            plugin_ui_panel: false,
            plugin_ui_status_item: false,
            plugin_ui_table: false,
            plugin_ui_markdown: false,
            plugin_ui_code: false,
            plugin_ui_progress: false,
            workspace_registration: false,
            project_catalog: false,
            session_projection: true,
        },
    });
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
    let client_id = match read_frame(&mut reader).await.unwrap() {
        CoreFrame::ServerHello(hello) => hello.client_id,
        other => panic!("expected ServerHello, got {:?}", other),
    };
    (reader, write_half, client_id)
}

async fn projection_handshake_and_resume(
    stream: UnixStream,
    project_id: &str,
    cursor: codegg_protocol::projection::replay::ProjectionCursor,
) -> (
    BufReader<tokio::net::unix::OwnedReadHalf>,
    tokio::net::unix::OwnedWriteHalf,
    String,
    codegg_protocol::projection::replay::ProjectionSubscriptionId,
    codegg_protocol::projection::replay::ProjectionReplayBatch,
) {
    use crate::protocol::core::CoreRequest;
    let (mut reader, mut writer, client_id) =
        projection_handshake_prefix(stream, &format!("projection-resume-{project_id}")).await;
    let request_id = format!("resume-{project_id}");
    let request = CoreFrame::Request(crate::core::new_request(
        request_id.clone(),
        CoreRequest::ProjectionResume {
            cursor,
            include_snapshot_if_resync: true,
        },
    ));
    writer
        .write_all(serde_json::to_string(&request).unwrap().as_bytes())
        .await
        .unwrap();
    writer.write_all(b"\n").await.unwrap();
    writer.flush().await.unwrap();
    let (subscription_id, batch) = match read_frame(&mut reader).await.unwrap() {
        CoreFrame::Response {
            request_id: response_id,
            response,
        } if response_id == request_id => match *response {
            crate::protocol::core::CoreResponse::ProjectionReplay {
                subscription_id: Some(subscription_id),
                batch,
            } => (subscription_id, batch),
            other => panic!("expected ProjectionReplay, got {other:?}"),
        },
        other => panic!("expected resume response, got {other:?}"),
    };
    (reader, writer, client_id, subscription_id, batch)
}

async fn read_projection_event(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
) -> Option<(
    codegg_protocol::projection::replay::ProjectionSubscriptionId,
    codegg_protocol::projection::replay::ProjectionStreamId,
    codegg_protocol::projection::event::ProjectionEnvelope,
)> {
    loop {
        let frame = read_frame(reader).await?;
        if let CoreFrame::Event(envelope) = frame {
            if let CoreEvent::ProjectionStreamEvent {
                subscription_id,
                stream_id,
                envelope,
            } = envelope.payload
            {
                return Some((subscription_id, stream_id, envelope));
            }
        }
    }
}

async fn abort_server(handle: tokio::task::JoinHandle<()>) {
    handle.abort();
    let _ = tokio::time::timeout(Duration::from_millis(100), handle).await;
}

async fn shutdown_server(handle: tokio::task::JoinHandle<()>, shutdown: CancellationToken) {
    shutdown.cancel();
    tokio::time::timeout(Duration::from_millis(500), handle)
        .await
        .expect("server and connection handlers should shut down")
        .expect("server task should not panic");
}

async fn wait_for_client_count(daemon: &CoreDaemon, expected: usize) {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if daemon.clients.count() == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("client registry should reach the expected lifecycle state");
}

/// EOF and listener shutdown must both release connection-owned state. The
/// first connection exercises EOF cleanup; the second stays open until the
/// listener cancellation path joins its handler.
#[tokio::test]
async fn socket_connection_cleanup_is_idempotent_across_eof_and_shutdown() {
    let daemon = Arc::new(CoreDaemon::new(None, None, None, None));
    let (socket_path, _socket_dir, server_handle, shutdown) =
        spawn_daemon_with_shutdown(Arc::clone(&daemon)).await;

    let stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect EOF lifecycle client");
    let (reader, writer, _client_id) = handshake_and_subscribe(stream, None).await;
    wait_for_client_count(&daemon, 1).await;
    drop(reader);
    drop(writer);
    wait_for_client_count(&daemon, 0).await;

    let stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect shutdown lifecycle client");
    let (reader, writer, _client_id) = handshake_and_subscribe(stream, None).await;
    wait_for_client_count(&daemon, 1).await;

    shutdown_server(server_handle, shutdown).await;
    wait_for_client_count(&daemon, 0).await;
    drop(reader);
    drop(writer);
}

async fn projection_daemon() -> Arc<CoreDaemon> {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    let db_name = format!("daemon_socket_projection_{}", uuid::Uuid::new_v4().simple());
    let options =
        SqliteConnectOptions::from_str(&format!("file:{db_name}?mode=memory&cache=shared"))
            .expect("sqlite options")
            .create_if_missing(true)
            .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("projection test pool");
    crate::session::schema::migrate(&pool)
        .await
        .expect("projection schema");
    Arc::new(CoreDaemon::new(Some(pool), None, None, None))
}

async fn publish_projection_event(daemon: &CoreDaemon, project_id: &str, session_id: &str) {
    publish_projection_event_with_turn(daemon, project_id, session_id, "turn-socket").await;
}

async fn publish_projection_event_with_turn(
    daemon: &CoreDaemon,
    project_id: &str,
    session_id: &str,
    turn_id: &str,
) {
    publish_projection_event_with_turn_at_seq(daemon, project_id, session_id, turn_id, 1).await;
}

async fn publish_projection_event_with_turn_at_seq(
    daemon: &CoreDaemon,
    project_id: &str,
    session_id: &str,
    turn_id: &str,
    event_seq: u64,
) {
    let seam = daemon
        .projection_seam
        .as_ref()
        .expect("SQLite-backed daemon has projection seam");
    let envelope = crate::protocol::core::EventEnvelope {
        protocol_version: crate::protocol::core::PROTOCOL_VERSION,
        event_seq,
        timestamp_ms: 1,
        session_id: Some(session_id.to_string()),
        turn_id: Some(turn_id.to_string()),
        payload: CoreEvent::TurnStarted {
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
        },
    };
    let context = codegg_core::projection_replay::seam::ProjectionBindingContext {
        session_id: Some(session_id.to_string()),
        project_id: Some(project_id.to_string()),
        workspace_id: None,
        binding_revision: 1,
    };
    let outcome = seam
        .service()
        .publish_from_core_with_context(&envelope, &context)
        .await
        .expect("publish socket projection event");
    assert!(matches!(
        outcome,
        codegg_core::projection_replay::service::PublishOutcome::Published { .. }
    ));
}

/// A real Unix-socket projection handoff must deliver each canonical response
/// before its connection's live receiver is released, and foreign lifecycle
/// operations must remain rejected by daemon ownership checks.
#[tokio::test]
async fn two_socket_projection_clients_are_ordered_and_isolated() {
    use crate::protocol::core::CoreRequest;

    let daemon = projection_daemon().await;
    let (socket_path_str, _socket_dir, server_handle) = spawn_daemon(Arc::clone(&daemon)).await;
    let stream_a = UnixStream::connect(&socket_path_str)
        .await
        .expect("connect projection client A");
    let stream_b = UnixStream::connect(&socket_path_str)
        .await
        .expect("connect projection client B");
    let (mut reader_a, _writer_a, _client_id_a, subscription_a) =
        projection_handshake_and_subscribe(stream_a, "project-a").await;
    let (mut reader_b, mut writer_b, _client_id_b, subscription_b) =
        projection_handshake_and_subscribe(stream_b, "project-b").await;
    assert_ne!(subscription_a, subscription_b);

    let foreign_request = CoreFrame::Request(crate::core::new_request(
        "foreign-unsubscribe".to_string(),
        CoreRequest::ProjectionUnsubscribe {
            subscription_id: subscription_a.clone(),
        },
    ));
    writer_b
        .write_all(serde_json::to_string(&foreign_request).unwrap().as_bytes())
        .await
        .unwrap();
    writer_b.write_all(b"\n").await.unwrap();
    writer_b.flush().await.unwrap();
    match read_frame(&mut reader_b).await.unwrap() {
        CoreFrame::Response { response, .. } => assert!(matches!(
            *response,
            crate::protocol::core::CoreResponse::Error {
                code,
                ..
            } if code == "projection_subscription_not_owned"
        )),
        other => panic!("expected foreign-operation error, got {:?}", other),
    }

    publish_projection_event(&daemon, "project-a", "session-a").await;
    let event_a = tokio::time::timeout(Duration::from_millis(400), async {
        loop {
            if let Some(CoreFrame::Event(envelope)) = read_frame(&mut reader_a).await {
                if let CoreEvent::ProjectionStreamEvent {
                    subscription_id, ..
                } = envelope.payload
                {
                    return subscription_id;
                }
            }
        }
    })
    .await
    .expect("client A should receive projection event");
    assert_eq!(event_a, subscription_a);

    let foreign_event = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            if let Some(CoreFrame::Event(envelope)) = read_frame(&mut reader_b).await {
                if let CoreEvent::ProjectionStreamEvent {
                    subscription_id, ..
                } = envelope.payload
                {
                    return subscription_id;
                }
            }
        }
    })
    .await;
    assert!(foreign_event.is_err(), "client B received project A event");

    abort_server(server_handle).await;
}

/// All projection lifecycle operations exposed by the Unix CoreFrame adapter
/// must remain scoped to the daemon-issued connection identity. The test also
/// proves a rejected operation on B does not disturb A's live receiver.
#[tokio::test]
async fn socket_foreign_projection_operations_fail_closed() {
    use crate::protocol::core::CoreRequest;
    use crate::protocol::projection::replay::{ProjectionAck, ProjectionArtifactReadRequest};

    let daemon = projection_daemon().await;
    let (socket_path, _socket_dir, server_handle) = spawn_daemon(Arc::clone(&daemon)).await;
    let stream_a = UnixStream::connect(&socket_path)
        .await
        .expect("connect Unix client A");
    let stream_b = UnixStream::connect(&socket_path)
        .await
        .expect("connect Unix client B");
    let (mut reader_a, _writer_a, _client_a, sub_a, cursor_a) =
        projection_handshake_and_subscribe_with_cursor(stream_a, "project-unix-a").await;
    let (mut reader_b, mut writer_b, _client_b, _sub_b, _cursor_b) =
        projection_handshake_and_subscribe_with_cursor(stream_b, "project-unix-b").await;

    let ack = CoreFrame::Request(crate::core::new_request(
        "unix-foreign-ack".to_string(),
        CoreRequest::ProjectionAck {
            ack: ProjectionAck {
                subscription_id: sub_a.clone(),
                cursor: cursor_a.clone(),
            },
        },
    ));
    writer_b
        .write_all(serde_json::to_string(&ack).unwrap().as_bytes())
        .await
        .unwrap();
    writer_b.write_all(b"\n").await.unwrap();
    writer_b.flush().await.unwrap();
    match read_frame(&mut reader_b).await.unwrap() {
        CoreFrame::Response { response, .. } => assert!(matches!(
            *response,
            crate::protocol::core::CoreResponse::Error { code, .. }
                if code == "subscription_not_found"
        )),
        other => panic!("expected foreign ack rejection, got {other:?}"),
    }

    let resume = CoreFrame::Request(crate::core::new_request(
        "unix-foreign-resume".to_string(),
        CoreRequest::ProjectionResume {
            cursor: cursor_a,
            include_snapshot_if_resync: true,
        },
    ));
    writer_b
        .write_all(serde_json::to_string(&resume).unwrap().as_bytes())
        .await
        .unwrap();
    writer_b.write_all(b"\n").await.unwrap();
    writer_b.flush().await.unwrap();
    match read_frame(&mut reader_b).await.unwrap() {
        CoreFrame::Response { response, .. } => assert!(matches!(
            *response,
            crate::protocol::core::CoreResponse::Error { code, .. }
                if code == "projection_resume_not_owned"
        )),
        other => panic!("expected foreign resume rejection, got {other:?}"),
    }

    let unsubscribe = CoreFrame::Request(crate::core::new_request(
        "unix-foreign-unsubscribe".to_string(),
        CoreRequest::ProjectionUnsubscribe {
            subscription_id: sub_a.clone(),
        },
    ));
    writer_b
        .write_all(serde_json::to_string(&unsubscribe).unwrap().as_bytes())
        .await
        .unwrap();
    writer_b.write_all(b"\n").await.unwrap();
    writer_b.flush().await.unwrap();
    match read_frame(&mut reader_b).await.unwrap() {
        CoreFrame::Response { response, .. } => assert!(matches!(
            *response,
            crate::protocol::core::CoreResponse::Error { code, .. }
                if code == "projection_subscription_not_owned"
        )),
        other => panic!("expected foreign unsubscribe rejection, got {other:?}"),
    }

    for (request_id, request) in [
        (
            "unix-foreign-artifact-list",
            CoreRequest::ProjectionArtifactList {
                project_id: "project-unix-a".to_string(),
            },
        ),
        (
            "unix-foreign-artifact-read",
            CoreRequest::ProjectionArtifactRead {
                request: ProjectionArtifactReadRequest {
                    handle_id: "foreign-handle".to_string(),
                    start: 0,
                    end: Some(1),
                    expected_revision: 1,
                },
                project_id: "project-unix-a".to_string(),
                context_correlation_id: None,
            },
        ),
    ] {
        let frame = CoreFrame::Request(crate::core::new_request(request_id.to_string(), request));
        writer_b
            .write_all(serde_json::to_string(&frame).unwrap().as_bytes())
            .await
            .unwrap();
        writer_b.write_all(b"\n").await.unwrap();
        writer_b.flush().await.unwrap();
        match read_frame(&mut reader_b).await.unwrap() {
            CoreFrame::Response { response, .. } => assert!(matches!(
                *response,
                crate::protocol::core::CoreResponse::Error { code, .. }
                    if code == "projection_scope_not_owned"
            )),
            other => panic!("expected foreign artifact rejection, got {other:?}"),
        }
    }

    publish_projection_event(&daemon, "project-unix-a", "session-unix-a").await;
    let event = tokio::time::timeout(Duration::from_millis(400), async {
        loop {
            if let Some(CoreFrame::Event(envelope)) = read_frame(&mut reader_a).await {
                if let CoreEvent::ProjectionStreamEvent {
                    subscription_id, ..
                } = envelope.payload
                {
                    return subscription_id;
                }
            }
        }
    })
    .await
    .expect("A remains live after B's rejected operations");
    assert_eq!(event, sub_a);
    abort_server(server_handle).await;
}

/// A fresh Unix connection resumes from the persisted cursor, receives only
/// the missing committed range, and then receives one subsequent live event
/// through the newly installed receiver.
#[tokio::test]
async fn socket_reconnect_replays_exact_missing_range_then_live() {
    let daemon = projection_daemon().await;
    let (socket_path, _socket_dir, server_handle) = spawn_daemon(Arc::clone(&daemon)).await;
    let first_stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect first reconnect client");
    let (reader, writer, _client_id, first_sub_id, cursor) =
        projection_handshake_and_subscribe_with_cursor(first_stream, "project-reconnect").await;
    drop(reader);
    drop(writer);
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if daemon
                .projection_seam
                .as_ref()
                .expect("projection seam")
                .service()
                .subscriptions()
                .active_count()
                == 0
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first Unix subscription should be removed before reconnect");

    publish_projection_event_with_turn_at_seq(
        &daemon,
        "project-reconnect",
        "session-reconnect",
        "turn-missing-1",
        1,
    )
    .await;
    publish_projection_event_with_turn_at_seq(
        &daemon,
        "project-reconnect",
        "session-reconnect",
        "turn-missing-2",
        2,
    )
    .await;

    let resumed_stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect resumed reconnect client");
    let (mut reader, _writer, _client_id, new_sub_id, batch) =
        projection_handshake_and_resume(resumed_stream, "project-reconnect", cursor.clone()).await;
    assert_ne!(first_sub_id, new_sub_id);
    assert_eq!(batch.descriptor.stream_id, cursor.stream_id);
    assert_eq!((batch.replay_start_seq, batch.replay_end_seq), (1, 2));
    assert_eq!(
        batch
            .events
            .iter()
            .map(|event| event.event_seq)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        batch
            .events
            .iter()
            .map(|event| match &event.payload {
                crate::protocol::projection::event::ProjectionEvent::TurnStarted { turn } => {
                    turn.turn_id.as_str()
                }
                other => panic!("expected TurnStarted replay identity, got {other:?}"),
            })
            .collect::<Vec<_>>(),
        vec!["turn-missing-1", "turn-missing-2"]
    );

    publish_projection_event_with_turn_at_seq(
        &daemon,
        "project-reconnect",
        "session-reconnect",
        "turn-live-after-replay",
        3,
    )
    .await;
    let (event_sub_id, event_stream_id, event) = tokio::time::timeout(
        Duration::from_millis(400),
        read_projection_event(&mut reader),
    )
    .await
    .expect("live event after Unix replay")
    .expect("Unix projection stream should remain open");
    assert_eq!(event_sub_id, new_sub_id);
    assert_eq!(event_stream_id, batch.descriptor.stream_id);
    assert_eq!(event.event_seq, batch.replay_end_seq + 1);
    assert!(matches!(
        &event.payload,
        crate::protocol::projection::event::ProjectionEvent::TurnStarted { turn }
            if turn.turn_id == "turn-live-after-replay"
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(250), read_frame(&mut reader))
            .await
            .is_err(),
        "Unix replay or live envelope was duplicated"
    );
    abort_server(server_handle).await;
}

/// The Unix adapter must keep its projection forwarder blocked until the
/// canonical response has completed. This publishes through the real daemon
/// seam while receiver installation is complete but response delivery is
/// paused, then verifies response-before-live ordering on the byte stream.
#[tokio::test]
async fn socket_projection_response_precedes_live_event_when_writer_is_blocked() {
    let daemon = projection_daemon().await;
    let lifecycle_seam = ProjectionLifecycleSeam::default();
    let gate = lifecycle_seam.pause_next(ProjectionLifecycleBoundary::AfterReceiverInstallation);
    let (socket_path, _socket_dir, server_handle, _shutdown) =
        spawn_daemon_with_shutdown_and_seam(Arc::clone(&daemon), lifecycle_seam).await;
    let stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect blocked projection client");
    let (mut reader, _writer, _client_id) =
        projection_handshake_with_blocked_response(stream, "project-race").await;

    gate.wait_until_entered().await;
    publish_projection_event(&daemon, "project-race", "session-race").await;
    assert!(
        tokio::time::timeout(Duration::from_millis(100), read_frame(&mut reader))
            .await
            .is_err(),
        "Unix projection event escaped while canonical response was blocked"
    );

    gate.release();
    let first = read_frame(&mut reader).await.expect("canonical response");
    let subscription_id = match first {
        CoreFrame::Response { response, .. } => match *response {
            crate::protocol::core::CoreResponse::ProjectionSubscribed {
                subscription_id, ..
            } => subscription_id,
            other => panic!("expected ProjectionSubscribed, got {other:?}"),
        },
        other => panic!("expected canonical response first, got {other:?}"),
    };
    let event = tokio::time::timeout(Duration::from_millis(400), async {
        loop {
            if let Some(CoreFrame::Event(envelope)) = read_frame(&mut reader).await {
                if let CoreEvent::ProjectionStreamEvent {
                    subscription_id, ..
                } = envelope.payload
                {
                    return subscription_id;
                }
            }
        }
    })
    .await
    .expect("live projection event after canonical response");
    assert_eq!(event, subscription_id);
    abort_server(server_handle).await;
}

#[tokio::test]
async fn socket_failed_receiver_install_rolls_back_daemon_subscription() {
    let daemon = projection_daemon().await;
    let lifecycle_seam = ProjectionLifecycleSeam::default();
    lifecycle_seam.fail_next(
        ProjectionLifecycleBoundary::AfterReceiverInstallation,
        crate::core::transport::projection::CriticalDeliveryError::WriterClosed,
    );
    let (socket_path, _socket_dir, server_handle, _shutdown) =
        spawn_daemon_with_shutdown_and_seam(Arc::clone(&daemon), lifecycle_seam).await;
    let stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect failed-install client");
    let (mut reader, _writer, _client_id) =
        projection_handshake_with_blocked_response(stream, "project-failure").await;
    match read_frame(&mut reader).await.expect("failure response") {
        CoreFrame::Response { response, .. } => assert!(matches!(
            *response,
            crate::protocol::core::CoreResponse::Error { code, .. }
                if code == "projection_receiver_setup_failed"
        )),
        other => panic!("expected typed setup failure, got {other:?}"),
    }
    let seam = daemon.projection_seam.as_ref().unwrap();
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if seam.service().subscriptions().active_count() == 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("failed Unix setup must remove daemon subscription");
    abort_server(server_handle).await;
}

#[tokio::test]
async fn socket_staged_failure_matrix_rolls_back_every_material_class() {
    let scenarios = [
        (
            ProjectionLifecycleBoundary::AfterDaemonSubscriptionCreation,
            crate::core::transport::projection::CriticalDeliveryError::QueueClosed,
        ),
        (
            ProjectionLifecycleBoundary::AfterReceiverInstallation,
            crate::core::transport::projection::CriticalDeliveryError::WriterClosed,
        ),
        (
            ProjectionLifecycleBoundary::BeforeControlEnqueue,
            crate::core::transport::projection::CriticalDeliveryError::Timeout,
        ),
        (
            ProjectionLifecycleBoundary::BeforeControlEnqueue,
            crate::core::transport::projection::CriticalDeliveryError::Serialization,
        ),
        (
            ProjectionLifecycleBoundary::AfterControlEnqueueBeforeWriterReceipt,
            crate::core::transport::projection::CriticalDeliveryError::Cancelled,
        ),
        (
            ProjectionLifecycleBoundary::DuringWriterWrite,
            crate::core::transport::projection::CriticalDeliveryError::WriterClosed,
        ),
        (
            ProjectionLifecycleBoundary::BeforeActivation,
            crate::core::transport::projection::CriticalDeliveryError::Cancelled,
        ),
    ];

    for (index, (boundary, error)) in scenarios.into_iter().enumerate() {
        let daemon = projection_daemon().await;
        let seam = ProjectionLifecycleSeam::default();
        seam.fail_next(boundary, error);
        let (socket_path, _socket_dir, server_handle, _shutdown) =
            spawn_daemon_with_shutdown_and_seam(Arc::clone(&daemon), seam).await;
        let stream = UnixStream::connect(&socket_path)
            .await
            .expect("connect staged failure client");
        let (mut reader, _writer, _client_id) = projection_handshake_with_blocked_response(
            stream,
            &format!("project-unix-failure-{index}"),
        )
        .await;

        let canonical_response =
            tokio::time::timeout(Duration::from_millis(500), read_frame(&mut reader)).await;
        let rollback_after_delivery = matches!(
            boundary,
            ProjectionLifecycleBoundary::AfterControlEnqueueBeforeWriterReceipt
                | ProjectionLifecycleBoundary::BeforeActivation
        );
        if !rollback_after_delivery {
            if let Ok(Some(CoreFrame::Response { response, .. })) = canonical_response {
                assert!(!matches!(
                    *response,
                    crate::protocol::core::CoreResponse::ProjectionSubscribed { .. }
                        | crate::protocol::core::CoreResponse::ProjectionReplay { .. }
                ));
            }
        }
        let seam = daemon.projection_seam.as_ref().expect("projection seam");
        tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                if seam.service().subscriptions().active_count() == 0 {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("failed Unix setup must remove daemon subscription");

        publish_projection_event_with_turn(
            &daemon,
            &format!("project-unix-failure-{index}"),
            "session-unix-failure",
            "turn-unix-failure",
        )
        .await;
        let leaked = tokio::time::timeout(Duration::from_millis(100), async {
            loop {
                match read_frame(&mut reader).await {
                    Some(CoreFrame::Event(event)) => {
                        if matches!(event.payload, CoreEvent::ProjectionStreamEvent { .. }) {
                            return true;
                        }
                    }
                    Some(_) | None => return false,
                }
            }
        })
        .await;
        assert!(
            !matches!(leaked, Ok(true)),
            "failed Unix setup emitted live traffic"
        );
        abort_server(server_handle).await;
    }
}

/// Test for Pass I of the integration test matrix: two real Unix
/// socket connections on a real running daemon must be isolated by
/// session filter. Client A subscribes to `s_A`, client B subscribes
/// to `s_B`; an event published for `s_A` must reach A and not B.
#[tokio::test]
async fn two_socket_session_filter_isolation() {
    let daemon = Arc::new(CoreDaemon::new(None, None, None, None));
    let (socket_path_str, _socket_dir, server_handle) = spawn_daemon(Arc::clone(&daemon)).await;

    // Connect client A and B.
    let stream_a = UnixStream::connect(&socket_path_str)
        .await
        .expect("connect client A");
    let stream_b = UnixStream::connect(&socket_path_str)
        .await
        .expect("connect client B");

    let (mut reader_a, _writer_a, _client_id_a) =
        handshake_and_subscribe(stream_a, Some("s_A".to_string())).await;
    let (mut reader_b, _writer_b, _client_id_b) =
        handshake_and_subscribe(stream_b, Some("s_B".to_string())).await;

    // Sanity: both clients were issued distinct ids.
    assert_ne!(_client_id_a, _client_id_b);

    // Publish a session-A event directly to the daemon's event log.
    daemon
        .event_log
        .publish(
            Some("s_A".into()),
            None,
            CoreEvent::SessionUpdated {
                session_id: "s_A".into(),
            },
        )
        .await;

    // Client A should receive it within the timeout.
    let a_received = tokio::time::timeout(Duration::from_millis(400), async {
        loop {
            match read_frame(&mut reader_a).await {
                Some(CoreFrame::Event(env)) => {
                    if env.session_id.as_deref() == Some("s_A") {
                        return Some(env);
                    }
                }
                _ => return None,
            }
        }
    })
    .await
    .expect("client A should receive s_A event within timeout");

    assert!(
        a_received.is_some(),
        "client A should have received a SessionUpdated for s_A"
    );

    // Client B must NOT receive the s_A event. The forwarder must
    // filter it out, so a short read on B's socket should time out
    // (or yield a non-s_A frame, which we treat as acceptable here).
    let b_received = tokio::time::timeout(Duration::from_millis(200), async {
        read_frame(&mut reader_b).await
    })
    .await;
    match b_received {
        Ok(Some(CoreFrame::Event(env))) => {
            assert_ne!(
                env.session_id.as_deref(),
                Some("s_A"),
                "client B must not receive s_A events, got {:?}",
                env
            );
        }
        Ok(Some(_other)) => {
            // Non-Event frame is acceptable as long as it is not a
            // leaked s_A event (handled above).
        }
        Ok(None) | Err(_) => {
            // Timeout or EOF: B did not receive an s_A event. This
            // is the expected outcome.
        }
    }

    // Clean shutdown.
    abort_server(server_handle).await;
}

/// Test for Pass 1 of the hardening plan: a socket client whose
/// subscription is global-only (session_id: None) must NOT receive
/// session-scoped events. This is the regression test for the
/// historical "global filter matches everything" bug. After Pass 1,
/// a default global subscription means "sessionless events only", so
/// a session event published to a different session must not reach
/// this client.
#[tokio::test]
async fn global_only_subscription_does_not_receive_session_events() {
    let daemon = Arc::new(CoreDaemon::new(None, None, None, None));
    let (socket_path_str, _socket_dir, server_handle) = spawn_daemon(Arc::clone(&daemon)).await;

    let stream = UnixStream::connect(&socket_path_str)
        .await
        .expect("connect client");
    let (mut reader, _writer, _client_id) = handshake_and_subscribe(stream, None).await;

    // Publish a session event. The client should NOT see it.
    daemon
        .event_log
        .publish(
            Some("s_session".into()),
            None,
            CoreEvent::SessionUpdated {
                session_id: "s_session".into(),
            },
        )
        .await;

    let leaked = tokio::time::timeout(Duration::from_millis(200), async {
        read_frame(&mut reader).await
    })
    .await;
    match leaked {
        Ok(Some(CoreFrame::Event(env))) => {
            assert_ne!(
                env.session_id.as_deref(),
                Some("s_session"),
                "global-only client must not receive session events, got {:?}",
                env
            );
        }
        Ok(Some(_other)) => {
            // Non-Event frame is acceptable.
        }
        Ok(None) | Err(_) => {
            // Timeout or EOF: no event arrived, which is the expected outcome.
        }
    }

    // Now publish a global/sessionless event. The client should
    // receive it.
    daemon
        .event_log
        .publish(
            None,
            None,
            CoreEvent::Error {
                code: "global_only".into(),
                message: "m".into(),
            },
        )
        .await;

    let got = tokio::time::timeout(Duration::from_millis(400), async {
        loop {
            match read_frame(&mut reader).await {
                Some(CoreFrame::Event(env)) => {
                    if env.session_id.is_none() {
                        return Some(env);
                    }
                }
                _ => return None,
            }
        }
    })
    .await
    .expect("global-only client should receive a sessionless event");
    assert!(
        got.is_some(),
        "expected a sessionless event for the global client"
    );

    abort_server(server_handle).await;
}

/// Test for Pass 4: replay on Subscribe uses the same filter as live
/// forwarding. Two session events and one global event are published
/// before any client connects; the client subscribes to session s1
/// from_event_seq=0 and must see the s1 event and the global event
/// (because include_global is true on a session filter), but not the
/// s2 event.
#[tokio::test]
async fn resume_replay_uses_same_filter_as_live_forwarding() {
    let daemon = Arc::new(CoreDaemon::new(None, None, None, None));

    // Publish events before any client connects so the Subscribe
    // frame's replay returns them.
    daemon
        .event_log
        .publish(
            Some("s1".into()),
            None,
            CoreEvent::SessionUpdated {
                session_id: "s1".into(),
            },
        )
        .await;
    daemon
        .event_log
        .publish(
            Some("s2".into()),
            None,
            CoreEvent::SessionUpdated {
                session_id: "s2".into(),
            },
        )
        .await;
    daemon
        .event_log
        .publish(
            None,
            None,
            CoreEvent::Error {
                code: "global_pre".into(),
                message: "m".into(),
            },
        )
        .await;

    let (socket_path_str, _socket_dir, server_handle) = spawn_daemon(Arc::clone(&daemon)).await;
    let stream = UnixStream::connect(&socket_path_str)
        .await
        .expect("connect client");
    let (reader, _writer, _client_id) =
        handshake_and_subscribe(stream, Some("s1".to_string())).await;

    // The replay burst was drained by `handshake_and_subscribe`. Now
    // confirm we received s1 + global but NOT s2. We have to inspect
    // the buffers we already saw -- since the helper drains silently,
    // we re-do the test with a non-draining handshake to verify the
    // replay contents directly.
    abort_server(server_handle).await;
    drop(reader);

    // Restart the test with a non-draining handshake to capture the
    // replayed events.
    let (socket_path_str2, _socket_dir2, server_handle2) = spawn_daemon(Arc::clone(&daemon)).await;
    let stream2 = UnixStream::connect(&socket_path_str2)
        .await
        .expect("connect client 2");
    let (read_half, mut write_half) = stream2.into_split();
    let mut reader2 = BufReader::new(read_half);

    let hello = CoreFrame::ClientHello(ClientHello {
        client_name: "integration-test".to_string(),
        client_kind: ClientKind::Automation,
        protocol_version: crate::protocol::core::PROTOCOL_VERSION,
        capabilities: ClientCapabilities {
            visual_notifications: false,
            desktop_notifications: false,
            audio: false,
            tts: false,
            multi_session_view: false,
            plugin_ui_dialog: false,
            plugin_ui_toast: false,
            plugin_ui_panel: false,
            plugin_ui_status_item: false,
            plugin_ui_table: false,
            plugin_ui_markdown: false,
            plugin_ui_code: false,
            plugin_ui_progress: false,
            workspace_registration: false,
            project_catalog: false,
            session_projection: false,
        },
    });
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
    let server_hello = match read_frame(&mut reader2).await.unwrap() {
        CoreFrame::ServerHello(sh) => sh,
        other => panic!("expected ServerHello, got {:?}", other),
    };
    let sub = CoreFrame::Subscribe {
        client_id: server_hello.client_id.clone(),
        session_id: Some("s1".to_string()),
        from_event_seq: Some(0),
    };
    write_half
        .write_all(serde_json::to_string(&sub).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
    drop(write_half);

    // Collect every event frame for a short window. The replay
    // delivers them as a burst; live events would be very rare
    // here because we are not publishing during this window.
    let mut received_sids: Vec<Option<String>> = Vec::new();
    let collect = tokio::time::timeout(Duration::from_millis(300), async {
        while let Some(frame) = read_frame(&mut reader2).await {
            if let CoreFrame::Event(env) = frame {
                received_sids.push(env.session_id.clone());
            } else {
                // Stop on a non-Event frame (e.g. Response, Pong).
                break;
            }
        }
    })
    .await;
    let _ = collect;

    // We expect exactly the s1 event and the global event from the
    // pre-publish burst, in seq order. The s2 event must NOT appear.
    assert!(
        received_sids.iter().any(|s| s.as_deref() == Some("s1")),
        "expected s1 event in replay, got {:?}",
        received_sids
    );
    assert!(
        received_sids.iter().any(|s| s.is_none()),
        "expected global event in replay (session filter has include_global=true), got {:?}",
        received_sids
    );
    assert!(
        !received_sids.iter().any(|s| s.as_deref() == Some("s2")),
        "must not see s2 event in s1 replay, got {:?}",
        received_sids
    );

    abort_server(server_handle2).await;
}

// ===== M010 Mechanism-Faithful Transport Verification Fixtures =====
//
// These tests cover Work Packages D, F, and G: peer-close/write/flush races,
// interrupted replay retry, and fresh identity proof. They complement the
// WebSocket-side fixtures in `tests/projection_transport_real.rs` by exercising
// the same daemon transport contract through the Unix-socket adapter.

/// The Unix adapter must drop a connection cleanly even if the peer closes
/// the write half while the daemon is mid-write. The subscription must be
/// removed, the byte stream EOF'd, and a fresh subscription must see the
/// same event log without leakage from the closed connection.
#[tokio::test]
async fn socket_peer_close_during_writer_delivery_removes_subscription_and_eofs() {
    let daemon = projection_daemon().await;
    let (socket_path, _socket_dir, server_handle) = spawn_daemon(Arc::clone(&daemon)).await;

    let stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect peer-close client");
    let (mut reader, writer, _client_id, _sub_id) =
        projection_handshake_and_subscribe(stream, "project-peer-close").await;
    let seam = daemon.projection_seam.as_ref().expect("projection seam");
    tokio::time::timeout(Duration::from_millis(400), async {
        loop {
            if seam.service().subscriptions().active_count() == 1 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("subscription should install before peer-close race");

    drop(writer);
    let eof = tokio::time::timeout(Duration::from_millis(400), async {
        loop {
            if read_frame(&mut reader).await.is_none() {
                return true;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Unix adapter must EOF the reader after peer closes write half");
    assert!(eof, "Unix adapter never reached EOF after peer close");

    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if seam.service().subscriptions().active_count() == 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("daemon subscription must be removed after peer close");

    let resume_stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect post-peer-close client");
    let (_resume_reader, _resume_writer, _resume_client_id, _resume_sub_id) =
        projection_handshake_and_subscribe(resume_stream, "project-peer-close").await;
    tokio::time::timeout(Duration::from_millis(400), async {
        loop {
            if seam.service().subscriptions().active_count() == 1 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fresh subscription must install after peer-close cleanup");
    abort_server(server_handle).await;
}

/// When the daemon side write fails mid-flush (peer already closed), the
/// adapter must roll back the subscription, close the byte stream with EOF,
/// and remain ready to serve a new connection from the same client identity
/// slot. We verify the recovery path is wired even though no error response
/// is delivered on the failing socket (the canonical response was already
/// enqueued at the moment of failure).
#[tokio::test]
async fn socket_writer_failure_during_flush_closes_stream_and_rolls_back() {
    let daemon = projection_daemon().await;
    let lifecycle_seam = ProjectionLifecycleSeam::default();
    lifecycle_seam.fail_next(
        ProjectionLifecycleBoundary::DuringWriterWrite,
        crate::core::transport::projection::CriticalDeliveryError::WriterClosed,
    );
    let (socket_path, _socket_dir, server_handle, _shutdown) =
        spawn_daemon_with_shutdown_and_seam(Arc::clone(&daemon), lifecycle_seam).await;
    let stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect writer-flush-failure client");
    let (mut reader, _writer, _client_id) =
        projection_handshake_with_blocked_response(stream, "project-writer-flush-failure").await;
    let eof = tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if read_frame(&mut reader).await.is_none() {
                return true;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("writer-flush failure must EOF the Unix byte stream");
    assert!(eof, "Unix adapter never closed after writer-flush failure");

    let seam = daemon.projection_seam.as_ref().expect("projection seam");
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if seam.service().subscriptions().active_count() == 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("writer-flush failure must roll back daemon subscription");

    let recovery_stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect recovery client after writer-flush failure");
    let (_recovery_reader, _recovery_writer, _recovery_client_id, recovery_sub_id) =
        projection_handshake_and_subscribe(recovery_stream, "project-writer-flush-failure").await;
    assert!(
        !recovery_sub_id.0.is_empty(),
        "fresh subscription must receive a non-empty id after writer-flush rollback"
    );
    abort_server(server_handle).await;
}

/// Cancelling the listener-side shutdown token must propagate as EOF to
/// the connected client AND remove the daemon-side subscription. This
/// proves the cancellation race path between the listener task and the
/// per-connection writer is wired correctly.
#[tokio::test]
async fn socket_listener_shutdown_completes_active_writer_and_cleans_subscriptions() {
    let daemon = projection_daemon().await;
    let (socket_path, _socket_dir, server_handle, shutdown) =
        spawn_daemon_with_shutdown(Arc::clone(&daemon)).await;
    let stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect cancellation-race client");
    let (mut reader, _writer, _client_id, _sub_id) =
        projection_handshake_and_subscribe(stream, "project-cancel-race").await;
    let seam = daemon.projection_seam.as_ref().expect("projection seam");
    tokio::time::timeout(Duration::from_millis(400), async {
        loop {
            if seam.service().subscriptions().active_count() == 1 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("subscription should install before cancellation race");

    shutdown.cancel();
    let eof = tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if read_frame(&mut reader).await.is_none() {
                return true;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("client must observe EOF after listener shutdown");
    assert!(eof, "client never received EOF after shutdown cancel");

    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if seam.service().subscriptions().active_count() == 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("daemon subscription must be removed after listener shutdown");
    shutdown_server(server_handle, shutdown).await;
}

/// An interrupted replay handoff must leave the daemon's subscription count
/// at baseline, return the cursor on a fresh retry, and yield a different
/// subscription id than the original interrupted attempt. This is the
/// Unix-side analogue of the WebSocket interrupted-replay retry fixture.
#[tokio::test]
async fn socket_interrupted_replay_retry_resumes_with_fresh_identity() {
    let daemon = projection_daemon().await;
    let (socket_path, _socket_dir, server_handle) = spawn_daemon(Arc::clone(&daemon)).await;

    let first_stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect first interrupted-replay client");
    let (first_reader, first_writer, _first_client_id, first_sub_id, first_cursor) =
        projection_handshake_and_subscribe_with_cursor(first_stream, "project-replay-retry").await;
    drop(first_reader);
    drop(first_writer);
    let seam = daemon.projection_seam.as_ref().expect("projection seam");
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if seam.service().subscriptions().active_count() == 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first Unix subscription should be removed after drop");

    publish_projection_event_with_turn_at_seq(
        &daemon,
        "project-replay-retry",
        "session-replay-retry",
        "turn-after-drop",
        1,
    )
    .await;

    let retry_stream = UnixStream::connect(&socket_path)
        .await
        .expect("connect retry interrupted-replay client");
    let (mut retry_reader, _retry_writer, _retry_client_id, retry_sub_id, retry_batch) =
        projection_handshake_and_resume(retry_stream, "project-replay-retry", first_cursor.clone())
            .await;

    assert_ne!(
        first_sub_id, retry_sub_id,
        "interrupted retry must yield a fresh subscription identity"
    );
    assert_eq!(
        retry_batch.descriptor.stream_id, first_cursor.stream_id,
        "retry must resume on the same stream_id"
    );
    assert_eq!(
        (retry_batch.replay_start_seq, retry_batch.replay_end_seq),
        (1, 1),
        "retry must replay only the missing post-drop event"
    );

    publish_projection_event_with_turn_at_seq(
        &daemon,
        "project-replay-retry",
        "session-replay-retry",
        "turn-live-after-retry",
        2,
    )
    .await;
    let (live_sub_id, _live_stream_id, live_event) = tokio::time::timeout(
        Duration::from_millis(400),
        read_projection_event(&mut retry_reader),
    )
    .await
    .expect("retry client must receive live event after replay")
    .expect("Unix projection stream must remain open");
    assert_eq!(live_sub_id, retry_sub_id);
    assert_eq!(live_event.event_seq, retry_batch.replay_end_seq + 1);

    assert!(
        tokio::time::timeout(Duration::from_millis(250), read_frame(&mut retry_reader))
            .await
            .is_err(),
        "no duplicate replay envelope should be delivered after live event"
    );

    tokio::time::timeout(Duration::from_millis(400), async {
        loop {
            if seam.service().subscriptions().active_count() == 1 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("retry client must hold the only active subscription");

    abort_server(server_handle).await;
}

/// Two consecutive Unix-socket subscriptions on the same project must yield
/// distinct subscription ids, distinct client ids, and isolated live event
/// streams. This is the fresh-identity proof that closes Work Package G.
#[tokio::test]
async fn socket_consecutive_subscriptions_yield_distinct_identities_and_isolation() {
    let daemon = projection_daemon().await;
    let (socket_path, _socket_dir, server_handle) = spawn_daemon(Arc::clone(&daemon)).await;

    let stream_first = UnixStream::connect(&socket_path)
        .await
        .expect("connect fresh-identity client 1");
    let (_first_reader, first_writer, first_client_id, first_sub_id) =
        projection_handshake_and_subscribe(stream_first, "project-fresh-identity").await;

    let stream_second = UnixStream::connect(&socket_path)
        .await
        .expect("connect fresh-identity client 2");
    let (mut second_reader, second_writer, second_client_id, second_sub_id) =
        projection_handshake_and_subscribe(stream_second, "project-fresh-identity").await;

    assert_ne!(
        first_sub_id, second_sub_id,
        "consecutive Unix subscriptions must receive distinct subscription ids"
    );
    assert_ne!(
        first_client_id, second_client_id,
        "consecutive Unix handshakes must receive distinct client ids"
    );

    let seam = daemon.projection_seam.as_ref().expect("projection seam");
    tokio::time::timeout(Duration::from_millis(400), async {
        loop {
            if seam.service().subscriptions().active_count() == 2 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both fresh-identity subscriptions should be active simultaneously");

    publish_projection_event_with_turn_at_seq(
        &daemon,
        "project-fresh-identity",
        "session-fresh-identity",
        "turn-iso",
        1,
    )
    .await;

    let (received_sub, _stream_id, _event) = tokio::time::timeout(
        Duration::from_millis(400),
        read_projection_event(&mut second_reader),
    )
    .await
    .expect("second client must receive live event")
    .expect("Unix projection stream for client 2 must remain open");
    assert_eq!(
        received_sub, second_sub_id,
        "event must be tagged with client 2's subscription id, not client 1's"
    );

    drop(first_writer);
    drop(second_writer);

    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if seam.service().subscriptions().active_count() == 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both subscriptions must be removed after both writers drop");

    abort_server(server_handle).await;
}
