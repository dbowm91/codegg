use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::core::daemon::CoreDaemon;
use crate::protocol::core::CoreEvent;
use crate::protocol::frames::{ClientCapabilities, ClientHello, ClientKind, CoreFrame};

/// Read a single JSON frame (newline-delimited) from a `BufReader`.
async fn read_frame(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
) -> Option<CoreFrame> {
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

/// Drive a complete `ClientHello` + `Subscribe` handshake against the
/// running daemon, then drain any replayed events. Returns the
/// `BufReader` positioned at the live event boundary, plus the
/// negotiated `client_id`.
async fn handshake_and_subscribe(
    stream: UnixStream,
    session_id: Option<String>,
) -> (BufReader<tokio::net::unix::OwnedReadHalf>, String) {
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

    // Drop the writer to release the read half. The server's forwarder
    // task does not need the writer to stay open after the handshake
    // for live events to flow; the writer side stays open on the
    // server because the server holds its own OwnedWriteHalf. We just
    // need to drop our local copy so the BufReader can complete reads.
    drop(write_half);

    // Drain any replayed events. The replay delivers events as a
    // burst; a short timeout is enough to surface the historical
    // events before live ones start flowing.
    let drain = tokio::time::timeout(Duration::from_millis(150), async {
        while let Some(CoreFrame::Event(_)) = read_frame(&mut reader).await {
        }
    })
    .await;
    let _ = drain;

    (reader, client_id)
}

/// Test for Pass I of the integration test matrix: two real Unix
/// socket connections on a real running daemon must be isolated by
/// session filter. Client A subscribes to `s_A`, client B subscribes
/// to `s_B`; an event published for `s_A` must reach A and not B.
#[tokio::test]
async fn two_socket_session_filter_isolation() {
    use sqlx::sqlite::SqlitePoolOptions;
    use crate::session::schema::migrate;

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("daemon.sock");
    let socket_path_str = socket_path.to_string_lossy().to_string();

    let db_path = dir.path().join("test.db");
    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&db_url)
        .await
        .unwrap();
    sqlx::query("PRAGMA journal_mode=WAL")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("PRAGMA busy_timeout=5000")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    migrate(&pool).await.unwrap();

    let daemon = Arc::new(CoreDaemon::new(Some(pool), None, None, None));

    // Spawn the daemon socket server. It runs until the test ends.
    let daemon_for_server = Arc::clone(&daemon);
    let socket_path_for_server = socket_path_str.clone();
    let server_handle = tokio::spawn(async move {
        let _ = crate::core::transport::daemon_socket::run_core_socket(
            daemon_for_server,
            &socket_path_for_server,
        )
        .await;
    });

    // Wait for the listener to come up. `run_core_socket` binds
    // synchronously inside the spawned task; a short sleep is enough
    // to let the OS register the listener.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect client A and B.
    let stream_a = UnixStream::connect(&socket_path_str)
        .await
        .expect("connect client A");
    let stream_b = UnixStream::connect(&socket_path_str)
        .await
        .expect("connect client B");

    let (mut reader_a, _client_id_a) =
        handshake_and_subscribe(stream_a, Some("s_A".to_string())).await;
    let (mut reader_b, _client_id_b) =
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
    server_handle.abort();
    let _ = tokio::time::timeout(Duration::from_millis(100), server_handle).await;
}
