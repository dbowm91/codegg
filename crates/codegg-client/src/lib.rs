//! Frontend-side native protocol client contracts.
//!
//! This crate depends on the frontend-neutral protocol plus transport utilities;
//! it contains no daemon, UI, provider, or authorization implementation.

use codegg_protocol::frames::{ClientCapabilities, ClientKind};

mod compose;
mod connect;
mod driver;
mod local;
mod paths;
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_pipe_security;
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_process;

pub use compose::{
    compose_turn_submit, ComposerError, PromptIntent, PromptIntentState, TurnSubmitInput,
    MAX_PROMPT_TEXT_CHARS,
};
pub use connect::{connect_or_start_local_daemon, LocalDaemonOptions, LocalDaemonOutcome};
pub use driver::{
    DriverConfig, DriverError, DriverSnapshotView, DriverState, SessionProjectionDriver,
    StoppedDriver,
};
pub use local::{ClientEvent, LocalSocketClient};
pub use paths::{LocalDaemonPaths, LocalEndpoint};
#[cfg(windows)]
#[doc(hidden)]
pub use windows_pipe_security::PipeSecurity;
#[cfg(windows)]
#[doc(hidden)]
pub use windows_process::is_process_alive as windows_is_process_alive;

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
    #[error("unsupported local daemon endpoint: {0}")]
    InvalidEndpoint(String),
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

    #[test]
    fn local_endpoint_uses_only_platform_local_schemes() {
        #[cfg(unix)]
        let (input, expected_uri, expected_argument) = (
            "unix:///tmp/codegg.sock",
            "unix:///tmp/codegg.sock",
            "/tmp/codegg.sock",
        );
        #[cfg(windows)]
        let (input, expected_uri, expected_argument) = (
            "npipe://codegg-test",
            "npipe://codegg-test",
            r"\\.\pipe\codegg-test",
        );
        let endpoint = LocalEndpoint::parse(input).unwrap();
        assert_eq!(endpoint.as_uri(), expected_uri);
        assert_eq!(endpoint.native_argument(), expected_argument);
        assert!(LocalEndpoint::parse("tcp://127.0.0.1:9000").is_err());
    }
}
