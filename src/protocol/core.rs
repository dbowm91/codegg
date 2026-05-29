use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope<T> {
    pub protocol_version: u32,
    pub request_id: String,
    pub payload: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    pub protocol_version: u32,
    pub event_seq: u64,
    pub timestamp_ms: i64,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub payload: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreResponse {
    Ack,
    Json {
        data: serde_json::Value,
    },
    Session {
        session: crate::session::Session,
    },
    SessionMessages {
        session_id: String,
        messages: Vec<crate::session::message::Message>,
    },
    SessionMessageCounts {
        counts: std::collections::HashMap<String, usize>,
    },
    SessionList {
        sessions: Vec<crate::session::Session>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreRequest {
    Initialize,
    Subscribe {
        session_id: Option<String>,
    },
    Resume {
        session_id: Option<String>,
        from_event_seq: u64,
    },
    SessionList {
        project_id: String,
        show_archived: bool,
        limit: usize,
    },
    SessionCreate {
        directory: String,
        title: Option<String>,
    },
    SessionAttach {
        session_id: String,
    },
    SessionLoad {
        session_id: String,
    },
    SessionMessagesLoad {
        session_id: String,
    },
    SessionMessageCounts {
        session_ids: Vec<String>,
    },
    SessionFork {
        session_id: String,
    },
    SessionDelete {
        session_id: String,
        permanent: bool,
    },
    SessionArchive {
        session_id: String,
        unarchive: bool,
    },
    SessionRestore {
        session_id: String,
    },
    SessionShare {
        session_id: String,
    },
    SessionUnshare {
        session_id: String,
    },
    SessionRename {
        session_id: String,
        new_title: String,
    },
    SessionExport {
        session_id: String,
    },
    SessionImportData {
        data: serde_json::Value,
    },
    SessionCreateFromTemplate {
        template: crate::config::schema::SessionTemplate,
        project_id: String,
        directory: String,
    },
    TurnSubmit {
        session_id: String,
        text: String,
        plan_mode: bool,
        model: String,
        agents: Vec<crate::agent::Agent>,
        current_agent_idx: usize,
        messages: Vec<crate::provider::Message>,
    },
    TurnCancel {
        session_id: String,
        turn_id: String,
    },
    TurnSteer {
        session_id: String,
        turn_id: String,
        text: String,
    },
    AgentSelect {
        session_id: String,
        agent_name: String,
    },
    ModelSelect {
        session_id: String,
        model: String,
    },
    ModelsRefresh,
    PermissionRespond {
        id: String,
        choice: String,
    },
    QuestionRespond {
        id: String,
        answers: serde_json::Value,
    },
    MemorySearch {
        query: String,
    },
    MemoryList {
        namespace: String,
    },
    MemoryRemember {
        text: String,
        namespace: Option<String>,
    },
    MemoryForget {
        id: String,
    },
    TaskList,
    TaskSchedule {
        session_id: String,
        interval_secs: u64,
        message: String,
    },
    TaskDelete {
        id: u64,
    },
    WorktreeList {
        project_dir: String,
    },
    GoalSet {
        session_id: String,
        project_id: String,
        objective: String,
    },
    GoalFromFile {
        session_id: String,
        project_id: String,
        path: String,
    },
    GoalShow {
        session_id: String,
    },
    GoalPause {
        session_id: String,
    },
    GoalResume {
        session_id: String,
    },
    GoalClear {
        session_id: String,
    },
    GoalDone {
        session_id: String,
    },
    GoalCheckpoint {
        session_id: String,
        project_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreEvent {
    SnapshotSession {
        session_id: String,
    },
    SnapshotWorkspace {
        project_dir: String,
    },
    SnapshotModels {
        #[serde(skip_serializing_if = "Option::is_none")]
        current_model: Option<String>,
        models: Vec<String>,
    },
    TurnStarted {
        session_id: String,
        turn_id: String,
    },
    TurnTextDelta {
        session_id: String,
        turn_id: String,
        delta: String,
    },
    TurnReasoningDelta {
        session_id: String,
        turn_id: String,
        delta: String,
    },
    ToolStarted {
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        tool_name: String,
        tool_id: String,
        arguments: String,
    },
    ToolCompleted {
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        tool_id: String,
        output: String,
        success: bool,
    },
    PermissionPending {
        id: String,
        tool: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    QuestionPending {
        id: String,
        questions: serde_json::Value,
    },
    TurnCompleted {
        session_id: String,
        turn_id: String,
        stop_reason: String,
    },
    TurnFailed {
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        message: String,
    },
    SessionUpdated {
        session_id: String,
    },
    FileChanged {
        path: String,
        action: String,
    },
    SubagentStarted {
        session_id: String,
        task_id: u64,
        agent: String,
        description: String,
    },
    SubagentProgress {
        session_id: String,
        task_id: u64,
        agent: String,
        message: String,
    },
    SubagentCompleted {
        session_id: String,
        task_id: u64,
        agent: String,
        result_summary: String,
    },
    SubagentFailed {
        session_id: String,
        task_id: u64,
        agent: String,
        error: String,
    },
    Error {
        code: String,
        message: String,
    },
}
