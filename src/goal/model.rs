use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::bus::events::{GoalBudgetSnapshot, GoalSnapshot, GoalUsageSnapshot};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    AwaitingUser,
    BudgetLimited,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoalBudget {
    #[serde(default)]
    pub max_turns: Option<i64>,
    #[serde(default)]
    pub max_model_tokens: Option<i64>,
    #[serde(default)]
    pub max_tool_calls: Option<i64>,
    /// Wall-clock budget in seconds. When the active goal has spent this
    /// many seconds (across sessions, durable in the DB), it will be
    /// transitioned to `BudgetLimited` and the agent will be told to
    /// wrap up. Optional — only enforced when set.
    #[serde(default)]
    pub max_wallclock_secs: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoalUsage {
    #[serde(default)]
    pub turns_used: i64,
    #[serde(default)]
    pub input_tokens: i64,
    #[serde(default)]
    pub output_tokens: i64,
    #[serde(default)]
    pub tool_calls: i64,
    /// Wall-clock seconds spent actively working on the goal. Persisted
    /// so usage survives session restarts. Reset to 0 when the goal
    /// transitions to a non-active status.
    #[serde(default)]
    pub wallclock_secs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub session_id: String,
    pub project_id: String,
    pub title: String,
    pub objective: String,
    pub status: GoalStatus,

    pub plan_path: Option<String>,
    pub checkpoint_path: Option<String>,

    pub current_phase: Option<String>,
    pub progress_summary: String,
    pub next_action: Option<String>,
    pub completion_criteria: Vec<String>,
    pub open_questions: Vec<String>,

    pub budget: GoalBudget,
    pub usage: GoalUsage,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoalProgressUpdate {
    pub current_phase: Option<String>,
    pub progress_summary: Option<String>,
    pub next_action: Option<String>,
    pub completed_items: Vec<String>,
    pub remaining_items: Vec<String>,
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub evidence: String,
    pub files_changed: Vec<String>,
    pub tests_run: Vec<String>,
    pub remaining_risks: Vec<String>,
}

impl Goal {
    /// Stable lowercase string form of the status, suitable for
    /// serialization and UI display.
    pub fn status_as_str(&self) -> &'static str {
        match self.status {
            GoalStatus::Active => "active",
            GoalStatus::Paused => "paused",
            GoalStatus::AwaitingUser => "awaiting_user",
            GoalStatus::BudgetLimited => "budget_limited",
            GoalStatus::Complete => "complete",
            GoalStatus::Failed => "failed",
            GoalStatus::Cancelled => "cancelled",
        }
    }

    /// True if the goal is in a terminal state (cannot be auto-continued).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            GoalStatus::Complete
                | GoalStatus::Failed
                | GoalStatus::Cancelled
                | GoalStatus::BudgetLimited
        )
    }

    /// True if the agent is actively working on this goal.
    pub fn is_active(&self) -> bool {
        matches!(self.status, GoalStatus::Active)
    }

    pub fn to_snapshot(&self) -> GoalSnapshot {
        GoalSnapshot {
            id: self.id.clone(),
            session_id: self.session_id.clone(),
            project_id: self.project_id.clone(),
            title: self.title.clone(),
            objective: self.objective.clone(),
            status: self.status_as_str().to_string(),
            current_phase: self.current_phase.clone(),
            progress_summary: self.progress_summary.clone(),
            next_action: self.next_action.clone(),
            completion_criteria: self.completion_criteria.clone(),
            open_questions: self.open_questions.clone(),
            budget: GoalBudgetSnapshot {
                max_turns: self.budget.max_turns,
                max_model_tokens: self.budget.max_model_tokens,
                max_tool_calls: self.budget.max_tool_calls,
                max_wallclock_secs: self.budget.max_wallclock_secs,
            },
            usage: GoalUsageSnapshot {
                turns_used: self.usage.turns_used,
                input_tokens: self.usage.input_tokens,
                output_tokens: self.usage.output_tokens,
                tool_calls: self.usage.tool_calls,
                wallclock_secs: self.usage.wallclock_secs,
            },
            created_at_ms: self.created_at.timestamp_millis(),
            updated_at_ms: self.updated_at.timestamp_millis(),
            started_at_ms: self.started_at.map(|d| d.timestamp_millis()),
            completed_at_ms: self.completed_at.map(|d| d.timestamp_millis()),
        }
    }

    /// Render a one-line summary from a `GoalSnapshot` so the TUI can
    /// call the same formatter whether it has the full `Goal` (from the
    /// DB) or just the snapshot (from the bus).
    pub fn summary_from_snapshot(snap: &crate::bus::events::GoalSnapshot) -> String {
        let budget_label = match (
            snap.budget.max_model_tokens,
            snap.budget.max_tool_calls,
            snap.budget.max_wallclock_secs,
        ) {
            (Some(t), _, _) => format!(
                " {} / {} tokens",
                Self::format_token_count(snap.usage.input_tokens + snap.usage.output_tokens),
                Self::format_token_count(t)
            ),
            (None, Some(c), _) => format!(
                " {} / {} tool calls",
                snap.usage.tool_calls, c
            ),
            (None, None, Some(s)) => format!(
                " {} / {} wall",
                Self::format_duration(snap.usage.wallclock_secs),
                Self::format_duration(s)
            ),
            _ => format!(
                " {}t in {}t out · {} turns · {} tools",
                snap.usage.input_tokens,
                snap.usage.output_tokens,
                snap.usage.turns_used,
                snap.usage.tool_calls
            ),
        };
        format!("[{}] {}{}", snap.status, snap.title, budget_label)
    }

    /// Render a one-line summary suitable for the TUI sidebar header.
    pub fn sidebar_summary(&self) -> String {
        let status = self.status_as_str();
        let budget_label = match (
            self.budget.max_model_tokens,
            self.budget.max_tool_calls,
            self.budget.max_wallclock_secs,
        ) {
            (Some(t), _, _) => format!(
                " {} / {} tokens",
                Self::format_token_count(self.usage.input_tokens + self.usage.output_tokens),
                Self::format_token_count(t)
            ),
            (None, Some(c), _) => format!(
                " {} / {} tool calls",
                self.usage.tool_calls, c
            ),
            (None, None, Some(s)) => format!(
                " {} / {} wall",
                Self::format_duration(self.usage.wallclock_secs),
                Self::format_duration(s)
            ),
            _ => format!(
                " {}t in {}t out · {} turns · {} tools",
                self.usage.input_tokens,
                self.usage.output_tokens,
                self.usage.turns_used,
                self.usage.tool_calls
            ),
        };
        format!("[{}] {}{}", status, self.title, budget_label)
    }

    fn format_token_count(n: i64) -> String {
        if n >= 1_000_000 {
            format!("{:.1}M", n as f64 / 1_000_000.0)
        } else if n >= 1_000 {
            format!("{:.1}K", n as f64 / 1_000.0)
        } else {
            n.to_string()
        }
    }

    fn format_duration(secs: i64) -> String {
        if secs >= 3600 {
            format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
        } else if secs >= 60 {
            format!("{}m", secs / 60)
        } else {
            format!("{}s", secs)
        }
    }
}

