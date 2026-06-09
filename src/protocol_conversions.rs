/// Protocol conversion helpers.
///
/// Core conversions (session, message, provider, config types) live in
/// `codegg_core::protocol_conversions`. Agent-related conversions remain
/// here because the agent module is not extracted into `codegg-core`.

pub use codegg_core::protocol_conversions::*;

// ── Agent-specific conversions (root-only) ─────────────────────────────

pub fn agent_to_dto(a: crate::agent::Agent) -> codegg_protocol::dto::Agent {
    let json = serde_json::to_value(&a).expect("agent::Agent is always serializable");
    serde_json::from_value(json).expect("dto::Agent is always deserializable from agent::Agent")
}

pub fn agents_to_dtos(agents: Vec<crate::agent::Agent>) -> Vec<codegg_protocol::dto::Agent> {
    agents.into_iter().map(agent_to_dto).collect()
}

pub fn dto_to_agent(a: codegg_protocol::dto::Agent) -> crate::agent::Agent {
    let json = serde_json::to_value(&a).expect("dto::Agent is always serializable");
    serde_json::from_value(json)
        .expect("agent::Agent is always deserializable from dto::Agent")
}

pub fn dtos_to_agents(agents: Vec<codegg_protocol::dto::Agent>) -> Vec<crate::agent::Agent> {
    agents.into_iter().map(dto_to_agent).collect()
}
