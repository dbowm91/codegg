use crate::agent::Agent;

pub struct AgentState {
    pub agents: Vec<Agent>,
    pub current_agent: usize,
    pub current_model: String,
    pub models: Vec<String>,
    pub model_idx: usize,
    pub plan_mode: bool,
    pub plan_topic: Option<String>,
}
