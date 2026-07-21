use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::core::daemon::CoreDaemon;
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
                subscription_id, ..
            } => subscription_id,
            other => panic!("expected ProjectionSubscribed, got {:?}", other),
        },
        other => panic!("expected projection response, got {:?}", other),
    };
    (reader, write_half, server_hello.client_id, subscription_id)
}

async fn abort_server(handle: tokio::task::JoinHandle<()>) {
    handle.abort();
    let _ = tokio::time::timeout(Duration::from_millis(100), handle).await;
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
    let seam = daemon
        .projection_seam
        .as_ref()
        .expect("SQLite-backed daemon has projection seam");
    let envelope = crate::protocol::core::EventEnvelope {
        protocol_version: crate::protocol::core::PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1,
        session_id: Some(session_id.to_string()),
        turn_id: Some("turn-socket".to_string()),
        payload: CoreEvent::TurnStarted {
            session_id: session_id.to_string(),
            turn_id: "turn-socket".to_string(),
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
