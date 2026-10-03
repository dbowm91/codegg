use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSnapshot {
    pub state: String,
    pub daemon_id: Option<String>,
    pub protocol_version: Option<u32>,
    pub uptime_seconds: Option<u64>,
    pub active_sessions: Option<usize>,
    pub error: Option<String>,
    pub connection_generation: u64,
}

impl ConnectionSnapshot {
    pub fn disconnected(error: Option<String>, connection_generation: u64) -> Self {
        Self {
            state: "disconnected".into(),
            daemon_id: None,
            protocol_version: None,
            uptime_seconds: None,
            active_sessions: None,
            error,
            connection_generation,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub project_id: String,
    pub display_name: String,
    pub lifecycle: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopEvent {
    pub version: u8,
    pub event_seq: u64,
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionInfo {
    pub subscription_id: String,
    pub connection_generation: u64,
}

/// Explicit desktop route identity (M004 WP B). Renderers echo
/// `route_generation` back on mutating calls; the host drops any
/// completion for a superseded generation. Canonical workspace roots
/// never appear in this surface.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTokenView {
    pub connection_generation: u64,
    pub route_generation: u64,
    pub project_id: String,
    pub workspace_id: String,
    pub session_id: Option<String>,
}

/// Workspace display entry. The canonical root backing this workspace
/// stays Rust-host data and is not part of this DTO.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceView {
    pub workspace_id: String,
    pub display_name: String,
}

/// Authorized project detail: workspaces from `ProjectGet`, never from
/// path enumeration.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetailView {
    pub project_id: String,
    pub display_name: String,
    pub workspaces: Vec<WorkspaceView>,
    pub session_count: usize,
    pub route_token: RouteTokenView,
}

/// Bounded session summary. Identity only; no message bodies.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryView {
    pub session_id: String,
    pub title: String,
    pub project_id: String,
    pub workspace_id: Option<String>,
}

/// Bounded session list for the current route.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListView {
    pub sessions: Vec<SessionSummaryView>,
    pub route_token: RouteTokenView,
}

/// Opened/created session bound to the captured route.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub session: SessionSummaryView,
    pub route_token: RouteTokenView,
}
