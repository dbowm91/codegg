use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;

use crate::core::daemon::CoreDaemon;
use crate::core::event_log::EventFilter;
use crate::error::AppError;
use crate::protocol::core::{CoreEvent, EventEnvelope};
use crate::protocol::frames::{CoreFrame, ServerCapabilities, ServerHello};

/// Bind a Unix-domain socket listener to `endpoint`. Returns the bound
/// `UnixListener` plus the absolute path. Used by the singleton lifecycle
/// path so the caller can decide when to bind (after lock acquisition)
/// and can pass a pre-bound listener to [`run_core_socket_with_listener`].
pub fn bind_listener(endpoint: &Path) -> Result<UnixListener, AppError> {
    if let Some(parent) = endpoint.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    UnixListener::bind(endpoint).map_err(|e| {
        AppError::Other(anyhow::anyhow!(
            "failed to bind socket '{}': {}",
            endpoint.display(),
            e
        ))
    })
}

/// Serve a pre-bound listener until `shutdown` is cancelled. This is the
/// lifecycle-aware variant of [`run_core_socket`]; it never deletes paths
/// it does not own and stops accepting new connections cleanly when the
/// token fires.
pub async fn run_core_socket_with_listener(
    daemon: Arc<CoreDaemon>,
    listener: UnixListener,
    endpoint: &Path,
    shutdown: CancellationToken,
) -> Result<(), AppError> {
    tracing::info!("Core daemon listening on {}", endpoint.display());

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => {
                tracing::info!("Core daemon accept loop cancelled");
                break;
            }
            accept = listener.accept() => {
                let (stream, _addr) = accept
                    .map_err(|e| AppError::Other(anyhow::anyhow!("accept failed: {}", e)))?;
                let daemon = Arc::clone(&daemon);
                tokio::spawn(async move {
                    if let Err(e) = handle_client(daemon, stream).await {
                        tracing::error!("Client handler error: {}", e);
                    }
                });
            }
        }
    }
    Ok(())
}

/// Backwards-compatible serve entry point. Binds and serves without
/// lifecycle cancellation. The singleton-lifecycle path uses
/// [`run_core_socket_with_listener`] instead.
pub async fn run_core_socket(daemon: Arc<CoreDaemon>, endpoint: &str) -> Result<(), AppError> {
    let listener = bind_listener(std::path::Path::new(endpoint))?;
    run_core_socket_with_listener(
        daemon,
        listener,
        std::path::Path::new(endpoint),
        CancellationToken::new(),
    )
    .await
}

/// Match an event envelope against a single subscription filter.
///
/// Semantics (per the `include_global` interim contract):
///
/// - `session_id: Some(sid), include_global: true`  -> events for `sid` plus
///   global/sessionless events.
/// - `session_id: Some(sid), include_global: false` -> events for `sid` only.
/// - `session_id: None`                            -> global/sessionless
///   events only. `include_global` is ignored in this branch. A
///   `session_id: None` filter does NOT match all sessions; an
///   all-sessions subscription would require a distinct protocol field.
fn event_matches_filter(event: &EventEnvelope<CoreEvent>, filter: &EventFilter) -> bool {
    match (&filter.session_id, filter.include_global) {
        (Some(sid), true) => {
            event.session_id.as_deref() == Some(sid.as_str()) || event.session_id.is_none()
        }
        (Some(sid), false) => event.session_id.as_deref() == Some(sid.as_str()),
        (None, _) => event.session_id.is_none(),
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
                                Err(e) => crate::protocol::core::CoreResponse::Error {
                                    code: "handler_error".to_string(),
                                    message: e.to_string(),
                                },
                            };
                            let frame = CoreFrame::Response {
                                request_id,
                                response: Box::new(response),
                            };
                            send_frame(&writer, &frame).await;
                        }
                        CoreFrame::Subscribe {
                            client_id: _sub_client_id,
                            session_id,
                            from_event_seq,
                            ..
                        } => {
                            // Build the new filter from this Subscribe frame.
                            //
                            // A session_id produces a session-scoped filter
                            // (events for that session, plus sessionless
                            // events when `include_global: true`). The absence
                            // of session_id yields a global-only filter
                            // (`include_global: false`); it does NOT match
                            // every session. An all-sessions subscription
                            // would require a distinct protocol field.
                            let new_filter = if let Some(sid) = session_id.clone() {
                                EventFilter {
                                    session_id: Some(sid),
                                    client_id: None,
                                    include_global: true,
                                }
                            } else {
                                EventFilter {
                                    session_id: None,
                                    client_id: None,
                                    include_global: false,
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
                                    workspace_registration: true,
                                    workspace_snapshots: true,
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
            include_global: true,
        };
        assert!(event_matches_filter(&ev, &filter));
    }

    #[test]
    fn filter_session_rejects_other_session() {
        let ev = envelope(1, Some("s2"));
        let filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: true,
        };
        assert!(!event_matches_filter(&ev, &filter));
    }

    #[test]
    fn global_filter_rejects_session_event() {
        // session_id=None must NOT match session-scoped events, regardless
        // of include_global. This is the core Pass 1 invariant: a global
        // subscription is global-only, not all-sessions.
        let ev = envelope(1, Some("s1"));
        let filter = EventFilter {
            session_id: None,
            client_id: None,
            include_global: true,
        };
        assert!(
            !event_matches_filter(&ev, &filter),
            "global filter must not match session events"
        );

        let filter_no_global = EventFilter {
            session_id: None,
            client_id: None,
            include_global: false,
        };
        assert!(!event_matches_filter(&ev, &filter_no_global));
    }

    #[test]
    fn global_filter_matches_global_event() {
        let ev = envelope(1, None);
        let filter_with = EventFilter {
            session_id: None,
            client_id: None,
            include_global: true,
        };
        assert!(event_matches_filter(&ev, &filter_with));

        let filter_without = EventFilter {
            session_id: None,
            client_id: None,
            include_global: false,
        };
        assert!(event_matches_filter(&ev, &filter_without));
    }

    #[test]
    fn session_filter_can_include_global_event_if_configured() {
        // A session-specific filter with include_global=true must also
        // match sessionless/global events so session subscribers still
        // see updates that affect them (e.g. session list changes).
        let ev_global = envelope(1, None);
        let filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: true,
        };
        assert!(event_matches_filter(&ev_global, &filter));
    }

    #[test]
    fn session_filter_without_include_global_rejects_global_event() {
        let ev_global = envelope(1, None);
        let filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: false,
        };
        assert!(!event_matches_filter(&ev_global, &filter));
    }

    #[tokio::test]
    async fn event_log_replay_respects_filter() {
        // Construct an EventLog directly and verify replay uses the same
        // filter semantics the forwarder relies on. This protects against
        // divergent logic between live forwarding and replay.
        let log = crate::core::event_log::EventLog::new(64);
        log.publish(
            Some("s1".into()),
            None,
            CoreEvent::Error {
                code: "a".into(),
                message: "m".into(),
            },
        )
        .await;
        log.publish(
            Some("s2".into()),
            None,
            CoreEvent::Error {
                code: "b".into(),
                message: "m".into(),
            },
        )
        .await;
        log.publish(
            None,
            None,
            CoreEvent::Error {
                code: "c".into(),
                message: "m".into(),
            },
        )
        .await;

        // Session-scoped filter (include_global: true to also pull global events).
        let s1_filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: true,
        };
        let s1_events = log.replay_from(0, &s1_filter).await;
        // s1 events + the global event.
        assert_eq!(s1_events.len(), 2);
        for env in &s1_events {
            let sid = env.session_id.as_deref();
            assert!(sid == Some("s1") || sid.is_none());
        }

        // Session-scoped filter without include_global -> s1 only.
        let s1_strict = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: false,
        };
        let s1_strict_events = log.replay_from(0, &s1_strict).await;
        assert_eq!(s1_strict_events.len(), 1);
        assert_eq!(s1_strict_events[0].session_id.as_deref(), Some("s1"));

        // Global filter: session_id: None, regardless of include_global, must
        // NOT return session events. Only the sessionless one.
        let global_filter = EventFilter {
            session_id: None,
            client_id: None,
            include_global: true,
        };
        let global_events = log.replay_from(0, &global_filter).await;
        assert_eq!(global_events.len(), 1);
        assert!(global_events[0].session_id.is_none());
    }
}

#[cfg(test)]
#[path = "daemon_socket_integration_tests.rs"]
mod daemon_socket_integration_tests;
