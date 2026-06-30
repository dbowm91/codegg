pub const REMOTE_TUI_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(tag = "type")]
#[allow(clippy::large_enum_variant)]
pub enum TuiMessage {
    EventEnvelope {
        event_seq: u64,
        payload: Box<TuiMessage>,
    },
    Input {
        text: String,
    },
    KeyDown {
        key: String,
        modifiers: Vec<String>,
    },
    MouseClick {
        x: u16,
        y: u16,
    },
    Resize {
        w: u16,
        h: u16,
    },
    Resume {
        from_event_seq: u64,
    },
    PermissionResponse {
        id: String,
        choice: String,
    },
    QuestionResponse {
        id: String,
        answers: serde_json::Value,
    },
    RenderFrame {
        content: String,
    },
    TextDelta {
        delta: String,
    },
    PermissionPending {
        id: String,
        tool: String,
        path: Option<String>,
    },
    QuestionPending {
        id: String,
        questions: Vec<QuestionSpec>,
    },
    SessionInfo {
        id: String,
        model: String,
    },
    SessionEnded {
        stop_reason: String,
    },
    ToolCallStarted {
        tool_name: String,
        tool_id: String,
        arguments: String,
    },
    ToolResult {
        tool_id: String,
        output: String,
        success: bool,
    },
    Error {
        message: String,
    },
    StateSnapshot {
        sequence: u64,
        snapshot: RemoteTuiStateSnapshot,
    },
    RequestSnapshot {
        reason: Option<String>,
    },
    #[serde(rename = "resync_required")]
    ResyncRequired {
        reason: Option<String>,
        pending_permissions: Vec<String>,
        pending_questions: Vec<String>,
    },
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct QuestionSpec {
    pub id: String,
    pub prompt: String,
    pub default: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RemoteTuiStateSnapshot {
    pub protocol_version: u32,
    pub sequence: u64,
    pub session_id: Option<String>,
    pub route: String,
    pub model: String,
    pub agent: String,
    pub status: String,
    pub messages: Vec<RemoteMessageView>,
    pub prompt: String,
    pub dialog: Option<String>,
    pub toasts: Vec<RemoteToastView>,
    /// Cached git sidebar state (root, branch, dirty). Refreshed
    /// asynchronously on the server; the value here is the most
    /// recent successful refresh.
    pub git: Option<RemoteGitInfo>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RemoteGitInfo {
    pub root: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RemoteMessageView {
    pub role: String,
    pub content_preview: String,
    pub tool_calls: Vec<RemoteToolCallView>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RemoteToolCallView {
    pub tool_id: String,
    pub tool_name: String,
    pub status: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RemoteToastView {
    pub message: String,
    pub level: String,
}
