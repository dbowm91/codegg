use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, RwLock};

use crate::core::daemon::CoreDaemon;
use crate::core::event_log::EventFilter;
use crate::error::AppError;
use crate::protocol::frames::{CoreFrame, ServerCapabilities, ServerHello};
use crate::protocol::core::{CoreEvent, EventEnvelope};

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

/// Match an event envelope against a single subscription filter.
/// Session-specific filters only match events for that session; global
/// filters match either explicitly global events (no session_id) or any
/// event when `include_global` is set.
fn event_matches_filter(event: &EventEnvelope<CoreEvent>, filter: &EventFilter) -> bool {
    if let Some(ref sid) = filter.session_id {
        event.session_id.as_deref() == Some(sid.as_str())
    } else {
        filter.include_global || event.session_id.is_none()
    }
}

async fn handle_client(
    daemon: Arc<CoreDaemon>,
    stream: tokio::net::UnixStream,
) -> Result<(), AppError> {
    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let writer = Arc::new(tokio::sync::Mutex::new(write_half));

    // Negotiated per-connection client identity and per-connection filter set.
    // `client_id` is stable for the lifetime of the connection; `filters` is
    // mutated by the read loop on `Subscribe` frames and read by the live
    // event forwarder.
    let client_id = format!("client-{}", uuid::Uuid::new_v4());
    let filters: Arc<RwLock<Vec<EventFilter>>> = Arc::new(RwLock::new(Vec::new()));

    let event_rx = daemon.event_log.subscribe();
    let writer_for_forwarder = Arc::clone(&writer);
    let filters_for_forwarder = Arc::clone(&filters);
    tokio::spawn(async move {
        forward_events(event_rx, writer_for_forwarder, filters_for_forwarder).await;
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
                            client_id: _sub_client_id,
                            session_id,
                            from_event_seq,
                            ..
                        } => {
                            // Build the new filter from this Subscribe frame.
                            // A session_id produces a session-scoped filter
                            // (only events for that session); the absence of
                            // session_id yields a global filter (all events
                            // when `include_global` is true).
                            let new_filter = if let Some(sid) = session_id.clone() {
                                EventFilter {
                                    session_id: Some(sid),
                                    client_id: None,
                                    include_global: false,
                                }
                            } else {
                                EventFilter {
                                    session_id: None,
                                    client_id: None,
                                    include_global: true,
                                }
                            };
                            // Append to the connection's filter set. The wire
                            // format currently only advertises one filter per
                            // Subscribe frame, but the connection state holds
                            // a `Vec` so future wire extensions (e.g. a
                            // session_id list) can add to it without changing
                            // the forwarder logic.
                            {
                                let mut guard = filters.write().await;
                                guard.push(new_filter.clone());
                            }
                            if let Some(ref sid) = new_filter.session_id {
                                daemon.clients.attach_session(&client_id, sid);
                            }
                            let from = from_event_seq.unwrap_or(0);
                            let events = daemon.event_log.replay_from(from, &new_filter).await;
                            let mut w = writer.lock().await;
                            for event in events {
                                let frame = CoreFrame::Event(event);
                                if let Ok(json) = serde_json::to_string(&frame) {
                                    let _ = w.write_all(json.as_bytes()).await;
                                    let _ = w.write_all(b"\n").await;
                                }
                            }
                            let _ = w.flush().await;
                            // `_sub_client_id` is a wire field kept for
                            // compatibility; the daemon-issued id in
                            // `client_id` (the closure variable) is the one
                            // it actually trusts.
                        }
                        CoreFrame::ClientHello(hello) => {
                            tracing::info!(
                                "Client connected: {} (kind: {:?}, id={})",
                                hello.client_name,
                                hello.client_kind,
                                client_id
                            );
                            // Register the negotiated id with the actual
                            // client name from the hello. Registration is
                            // deferred until after ClientHello so the name
                            // is correct (previously this was a hardcoded
                            // "websocket" placeholder).
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
                                client_id: client_id.clone(),
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
    filters: Arc<RwLock<Vec<EventFilter>>>,
) {
    loop {
        match event_rx.recv().await {
            Ok(event) => {
                // Snapshot the filter list under the read lock, then drop the
                // lock before serializing/writing the frame. This keeps the
                // write path from blocking on Subscribe frame processing.
                let matches = {
                    let guard = filters.read().await;
                    if guard.is_empty() {
                        // No subscription yet: live events do not flow.
                        // Clients must send a Subscribe frame after the
                        // ClientHello/ServerHello handshake to opt in.
                        false
                    } else {
                        guard.iter().any(|f| event_matches_filter(&event, f))
                    }
                };
                if !matches {
                    continue;
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::core::{CoreEvent, EventEnvelope, PROTOCOL_VERSION};

    fn envelope(seq: u64, session_id: Option<&str>) -> EventEnvelope<CoreEvent> {
        EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: seq,
            timestamp_ms: 0,
            session_id: session_id.map(str::to_string),
            turn_id: None,
            payload: CoreEvent::Error {
                code: format!("e{}", seq),
                message: "m".into(),
            },
        }
    }

    #[test]
    fn filter_session_matches_event_for_session() {
        let ev = envelope(1, Some("s1"));
        let filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: false,
        };
        assert!(event_matches_filter(&ev, &filter));
    }

    #[test]
    fn filter_session_rejects_other_session() {
        let ev = envelope(1, Some("s2"));
        let filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: false,
        };
        assert!(!event_matches_filter(&ev, &filter));
    }

    #[test]
    fn global_filter_matches_global_event() {
        let ev = envelope(1, None);
        let filter = EventFilter {
            session_id: None,
            client_id: None,
            include_global: true,
        };
        assert!(event_matches_filter(&ev, &filter));
    }

    #[test]
    fn global_filter_with_include_global_matches_session_event() {
        let ev = envelope(1, Some("s1"));
        let filter = EventFilter {
            session_id: None,
            client_id: None,
            include_global: true,
        };
        assert!(event_matches_filter(&ev, &filter));
    }

    #[test]
    fn global_filter_without_include_global_only_matches_unscoped() {
        let ev_session = envelope(1, Some("s1"));
        let ev_global = envelope(2, None);
        let filter = EventFilter {
            session_id: None,
            client_id: None,
            include_global: false,
        };
        assert!(!event_matches_filter(&ev_session, &filter));
        assert!(event_matches_filter(&ev_global, &filter));
    }

    #[tokio::test]
    async fn event_log_replay_respects_filter() {
        // Construct an EventLog directly and verify replay uses the same
        // filter semantics the forwarder relies on. This protects against
        // divergent logic between live forwarding and replay.
        let log = crate::core::event_log::EventLog::new(64);
        log.publish(Some("s1".into()), None, CoreEvent::Error {
            code: "a".into(),
            message: "m".into(),
        })
        .await;
        log.publish(Some("s2".into()), None, CoreEvent::Error {
            code: "b".into(),
            message: "m".into(),
        })
        .await;
        log.publish(None, None, CoreEvent::Error {
            code: "c".into(),
            message: "m".into(),
        })
        .await;

        let s1_filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: false,
        };
        // `replay_from(0, ...)` replays events strictly after seq 0, so all
        // three events are candidates; the session filter then narrows to s1.
        let s1_events = log.replay_from(0, &s1_filter).await;
        assert_eq!(s1_events.len(), 1);
        assert_eq!(s1_events[0].session_id.as_deref(), Some("s1"));

        let global_filter = EventFilter {
            session_id: None,
            client_id: None,
            include_global: true,
        };
        let all_events = log.replay_from(0, &global_filter).await;
        assert_eq!(all_events.len(), 3);
    }
}

#[cfg(test)]
#[path = "daemon_socket_integration_tests.rs"]
mod daemon_socket_integration_tests;
