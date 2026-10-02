use codegg_protocol::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope};
use codegg_protocol::frames::{ClientHello, CoreFrame};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, Notify, OwnedSemaphorePermit, Semaphore};

use crate::{ClientError, FrontendDescriptor};

const EVENT_CAPACITY: usize = 256;
const REQUEST_CAPACITY: usize = 1024;
type PendingRequest = (
    oneshot::Sender<Result<CoreResponse, ClientError>>,
    OwnedSemaphorePermit,
);

struct PendingRequestGuard {
    request_id: String,
    pending: Arc<DashMap<String, PendingRequest>>,
}

impl Drop for PendingRequestGuard {
    fn drop(&mut self) {
        self.pending.remove(&self.request_id);
    }
}

/// Multiplexed local CoreFrame client. One reader owns the stream read half;
/// request waiters and event subscribers are bounded and connection-scoped.
#[derive(Clone)]
pub struct LocalSocketClient {
    endpoint: String,
    descriptor: FrontendDescriptor,
    writer: Arc<Mutex<Option<tokio::net::unix::OwnedWriteHalf>>>,
    pending: Arc<DashMap<String, PendingRequest>>,
    request_slots: Arc<Semaphore>,
    events: broadcast::Sender<EventEnvelope<CoreEvent>>,
    client_id: Arc<Mutex<Option<String>>>,
    daemon_id: Arc<Mutex<Option<String>>>,
    handshake_error: Arc<Mutex<Option<String>>>,
    hello_notify: Arc<Notify>,
    closed: Arc<AtomicBool>,
    reader_task: Arc<StdMutex<Option<tokio::task::JoinHandle<()>>>>,
    connection_generation: Arc<AtomicU64>,
}

impl LocalSocketClient {
    pub async fn connect(
        endpoint: impl Into<String>,
        descriptor: FrontendDescriptor,
    ) -> Result<Self, ClientError> {
        let endpoint = endpoint.into();
        let stream = open_unix(&endpoint).await?;
        let (read, write) = stream.into_split();
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let pending = Arc::new(DashMap::<String, PendingRequest>::new());
        let client = Self {
            endpoint,
            descriptor,
            writer: Arc::new(Mutex::new(Some(write))),
            pending: Arc::clone(&pending),
            request_slots: Arc::new(Semaphore::new(REQUEST_CAPACITY)),
            events: events.clone(),
            client_id: Arc::new(Mutex::new(None)),
            daemon_id: Arc::new(Mutex::new(None)),
            handshake_error: Arc::new(Mutex::new(None)),
            hello_notify: Arc::new(Notify::new()),
            closed: Arc::new(AtomicBool::new(false)),
            reader_task: Arc::new(StdMutex::new(None)),
            connection_generation: Arc::new(AtomicU64::new(0)),
        };
        client.spawn_reader(BufReader::new(read), pending, events);
        client.send_hello().await?;
        client.daemon_id().await?;
        Ok(client)
    }

    pub async fn reconnect(&self) -> Result<(), ClientError> {
        self.abort_reader_task();
        self.fail_pending();
        self.closed.store(true, Ordering::Release);
        *self.writer.lock().await = None;
        let stream = open_unix(&self.endpoint).await?;
        let (read, write) = stream.into_split();
        *self.writer.lock().await = Some(write);
        self.closed.store(false, Ordering::Release);
        *self.client_id.lock().await = None;
        *self.daemon_id.lock().await = None;
        *self.handshake_error.lock().await = None;
        self.spawn_reader(
            BufReader::new(read),
            Arc::clone(&self.pending),
            self.events.clone(),
        );
        self.send_hello().await?;
        self.daemon_id().await?;
        Ok(())
    }

    pub async fn daemon_id(&self) -> Result<String, ClientError> {
        loop {
            if let Some(id) = self.daemon_id.lock().await.clone() {
                return Ok(id);
            }
            if let Some(error) = self.handshake_error.lock().await.clone() {
                return Err(ClientError::Handshake(error));
            }
            if self.closed.load(Ordering::Acquire) {
                return Err(ClientError::PeerClosed);
            }
            let notified = self.hello_notify.notified();
            if let Some(id) = self.daemon_id.lock().await.clone() {
                return Ok(id);
            }
            tokio::time::timeout(Duration::from_secs(5), notified)
                .await
                .map_err(|_| ClientError::Handshake("ServerHello timed out".into()))?;
        }
    }

    pub async fn client_id(&self) -> Option<String> {
        self.client_id.lock().await.clone()
    }

    pub async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, ClientError> {
        let request_id = request.request_id.clone();
        let frame = CoreFrame::Request(request);
        let payload = serde_json::to_vec(&frame)?;
        let (tx, rx) = oneshot::channel();
        let permit = Arc::clone(&self.request_slots)
            .acquire_owned()
            .await
            .map_err(|_| ClientError::RequestCapacityClosed)?;
        match self.pending.entry(request_id.clone()) {
            Entry::Occupied(_) => return Err(ClientError::RequestConflict),
            Entry::Vacant(entry) => {
                entry.insert((tx, permit));
            }
        }
        let _pending_guard = PendingRequestGuard {
            request_id: request_id.clone(),
            pending: Arc::clone(&self.pending),
        };
        self.write_line(&payload).await?;
        rx.await.map_err(|_| ClientError::WaiterCancelled)?
    }

    pub fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (tx, rx) = mpsc::channel(EVENT_CAPACITY);
        let mut events = self.events.subscribe();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => {
                        if tx.send(event).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        tracing::warn!(dropped, "native client event subscriber lagged");
                    }
                }
            }
        });
        rx
    }

    pub async fn subscribe_session_events(
        &self,
        session_id: String,
        from_event_seq: Option<u64>,
    ) -> Result<(), ClientError> {
        let client_id = self
            .client_id()
            .await
            .ok_or_else(|| ClientError::Handshake("ServerHello not received".into()))?;
        let frame = CoreFrame::Subscribe {
            client_id,
            session_id: Some(session_id),
            from_event_seq,
        };
        let payload = serde_json::to_vec(&frame)?;
        self.write_line(&payload).await
    }

    async fn send_hello(&self) -> Result<(), ClientError> {
        let frame = CoreFrame::ClientHello(ClientHello {
            client_name: self.descriptor.client_name.clone(),
            client_kind: self.descriptor.client_kind.clone(),
            protocol_version: self.descriptor.protocol_version,
            capabilities: self.descriptor.capabilities.clone(),
        });
        self.write_line(&serde_json::to_vec(&frame)?).await
    }

    async fn write_line(&self, bytes: &[u8]) -> Result<(), ClientError> {
        let mut guard = self.writer.lock().await;
        let writer = guard.as_mut().ok_or(ClientError::PeerClosed)?;
        writer
            .write_all(bytes)
            .await
            .map_err(ClientError::Transport)?;
        writer
            .write_all(b"\n")
            .await
            .map_err(ClientError::Transport)?;
        writer.flush().await.map_err(ClientError::Transport)
    }

    fn spawn_reader(
        &self,
        mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
        pending: Arc<DashMap<String, PendingRequest>>,
        events: broadcast::Sender<EventEnvelope<CoreEvent>>,
    ) {
        let writer = Arc::clone(&self.writer);
        let client_id = Arc::clone(&self.client_id);
        let daemon_id = Arc::clone(&self.daemon_id);
        let handshake_error = Arc::clone(&self.handshake_error);
        let hello_notify = Arc::clone(&self.hello_notify);
        let closed = Arc::clone(&self.closed);
        let connection_generation = Arc::clone(&self.connection_generation);
        let generation = connection_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let task = tokio::spawn(async move {
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) if line.trim().is_empty() => continue,
                    Ok(_) => match serde_json::from_str::<CoreFrame>(line.trim()) {
                        Ok(CoreFrame::Response {
                            request_id,
                            response,
                        }) => {
                            if let Some((_, (waiter, _permit))) = pending.remove(&request_id) {
                                let _ = waiter.send(Ok(*response));
                            }
                        }
                        Ok(CoreFrame::Event(event)) => {
                            let _ = events.send(event);
                        }
                        Ok(CoreFrame::ServerHello(hello)) => {
                            if hello.protocol_version != codegg_protocol::core::PROTOCOL_VERSION {
                                let error = format!(
                                    "protocol version mismatch (daemon {}, client {})",
                                    hello.protocol_version,
                                    codegg_protocol::core::PROTOCOL_VERSION
                                );
                                tracing::warn!(%error, "native client protocol version mismatch");
                                *handshake_error.lock().await = Some(error);
                                break;
                            }
                            *daemon_id.lock().await = Some(hello.daemon_id);
                            *client_id.lock().await = Some(hello.client_id.clone());
                            hello_notify.notify_waiters();
                            let subscribe = CoreFrame::Subscribe {
                                client_id: hello.client_id,
                                session_id: None,
                                from_event_seq: Some(0),
                            };
                            let Ok(bytes) = serde_json::to_vec(&subscribe) else {
                                break;
                            };
                            let mut guard = writer.lock().await;
                            let Some(writer) = guard.as_mut() else { break };
                            if writer.write_all(&bytes).await.is_err()
                                || writer.write_all(b"\n").await.is_err()
                                || writer.flush().await.is_err()
                            {
                                break;
                            }
                        }
                        Ok(CoreFrame::Ping) => {
                            let Ok(bytes) = serde_json::to_vec(&CoreFrame::Pong) else {
                                break;
                            };
                            let mut guard = writer.lock().await;
                            if let Some(writer) = guard.as_mut() {
                                let _ = writer.write_all(&bytes).await;
                                let _ = writer.write_all(b"\n").await;
                                let _ = writer.flush().await;
                            }
                        }
                        Ok(_) => {}
                        Err(error) => tracing::warn!(%error, "invalid native protocol frame"),
                    },
                    Err(error) => {
                        tracing::warn!(%error, "native client reader failed");
                        break;
                    }
                }
            }
            if connection_generation.load(Ordering::Acquire) == generation {
                closed.store(true, Ordering::Release);
                *writer.lock().await = None;
                fail_pending_waiters(&pending);
                hello_notify.notify_waiters();
            }
        });
        if let Ok(mut slot) = self.reader_task.lock() {
            if let Some(previous) = slot.replace(task) {
                previous.abort();
            }
        }
    }

    fn abort_reader_task(&self) {
        if let Ok(mut task) = self.reader_task.lock() {
            if let Some(task) = task.take() {
                task.abort();
            }
        }
    }

    fn fail_pending(&self) {
        fail_pending_waiters(&self.pending);
    }
}

impl Drop for LocalSocketClient {
    fn drop(&mut self) {
        if Arc::strong_count(&self.reader_task) != 1 {
            return;
        }
        self.abort_reader_task();
        self.fail_pending();
    }
}

fn fail_pending_waiters(pending: &DashMap<String, PendingRequest>) {
    let pending_ids: Vec<_> = pending.iter().map(|entry| entry.key().clone()).collect();
    for id in pending_ids {
        if let Some((_, (waiter, _permit))) = pending.remove(&id) {
            let _ = waiter.send(Err(ClientError::PeerClosed));
        }
    }
}

async fn open_unix(endpoint: &str) -> Result<UnixStream, ClientError> {
    let path = endpoint.strip_prefix("unix://").unwrap_or(endpoint);
    UnixStream::connect(path)
        .await
        .map_err(ClientError::Connect)
}
