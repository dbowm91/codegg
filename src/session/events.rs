//! Typed session events for structured TUI state reconstruction.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Metadata attached to every session event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMeta {
    pub id: String,
    pub session_id: String,
    pub created_at: DateTime<Utc>,
}

impl EventMeta {
    pub fn new(session_id: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            created_at: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ToolRisk {
    Read,
    Write,
    GitMutation,
    DependencyMutation,
    Network,
    Destructive,
    CredentialAdjacent,
    Unknown,
}

impl std::fmt::Display for ToolRisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolRisk::Read => write!(f, "read-only"),
            ToolRisk::Write => write!(f, "workspace-write"),
            ToolRisk::GitMutation => write!(f, "git-mutation"),
            ToolRisk::DependencyMutation => write!(f, "dependency-mutation"),
            ToolRisk::Network => write!(f, "network"),
            ToolRisk::Destructive => write!(f, "destructive"),
            ToolRisk::CredentialAdjacent => write!(f, "credential-adjacent"),
            ToolRisk::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ToolCallStatus {
    Running,
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileChangeKind {
    Created,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlanItemStatus {
    Pending,
    InProgress,
    Done,
    Skipped,
    Blocked,
}

// ---------------------------------------------------------------------------
// Plan types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlanItem {
    pub id: String,
    pub text: String,
    pub status: PlanItemStatus,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlan {
    pub items: Vec<AgentPlanItem>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Event payload structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSetEvent {
    pub meta: EventMeta,
    pub goal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanUpdatedEvent {
    pub meta: EventMeta,
    pub plan: AgentPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItemUpdatedEvent {
    pub meta: EventMeta,
    pub item_id: String,
    pub status: PlanItemStatus,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessageEvent {
    pub meta: EventMeta,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessageEvent {
    pub meta: EventMeta,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallStartedEvent {
    pub meta: EventMeta,
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub risk: ToolRisk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFinishedEvent {
    pub meta: EventMeta,
    pub tool_call_id: String,
    pub tool_name: String,
    pub status: ToolCallStatus,
    pub duration_ms: Option<u64>,
    pub output_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequestedEvent {
    pub meta: EventMeta,
    pub tool_name: String,
    pub path: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResolvedEvent {
    pub meta: EventMeta,
    pub tool_name: String,
    pub allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChangedEvent {
    pub meta: EventMeta,
    pub path: String,
    pub kind: FileChangeKind,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunStartedEvent {
    pub meta: EventMeta,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunFinishedEvent {
    pub meta: EventMeta,
    pub command: String,
    pub passed: bool,
    pub duration_ms: Option<u64>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCompactedEvent {
    pub meta: EventMeta,
    pub messages_removed: usize,
    pub messages_remaining: usize,
    pub token_estimate_before: Option<usize>,
    pub token_estimate_after: Option<usize>,
    pub pinned_items: Vec<String>,
    pub summarized_items: Vec<String>,
    pub dropped_items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRoutedEvent {
    pub meta: EventMeta,
    pub model: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentStartedEvent {
    pub meta: EventMeta,
    pub subagent_id: String,
    pub agent: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentFinishedEvent {
    pub meta: EventMeta,
    pub subagent_id: String,
    pub agent: String,
    pub success: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingRaisedEvent {
    pub meta: EventMeta,
    pub severity: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointCreatedEvent {
    pub meta: EventMeta,
    pub checkpoint_id: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExportedEvent {
    pub meta: EventMeta,
    pub format: String,
}

// ---------------------------------------------------------------------------
// Top-level enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionEvent {
    GoalSet(GoalSetEvent),
    PlanUpdated(PlanUpdatedEvent),
    PlanItemUpdated(PlanItemUpdatedEvent),
    AgentMessage(AgentMessageEvent),
    UserMessage(UserMessageEvent),
    ToolCallStarted(ToolCallStartedEvent),
    ToolCallFinished(ToolCallFinishedEvent),
    PermissionRequested(PermissionRequestedEvent),
    PermissionResolved(PermissionResolvedEvent),
    FileChanged(FileChangedEvent),
    TestRunStarted(TestRunStartedEvent),
    TestRunFinished(TestRunFinishedEvent),
    ContextCompacted(ContextCompactedEvent),
    ModelRouted(ModelRoutedEvent),
    SubagentStarted(SubagentStartedEvent),
    SubagentFinished(SubagentFinishedEvent),
    FindingRaised(FindingRaisedEvent),
    CheckpointCreated(CheckpointCreatedEvent),
    SessionExported(SessionExportedEvent),
}

impl SessionEvent {
    /// Returns the event type as a string tag for database storage.
    pub fn event_type_tag(&self) -> &'static str {
        match self {
            SessionEvent::GoalSet(_) => "goal_set",
            SessionEvent::PlanUpdated(_) => "plan_updated",
            SessionEvent::PlanItemUpdated(_) => "plan_item_updated",
            SessionEvent::AgentMessage(_) => "agent_message",
            SessionEvent::UserMessage(_) => "user_message",
            SessionEvent::ToolCallStarted(_) => "tool_call_started",
            SessionEvent::ToolCallFinished(_) => "tool_call_finished",
            SessionEvent::PermissionRequested(_) => "permission_requested",
            SessionEvent::PermissionResolved(_) => "permission_resolved",
            SessionEvent::FileChanged(_) => "file_changed",
            SessionEvent::TestRunStarted(_) => "test_run_started",
            SessionEvent::TestRunFinished(_) => "test_run_finished",
            SessionEvent::ContextCompacted(_) => "context_compacted",
            SessionEvent::ModelRouted(_) => "model_routed",
            SessionEvent::SubagentStarted(_) => "subagent_started",
            SessionEvent::SubagentFinished(_) => "subagent_finished",
            SessionEvent::FindingRaised(_) => "finding_raised",
            SessionEvent::CheckpointCreated(_) => "checkpoint_created",
            SessionEvent::SessionExported(_) => "session_exported",
        }
    }

    /// Returns a reference to the event's metadata.
    pub fn meta(&self) -> &EventMeta {
        match self {
            SessionEvent::GoalSet(e) => &e.meta,
            SessionEvent::PlanUpdated(e) => &e.meta,
            SessionEvent::PlanItemUpdated(e) => &e.meta,
            SessionEvent::AgentMessage(e) => &e.meta,
            SessionEvent::UserMessage(e) => &e.meta,
            SessionEvent::ToolCallStarted(e) => &e.meta,
            SessionEvent::ToolCallFinished(e) => &e.meta,
            SessionEvent::PermissionRequested(e) => &e.meta,
            SessionEvent::PermissionResolved(e) => &e.meta,
            SessionEvent::FileChanged(e) => &e.meta,
            SessionEvent::TestRunStarted(e) => &e.meta,
            SessionEvent::TestRunFinished(e) => &e.meta,
            SessionEvent::ContextCompacted(e) => &e.meta,
            SessionEvent::ModelRouted(e) => &e.meta,
            SessionEvent::SubagentStarted(e) => &e.meta,
            SessionEvent::SubagentFinished(e) => &e.meta,
            SessionEvent::FindingRaised(e) => &e.meta,
            SessionEvent::CheckpointCreated(e) => &e.meta,
            SessionEvent::SessionExported(e) => &e.meta,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_tag_coverage() {
        let session_id = "test-session";
        let meta = EventMeta::new(session_id);

        let events = vec![
            SessionEvent::GoalSet(GoalSetEvent {
                meta: meta.clone(),
                goal: "test".into(),
            }),
            SessionEvent::PlanUpdated(PlanUpdatedEvent {
                meta: meta.clone(),
                plan: AgentPlan {
                    items: vec![],
                    updated_at: Utc::now(),
                },
            }),
            SessionEvent::ToolCallStarted(ToolCallStartedEvent {
                meta: meta.clone(),
                tool_call_id: "tc-1".into(),
                tool_name: "bash".into(),
                arguments: "{}".into(),
                risk: ToolRisk::Write,
            }),
            SessionEvent::ToolCallFinished(ToolCallFinishedEvent {
                meta: meta.clone(),
                tool_call_id: "tc-1".into(),
                tool_name: "bash".into(),
                status: ToolCallStatus::Success,
                duration_ms: Some(100),
                output_preview: None,
            }),
            SessionEvent::FileChanged(FileChangedEvent {
                meta: meta.clone(),
                path: "src/main.rs".into(),
                kind: FileChangeKind::Modified,
                additions: Some(5),
                deletions: Some(2),
            }),
            SessionEvent::ContextCompacted(ContextCompactedEvent {
                meta: meta.clone(),
                messages_removed: 10,
                messages_remaining: 5,
                token_estimate_before: Some(10000),
                token_estimate_after: Some(3000),
                pinned_items: vec!["goal".into()],
                summarized_items: vec![],
                dropped_items: vec![],
            }),
            SessionEvent::ModelRouted(ModelRoutedEvent {
                meta: meta.clone(),
                model: "claude-sonnet-4-20250514".into(),
                reason: "complex task".into(),
            }),
        ];

        for event in &events {
            let tag = event.event_type_tag();
            assert!(!tag.is_empty());
            let m = event.meta();
            assert_eq!(m.session_id, session_id);
        }
    }

    #[test]
    fn test_serialization_roundtrip() {
        let meta = EventMeta::new("sess-1");
        let event = SessionEvent::GoalSet(GoalSetEvent {
            meta,
            goal: "Implement feature X".into(),
        });

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: SessionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.event_type_tag(), "goal_set");
    }
}
