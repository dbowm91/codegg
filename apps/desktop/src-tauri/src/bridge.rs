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
}

impl ConnectionSnapshot {
    pub fn disconnected(error: Option<String>) -> Self {
        Self {
            state: "disconnected".into(),
            daemon_id: None,
            protocol_version: None,
            uptime_seconds: None,
            active_sessions: None,
            error,
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
