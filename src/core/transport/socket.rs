use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::core::CoreClient;
use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope};
use crate::protocol::frames::{ClientCapabilities, ClientKind};
use codegg_client::{FrontendDescriptor, LocalSocketClient};

/// Root compatibility adapter for the reusable frontend-side transport.
#[derive(Clone)]
pub struct SocketCoreClient {
    inner: LocalSocketClient,
}

impl SocketCoreClient {
    pub(crate) fn from_client(inner: LocalSocketClient) -> Self {
        Self { inner }
    }

    pub async fn connect(endpoint: &str) -> Result<Self, AppError> {
        Self::connect_with_descriptor(
            endpoint,
            FrontendDescriptor::new(
                "codegg-tui",
                ClientKind::Tui,
                Self::tui_client_capabilities(),
            ),
        )
        .await
    }

    pub async fn connect_with_descriptor(
        endpoint: &str,
        descriptor: FrontendDescriptor,
    ) -> Result<Self, AppError> {
        let inner = LocalSocketClient::connect(endpoint, descriptor)
            .await
            .map_err(client_error)?;
        Ok(Self { inner })
    }

    pub async fn reconnect(&self) -> Result<(), AppError> {
        self.inner.reconnect().await.map_err(client_error)
    }

    pub async fn daemon_id(&self) -> Result<String, AppError> {
        self.inner.daemon_id().await.map_err(client_error)
    }

    pub async fn client_id(&self) -> Option<String> {
        self.inner.client_id().await
    }

    pub async fn subscribe_session_events(
        &self,
        session_id: String,
        from_event_seq: Option<u64>,
    ) -> Result<(), AppError> {
        self.inner
            .subscribe_session_events(session_id, from_event_seq)
            .await
            .map_err(client_error)
    }

    /// Capabilities historically advertised by the TUI composition.
    pub fn tui_client_capabilities() -> ClientCapabilities {
        ClientCapabilities {
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
        }
    }
}

fn client_error(error: codegg_client::ClientError) -> AppError {
    AppError::Other(anyhow::anyhow!(error.to_string()))
}

#[async_trait]
impl CoreClient for SocketCoreClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        self.inner.request(request).await.map_err(client_error)
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        self.inner.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::frames::CoreFrame;
    use crate::protocol::frames::{ServerCapabilities, ServerHello};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    #[tokio::test(flavor = "current_thread")]
    async fn gui_descriptor_is_sent_during_native_handshake() {
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
            reader.read_line(&mut line).await.expect("read hello");
            let CoreFrame::ClientHello(hello) =
                serde_json::from_str(line.trim()).expect("decode hello")
            else {
                panic!("first frame must be ClientHello");
            };
            assert_eq!(hello.client_name, "codegg-desktop-test");
            assert!(matches!(hello.client_kind, ClientKind::Gui));
            assert!(hello.capabilities.multi_session_view);
            let response = CoreFrame::ServerHello(ServerHello {
                daemon_id: "gui-test-daemon".into(),
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
                client_id: "gui-test-client".into(),
            });
            write_half
                .write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes())
                .await
                .expect("write server hello");
            write_half.flush().await.expect("flush server hello");
            hello.client_name
        });

        let mut capabilities = SocketCoreClient::tui_client_capabilities();
        capabilities.multi_session_view = true;
        let descriptor =
            FrontendDescriptor::new("codegg-desktop-test", ClientKind::Gui, capabilities);
        let client = SocketCoreClient::connect_with_descriptor(
            &format!("unix://{}", socket.display()),
            descriptor,
        )
        .await
        .expect("connect gui test client");
        assert_eq!(client.client_id().await.as_deref(), Some("gui-test-client"));
        assert_eq!(server.await.expect("server fixture"), "codegg-desktop-test");
        let _ = std::fs::remove_file(socket);
    }

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
