use serde::{Deserialize, Serialize};

use crate::protocol::core::{CoreEvent, CoreResponse, EventEnvelope, RequestEnvelope, CoreRequest};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientHello {
    pub client_name: String,
    pub client_kind: ClientKind,
    pub protocol_version: u32,
    pub capabilities: ClientCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientKind {
    Tui,
    Gui,
    Web,
    Cli,
    Automation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    pub visual_notifications: bool,
    pub desktop_notifications: bool,
    pub audio: bool,
    pub tts: bool,
    pub multi_session_view: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerHello {
    pub daemon_id: String,
    pub protocol_version: u32,
    pub server_capabilities: ServerCapabilities,
    /// Negotiated client_id assigned by the daemon. The client should use this
    /// when sending `Subscribe` frames so the daemon's `ClientRegistry` can
    /// record the negotiated identity (instead of trusting a client-supplied id).
    /// This is a wire-protocol addition; older clients that ignore it are still
    /// forward-compatible with the daemon.
    #[serde(default)]
    pub client_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub event_replay: bool,
    pub session_management: bool,
    pub permission_routing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreFrame {
    Request(RequestEnvelope<CoreRequest>),
    Response {
        request_id: String,
        response: CoreResponse,
    },
    Subscribe {
        client_id: String,
        session_id: Option<String>,
        from_event_seq: Option<u64>,
    },
    Event(EventEnvelope<CoreEvent>),
    Error {
        request_id: Option<String>,
        code: String,
        message: String,
    },
    ClientHello(ClientHello),
    ServerHello(ServerHello),
    Ping,
    Pong,
}
