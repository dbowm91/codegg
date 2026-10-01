//! Frontend-side native protocol client contracts.
//!
//! This crate depends on the frontend-neutral protocol plus transport utilities;
//! it contains no daemon, UI, provider, or authorization implementation.

use codegg_protocol::frames::{ClientCapabilities, ClientKind};

mod connect;
mod local;
mod paths;

pub use connect::{connect_or_start_local_daemon, LocalDaemonOptions, LocalDaemonOutcome};
pub use local::LocalSocketClient;
pub use paths::LocalDaemonPaths;

/// Trusted composition-time identity sent in `ClientHello`.
#[derive(Debug, Clone)]
pub struct FrontendDescriptor {
    client_name: String,
    client_kind: ClientKind,
    capabilities: ClientCapabilities,
    protocol_version: u32,
}

impl FrontendDescriptor {
    pub fn new(
        client_name: impl Into<String>,
        client_kind: ClientKind,
        capabilities: ClientCapabilities,
    ) -> Self {
        Self {
            client_name: client_name.into(),
            client_kind,
            capabilities,
            protocol_version: codegg_protocol::core::PROTOCOL_VERSION,
        }
    }

    pub fn client_name(&self) -> &str {
        &self.client_name
    }

    pub fn client_kind(&self) -> &ClientKind {
        &self.client_kind
    }

    pub fn capabilities(&self) -> &ClientCapabilities {
        &self.capabilities
    }

    pub fn protocol_version(&self) -> u32 {
        self.protocol_version
    }
}

/// Bounded error classes shared by frontend adapters.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("local transport connection failed: {0}")]
    Connect(#[source] std::io::Error),
    #[error("client handshake failed: {0}")]
    Handshake(String),
    #[error("native protocol serialization failed: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error("daemon peer closed the connection")]
    PeerClosed,
    #[error("native transport operation failed: {0}")]
    Transport(#[source] std::io::Error),
    #[error("response waiter was cancelled")]
    WaiterCancelled,
    #[error("request id is already pending on this connection")]
    RequestConflict,
    #[error("client is no longer accepting requests")]
    RequestCapacityClosed,
    #[error("daemon startup timed out")]
    StartupTimeout,
    #[error("daemon singleton state is inconsistent: {0}")]
    InconsistentSingleton(String),
    #[error("launched daemon exited before becoming ready: {0}")]
    ChildExited(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities() -> ClientCapabilities {
        ClientCapabilities {
            visual_notifications: false,
            desktop_notifications: false,
            audio: false,
            tts: false,
            multi_session_view: true,
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

    #[test]
    fn descriptor_supports_gui_identity_and_current_protocol() {
        let descriptor = FrontendDescriptor::new("codegg-desktop", ClientKind::Gui, capabilities());
        assert_eq!(descriptor.client_name(), "codegg-desktop");
        assert!(matches!(descriptor.client_kind(), ClientKind::Gui));
        assert!(descriptor.capabilities().multi_session_view);
        assert_eq!(
            descriptor.protocol_version(),
            codegg_protocol::core::PROTOCOL_VERSION
        );
    }
}
