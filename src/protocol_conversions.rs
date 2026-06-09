/// Helper functions for converting between root domain types and protocol DTOs.
///
/// The protocol crate (`codegg_protocol`) uses self-contained DTO types
/// that mirror the wire format of root domain types. These helpers bridge
/// the two worlds via serde round-trips, which is safe because the wire
/// format is identical.

// ── Domain → DTO (for constructing protocol responses / requests) ───────

pub fn session_to_dto(s: crate::session::Session) -> codegg_protocol::dto::Session {
    let json = serde_json::to_value(&s).expect("session::Session is always serializable");
    serde_json::from_value(json).expect("dto::Session is always deserializable from session::Session")
}

pub fn message_to_dto(m: crate::session::message::Message) -> codegg_protocol::dto::Message {
    let json = serde_json::to_value(&m).expect("message::Message is always serializable");
    serde_json::from_value(json)
        .expect("dto::Message is always deserializable from message::Message")
}

pub fn agent_to_dto(a: crate::agent::Agent) -> codegg_protocol::dto::Agent {
    let json = serde_json::to_value(&a).expect("agent::Agent is always serializable");
    serde_json::from_value(json).expect("dto::Agent is always deserializable from agent::Agent")
}

pub fn provider_message_to_dto(
    m: crate::provider::Message,
) -> codegg_protocol::dto::ProviderMessage {
    let json = serde_json::to_value(&m).expect("provider::Message is always serializable");
    serde_json::from_value(json)
        .expect("dto::ProviderMessage is always deserializable from provider::Message")
}

pub fn session_template_to_dto(
    t: crate::config::schema::SessionTemplate,
) -> codegg_protocol::dto::SessionTemplate {
    let json =
        serde_json::to_value(&t).expect("SessionTemplate is always serializable");
    serde_json::from_value(json)
        .expect("dto::SessionTemplate is always deserializable from SessionTemplate")
}

pub fn sessions_to_dtos(sessions: Vec<crate::session::Session>) -> Vec<codegg_protocol::dto::Session> {
    sessions.into_iter().map(session_to_dto).collect()
}

pub fn messages_to_dtos(
    messages: Vec<crate::session::message::Message>,
) -> Vec<codegg_protocol::dto::Message> {
    messages.into_iter().map(message_to_dto).collect()
}

pub fn agents_to_dtos(agents: Vec<crate::agent::Agent>) -> Vec<codegg_protocol::dto::Agent> {
    agents.into_iter().map(agent_to_dto).collect()
}

pub fn provider_messages_to_dtos(
    messages: Vec<crate::provider::Message>,
) -> Vec<codegg_protocol::dto::ProviderMessage> {
    messages.into_iter().map(provider_message_to_dto).collect()
}

// ── DTO → Domain (for consuming protocol responses in the application) ─

pub fn dto_to_session(s: codegg_protocol::dto::Session) -> crate::session::Session {
    let json = serde_json::to_value(&s).expect("dto::Session is always serializable");
    serde_json::from_value(json)
        .expect("session::Session is always deserializable from dto::Session")
}

pub fn dto_to_message(m: codegg_protocol::dto::Message) -> crate::session::message::Message {
    let json = serde_json::to_value(&m).expect("dto::Message is always serializable");
    serde_json::from_value(json)
        .expect("message::Message is always deserializable from dto::Message")
}

pub fn dto_to_agent(a: codegg_protocol::dto::Agent) -> crate::agent::Agent {
    let json = serde_json::to_value(&a).expect("dto::Agent is always serializable");
    serde_json::from_value(json)
        .expect("agent::Agent is always deserializable from dto::Agent")
}

pub fn dto_to_provider_message(
    m: codegg_protocol::dto::ProviderMessage,
) -> crate::provider::Message {
    let json = serde_json::to_value(&m).expect("dto::ProviderMessage is always serializable");
    serde_json::from_value(json)
        .expect("provider::Message is always deserializable from dto::ProviderMessage")
}

pub fn dto_to_session_template(
    t: codegg_protocol::dto::SessionTemplate,
) -> crate::config::schema::SessionTemplate {
    let json =
        serde_json::to_value(&t).expect("dto::SessionTemplate is always serializable");
    serde_json::from_value(json)
        .expect("SessionTemplate is always deserializable from dto::SessionTemplate")
}

pub fn dtos_to_sessions(sessions: Vec<codegg_protocol::dto::Session>) -> Vec<crate::session::Session> {
    sessions.into_iter().map(dto_to_session).collect()
}

pub fn dtos_to_messages(
    messages: Vec<codegg_protocol::dto::Message>,
) -> Vec<crate::session::message::Message> {
    messages.into_iter().map(dto_to_message).collect()
}

pub fn dtos_to_agents(agents: Vec<codegg_protocol::dto::Agent>) -> Vec<crate::agent::Agent> {
    agents.into_iter().map(dto_to_agent).collect()
}

pub fn dtos_to_provider_messages(
    messages: Vec<codegg_protocol::dto::ProviderMessage>,
) -> Vec<crate::provider::Message> {
    messages.into_iter().map(dto_to_provider_message).collect()
}
