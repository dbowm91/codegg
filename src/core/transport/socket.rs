use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};

use crate::core::CoreClient;
use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope};
use crate::protocol::frames::{ClientCapabilities, ClientHello, ClientKind, CoreFrame};

#[derive(Clone)]
pub struct SocketCoreClient {
    #[allow(dead_code)]
    endpoint: String,
    write_stream: Arc<Mutex<Option<tokio::net::unix::OwnedWriteHalf>>>,
    pending: Arc<DashMap<String, oneshot::Sender<CoreResponse>>>,
    event_bus: broadcast::Sender<EventEnvelope<CoreEvent>>,
    /// Client id negotiated by the daemon via `ServerHello`. Populated by the
    /// reader task once the handshake completes; readers should not assume it
    /// is set before the first `ServerHello` frame is processed.
    client_id: Arc<Mutex<Option<String>>>,
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
        let pending: Arc<DashMap<String, oneshot::Sender<CoreResponse>>> = Arc::new(DashMap::new());

        let client = Self {
            endpoint: endpoint.to_string(),
            write_stream: Arc::new(Mutex::new(Some(write_half))),
            pending: Arc::clone(&pending),
            event_bus: event_bus.clone(),
            client_id: Arc::new(Mutex::new(None)),
        };

        client.spawn_reader(reader, pending, event_bus);

        {
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
                },
            });
            if let Ok(json) = serde_json::to_string(&hello) {
                let mut guard = client.write_stream.lock().await;
                if let Some(stream) = guard.as_mut() {
                    let _ = stream.write_all(json.as_bytes()).await;
                    let _ = stream.write_all(b"\n").await;
                    let _ = stream.flush().await;
                }
            }
        }

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

        let reader = BufReader::new(read_half);
        self.spawn_reader(reader, Arc::clone(&self.pending), self.event_bus.clone());

        Ok(())
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
        pending: Arc<DashMap<String, oneshot::Sender<CoreResponse>>>,
        event_bus: broadcast::Sender<EventEnvelope<CoreEvent>>,
    ) {
        // Capture handles we need back in the parent so the reader can record
        // the negotiated client_id and send a default global Subscribe frame
        // after the handshake.
        let client_id_slot = Arc::clone(&self.client_id);
        let write_stream = Arc::clone(&self.write_stream);

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
                                        let _ = tx.send(*response);
                                    }
                                }
                                CoreFrame::Event(envelope) => {
                                    let _ = event_bus.send(envelope);
                                }
                                CoreFrame::Pong => {}
                                CoreFrame::ServerHello(hello) => {
                                    tracing::info!(
                                        "Server connected: {} (protocol v{}, client_id={})",
                                        hello.daemon_id,
                                        hello.protocol_version,
                                        hello.client_id
                                    );
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
        });
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
            let stream = guard.as_mut().ok_or_else(|| {
                AppError::Other(anyhow::anyhow!("socket core stream unavailable"))
            })?;
            if let Err(e) = stream.write_all(payload.as_bytes()).await {
                drop(guard);
                if self.reconnect().await.is_ok() {
                    let mut guard = self.write_stream.lock().await;
                    let stream = guard.as_mut().ok_or_else(|| {
                        AppError::Other(anyhow::anyhow!(
                            "socket core stream unavailable after reconnect"
                        ))
                    })?;
                    stream.write_all(payload.as_bytes()).await.map_err(|e| {
                        AppError::Other(anyhow::anyhow!(
                            "socket write failed after reconnect: {}",
                            e
                        ))
                    })?;
                    stream.write_all(b"\n").await.map_err(|e| {
                        AppError::Other(anyhow::anyhow!(
                            "socket write failed after reconnect: {}",
                            e
                        ))
                    })?;
                    stream.flush().await.map_err(|e| {
                        AppError::Other(anyhow::anyhow!(
                            "socket flush failed after reconnect: {}",
                            e
                        ))
                    })?;
                } else {
                    return Err(AppError::Other(anyhow::anyhow!(
                        "socket write failed and reconnect failed: {}",
                        e
                    )));
                }
            } else {
                stream
                    .write_all(b"\n")
                    .await
                    .map_err(|e| AppError::Other(anyhow::anyhow!("socket write failed: {}", e)))?;
                stream
                    .flush()
                    .await
                    .map_err(|e| AppError::Other(anyhow::anyhow!("socket flush failed: {}", e)))?;
            }
        }

        rx.await.map_err(|_| {
            self.pending.remove(&request_id);
            AppError::Other(anyhow::anyhow!("response channel closed"))
        })
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
