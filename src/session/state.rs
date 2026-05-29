//! Derived TUI session state reconstructed from session events.

use super::events::{
    AgentPlan, FileChangeKind, SessionEvent, ToolCallStatus, ToolRisk,
};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Summary types for TUI display
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallSummary {
    pub tool_call_id: String,
    pub tool_name: String,
    pub risk: ToolRisk,
    pub status: ToolCallStatus,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub duration_ms: Option<u64>,
    pub output_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedFileSummary {
    pub path: String,
    pub kind: FileChangeKind,
    pub additions: u64,
    pub deletions: u64,
    pub changed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingSummary {
    pub severity: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u64>,
    pub raised_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentSummary {
    pub subagent_id: String,
    pub agent: String,
    pub description: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub success: Option<bool>,
    pub summary: Option<String>,
}

// ---------------------------------------------------------------------------
// Test state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TestState {
    Unknown,
    Stale,
    Running { command: String },
    Passed {
        command: String,
        duration_ms: Option<u64>,
    },
    Failed {
        command: String,
        duration_ms: Option<u64>,
        summary: String,
    },
}

impl Default for TestState {
    fn default() -> Self {
        TestState::Unknown
    }
}

// ---------------------------------------------------------------------------
// Context state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextItem {
    pub label: String,
    pub token_hint: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextState {
    pub messages_remaining: usize,
    pub token_estimate: Option<usize>,
    pub compact_count: usize,
    pub pinned_items: Vec<ContextItem>,
    pub summarized_items: Vec<ContextItem>,
    pub retrieved_items: Vec<ContextItem>,
    pub excluded_items: Vec<ContextItem>,
}

// ---------------------------------------------------------------------------
// Model state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelState {
    pub current_model: Option<String>,
    pub route_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// TuiSessionState
// ---------------------------------------------------------------------------

/// A fully derived session state that can be reconstructed from events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiSessionState {
    pub goal: Option<String>,
    pub plan: Option<AgentPlan>,
    pub active_tool_calls: Vec<ToolCallSummary>,
    pub recent_tool_calls: Vec<ToolCallSummary>,
    pub changed_files: Vec<ChangedFileSummary>,
    pub test_state: TestState,
    pub context_state: ContextState,
    pub model_state: ModelState,
    pub findings: Vec<FindingSummary>,
    pub subagents: Vec<SubagentSummary>,
}

impl Default for TuiSessionState {
    fn default() -> Self {
        Self {
            goal: None,
            plan: None,
            active_tool_calls: Vec::new(),
            recent_tool_calls: Vec::new(),
            changed_files: Vec::new(),
            test_state: TestState::Unknown,
            context_state: ContextState::default(),
            model_state: ModelState::default(),
            findings: Vec::new(),
            subagents: Vec::new(),
        }
    }
}

const MAX_RECENT_TOOL_CALLS: usize = 50;

impl TuiSessionState {
    /// Build the state by applying a list of events in order.
    pub fn from_events(events: &[SessionEvent]) -> Self {
        let mut state = Self::default();
        for event in events {
            state.apply_event(event);
        }
        state
    }

    /// Apply a single event, mutating the state in place.
    pub fn apply_event(&mut self, event: &SessionEvent) {
        match event {
            SessionEvent::GoalSet(e) => {
                self.goal = Some(e.goal.clone());
            }

            SessionEvent::PlanUpdated(e) => {
                self.plan = Some(e.plan.clone());
            }

            SessionEvent::PlanItemUpdated(e) => {
                if let Some(ref mut plan) = self.plan {
                    if let Some(item) = plan.items.iter_mut().find(|i| i.id == e.item_id) {
                        item.status = e.status.clone();
                        item.note = e.note.clone();
                    }
                    plan.updated_at = e.meta.created_at;
                }
            }

            SessionEvent::ToolCallStarted(e) => {
                let summary = ToolCallSummary {
                    tool_call_id: e.tool_call_id.clone(),
                    tool_name: e.tool_name.clone(),
                    risk: e.risk.clone(),
                    status: ToolCallStatus::Running,
                    started_at: Some(e.meta.created_at),
                    finished_at: None,
                    duration_ms: None,
                    output_preview: None,
                };
                self.active_tool_calls.push(summary);
            }

            SessionEvent::ToolCallFinished(e) => {
                if let Some(idx) = self
                    .active_tool_calls
                    .iter()
                    .position(|tc| tc.tool_call_id == e.tool_call_id)
                {
                    let mut tc = self.active_tool_calls.remove(idx);
                    tc.status = e.status.clone();
                    tc.finished_at = Some(e.meta.created_at);
                    tc.duration_ms = e.duration_ms;
                    tc.output_preview = e.output_preview.clone();
                    self.recent_tool_calls.push(tc);
                    if self.recent_tool_calls.len() > MAX_RECENT_TOOL_CALLS {
                        self.recent_tool_calls.remove(0);
                    }
                }
            }

            SessionEvent::FileChanged(e) => {
                // Mark test state as stale if it was passed
                if matches!(self.test_state, TestState::Passed { .. }) {
                    self.test_state = TestState::Stale;
                }
                // Merge with existing entry for the same path, or insert new.
                if let Some(existing) = self.changed_files.iter_mut().find(|f| f.path == e.path) {
                    existing.kind = e.kind.clone();
                    existing.additions = e.additions.unwrap_or(0);
                    existing.deletions = e.deletions.unwrap_or(0);
                    existing.changed_at = e.meta.created_at;
                } else {
                    self.changed_files.push(ChangedFileSummary {
                        path: e.path.clone(),
                        kind: e.kind.clone(),
                        additions: e.additions.unwrap_or(0),
                        deletions: e.deletions.unwrap_or(0),
                        changed_at: e.meta.created_at,
                    });
                }
            }

            SessionEvent::TestRunStarted(e) => {
                self.test_state = TestState::Running {
                    command: e.command.clone(),
                };
            }

            SessionEvent::TestRunFinished(e) => {
                self.test_state = if e.passed {
                    TestState::Passed {
                        command: e.command.clone(),
                        duration_ms: e.duration_ms,
                    }
                } else {
                    TestState::Failed {
                        command: e.command.clone(),
                        duration_ms: e.duration_ms,
                        summary: e.summary.clone(),
                    }
                };
            }

            SessionEvent::ContextCompacted(e) => {
                self.context_state.messages_remaining = e.messages_remaining;
                self.context_state.token_estimate = e.token_estimate_after;
                self.context_state.compact_count += 1;
                self.context_state.pinned_items = e
                    .pinned_items
                    .iter()
                    .map(|l| ContextItem {
                        label: l.clone(),
                        token_hint: None,
                    })
                    .collect();
                self.context_state.summarized_items = e
                    .summarized_items
                    .iter()
                    .map(|l| ContextItem {
                        label: l.clone(),
                        token_hint: None,
                    })
                    .collect();
                self.context_state.excluded_items = e
                    .dropped_items
                    .iter()
                    .map(|l| ContextItem {
                        label: l.clone(),
                        token_hint: None,
                    })
                    .collect();
            }

            SessionEvent::ModelRouted(e) => {
                self.model_state.current_model = Some(e.model.clone());
                self.model_state.route_reason = Some(e.reason.clone());
            }

            SessionEvent::SubagentStarted(e) => {
                self.subagents.push(SubagentSummary {
                    subagent_id: e.subagent_id.clone(),
                    agent: e.agent.clone(),
                    description: e.description.clone(),
                    started_at: e.meta.created_at,
                    finished_at: None,
                    success: None,
                    summary: None,
                });
            }

            SessionEvent::SubagentFinished(e) => {
                if let Some(sa) = self
                    .subagents
                    .iter_mut()
                    .find(|s| s.subagent_id == e.subagent_id)
                {
                    sa.finished_at = Some(e.meta.created_at);
                    sa.success = Some(e.success);
                    sa.summary = Some(e.summary.clone());
                }
            }

            SessionEvent::FindingRaised(e) => {
                self.findings.push(FindingSummary {
                    severity: e.severity.clone(),
                    message: e.message.clone(),
                    file: e.file.clone(),
                    line: e.line,
                    raised_at: e.meta.created_at,
                });
            }

            // These events don't directly mutate TuiSessionState fields
            SessionEvent::AgentMessage(_)
            | SessionEvent::UserMessage(_)
            | SessionEvent::PermissionRequested(_)
            | SessionEvent::PermissionResolved(_)
            | SessionEvent::CheckpointCreated(_)
            | SessionEvent::SessionExported(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::events::{
        AgentPlanItem, ContextCompactedEvent, EventMeta, FileChangedEvent,
        GoalSetEvent, ModelRoutedEvent, PlanItemStatus, PlanItemUpdatedEvent, PlanUpdatedEvent,
        TestRunFinishedEvent, TestRunStartedEvent, ToolCallFinishedEvent, ToolCallStartedEvent,
    };

    fn meta(session_id: &str) -> EventMeta {
        EventMeta::new(session_id)
    }

    #[test]
    fn test_goal_set() {
        let m = meta("s1");
        let events = vec![SessionEvent::GoalSet(GoalSetEvent {
            meta: m,
            goal: "Fix the bug".into(),
        })];
        let state = TuiSessionState::from_events(&events);
        assert_eq!(state.goal.as_deref(), Some("Fix the bug"));
    }

    #[test]
    fn test_tool_call_lifecycle() {
        let m1 = meta("s1");
        let m2 = meta("s1");
        let events = vec![
            SessionEvent::ToolCallStarted(ToolCallStartedEvent {
                meta: m1,
                tool_call_id: "tc-1".into(),
                tool_name: "bash".into(),
                arguments: r#"{"cmd":"ls"}"#.into(),
                risk: ToolRisk::Read,
            }),
            SessionEvent::ToolCallFinished(ToolCallFinishedEvent {
                meta: m2,
                tool_call_id: "tc-1".into(),
                tool_name: "bash".into(),
                status: ToolCallStatus::Success,
                duration_ms: Some(42),
                output_preview: Some("file1\nfile2".into()),
            }),
        ];
        let state = TuiSessionState::from_events(&events);
        assert!(state.active_tool_calls.is_empty());
        assert_eq!(state.recent_tool_calls.len(), 1);
        assert_eq!(state.recent_tool_calls[0].tool_name, "bash");
        assert_eq!(state.recent_tool_calls[0].duration_ms, Some(42));
        assert_eq!(
            state.recent_tool_calls[0].output_preview.as_deref(),
            Some("file1\nfile2")
        );
    }

    #[test]
    fn test_file_change_accumulation() {
        let m1 = meta("s1");
        let m2 = meta("s1");
        let m3 = meta("s1");
        let events = vec![
            SessionEvent::FileChanged(FileChangedEvent {
                meta: m1,
                path: "src/main.rs".into(),
                kind: FileChangeKind::Modified,
                additions: Some(5),
                deletions: Some(2),
            }),
            SessionEvent::FileChanged(FileChangedEvent {
                meta: m2,
                path: "src/lib.rs".into(),
                kind: FileChangeKind::Created,
                additions: Some(20),
                deletions: None,
            }),
            SessionEvent::FileChanged(FileChangedEvent {
                meta: m3,
                path: "src/main.rs".into(),
                kind: FileChangeKind::Modified,
                additions: Some(10),
                deletions: Some(3),
            }),
        ];
        let state = TuiSessionState::from_events(&events);
        assert_eq!(state.changed_files.len(), 2);
        let main = state.changed_files.iter().find(|f| f.path == "src/main.rs").unwrap();
        assert_eq!(main.additions, 10);
        assert_eq!(main.deletions, 3);
    }

    #[test]
    fn test_context_compaction() {
        let m1 = meta("s1");
        let m2 = meta("s1");
        let events = vec![
            SessionEvent::ContextCompacted(ContextCompactedEvent {
                meta: m1,
                messages_removed: 10,
                messages_remaining: 5,
                token_estimate_before: Some(8000),
                token_estimate_after: Some(2000),
                pinned_items: vec!["current goal".into()],
                summarized_items: vec!["previous exploration".into()],
                dropped_items: vec!["raw shell logs".into()],
            }),
            SessionEvent::ContextCompacted(ContextCompactedEvent {
                meta: m2,
                messages_removed: 3,
                messages_remaining: 2,
                token_estimate_before: Some(2000),
                token_estimate_after: Some(800),
                pinned_items: vec!["updated goal".into()],
                summarized_items: vec![],
                dropped_items: vec!["resolved errors".into()],
            }),
        ];
        let state = TuiSessionState::from_events(&events);
        assert_eq!(state.context_state.compact_count, 2);
        assert_eq!(state.context_state.messages_remaining, 2);
        assert_eq!(state.context_state.token_estimate, Some(800));
        assert_eq!(state.context_state.pinned_items.len(), 1);
        assert_eq!(state.context_state.pinned_items[0].label, "updated goal");
    }

    #[test]
    fn test_plan_item_status_transitions() {
        let m1 = meta("s1");
        let m2 = meta("s1");
        let m3 = meta("s1");
        let m4 = meta("s1");
        let events = vec![
            SessionEvent::PlanUpdated(PlanUpdatedEvent {
                meta: m1,
                plan: AgentPlan {
                    items: vec![
                        AgentPlanItem {
                            id: "item-1".into(),
                            text: "Step 1".into(),
                            status: PlanItemStatus::Pending,
                            note: None,
                        },
                        AgentPlanItem {
                            id: "item-2".into(),
                            text: "Step 2".into(),
                            status: PlanItemStatus::Pending,
                            note: None,
                        },
                    ],
                    updated_at: chrono::Utc::now(),
                },
            }),
            SessionEvent::PlanItemUpdated(PlanItemUpdatedEvent {
                meta: m2,
                item_id: "item-1".into(),
                status: PlanItemStatus::InProgress,
                note: None,
            }),
            SessionEvent::PlanItemUpdated(PlanItemUpdatedEvent {
                meta: m3,
                item_id: "item-1".into(),
                status: PlanItemStatus::Done,
                note: Some("Completed".into()),
            }),
            SessionEvent::PlanItemUpdated(PlanItemUpdatedEvent {
                meta: m4,
                item_id: "item-2".into(),
                status: PlanItemStatus::Skipped,
                note: Some("Not needed".into()),
            }),
        ];
        let state = TuiSessionState::from_events(&events);
        let plan = state.plan.unwrap();
        assert_eq!(plan.items[0].status, PlanItemStatus::Done);
        assert_eq!(plan.items[0].note.as_deref(), Some("Completed"));
        assert_eq!(plan.items[1].status, PlanItemStatus::Skipped);
        assert_eq!(plan.items[1].note.as_deref(), Some("Not needed"));
    }

    #[test]
    fn test_goal_set_and_clear() {
        let m1 = meta("s1");
        let m2 = meta("s1");
        let events = vec![
            SessionEvent::GoalSet(GoalSetEvent {
                meta: m1,
                goal: "Implement feature X".into(),
            }),
            SessionEvent::GoalSet(GoalSetEvent {
                meta: m2,
                goal: "Updated goal".into(),
            }),
        ];
        let state = TuiSessionState::from_events(&events);
        assert_eq!(state.goal.as_deref(), Some("Updated goal"));
    }

    #[test]
    fn test_plan_add_multiple_items() {
        let m1 = meta("s1");
        let m2 = meta("s1");
        let m3 = meta("s1");
        let events = vec![
            SessionEvent::PlanUpdated(PlanUpdatedEvent {
                meta: m1,
                plan: AgentPlan {
                    items: vec![AgentPlanItem {
                        id: "item-1".into(),
                        text: "First step".into(),
                        status: PlanItemStatus::Pending,
                        note: None,
                    }],
                    updated_at: chrono::Utc::now(),
                },
            }),
            SessionEvent::PlanUpdated(PlanUpdatedEvent {
                meta: m2,
                plan: AgentPlan {
                    items: vec![
                        AgentPlanItem {
                            id: "item-1".into(),
                            text: "First step".into(),
                            status: PlanItemStatus::Pending,
                            note: None,
                        },
                        AgentPlanItem {
                            id: "item-2".into(),
                            text: "Second step".into(),
                            status: PlanItemStatus::Pending,
                            note: None,
                        },
                    ],
                    updated_at: chrono::Utc::now(),
                },
            }),
            SessionEvent::PlanUpdated(PlanUpdatedEvent {
                meta: m3,
                plan: AgentPlan {
                    items: vec![
                        AgentPlanItem {
                            id: "item-1".into(),
                            text: "First step".into(),
                            status: PlanItemStatus::Done,
                            note: None,
                        },
                        AgentPlanItem {
                            id: "item-2".into(),
                            text: "Second step".into(),
                            status: PlanItemStatus::InProgress,
                            note: None,
                        },
                        AgentPlanItem {
                            id: "item-3".into(),
                            text: "Third step".into(),
                            status: PlanItemStatus::Pending,
                            note: None,
                        },
                    ],
                    updated_at: chrono::Utc::now(),
                },
            }),
        ];
        let state = TuiSessionState::from_events(&events);
        let plan = state.plan.unwrap();
        assert_eq!(plan.items.len(), 3);
        assert_eq!(plan.items[0].status, PlanItemStatus::Done);
        assert_eq!(plan.items[1].status, PlanItemStatus::InProgress);
        assert_eq!(plan.items[2].status, PlanItemStatus::Pending);
    }

    #[test]
    fn test_empty_events_produces_default_state() {
        let state = TuiSessionState::from_events(&[]);
        assert!(state.goal.is_none());
        assert!(state.plan.is_none());
        assert!(state.active_tool_calls.is_empty());
        assert!(state.changed_files.is_empty());
        assert_eq!(state.test_state, TestState::Unknown);
    }

    #[test]
    fn test_test_state_stale_on_file_change() {
        let m1 = meta("s1");
        let m2 = meta("s1");
        let m3 = meta("s1");
        let events = vec![
            SessionEvent::TestRunStarted(TestRunStartedEvent {
                meta: m1,
                command: "cargo test".into(),
            }),
            SessionEvent::TestRunFinished(TestRunFinishedEvent {
                meta: m2,
                command: "cargo test".into(),
                passed: true,
                duration_ms: Some(1234),
                summary: "2 passed".into(),
            }),
            SessionEvent::FileChanged(FileChangedEvent {
                meta: m3,
                path: "src/main.rs".into(),
                kind: FileChangeKind::Modified,
                additions: Some(5),
                deletions: Some(2),
            }),
        ];
        let state = TuiSessionState::from_events(&events);
        assert_eq!(state.test_state, TestState::Stale);
    }

    #[test]
    fn test_test_state_not_stale_when_failed() {
        let m1 = meta("s1");
        let m2 = meta("s1");
        let m3 = meta("s1");
        let events = vec![
            SessionEvent::TestRunStarted(TestRunStartedEvent {
                meta: m1,
                command: "cargo test".into(),
            }),
            SessionEvent::TestRunFinished(TestRunFinishedEvent {
                meta: m2,
                command: "cargo test".into(),
                passed: false,
                duration_ms: Some(500),
                summary: "1 failed".into(),
            }),
            SessionEvent::FileChanged(FileChangedEvent {
                meta: m3,
                path: "src/main.rs".into(),
                kind: FileChangeKind::Modified,
                additions: Some(5),
                deletions: Some(2),
            }),
        ];
        let state = TuiSessionState::from_events(&events);
        // Failed state should remain failed, not become stale
        assert!(matches!(state.test_state, TestState::Failed { .. }));
    }

    #[test]
    fn test_test_state_stale_only_from_passed() {
        let m1 = meta("s1");
        let m2 = meta("s1");
        let m3 = meta("s1");
        let events = vec![
            SessionEvent::TestRunStarted(TestRunStartedEvent {
                meta: m1,
                command: "cargo test".into(),
            }),
            SessionEvent::TestRunFinished(TestRunFinishedEvent {
                meta: m2,
                command: "cargo test".into(),
                passed: true,
                duration_ms: Some(1000),
                summary: "passed".into(),
            }),
            SessionEvent::FileChanged(FileChangedEvent {
                meta: m3,
                path: "README.md".into(),
                kind: FileChangeKind::Modified,
                additions: Some(1),
                deletions: Some(0),
            }),
        ];
        let state = TuiSessionState::from_events(&events);
        assert_eq!(state.test_state, TestState::Stale);
    }

    #[test]
    fn test_file_changed_merges_existing() {
        let m1 = meta("s1");
        let m2 = meta("s1");
        let events = vec![
            SessionEvent::FileChanged(FileChangedEvent {
                meta: m1,
                path: "src/main.rs".into(),
                kind: FileChangeKind::Modified,
                additions: Some(5),
                deletions: Some(2),
            }),
            SessionEvent::FileChanged(FileChangedEvent {
                meta: m2,
                path: "src/main.rs".into(),
                kind: FileChangeKind::Modified,
                additions: Some(10),
                deletions: Some(5),
            }),
        ];
        let state = TuiSessionState::from_events(&events);
        assert_eq!(state.changed_files.len(), 1);
        assert_eq!(state.changed_files[0].additions, 10);
        assert_eq!(state.changed_files[0].deletions, 5);
    }

    #[test]
    fn test_file_changed_creates_new_entry() {
        let m1 = meta("s1");
        let m2 = meta("s1");
        let events = vec![
            SessionEvent::FileChanged(FileChangedEvent {
                meta: m1,
                path: "src/main.rs".into(),
                kind: FileChangeKind::Modified,
                additions: Some(5),
                deletions: Some(2),
            }),
            SessionEvent::FileChanged(FileChangedEvent {
                meta: m2,
                path: "src/lib.rs".into(),
                kind: FileChangeKind::Created,
                additions: Some(20),
                deletions: None,
            }),
        ];
        let state = TuiSessionState::from_events(&events);
        assert_eq!(state.changed_files.len(), 2);
    }
}

