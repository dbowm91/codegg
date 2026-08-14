use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, Notify};

use crate::core::CoreClient;
use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope};
use crate::protocol::frames::{ClientCapabilities, ClientHello, ClientKind, CoreFrame};

#[derive(Clone)]
pub struct SocketCoreClient {
    #[allow(dead_code)]
    endpoint: String,
    write_stream: Arc<Mutex<Option<tokio::net::unix::OwnedWriteHalf>>>,
    pending: Arc<DashMap<String, oneshot::Sender<Result<CoreResponse, AppError>>>>,
    event_bus: broadcast::Sender<EventEnvelope<CoreEvent>>,
    /// Client id negotiated by the daemon via `ServerHello`. Populated by the
    /// reader task once the handshake completes; readers should not assume it
    /// is set before the first `ServerHello` frame is processed.
    client_id: Arc<Mutex<Option<String>>>,
    /// Daemon identity negotiated by the server's `ServerHello`.
    server_daemon_id: Arc<Mutex<Option<String>>>,
    server_hello_notify: Arc<Notify>,
    connection_closed: Arc<AtomicBool>,
}

impl SocketCoreClient {
    pub async fn connect(endpoint: &str) -> Result<Self, AppError> {
        let path = endpoint.strip_prefix("unix://").unwrap_or(endpoint);
        let stream = UnixStream::connect(path).await.map_err(|e| {
            AppError::Other(anyhow::anyhow!(
                "failed to connect socket core '{}': {}",
                path,
                e
            ))
        })?;

        let (read_half, write_half) = stream.into_split();
        let reader = BufReader::new(read_half);

        let (event_bus, _) = broadcast::channel(256);
        let pending = Arc::new(DashMap::new());

        let client = Self {
            endpoint: endpoint.to_string(),
            write_stream: Arc::new(Mutex::new(Some(write_half))),
            pending: Arc::clone(&pending),
            event_bus: event_bus.clone(),
            client_id: Arc::new(Mutex::new(None)),
            server_daemon_id: Arc::new(Mutex::new(None)),
            server_hello_notify: Arc::new(Notify::new()),
            connection_closed: Arc::new(AtomicBool::new(false)),
        };

        client.spawn_reader(reader, pending, event_bus);

        client.send_client_hello().await?;
        client.daemon_id().await?;

        Ok(client)
    }

    pub async fn reconnect(&self) -> Result<(), AppError> {
        let path = self
            .endpoint
            .strip_prefix("unix://")
            .unwrap_or(&self.endpoint);
        let stream = UnixStream::connect(path)
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("failed to reconnect: {}", e)))?;

        let (read_half, write_half) = stream.into_split();
        *self.write_stream.lock().await = Some(write_half);
        self.connection_closed.store(false, Ordering::Release);
        *self.client_id.lock().await = None;
        *self.server_daemon_id.lock().await = None;
        let reader = BufReader::new(read_half);
        self.spawn_reader(reader, Arc::clone(&self.pending), self.event_bus.clone());
        self.send_client_hello().await?;
        self.daemon_id().await?;

        Ok(())
    }

    async fn send_client_hello(&self) -> Result<(), AppError> {
        let hello = CoreFrame::ClientHello(ClientHello {
            client_name: "codegg-tui".to_string(),
            client_kind: ClientKind::Tui,
            protocol_version: crate::protocol::core::PROTOCOL_VERSION,
            capabilities: ClientCapabilities {
                visual_notifications: true,
                desktop_notifications: true,
                audio: true,
                tts: true,
                multi_session_view: false,
                plugin_ui_dialog: false,
                plugin_ui_toast: false,
                plugin_ui_panel: false,
                plugin_ui_status_item: false,
                plugin_ui_table: false,
                plugin_ui_markdown: false,
                plugin_ui_code: false,
                plugin_ui_progress: false,
                workspace_registration: true,
                project_catalog: true,
                session_projection: true,
            },
        });
        self.send_frame(&hello).await
    }

    /// Return the negotiated `client_id` once the `ServerHello` has been
    /// processed. Returns `None` if the handshake has not completed yet.
    pub async fn client_id(&self) -> Option<String> {
        self.client_id.lock().await.clone()
    }

    /// Send a session-scoped `Subscribe` frame to the daemon. The
    /// resulting filter on the server is `EventFilter { session_id,
    /// include_global: true }` so the client sees events for that
    /// session plus sessionless/global events. The default global
    /// subscription installed after `ServerHello` remains active
    /// (filters are append-only per connection) and may be used to
    /// receive additional session updates.
    pub async fn subscribe_session_events(
        &self,
        session_id: String,
        from_event_seq: Option<u64>,
    ) -> Result<(), AppError> {
        let client_id = self.client_id().await.ok_or_else(|| {
            AppError::Other(anyhow::anyhow!(
                "socket client has not received ServerHello"
            ))
        })?;
        let frame = CoreFrame::Subscribe {
            client_id,
            session_id: Some(session_id),
            from_event_seq,
        };
        self.send_frame(&frame).await
    }

    /// Send a single `CoreFrame` over the socket. Used by
    /// `subscribe_session_events` and any future socket-only helpers.
    async fn send_frame(&self, frame: &CoreFrame) -> Result<(), AppError> {
        let json = serde_json::to_string(frame).map_err(AppError::Json)?;
        let mut guard = self.write_stream.lock().await;
        let stream = guard
            .as_mut()
            .ok_or_else(|| AppError::Other(anyhow::anyhow!("socket core stream unavailable")))?;
        stream
            .write_all(json.as_bytes())
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("socket write failed: {}", e)))?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("socket write failed: {}", e)))?;
        stream
            .flush()
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("socket flush failed: {}", e)))?;
        Ok(())
    }

    fn spawn_reader(
        &self,
        mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
        pending: Arc<DashMap<String, oneshot::Sender<Result<CoreResponse, AppError>>>>,
        event_bus: broadcast::Sender<EventEnvelope<CoreEvent>>,
    ) {
        // Capture handles we need back in the parent so the reader can record
        // the negotiated client_id and send a default global Subscribe frame
        // after the handshake.
        let client_id_slot = Arc::clone(&self.client_id);
        let server_daemon_id_slot = Arc::clone(&self.server_daemon_id);
        let server_hello_notify = Arc::clone(&self.server_hello_notify);
        let write_stream = Arc::clone(&self.write_stream);
        let connection_closed = Arc::clone(&self.connection_closed);

        tokio::spawn(async move {
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
                        match serde_json::from_str::<CoreFrame>(trimmed) {
                            Ok(frame) => match frame {
                                CoreFrame::Response {
                                    request_id,
                                    response,
                                } => {
                                    if let Some((_, tx)) = pending.remove(&request_id) {
                                        let _ = tx.send(Ok(*response));
                                    }
                                }
                                CoreFrame::Event(envelope) => {
                                    let _ = event_bus.send(envelope);
                                }
                                CoreFrame::Pong => {}
                                CoreFrame::ServerHello(hello) => {
                                    if hello.protocol_version
                                        != crate::protocol::core::PROTOCOL_VERSION
                                    {
                                        tracing::warn!(
                                            "incompatible daemon protocol version {} (expected {})",
                                            hello.protocol_version,
                                            crate::protocol::core::PROTOCOL_VERSION
                                        );
                                        break;
                                    }
                                    tracing::info!(
                                        "Server connected: {} (protocol v{}, client_id={})",
                                        hello.daemon_id,
                                        hello.protocol_version,
                                        hello.client_id
                                    );
                                    *server_daemon_id_slot.lock().await =
                                        Some(hello.daemon_id.clone());
                                    server_hello_notify.notify_waiters();
                                    // Record the negotiated id so callers can
                                    // correlate the connection in the daemon's
                                    // `ClientRegistry`.
                                    *client_id_slot.lock().await = Some(hello.client_id.clone());

                                    // Default global subscription: a TUI
                                    // client typically wants to see global
                                    // events (e.g. session updates). Pass
                                    // `from_event_seq: Some(0)` so any
                                    // subsequent live events flow but no
                                    // historical replay is sent on connect.
                                    // Specific session subscriptions can be
                                    // added later by sending another
                                    // Subscribe frame with `session_id`.
                                    let default_sub = CoreFrame::Subscribe {
                                        client_id: hello.client_id.clone(),
                                        session_id: None,
                                        from_event_seq: Some(0),
                                    };
                                    if let Ok(json) = serde_json::to_string(&default_sub) {
                                        let mut guard = write_stream.lock().await;
                                        if let Some(stream) = guard.as_mut() {
                                            let _ = stream.write_all(json.as_bytes()).await;
                                            let _ = stream.write_all(b"\n").await;
                                            let _ = stream.flush().await;
                                        }
                                    }
                                }
                                _ => {}
                            },
                            Err(e) => {
                                tracing::warn!("Failed to deserialize core frame: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Socket reader error: {}", e);
                        break;
                    }
                }
            }
            connection_closed.store(true, Ordering::Release);
            *write_stream.lock().await = None;
            for entry in pending.iter() {
                let request_id = entry.key().clone();
                if let Some((_, tx)) = pending.remove(&request_id) {
                    let _ = tx.send(Err(AppError::Other(anyhow::anyhow!(
                        "socket core connection closed"
                    ))));
                }
            }
            server_hello_notify.notify_waiters();
        });
    }

    /// Return the live daemon identity negotiated by `ServerHello`.
    pub async fn daemon_id(&self) -> Result<String, AppError> {
        loop {
            if let Some(daemon_id) = self.server_daemon_id.lock().await.clone() {
                return Ok(daemon_id);
            }
            if self.connection_closed.load(Ordering::Acquire) {
                return Err(AppError::Other(anyhow::anyhow!(
                    "socket core connection closed before ServerHello"
                )));
            }

            let notified = self.server_hello_notify.notified();
            if let Some(daemon_id) = self.server_daemon_id.lock().await.clone() {
                return Ok(daemon_id);
            }
            tokio::time::timeout(std::time::Duration::from_secs(5), notified)
                .await
                .map_err(|_| {
                    AppError::Other(anyhow::anyhow!("socket core did not receive ServerHello"))
                })?;
        }
    }
}

#[async_trait]
impl CoreClient for SocketCoreClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        let request_id = request.request_id.clone();
        let frame = CoreFrame::Request(request);
        let payload = serde_json::to_string(&frame).map_err(AppError::Json)?;

        let (tx, rx) = oneshot::channel();
        self.pending.insert(request_id.clone(), tx);

        {
            let mut guard = self.write_stream.lock().await;
            let Some(stream) = guard.as_mut() else {
                self.pending.remove(&request_id);
                return Err(AppError::Other(anyhow::anyhow!(
                    "socket core stream unavailable"
                )));
            };
            if let Err(e) = stream.write_all(payload.as_bytes()).await {
                self.pending.remove(&request_id);
                return Err(AppError::Other(anyhow::anyhow!(
                    "socket write failed: {}",
                    e
                )));
            }
            if let Err(e) = stream.write_all(b"\n").await {
                self.pending.remove(&request_id);
                return Err(AppError::Other(anyhow::anyhow!(
                    "socket write failed: {}",
                    e
                )));
            }
            if let Err(e) = stream.flush().await {
                self.pending.remove(&request_id);
                return Err(AppError::Other(anyhow::anyhow!(
                    "socket flush failed: {}",
                    e
                )));
            }
        }

        Ok(rx.await.map_err(|_| {
            self.pending.remove(&request_id);
            AppError::Other(anyhow::anyhow!("response channel closed"))
        })??)
    }

    fn subscribe(&self) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut broadcast_rx = self.event_bus.subscribe();
        tokio::spawn(async move {
            loop {
                match broadcast_rx.recv().await {
                    Ok(event) => {
                        if tx.send(event).is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Socket event subscriber lagged, {} events dropped", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::frames::{ServerCapabilities, ServerHello};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    #[tokio::test(flavor = "current_thread")]
    async fn peer_death_releases_pending_request_with_error() {
        let socket = std::path::PathBuf::from(format!(
            "/tmp/cgpd-{}.sock",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        let listener = UnixListener::bind(&socket).expect("bind test socket");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept client");
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                reader.read_line(&mut line),
            )
            .await
            .expect("read ClientHello timeout")
            .expect("read ClientHello");
            let hello = CoreFrame::ServerHello(ServerHello {
                daemon_id: "peer-death-daemon".into(),
                protocol_version: crate::protocol::core::PROTOCOL_VERSION,
                server_capabilities: ServerCapabilities {
                    event_replay: true,
                    session_management: true,
                    permission_routing: true,
                    workspace_registration: true,
                    workspace_snapshots: true,
                    durable_jobs: true,
                    durable_schedules: true,
                    identity_aware_context: true,
                    project_catalog: true,
                    session_projection: true,
                },
                client_id: "peer-death-client".into(),
            });
            let encoded = serde_json::to_string(&hello).expect("serialize ServerHello");
            write_half
                .write_all(format!("{encoded}\n").as_bytes())
                .await
                .expect("write ServerHello");
            write_half.flush().await.expect("flush ServerHello");
            drop(reader);
            drop(write_half);
        });

        let endpoint = format!("unix://{}", socket.display());
        let client = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            SocketCoreClient::connect(&endpoint),
        )
        .await
        .expect("handshake with fixture timeout")
        .expect("handshake with fixture");
        let request =
            crate::core::new_request("peer-death-request".into(), CoreRequest::SnapshotDaemon);
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(2), client.request(request))
                .await
                .expect("pending request must resolve after peer death");
        assert!(result.is_err(), "peer death must fail the request waiter");
        server.await.expect("peer-death fixture");
        let _ = std::fs::remove_file(socket);
    }
}
