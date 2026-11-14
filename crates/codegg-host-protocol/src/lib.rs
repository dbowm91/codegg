//! Wire types for the embedding host protocol, `apiVersion: 1`.
//!
//! The contract is `docs/embedding.md`; the stability policy behind it is
//! ADR-0060. This module is the single definition of the wire: the host binary
//! and (from E003) `codegg-host-protocol` are the same types, never a copy.
//!
//! Framing is JSON-RPC 2.0 over newline-delimited JSON on stdio. A frame is one
//! object, one line, bounded by [`MAX_FRAME_BYTES`]. Every field is bounded and
//! every rejection names a field, never a value.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The protocol version this build speaks and expects.
pub const API_VERSION: u32 = 1;

/// One frame, both directions. A larger frame drops the connection.
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
/// One prompt.
pub const MAX_PROMPT_BYTES: usize = 1024 * 1024;
/// One tool output in `turn/tool`.
pub const MAX_TOOL_OUTPUT_BYTES: usize = 8 * 1024;
/// One client or display name.
pub const MAX_NAME_CHARS: usize = 64;
/// One model id, agent name, workspace path or session/turn id.
pub const MAX_IDENTIFIER_CHARS: usize = 512;
/// Capabilities a v1 host may require of CodeGG.
pub const CAPABILITIES: &[&str] = &["descriptor", "streaming", "cancellation", "usage"];

/// JSON-RPC error codes. Application failures all use [`code::APPLICATION`] and
/// name their kind in `error.data.class`.
pub mod code {
    /// A malformed request object.
    pub const INVALID: i64 = -32600;
    /// An unknown method.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// Invalid params.
    pub const INVALID_PARAMS: i64 = -32602;
    /// A defect inside CodeGG.
    pub const INTERNAL: i64 = -32603;
    /// Any application-level failure; `data.class` names it.
    pub const APPLICATION: i64 = -32000;
    /// A turn is already active.
    pub const BUSY: i64 = -32001;
    /// `initialize` has not succeeded.
    pub const NOT_INITIALIZED: i64 = -32002;
    /// A required capability is not advertised.
    pub const UNSUPPORTED_CAPABILITY: i64 = -32003;
}

/// The error taxonomy. A host acts on this and never on the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    /// The upstream rejected the credential.
    Auth,
    /// The upstream does not have that model id.
    ModelNotFound,
    /// The model exists but this request shape is unsupported.
    ModelNotSupported,
    /// The request exceeds the model's context.
    ContextLength,
    /// The upstream is throttling.
    RateLimit,
    /// The request or the descriptor is malformed.
    InvalidRequest,
    /// The upstream could not be reached or the stream broke.
    Transport,
    /// The upstream answered something this build cannot parse.
    Protocol,
    /// Any other provider-reported failure.
    Upstream,
    /// A turn is already active.
    Busy,
    /// The turn was cancelled.
    Cancelled,
    /// The workspace root is missing or unusable.
    Workspace,
    /// `initialize` has not succeeded.
    NotInitialized,
    /// A required capability is not advertised.
    UnsupportedCapability,
    /// A CodeGG defect.
    Internal,
}

impl ErrorClass {
    /// Whether a host may reasonably retry the same request.
    pub fn retryable(self) -> bool {
        matches!(self, Self::RateLimit | Self::Transport | Self::Busy)
    }

    /// The JSON-RPC code this class travels with.
    pub fn rpc_code(self) -> i64 {
        match self {
            Self::Busy => code::BUSY,
            Self::NotInitialized => code::NOT_INITIALIZED,
            Self::UnsupportedCapability => code::UNSUPPORTED_CAPABILITY,
            Self::InvalidRequest => code::INVALID_PARAMS,
            Self::Internal => code::INTERNAL,
            _ => code::APPLICATION,
        }
    }
}

/// The `data` object every error carries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorData {
    pub class: ErrorClass,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_status: Option<u16>,
}

impl ErrorData {
    pub fn new(class: ErrorClass) -> Self {
        Self {
            class,
            retryable: class.retryable(),
            provider_status: None,
        }
    }

    pub fn with_status(class: ErrorClass, status: u16) -> Self {
        Self {
            class,
            retryable: class.retryable(),
            provider_status: Some(status),
        }
    }
}

/// A JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ErrorData>,
}

impl RpcError {
    pub fn new(class: ErrorClass, message: impl Into<String>) -> Self {
        Self {
            code: class.rpc_code(),
            message: message.into(),
            data: Some(ErrorData::new(class)),
        }
    }

    pub fn with_status(class: ErrorClass, message: impl Into<String>, status: u16) -> Self {
        Self {
            code: class.rpc_code(),
            message: message.into(),
            data: Some(ErrorData::with_status(class, status)),
        }
    }

    /// An error that is not an application failure (malformed frame, unknown
    /// method). It still carries a class so a host never has to branch on code.
    pub fn protocol(code: i64, class: ErrorClass, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(ErrorData::new(class)),
        }
    }
}

/// A request id. JSON-RPC allows a string or a number; both are echoed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    Text(String),
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(value) => write!(f, "{value}"),
            Self::Text(value) => write!(f, "{value}"),
        }
    }
}

/// A request frame from the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestFrame {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(flatten)]
    pub body: RequestBody,
}

/// The five methods. `method` names the variant and `params` is its content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum RequestBody {
    #[serde(rename = "initialize")]
    Initialize(InitializeParams),
    #[serde(rename = "session/open")]
    SessionOpen(SessionOpenParams),
    #[serde(rename = "turn/submit")]
    TurnSubmit(TurnSubmitParams),
    #[serde(rename = "turn/cancel")]
    TurnCancel(TurnCancelParams),
    #[serde(rename = "session/close")]
    SessionClose(SessionCloseParams),
}

impl RequestBody {
    pub fn method(&self) -> &'static str {
        match self {
            Self::Initialize(_) => "initialize",
            Self::SessionOpen(_) => "session/open",
            Self::TurnSubmit(_) => "turn/submit",
            Self::TurnCancel(_) => "turn/cancel",
            Self::SessionClose(_) => "session/close",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InitializeParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub api_version: u32,
    pub codegg_version: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOpenParams {
    pub workspace: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The inline provider descriptor from E001. Carried as an object so the
    /// wire crate does not depend on the provider crate; `session/open`
    /// deserializes it into `codegg_providers::InlineProviderDescriptor`, which
    /// is the single definition of its shape and its validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionOpenResult {
    pub session_id: String,
    pub model: String,
    pub agent: String,
    pub workspace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSubmitParams {
    pub session_id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnSubmitResult {
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCancelParams {
    pub session_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnCancelResult {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCloseParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionCloseResult {
    pub closed: bool,
}

/// A notification frame from CodeGG. It has no `id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationFrame {
    pub jsonrpc: String,
    #[serde(flatten)]
    pub body: NotificationBody,
}

/// The five notification kinds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum NotificationBody {
    #[serde(rename = "turn/text")]
    Text(TurnText),
    #[serde(rename = "turn/tool")]
    Tool(TurnTool),
    #[serde(rename = "turn/usage")]
    Usage(TurnUsage),
    #[serde(rename = "turn/done")]
    Done(TurnDone),
    #[serde(rename = "turn/error")]
    Error(TurnError),
}

impl NotificationBody {
    pub fn method(&self) -> &'static str {
        match self {
            Self::Text(_) => "turn/text",
            Self::Tool(_) => "turn/tool",
            Self::Usage(_) => "turn/usage",
            Self::Done(_) => "turn/done",
            Self::Error(_) => "turn/error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnText {
    pub session_id: String,
    pub turn_id: String,
    pub delta: String,
}

/// How a turn ended. A cancelled turn is a `turn/done`, never a `turn/error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    MaxTurns,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPhase {
    Started,
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnTool {
    pub session_id: String,
    pub turn_id: String,
    pub phase: ToolPhase,
    pub tool_id: String,
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

/// Provider usage. `reported: false` is not zero usage; the counters are absent
/// because zero is a real value a host may budget against.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub reported: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

impl Usage {
    /// Usage the provider never reported.
    pub fn absent() -> Self {
        Self::default()
    }

    /// Usage from the loop's token counts, when the provider reported any.
    pub fn reported(input: Option<u64>, output: Option<u64>, cached: Option<u64>) -> Self {
        match (input, output) {
            (None, None) => Self::absent(),
            (input, output) => Self {
                reported: true,
                input_tokens: input,
                output_tokens: output,
                cached_tokens: cached,
                reasoning_tokens: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsage {
    pub session_id: String,
    pub turn_id: String,
    #[serde(flatten)]
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnDone {
    pub session_id: String,
    pub turn_id: String,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnError {
    pub session_id: String,
    pub turn_id: String,
    pub class: ErrorClass,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_status: Option<u16>,
    pub usage: Usage,
}

impl TurnError {
    pub fn new(
        session_id: impl Into<String>,
        turn_id: impl Into<String>,
        class: ErrorClass,
        message: impl Into<String>,
        usage: Usage,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            class,
            message: message.into(),
            retryable: class.retryable(),
            provider_status: None,
            usage,
        }
    }
}

/// A response frame, either a result or an error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseFrame {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl ResponseFrame {
    pub fn result(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: RequestId, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// A frame the host sent, classified.
#[derive(Debug, Clone)]
pub enum Incoming {
    Request(RequestFrame),
    /// A frame with a `method` and no `id`. A host is not allowed to send
    /// notifications, so this is refused by name and never dispatched.
    Notification {
        method: String,
    },
    /// A method this version does not have. The id travels so the refusal is a
    /// correlated response rather than a dropped frame.
    UnknownMethod {
        id: RequestId,
        method: String,
    },
    /// A request this version could not read. Refused with the id when there
    /// was one.
    Unusable {
        id: Option<RequestId>,
        detail: String,
    },
}

/// Why a frame could not be accepted at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// The line is larger than [`MAX_FRAME_BYTES`].
    TooLarge,
    /// The line is not UTF-8 or not a JSON object.
    Malformed,
}

impl FrameError {
    pub fn rpc(&self) -> RpcError {
        match self {
            Self::TooLarge => RpcError::protocol(
                code::INVALID,
                ErrorClass::InvalidRequest,
                format!("frame exceeds {MAX_FRAME_BYTES} bytes"),
            ),
            Self::Malformed => RpcError::protocol(
                code::INVALID,
                ErrorClass::InvalidRequest,
                "frame is not a UTF-8 JSON object",
            ),
        }
    }
}

/// Classify one newline-stripped frame.
pub fn parse_frame(line: &[u8]) -> Result<Incoming, FrameError> {
    if line.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge);
    }
    let text = std::str::from_utf8(line).map_err(|_| FrameError::Malformed)?;
    let value: Value = serde_json::from_str(text).map_err(|_| FrameError::Malformed)?;
    let Value::Object(object) = value else {
        return Err(FrameError::Malformed);
    };
    let id = object
        .get("id")
        .and_then(|value| serde_json::from_value::<RequestId>(value.clone()).ok());
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let has_result = object.contains_key("result") || object.contains_key("error");
    match (method, id.clone(), has_result) {
        (Some(method), Some(id), _) => {
            if !matches!(
                method.as_str(),
                "initialize" | "session/open" | "turn/submit" | "turn/cancel" | "session/close"
            ) {
                return Ok(Incoming::UnknownMethod { id, method });
            }
            serde_json::from_value::<RequestFrame>(Value::Object(object))
                .map(Incoming::Request)
                .or_else(|error| {
                    Ok(Incoming::Unusable {
                        id: Some(id),
                        detail: format!("request is not a valid v1 frame: {error}"),
                    })
                })
        }
        (Some(method), None, _) => Ok(Incoming::Notification { method }),
        (None, _, true) => Ok(Incoming::Unusable {
            id: None,
            detail: "a host may not send a response".to_owned(),
        }),
        (None, Some(_), false) => Ok(Incoming::Unusable {
            id,
            detail: "request has no method".to_owned(),
        }),
        (None, None, false) => Ok(Incoming::Unusable {
            id: None,
            detail: "object is neither a request nor a notification".to_owned(),
        }),
    }
}

/// Serialize a frame to one line, without the trailing newline.
pub fn encode<T: Serialize>(frame: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(frame)
}

/// Bound a string, naming the field rather than echoing the value.
pub fn check_len(field: &str, value: &str, max: usize) -> Result<(), RpcError> {
    if value.is_empty() {
        return Err(RpcError::new(
            ErrorClass::InvalidRequest,
            format!("{field} must not be empty"),
        ));
    }
    if value.chars().count() > max {
        return Err(RpcError::new(
            ErrorClass::InvalidRequest,
            format!("{field} exceeds {max} characters"),
        ));
    }
    Ok(())
}

impl InitializeParams {
    /// Refuse a client name or version that is out of bounds.
    pub fn validate(&self) -> Result<(), RpcError> {
        if let Some(name) = &self.client_name {
            check_len("clientName", name, MAX_NAME_CHARS)?;
        }
        if let Some(version) = &self.client_version {
            check_len("clientVersion", version, MAX_NAME_CHARS)?;
        }
        Ok(())
    }

    /// The capabilities this host requires, checked against what v1 has.
    pub fn required_capabilities(&self) -> Result<Vec<String>, RpcError> {
        let requested = self.capabilities.clone().unwrap_or_default();
        for capability in &requested {
            if !CAPABILITIES.contains(&capability.as_str()) {
                return Err(RpcError::protocol(
                    code::UNSUPPORTED_CAPABILITY,
                    ErrorClass::UnsupportedCapability,
                    format!(
                        "capability \"{capability}\" is not supported by apiVersion {API_VERSION}"
                    ),
                ));
            }
        }
        Ok(requested)
    }
}

impl SessionOpenParams {
    /// Structural validation. The descriptor and the workspace are validated by
    /// the session layer, which owns those authorities.
    pub fn validate(&self) -> Result<(), RpcError> {
        check_len("workspace", &self.workspace, MAX_IDENTIFIER_CHARS)?;
        check_len("model", &self.model, MAX_IDENTIFIER_CHARS)?;
        if let Some(agent) = &self.agent {
            check_len("agent", agent, MAX_NAME_CHARS)?;
        }
        if let Some(session_id) = &self.session_id {
            check_len("sessionId", session_id, MAX_IDENTIFIER_CHARS)?;
        }
        Ok(())
    }
}

impl TurnSubmitParams {
    pub fn validate(&self) -> Result<(), RpcError> {
        check_len("sessionId", &self.session_id, MAX_IDENTIFIER_CHARS)?;
        if self.prompt.is_empty() {
            return Err(RpcError::new(
                ErrorClass::InvalidRequest,
                "prompt must not be empty",
            ));
        }
        if self.prompt.len() > MAX_PROMPT_BYTES {
            return Err(RpcError::new(
                ErrorClass::InvalidRequest,
                format!("prompt exceeds {MAX_PROMPT_BYTES} bytes"),
            ));
        }
        if let Some(turn_id) = &self.turn_id {
            check_len("turnId", turn_id, MAX_IDENTIFIER_CHARS)?;
        }
        Ok(())
    }
}

/// Truncate a tool output to the wire bound, on a character boundary.
pub fn bound_tool_output(output: &str) -> (String, bool) {
    if output.len() <= MAX_TOOL_OUTPUT_BYTES {
        return (output.to_owned(), false);
    }
    let mut cut = MAX_TOOL_OUTPUT_BYTES;
    while cut > 0 && !output.is_char_boundary(cut) {
        cut -= 1;
    }
    (output[..cut].to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(line: &str) -> RequestFrame {
        match parse_frame(line.as_bytes()) {
            Ok(Incoming::Request(frame)) => frame,
            other => panic!("expected a request, got {other:?}"),
        }
    }

    #[test]
    fn initialize_request_round_trips() {
        let frame = request(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientName":"host","clientVersion":"1","capabilities":["descriptor"]}}"#,
        );
        assert_eq!(frame.id, RequestId::Number(1));
        match &frame.body {
            RequestBody::Initialize(params) => {
                assert_eq!(params.client_name.as_deref(), Some("host"));
                assert_eq!(
                    params.required_capabilities().expect("known"),
                    vec!["descriptor"]
                );
            }
            other => panic!("expected initialize, got {other:?}"),
        }
        let encoded = encode(&frame).expect("serialize");
        assert_eq!(
            encoded,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientName":"host","clientVersion":"1","capabilities":["descriptor"]}}"#
        );
    }

    #[test]
    fn initialize_result_golden_frame() {
        let result = InitializeResult {
            api_version: API_VERSION,
            codegg_version: "0.1.0".to_owned(),
            capabilities: CAPABILITIES
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        };
        assert_eq!(
            serde_json::to_value(&result).expect("serialize"),
            json!({
                "apiVersion": 1,
                "codeggVersion": "0.1.0",
                "capabilities": ["descriptor", "streaming", "cancellation", "usage"]
            })
        );
    }

    #[test]
    fn session_open_round_trips_with_a_descriptor_object() {
        let frame = request(
            r#"{"jsonrpc":"2.0","id":"open-1","method":"session/open","params":{"workspace":"/workspace","model":"openai/x","agent":"build","approval":"yolo","sandbox":"full-host","provider":{"id":"eggui","baseUrl":"https://example.invalid/v1","protocol":"chat_completions","credential":{"kind":"api_key","value":"dummy"}}}}"#,
        );
        assert_eq!(frame.id, RequestId::Text("open-1".to_owned()));
        match &frame.body {
            RequestBody::SessionOpen(params) => {
                params.validate().expect("valid");
                let provider = params.provider.as_ref().expect("descriptor");
                assert_eq!(provider["id"], "eggui");
                assert_eq!(provider["credential"]["value"], "dummy");
            }
            other => panic!("expected session/open, got {other:?}"),
        }
        let encoded = encode(&frame).expect("serialize");
        assert!(encoded.contains(r#""method":"session/open""#));
        assert!(encoded.contains(r#""workspace":"/workspace""#));
    }

    #[test]
    fn unknown_fields_are_ignored_and_known_ones_survive() {
        let frame = request(
            r#"{"jsonrpc":"2.0","id":7,"method":"turn/submit","params":{"sessionId":"s1","prompt":"hi","futureField":true},"extra":1}"#,
        );
        match &frame.body {
            RequestBody::TurnSubmit(params) => {
                params.validate().expect("valid");
                assert_eq!(params.prompt, "hi");
            }
            other => panic!("expected turn/submit, got {other:?}"),
        }
    }

    #[test]
    fn turn_frames_round_trip() {
        let submit = request(
            r#"{"jsonrpc":"2.0","id":3,"method":"turn/submit","params":{"sessionId":"s1","turnId":"t1","prompt":"go"}}"#,
        );
        match &submit.body {
            RequestBody::TurnSubmit(params) => {
                params.validate().expect("valid");
                assert_eq!(params.turn_id.as_deref(), Some("t1"));
            }
            other => panic!("expected turn/submit, got {other:?}"),
        }
        let cancel = request(
            r#"{"jsonrpc":"2.0","id":4,"method":"turn/cancel","params":{"sessionId":"s1","turnId":"t1"}}"#,
        );
        assert_eq!(cancel.body.method(), "turn/cancel");
        let close = request(
            r#"{"jsonrpc":"2.0","id":5,"method":"session/close","params":{"sessionId":"s1"}}"#,
        );
        assert_eq!(close.body.method(), "session/close");
    }

    #[test]
    fn notification_frames_are_refused_and_requests_are_not() {
        let incoming =
            parse_frame(br#"{"jsonrpc":"2.0","method":"turn/text","params":{}}"#).expect("parses");
        match incoming {
            Incoming::Notification { method } => assert_eq!(method, "turn/text"),
            other => panic!("expected notification, got {other:?}"),
        }
        let response = parse_frame(br#"{"jsonrpc":"2.0","id":1,"result":{}}"#).expect("parses");
        assert!(matches!(response, Incoming::Unusable { .. }));
        let bare = parse_frame(br#"{"hello":"world"}"#).expect("parses");
        assert!(matches!(bare, Incoming::Unusable { .. }));
    }

    #[test]
    fn unknown_method_is_refused_with_its_id() {
        let incoming =
            parse_frame(br#"{"jsonrpc":"2.0","id":1,"method":"turn/steer","params":{}}"#)
                .expect("parses");
        match incoming {
            Incoming::UnknownMethod { id, method } => {
                assert_eq!(id, RequestId::Number(1));
                assert_eq!(method, "turn/steer");
            }
            other => panic!("expected an unknown method, got {other:?}"),
        }
    }

    #[test]
    fn a_known_method_with_bad_params_keeps_its_id() {
        let incoming =
            parse_frame(br#"{"jsonrpc":"2.0","id":9,"method":"turn/submit","params":{}}"#)
                .expect("parses");
        match incoming {
            Incoming::Unusable { id, detail } => {
                assert_eq!(id, Some(RequestId::Number(9)));
                assert!(detail.contains("turn/submit") || detail.contains("missing"));
            }
            other => panic!("expected an unusable request, got {other:?}"),
        }
    }

    #[test]
    fn capabilities_that_v1_does_not_have_are_refused() {
        let params = InitializeParams {
            capabilities: Some(vec!["descriptor".to_owned(), "telepathy".to_owned()]),
            ..Default::default()
        };
        let error = params.required_capabilities().expect_err("refused");
        assert_eq!(error.code, code::UNSUPPORTED_CAPABILITY);
        assert_eq!(
            error.data.expect("class").class,
            ErrorClass::UnsupportedCapability
        );
    }

    #[test]
    fn params_bounds_name_the_field_not_the_value() {
        let params = SessionOpenParams {
            workspace: String::new(),
            model: "m".to_owned(),
            agent: None,
            approval: None,
            sandbox: None,
            session_id: None,
            provider: None,
        };
        let error = params.validate().expect_err("empty workspace refused");
        assert!(error.message.contains("workspace"));
        assert!(!error.message.contains("model"));

        let prompt = "x".repeat(MAX_PROMPT_BYTES + 1);
        let params = TurnSubmitParams {
            session_id: "s1".to_owned(),
            prompt,
            turn_id: None,
        };
        let error = params.validate().expect_err("oversized prompt refused");
        assert!(error.message.contains("prompt"));
    }

    #[test]
    fn initialize_bounds_are_enforced() {
        let params = InitializeParams {
            client_name: Some("n".repeat(MAX_NAME_CHARS + 1)),
            ..Default::default()
        };
        let error = params.validate().expect_err("refused");
        assert!(error.message.contains("clientName"));
    }

    #[test]
    fn text_notification_golden_frame() {
        let frame = NotificationFrame {
            jsonrpc: "2.0".to_owned(),
            body: NotificationBody::Text(TurnText {
                session_id: "s1".to_owned(),
                turn_id: "t1".to_owned(),
                delta: "I'll ".to_owned(),
            }),
        };
        assert_eq!(
            encode(&frame).expect("serialize"),
            r#"{"jsonrpc":"2.0","method":"turn/text","params":{"sessionId":"s1","turnId":"t1","delta":"I'll "}}"#
        );
    }

    #[test]
    fn tool_notifications_golden_frames() {
        let started = NotificationBody::Tool(TurnTool {
            session_id: "s1".to_owned(),
            turn_id: "t1".to_owned(),
            phase: ToolPhase::Started,
            tool_id: "call_1".to_owned(),
            tool_name: "write".to_owned(),
            arguments: Some(r#"{"path":"hello.txt"}"#.to_owned()),
            output: None,
            success: None,
            truncated: None,
        });
        assert_eq!(
            serde_json::to_value(&started).expect("serialize"),
            json!({"method":"turn/tool","params":{
                "sessionId":"s1","turnId":"t1","phase":"started",
                "toolId":"call_1","toolName":"write",
                "arguments":"{\"path\":\"hello.txt\"}"}})
        );
        let finished = NotificationBody::Tool(TurnTool {
            session_id: "s1".to_owned(),
            turn_id: "t1".to_owned(),
            phase: ToolPhase::Finished,
            tool_id: "call_1".to_owned(),
            tool_name: "write".to_owned(),
            arguments: None,
            output: Some("wrote 2 bytes".to_owned()),
            success: Some(true),
            truncated: Some(false),
        });
        assert_eq!(
            serde_json::to_value(&finished).expect("serialize"),
            json!({"method":"turn/tool","params":{
                "sessionId":"s1","turnId":"t1","phase":"finished",
                "toolId":"call_1","toolName":"write",
                "output":"wrote 2 bytes","success":true,"truncated":false}})
        );
    }

    #[test]
    fn usage_absence_is_not_zero() {
        let absent = NotificationBody::Usage(TurnUsage {
            session_id: "s1".to_owned(),
            turn_id: "t1".to_owned(),
            usage: Usage::absent(),
        });
        assert_eq!(
            serde_json::to_value(&absent).expect("serialize"),
            json!({"method":"turn/usage","params":{
                "sessionId":"s1","turnId":"t1","reported":false}})
        );
        let reported = NotificationBody::Usage(TurnUsage {
            session_id: "s1".to_owned(),
            turn_id: "t1".to_owned(),
            usage: Usage::reported(Some(812), Some(96), Some(640)),
        });
        assert_eq!(
            serde_json::to_value(&reported).expect("serialize"),
            json!({"method":"turn/usage","params":{
                "sessionId":"s1","turnId":"t1","reported":true,
                "inputTokens":812,"outputTokens":96,"cachedTokens":640}})
        );
    }

    #[test]
    fn done_and_error_golden_frames() {
        let done = NotificationBody::Done(TurnDone {
            session_id: "s1".to_owned(),
            turn_id: "t1".to_owned(),
            stop_reason: StopReason::EndTurn,
            usage: Usage::reported(Some(812), Some(96), None),
        });
        assert_eq!(
            serde_json::to_value(&done).expect("serialize"),
            json!({"method":"turn/done","params":{
                "sessionId":"s1","turnId":"t1","stopReason":"end_turn",
                "usage":{"reported":true,"inputTokens":812,"outputTokens":96}}})
        );
        let cancelled = NotificationBody::Done(TurnDone {
            session_id: "s1".to_owned(),
            turn_id: "t2".to_owned(),
            stop_reason: StopReason::Cancelled,
            usage: Usage::absent(),
        });
        assert_eq!(
            serde_json::to_value(&cancelled).expect("serialize")["params"]["stopReason"],
            "cancelled"
        );
        let error = NotificationBody::Error(TurnError::new(
            "s1",
            "t1",
            ErrorClass::Auth,
            "upstream rejected the credential",
            Usage::absent(),
        ));
        assert_eq!(
            serde_json::to_value(&error).expect("serialize"),
            json!({"method":"turn/error","params":{
                "sessionId":"s1","turnId":"t1","class":"auth",
                "message":"upstream rejected the credential","retryable":false,
                "usage":{"reported":false}}})
        );
    }

    #[test]
    fn busy_error_golden_frame() {
        let error = RpcError::new(ErrorClass::Busy, "a turn is already active");
        assert_eq!(
            serde_json::to_value(&error).expect("serialize"),
            json!({"code":-32001,"message":"a turn is already active",
                   "data":{"class":"busy","retryable":true}})
        );
        let response = ResponseFrame::error(RequestId::Number(7), error);
        assert_eq!(
            encode(&response).expect("serialize"),
            r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32001,"message":"a turn is already active","data":{"class":"busy","retryable":true}}}"#
        );
    }

    #[test]
    fn provider_status_travels_when_known() {
        let error = RpcError::with_status(ErrorClass::Auth, "unauthorized", 401);
        let data = error.data.expect("data");
        assert_eq!(data.provider_status, Some(401));
        assert_eq!(data.class, ErrorClass::Auth);
    }

    #[test]
    fn every_class_round_trips_and_is_camel_free() {
        let classes = [
            (ErrorClass::Auth, "auth"),
            (ErrorClass::ModelNotFound, "model_not_found"),
            (ErrorClass::ModelNotSupported, "model_not_supported"),
            (ErrorClass::ContextLength, "context_length"),
            (ErrorClass::RateLimit, "rate_limit"),
            (ErrorClass::InvalidRequest, "invalid_request"),
            (ErrorClass::Transport, "transport"),
            (ErrorClass::Protocol, "protocol"),
            (ErrorClass::Upstream, "upstream"),
            (ErrorClass::Busy, "busy"),
            (ErrorClass::Cancelled, "cancelled"),
            (ErrorClass::Workspace, "workspace"),
            (ErrorClass::NotInitialized, "not_initialized"),
            (ErrorClass::UnsupportedCapability, "unsupported_capability"),
            (ErrorClass::Internal, "internal"),
        ];
        for (class, wire) in classes {
            assert_eq!(serde_json::to_value(class).expect("serialize"), json!(wire));
            let parsed: ErrorClass = serde_json::from_value(json!(wire)).expect("parse");
            assert_eq!(parsed, class);
        }
    }

    #[test]
    fn malformed_and_oversized_frames_are_refused() {
        assert!(matches!(
            parse_frame(b"not json"),
            Err(FrameError::Malformed)
        ));
        assert!(matches!(
            parse_frame(&vec![b'x'; MAX_FRAME_BYTES + 1]),
            Err(FrameError::TooLarge)
        ));
        assert!(matches!(
            parse_frame(&[0xff, 0xfe]),
            Err(FrameError::Malformed)
        ));
        let error = FrameError::TooLarge.rpc();
        assert_eq!(error.code, code::INVALID);
    }

    #[test]
    fn tool_output_is_bounded_on_a_character_boundary() {
        let small = "ok";
        assert_eq!(bound_tool_output(small), ("ok".to_owned(), false));
        let large = "é".repeat(MAX_TOOL_OUTPUT_BYTES);
        let (bounded, truncated) = bound_tool_output(&large);
        assert!(truncated);
        assert!(bounded.len() <= MAX_TOOL_OUTPUT_BYTES);
        assert!(bounded.chars().all(|character| character == 'é'));
    }
}
