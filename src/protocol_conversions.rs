/// Agent-specific protocol conversion helpers.
///
/// Core conversions (session, message, provider, config types) live in
/// `codegg_core::protocol_conversions` and are re-exported here via
/// `pub use codegg_core::protocol_conversions::*`. Agent-related
/// conversions remain in this root crate because the agent module
/// depends on agent runtime types not present in `codegg-core`.
///
/// # Transitional Notes
///
/// Like the core conversions, these agent conversions round-trip through
/// `serde_json::Value` as a **transitional compatibility bridge**. The
/// domain types and DTOs currently share compatible serde attributes, but
/// this implicit coupling is fragile. In a future cleanup pass, replace
/// these with explicit `From`/`TryFrom` implementations to get
/// compile-time safety and remove the serde overhead.
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
    serde_json::from_value(json).expect("agent::Agent is always deserializable from dto::Agent")
}

pub fn dtos_to_agents(agents: Vec<codegg_protocol::dto::Agent>) -> Vec<crate::agent::Agent> {
    agents.into_iter().map(dto_to_agent).collect()
}

// ── Run event construction helpers ─────────────────────────────────────

pub fn run_started_event(
    session_id: &str,
    manifest: &codegg_core::run_store::RunManifest,
) -> codegg_protocol::core::CoreEvent {
    codegg_protocol::core::CoreEvent::RunStarted {
        session_id: session_id.to_string(),
        run_id: manifest.run_id.to_string(),
        kind: format!("{:?}", manifest.kind),
        command: manifest.invocation.command.clone(),
    }
}

pub fn run_progress_event(
    session_id: &str,
    run_id: &str,
    message: &str,
) -> codegg_protocol::core::CoreEvent {
    codegg_protocol::core::CoreEvent::RunProgress {
        session_id: session_id.to_string(),
        run_id: run_id.to_string(),
        message: message.to_string(),
    }
}

pub fn run_completed_event(
    session_id: &str,
    manifest: &codegg_core::run_store::RunManifest,
) -> codegg_protocol::core::CoreEvent {
    let summary = match manifest.status {
        codegg_core::run_store::RunStatus::Complete => "completed".to_string(),
        codegg_core::run_store::RunStatus::Failed => "failed".to_string(),
        codegg_core::run_store::RunStatus::TimedOut => "timed out".to_string(),
        codegg_core::run_store::RunStatus::Cancelled => "cancelled".to_string(),
        codegg_core::run_store::RunStatus::Incomplete => "incomplete".to_string(),
        codegg_core::run_store::RunStatus::Running => "running".to_string(),
    };
    codegg_protocol::core::CoreEvent::RunCompleted {
        session_id: session_id.to_string(),
        run_id: manifest.run_id.to_string(),
        status: format!("{:?}", manifest.status),
        summary,
    }
}

pub fn run_artifact_created_event(
    session_id: &str,
    run_id: &str,
    artifact: &codegg_core::run_store::ArtifactRecord,
) -> codegg_protocol::core::CoreEvent {
    codegg_protocol::core::CoreEvent::RunArtifactCreated {
        session_id: session_id.to_string(),
        run_id: run_id.to_string(),
        artifact_id: artifact.artifact_id.to_string(),
        kind: format!("{:?}", artifact.kind),
        byte_length: artifact.byte_length,
    }
}

pub fn run_denied_event(
    session_id: &str,
    run_id: &str,
    reason: &str,
) -> codegg_protocol::core::CoreEvent {
    codegg_protocol::core::CoreEvent::RunDenied {
        session_id: session_id.to_string(),
        run_id: run_id.to_string(),
        reason: reason.to_string(),
    }
}
