use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::broadcast;

use crate::core::daemon::CoreDaemon;
use crate::error::AppError;
use crate::protocol::frames::{CoreFrame, ServerHello, ServerCapabilities};
use crate::protocol::core::{EventEnvelope, CoreEvent};

pub async fn run_core_socket(
    daemon: Arc<CoreDaemon>,
    endpoint: &str,
) -> Result<(), AppError> {
    let listener = UnixListener::bind(endpoint).map_err(|e| {
        AppError::Other(anyhow::anyhow!("failed to bind socket '{}': {}", endpoint, e))
    })?;

    tracing::info!("Core daemon listening on {}", endpoint);

    loop {
        let (stream, _addr) = listener.accept().await.map_err(|e| {
            AppError::Other(anyhow::anyhow!("accept failed: {}", e))
        })?;

        let daemon = Arc::clone(&daemon);
        tokio::spawn(async move {
            if let Err(e) = handle_client(daemon, stream).await {
                tracing::error!("Client handler error: {}", e);
            }
        });
    }
}

async fn handle_client(
    daemon: Arc<CoreDaemon>,
    stream: tokio::net::UnixStream,
) -> Result<(), AppError> {
    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let writer = Arc::new(tokio::sync::Mutex::new(write_half));

    let client_id = format!("client-{}", uuid::Uuid::new_v4());
    daemon
        .clients
        .register(client_id.clone(), "websocket".to_string(), None);

    let event_rx = daemon.event_log.subscribe();

    let writer_clone = Arc::clone(&writer);
    tokio::spawn(async move {
        forward_events(event_rx, writer_clone).await;
    });

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(frame) = serde_json::from_str::<CoreFrame>(trimmed) {
                    match frame {
                        CoreFrame::Request(envelope) => {
                            let request_id = envelope.request_id.clone();
                            let response = match daemon.handle_request(envelope).await {
                                Ok(resp) => resp,
                                Err(e) => {
                                    crate::protocol::core::CoreResponse::Error {
                                        code: "handler_error".to_string(),
                                        message: e.to_string(),
                                    }
                                }
                            };
                            let frame = CoreFrame::Response { request_id, response };
                            send_frame(&writer, &frame).await;
                        }
                        CoreFrame::Subscribe {
                            client_id: sub_client_id,
                            session_id,
                            from_event_seq,
                            ..
                        } => {
                            let filter = crate::core::event_log::EventFilter {
                                session_id: session_id.clone(),
                                client_id: None,
                                include_global: true,
                            };
                            let from = from_event_seq.unwrap_or(1);
                            let events = daemon.event_log.replay_from(from, &filter).await;
                            let mut w = writer.lock().await;
                            for event in events {
                                let frame = CoreFrame::Event(event);
                                if let Ok(json) = serde_json::to_string(&frame) {
                                    let _ = w.write_all(json.as_bytes()).await;
                                    let _ = w.write_all(b"\n").await;
                                }
                            }
                            let _ = w.flush().await;
                            if let Some(ref sid) = session_id {
                                daemon.clients.attach_session(&sub_client_id, sid);
                            }
                        }
                        CoreFrame::ClientHello(hello) => {
                            tracing::info!(
                                "Client connected: {} (kind: {:?})",
                                hello.client_name,
                                hello.client_kind
                            );
                            daemon.clients.register(
                                client_id.clone(),
                                hello.client_name.clone(),
                                Some(hello.capabilities.clone()),
                            );
                            let server_hello = CoreFrame::ServerHello(ServerHello {
                                daemon_id: daemon.daemon_id.clone(),
                                protocol_version: crate::protocol::core::PROTOCOL_VERSION,
                                server_capabilities: ServerCapabilities {
                                    event_replay: true,
                                    session_management: true,
                                    permission_routing: true,
                                },
                            });
                            send_frame(&writer, &server_hello).await;
                        }
                        CoreFrame::Ping => {
                            send_frame(&writer, &CoreFrame::Pong).await;
                        }
                        _ => {}
                    }
                }
            }
            Err(_) => break,
        }
    }

    daemon.clients.unregister(&client_id);

    Ok(())
}

async fn forward_events(
    mut event_rx: broadcast::Receiver<EventEnvelope<CoreEvent>>,
    writer: Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
) {
    loop {
        match event_rx.recv().await {
            Ok(event) => {
                let frame = CoreFrame::Event(event);
                if let Ok(json) = serde_json::to_string(&frame) {
                    let mut w = writer.lock().await;
                    if w.write_all(json.as_bytes()).await.is_err() {
                        break;
                    }
                    if w.write_all(b"\n").await.is_err() {
                        break;
                    }
                    let _ = w.flush().await;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("Event forwarder lagged, {} events dropped", n);
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn send_frame(
    writer: &Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
    frame: &CoreFrame,
) {
    if let Ok(json) = serde_json::to_string(frame) {
        let mut w = writer.lock().await;
        let _ = w.write_all(json.as_bytes()).await;
        let _ = w.write_all(b"\n").await;
        let _ = w.flush().await;
    }
}
