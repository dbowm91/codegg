use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
