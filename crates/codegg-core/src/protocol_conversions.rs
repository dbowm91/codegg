/// Core protocol conversion helpers for session, message, and provider types.
///
/// Agent-related conversions remain in root `src/protocol_conversions.rs`
/// because the agent module is not extracted into `codegg-core`.
///
/// # Transitional Notes
///
/// These conversions intentionally live in `codegg-core` rather than in
/// `codegg-protocol`. The protocol crate must remain a thin, dependency-free
/// DTO layer; it must not depend on domain or runtime crates such as
/// `codegg-core`, `codegg-providers`, or `codegg-config`.
///
/// Every conversion currently round-trips through `serde_json::Value`. This
/// works because the domain types and DTOs share the same serde attributes,
/// but it is a **transitional compatibility bridge**, not the intended
/// long-term pattern. In a future cleanup pass, prefer explicit
/// `From`/`TryFrom` implementations that avoid the intermediate JSON
/// serialization and provide better compile-time error messages.
// ── Domain → DTO (for constructing protocol responses / requests) ───────
pub fn session_to_dto(s: crate::session::Session) -> codegg_protocol::dto::Session {
    let json = serde_json::to_value(&s).expect("session::Session is always serializable");
    serde_json::from_value(json)
        .expect("dto::Session is always deserializable from session::Session")
}

pub fn message_to_dto(m: crate::session::message::Message) -> codegg_protocol::dto::Message {
    let json = serde_json::to_value(&m).expect("message::Message is always serializable");
    serde_json::from_value(json)
        .expect("dto::Message is always deserializable from message::Message")
}

pub fn provider_message_to_dto(
    m: codegg_providers::Message,
) -> codegg_protocol::dto::ProviderMessage {
    let json = serde_json::to_value(&m).expect("provider::Message is always serializable");
    serde_json::from_value(json)
        .expect("dto::ProviderMessage is always deserializable from provider::Message")
}

pub fn session_template_to_dto(
    t: codegg_config::schema::SessionTemplate,
) -> codegg_protocol::dto::SessionTemplate {
    let json = serde_json::to_value(&t).expect("SessionTemplate is always serializable");
    serde_json::from_value(json)
        .expect("dto::SessionTemplate is always deserializable from SessionTemplate")
}

pub fn sessions_to_dtos(
    sessions: Vec<crate::session::Session>,
) -> Vec<codegg_protocol::dto::Session> {
    sessions.into_iter().map(session_to_dto).collect()
}

pub fn messages_to_dtos(
    messages: Vec<crate::session::message::Message>,
) -> Vec<codegg_protocol::dto::Message> {
    messages.into_iter().map(message_to_dto).collect()
}

pub fn provider_messages_to_dtos(
    messages: Vec<codegg_providers::Message>,
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

pub fn dto_to_provider_message(
    m: codegg_protocol::dto::ProviderMessage,
) -> codegg_providers::Message {
    let json = serde_json::to_value(&m).expect("dto::ProviderMessage is always serializable");
    serde_json::from_value(json)
        .expect("provider::Message is always deserializable from dto::ProviderMessage")
}

pub fn dto_to_session_template(
    t: codegg_protocol::dto::SessionTemplate,
) -> codegg_config::schema::SessionTemplate {
    let json = serde_json::to_value(&t).expect("dto::SessionTemplate is always serializable");
    serde_json::from_value(json)
        .expect("SessionTemplate is always deserializable from dto::SessionTemplate")
}

pub fn dtos_to_sessions(
    sessions: Vec<codegg_protocol::dto::Session>,
) -> Vec<crate::session::Session> {
    sessions.into_iter().map(dto_to_session).collect()
}

pub fn dtos_to_messages(
    messages: Vec<codegg_protocol::dto::Message>,
) -> Vec<crate::session::message::Message> {
    messages.into_iter().map(dto_to_message).collect()
}

pub fn dtos_to_provider_messages(
    messages: Vec<codegg_protocol::dto::ProviderMessage>,
) -> Vec<codegg_providers::Message> {
    messages.into_iter().map(dto_to_provider_message).collect()
}

/// Convert a registered `WorkspaceRecord` into the wire DTO. The active
/// session count is provided by the daemon snapshot builder because the
/// record itself does not own a session index.
pub fn workspace_record_to_dto(
    record: &crate::workspace::WorkspaceRecord,
    active_sessions: usize,
) -> codegg_protocol::dto::WorkspaceSnapshot {
    codegg_protocol::dto::WorkspaceSnapshot {
        workspace_id: record.id.as_str().to_string(),
        canonical_root: record.canonical_root.to_string_lossy().into_owned(),
        display_name: record.display_name.clone(),
        created_at: record.created_at.timestamp_millis(),
        last_opened_at: record.last_opened_at.timestamp_millis(),
        archived_at: record.archived_at.map(|d| d.timestamp_millis()),
        active_sessions,
    }
}
