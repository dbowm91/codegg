use serde::{Deserialize, Serialize};

use crate::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope};

/// Capability identifier advertised by the additive Project Catalog protocol
/// family. Kept separate from the legacy hello struct fields so old callers
/// that construct those structs remain source-compatible.
pub const PROJECT_CATALOG_CAPABILITY: &str = "project_catalog.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectCatalogCapabilities {
    pub supported: bool,
    pub max_list_items: usize,
    pub max_workspaces_per_project: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientHello {
    pub client_name: String,
    pub client_kind: ClientKind,
    pub protocol_version: u32,
    pub capabilities: ClientCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientKind {
    Tui,
    Gui,
    Web,
    Cli,
    Automation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    pub visual_notifications: bool,
    pub desktop_notifications: bool,
    pub audio: bool,
    pub tts: bool,
    pub multi_session_view: bool,
    #[serde(default)]
    pub plugin_ui_dialog: bool,
    #[serde(default)]
    pub plugin_ui_toast: bool,
    #[serde(default)]
    pub plugin_ui_panel: bool,
    #[serde(default)]
    pub plugin_ui_status_item: bool,
    #[serde(default)]
    pub plugin_ui_table: bool,
    #[serde(default)]
    pub plugin_ui_markdown: bool,
    #[serde(default)]
    pub plugin_ui_code: bool,
    #[serde(default)]
    pub plugin_ui_progress: bool,
    /// Phase 2: client understands workspace registration requests and the
    /// `WorkspaceList`/`WorkspaceSnapshot` response variants.
    #[serde(default)]
    pub workspace_registration: bool,
    /// Project Catalog M004: client understands bounded project catalog
    /// operations and project-scoped lifecycle/health events.
    #[serde(default)]
    pub project_catalog: bool,
}

impl ClientCapabilities {
    pub fn plugin_ui_capabilities(&self) -> crate::ui::PluginUiCapabilities {
        crate::ui::PluginUiCapabilities {
            dialog: self.plugin_ui_dialog,
            toast: self.plugin_ui_toast,
            panel: self.plugin_ui_panel,
            status_item: self.plugin_ui_status_item,
            table: self.plugin_ui_table,
            markdown: self.plugin_ui_markdown,
            code: self.plugin_ui_code,
            progress: self.plugin_ui_progress,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerHello {
    pub daemon_id: String,
    pub protocol_version: u32,
    pub server_capabilities: ServerCapabilities,
    /// Negotiated client_id assigned by the daemon. The client should use this
    /// when sending `Subscribe` frames so the daemon's `ClientRegistry` can
    /// record the negotiated identity (instead of trusting a client-supplied id).
    /// This is a wire-protocol addition; older clients that ignore it are still
    /// forward-compatible with the daemon.
    #[serde(default)]
    pub client_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub event_replay: bool,
    pub session_management: bool,
    pub permission_routing: bool,
    /// Phase 2: daemon supports workspace registration and snapshots.
    /// Clients without this capability fall back to the legacy
    /// `SnapshotWorkspace { project_dir }` flow.
    #[serde(default)]
    pub workspace_registration: bool,
    /// Phase 2: daemon emits `WorkspaceSnapshot` records in turn snapshots
    /// when available.
    #[serde(default)]
    pub workspace_snapshots: bool,
    /// Phase 4: daemon supports durable job submission, listing, and
    /// cancellation through the new protocol variants.
    #[serde(default)]
    pub durable_jobs: bool,
    /// Phase 4: daemon supports durable schedules.
    #[serde(default)]
    pub durable_schedules: bool,
    /// Daemon and client understand canonical project/workspace context and
    /// session bindings in addition to legacy string projections.
    #[serde(default)]
    pub identity_aware_context: bool,
    /// Project Catalog M004: daemon supports the bounded project catalog
    /// request/response/event family.
    #[serde(default)]
    pub project_catalog: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_server_capabilities_fixture_defaults_identity_awareness() {
        let capabilities: ServerCapabilities = serde_json::from_str(
            r#"{
                "event_replay":true,
                "session_management":true,
                "permission_routing":false
            }"#,
        )
        .unwrap();

        assert!(!capabilities.identity_aware_context);
    }

    #[test]
    fn identity_aware_server_capability_round_trips() {
        let capabilities = ServerCapabilities {
            event_replay: true,
            session_management: true,
            permission_routing: true,
            workspace_registration: true,
            workspace_snapshots: true,
            durable_jobs: false,
            durable_schedules: false,
            identity_aware_context: true,
            project_catalog: true,
        };

        let json = serde_json::to_string(&capabilities).unwrap();
        assert!(json.contains("identity_aware_context"));
        let decoded: ServerCapabilities = serde_json::from_str(&json).unwrap();
        assert!(decoded.identity_aware_context);
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreFrame {
    Request(RequestEnvelope<CoreRequest>),
    Response {
        request_id: String,
        response: Box<CoreResponse>,
    },
    Subscribe {
        client_id: String,
        session_id: Option<String>,
        from_event_seq: Option<u64>,
    },
    Event(EventEnvelope<CoreEvent>),
    Error {
        request_id: Option<String>,
        code: String,
        message: String,
    },
    ClientHello(ClientHello),
    ServerHello(ServerHello),
    Ping,
    Pong,
}
