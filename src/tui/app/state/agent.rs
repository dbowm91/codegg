use std::sync::Arc;

use crate::agent::asset_snapshot::ProjectAssetSnapshot;
use crate::agent::Agent;

pub struct AgentState {
    /// Immutable snapshot of effective agents, skills, and project
    /// instructions. Daemon-owned consumers of agent resolution should
    /// read from this snapshot rather than calling
    /// `AgentRegistry::load` or `resolve_agents` directly.
    pub snapshot: Option<Arc<ProjectAssetSnapshot>>,
    /// Cached `Vec<Agent>` for legacy consumers that have not yet
    /// migrated to the snapshot. New code MUST read `snapshot` first.
    pub agents: Vec<Agent>,
    pub current_agent: usize,
    pub current_model: String,
    pub models: Vec<String>,
    pub model_idx: usize,
    pub plan_mode: bool,
    pub plan_topic: Option<String>,
}
