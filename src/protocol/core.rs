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
    SnapshotSession {
        event_seq: u64,
        session: crate::session::Session,
        messages: Vec<crate::session::message::Message>,
        status: String,
        selected_model: Option<String>,
        selected_agent: Option<String>,
        pending_permissions: Vec<String>,
        pending_questions: Vec<String>,
        input_tokens: Option<usize>,
        output_tokens: Option<usize>,
        active_subagents: usize,
    },
    SnapshotDaemon {
        event_seq: u64,
        daemon_id: String,
        uptime_secs: u64,
        active_sessions: Vec<SessionSnapshot>,
        connected_clients: Vec<ClientSnapshot>,
    },
    ModelsSnapshot {
        current_model: Option<String>,
        models: Vec<String>,
    },
    Events {
        events: Vec<EventEnvelope<CoreEvent>>,
        current_seq: u64,
    },
    ResyncRequired {
        from_event_seq: u64,
        current_seq: u64,
        session_id: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub project_id: String,
    pub status: String,
    pub selected_model: Option<String>,
    pub selected_agent: Option<String>,
    pub has_active_turn: bool,
    pub pending_permissions: Vec<String>,
    pub pending_questions: Vec<String>,
    pub input_tokens: Option<usize>,
    pub output_tokens: Option<usize>,
    pub active_subagents: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSnapshot {
    pub client_id: String,
    pub client_name: String,
    pub connected_at: String,
    pub attached_sessions: Vec<String>,
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
    /// Load the persisted todo list for a session so the TUI can render
    /// it without keeping a separate `Arc<Mutex<TodoState>>` in sync.
    TodoList { session_id: String },
    /// Load the active goal snapshot (and progress) for a session.
    ActiveGoalLoad { session_id: String },
    /// Set or replace the goal budget. The store revives a
    /// `BudgetLimited` goal to `Active` if the new budget is high
    /// enough to satisfy the existing usage.
    GoalSetBudget {
        session_id: String,
        max_turns: Option<i64>,
        max_model_tokens: Option<i64>,
        max_tool_calls: Option<i64>,
        max_wallclock_secs: Option<i64>,
    },
    SnapshotSession { session_id: String },
    SnapshotWorkspace { project_dir: String },
    SnapshotModels,
    SnapshotDaemon,
    /// Route a TTS speak request through the daemon's `NotificationRouter`
    /// rather than speaking locally. Used in `RemoteCore` mode where the
    /// local TUI has no audio output of its own.
    ///
    /// `kind` and `priority` are optional string labels (`turn_completed`,
    /// `turn_failed`, `awaiting_input`, `permission_required`,
    /// `question_required`, `subagent_completed`, `subagent_failed`,
    /// `error`; and `low` / `normal` / `high` / `urgent` respectively).
    /// Unknown values fall back to a normal-priority `AwaitingInput`
    /// event so the router still surfaces the message.
    NotificationSpeak {
        text: String,
        kind: Option<String>,
        priority: Option<String>,
        session_id: Option<String>,
    },
    /// Ask the daemon to stop any currently-active TTS playback
    /// (delegates to the `AudioArbiter` interrupt channel).
    NotificationStop,
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
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        tool: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    QuestionPending {
        id: String,
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_is_set() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }

    #[test]
    fn request_envelope_serializes() {
        let req = RequestEnvelope {
            protocol_version: 1,
            request_id: "test-1".to_string(),
            payload: CoreRequest::SessionCreate {
                directory: "/tmp".to_string(),
                title: Some("Test".to_string()),
            },
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("session_create"));
        assert!(json.contains("test-1"));
    }

    #[test]
    fn response_serializes_ack() {
        let resp = CoreResponse::Ack;
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("ack"));
    }

    #[test]
    fn response_serializes_error() {
        let resp = CoreResponse::Error {
            code: "test_error".to_string(),
            message: "test message".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("test_error"));
    }

    #[test]
    fn response_serializes_events() {
        let env = EventEnvelope {
            protocol_version: 1,
            event_seq: 7,
            timestamp_ms: 100,
            session_id: Some("s1".to_string()),
            turn_id: None,
            payload: CoreEvent::Error {
                code: "e".to_string(),
                message: "m".to_string(),
            },
        };
        let resp = CoreResponse::Events {
            events: vec![env],
            current_seq: 7,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"events\""));
        assert!(json.contains("\"current_seq\":7"));
        assert!(json.contains("\"event_seq\":7"));
        let back: CoreResponse = serde_json::from_str(&json).unwrap();
        match back {
            CoreResponse::Events { events, current_seq } => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].event_seq, 7);
                assert_eq!(current_seq, 7);
            }
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[test]
    fn response_serializes_resync_required() {
        let resp = CoreResponse::ResyncRequired {
            from_event_seq: 5,
            current_seq: 100,
            session_id: Some("s1".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"resync_required\""));
        assert!(json.contains("\"from_event_seq\":5"));
        assert!(json.contains("\"current_seq\":100"));
        let back: CoreResponse = serde_json::from_str(&json).unwrap();
        match back {
            CoreResponse::ResyncRequired {
                from_event_seq,
                current_seq,
                session_id,
            } => {
                assert_eq!(from_event_seq, 5);
                assert_eq!(current_seq, 100);
                assert_eq!(session_id.as_deref(), Some("s1"));
            }
            other => panic!("expected ResyncRequired, got {:?}", other),
        }
    }

    #[test]
    fn event_envelope_has_seq() {
        let env = EventEnvelope {
            protocol_version: 1,
            event_seq: 42,
            timestamp_ms: 1234567890,
            session_id: Some("s1".to_string()),
            turn_id: None,
            payload: CoreEvent::Error {
                code: "e".to_string(),
                message: "m".to_string(),
            },
        };
        assert_eq!(env.event_seq, 42);
        assert_eq!(env.session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn core_frame_tagged_correctly() {
        use crate::protocol::frames::CoreFrame;
        let frame = CoreFrame::Ping;
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("ping"));
    }
}
