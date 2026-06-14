use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

// --- Scenario types ---

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    steps: Vec<Step>,
    exit: ExitConfig,
    #[serde(default = "default_strict")]
    strict: bool,
}

fn default_strict() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Step {
    ExpectRequest {
        method: String,
        #[serde(default)]
        id: IdMatcher,
        #[serde(default)]
        params: ValueMatcher,
        #[serde(default)]
        then: Vec<Action>,
    },
    ExpectNotification {
        method: String,
        #[serde(default)]
        params: ValueMatcher,
        #[serde(default)]
        then: Vec<Action>,
    },
    ExpectResponse {
        id: IdMatcher,
        #[serde(default)]
        result: Option<ValueMatcher>,
        #[serde(default)]
        error: Option<ErrorMatcher>,
        #[serde(default)]
        then: Vec<Action>,
    },
    AllowNotification {
        method: String,
    },
    AllowRequest {
        method: String,
    },
    SendNotification {
        method: String,
        params: serde_json::Value,
    },
    SendRequest {
        method: String,
        params: serde_json::Value,
    },
    Delay {
        millis: u64,
    },
    #[serde(alias = "ExitNow")]
    Exit {
        code: i32,
    },
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
enum Action {
    RespondResult {
        result: serde_json::Value,
    },
    RespondError {
        code: i64,
        message: String,
        #[serde(default)]
        data: Option<serde_json::Value>,
    },
    SendNotification {
        method: String,
        params: serde_json::Value,
    },
    SendRequest {
        method: String,
        params: serde_json::Value,
    },
    SendRawBytes {
        bytes_base64: String,
    },
    SendRawFrame {
        body: String,
    },
    SendJsonWithDeclaredLength {
        value: serde_json::Value,
        declared_length: usize,
    },
    SendHeaderOnly {
        header: String,
    },
    SendBodyChunks {
        header: String,
        chunks: Vec<String>,
        delay_millis: u64,
    },
    SendFramesTogether {
        messages: Vec<serde_json::Value>,
    },
    CloseStdout,
    #[serde(alias = "ExitNow")]
    Exit {
        code: i32,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ExitConfig {
    ExitCode { code: i32 },
}

// --- Matchers ---

#[derive(Debug, Clone)]
enum IdMatcher {
    Any,
    Exact(serde_json::Value),
    Number,
    String,
}

impl Default for IdMatcher {
    fn default() -> Self {
        Self::Any
    }
}

impl<'de> Deserialize<'de> for IdMatcher {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(match value {
            serde_json::Value::Object(map) => parse_id_matcher_object(map),
            other => Self::Exact(other),
        })
    }
}

impl IdMatcher {
    fn matches(&self, actual: &serde_json::Value) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(expected) => expected == actual,
            Self::Number => actual.is_number(),
            Self::String => actual.is_string(),
        }
    }

    fn summary(&self) -> String {
        match self {
            Self::Any => "any".to_string(),
            Self::Exact(value) => format!("exact {}", compact_json(value)),
            Self::Number => "number".to_string(),
            Self::String => "string".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
enum ValueMatcher {
    Any,
    Exact(serde_json::Value),
    Null,
    ObjectContains(BTreeMap<String, ValueMatcher>),
    ArrayLen(usize),
    StringType,
    NumberType,
    BoolType,
    String(String),
    Number(i64),
    Bool(bool),
}

impl Default for ValueMatcher {
    fn default() -> Self {
        Self::Any
    }
}

impl<'de> Deserialize<'de> for ValueMatcher {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(match value {
            serde_json::Value::Object(map) => parse_value_matcher_object(map),
            other => Self::Exact(other),
        })
    }
}

impl ValueMatcher {
    fn matches(&self, actual: Option<&serde_json::Value>) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(expected) => actual == Some(expected),
            Self::Null => matches!(actual, Some(serde_json::Value::Null)),
            Self::ObjectContains(expected_fields) => match actual {
                Some(serde_json::Value::Object(actual_fields)) => expected_fields
                    .iter()
                    .all(|(key, matcher)| matcher.matches(actual_fields.get(key))),
                _ => false,
            },
            Self::ArrayLen(expected_len) => matches!(
                actual,
                Some(serde_json::Value::Array(items)) if items.len() == *expected_len
            ),
            Self::StringType => matches!(actual, Some(serde_json::Value::String(_))),
            Self::NumberType => matches!(actual, Some(serde_json::Value::Number(_))),
            Self::BoolType => matches!(actual, Some(serde_json::Value::Bool(_))),
            Self::String(expected) => matches!(
                actual,
                Some(serde_json::Value::String(actual_value)) if actual_value == expected
            ),
            Self::Number(expected) => matches!(
                actual,
                Some(serde_json::Value::Number(actual_value))
                    if number_matches_i64(actual_value, *expected)
            ),
            Self::Bool(expected) => matches!(
                actual,
                Some(serde_json::Value::Bool(actual_value)) if actual_value == expected
            ),
        }
    }

    fn summary(&self) -> String {
        match self {
            Self::Any => "any".to_string(),
            Self::Exact(value) => format!("exact {}", compact_json(value)),
            Self::Null => "null".to_string(),
            Self::ObjectContains(fields) => {
                let keys = fields.keys().cloned().collect::<Vec<_>>().join(",");
                format!("contains {{{keys}}}")
            }
            Self::ArrayLen(len) => format!("array_len={len}"),
            Self::StringType => "string".to_string(),
            Self::NumberType => "number".to_string(),
            Self::BoolType => "bool".to_string(),
            Self::String(value) => format!("string {value:?}"),
            Self::Number(value) => format!("number {value}"),
            Self::Bool(value) => format!("bool {value}"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ErrorMatcher {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<ValueMatcher>,
}

impl ErrorMatcher {
    fn matches(&self, actual: &JsonRpcError) -> bool {
        self.code == actual.code
            && self.message == actual.message
            && self
                .data
                .as_ref()
                .is_none_or(|matcher| matcher.matches(actual.data.as_ref()))
    }

    fn summary(&self) -> String {
        let mut out = format!("code={} message={:?}", self.code, self.message);
        if let Some(data) = &self.data {
            out.push_str(&format!(" data={}", data.summary()));
        }
        out
    }
}

fn parse_id_matcher_object(map: serde_json::Map<String, serde_json::Value>) -> IdMatcher {
    if map.get("any").and_then(serde_json::Value::as_bool) == Some(true) {
        return IdMatcher::Any;
    }
    if map.get("number").and_then(serde_json::Value::as_bool) == Some(true) {
        return IdMatcher::Number;
    }
    if map.get("string").and_then(serde_json::Value::as_bool) == Some(true) {
        return IdMatcher::String;
    }
    if let Some(tag) = map.get("type").and_then(serde_json::Value::as_str) {
        match tag {
            "Any" => return IdMatcher::Any,
            "Number" => return IdMatcher::Number,
            "String" => return IdMatcher::String,
            "Exact" => {
                return map
                    .get("value")
                    .cloned()
                    .map(IdMatcher::Exact)
                    .unwrap_or(IdMatcher::Any);
            }
            _ => {}
        }
    }
    if let Some(value) = map.get("exact").cloned() {
        return IdMatcher::Exact(value);
    }
    IdMatcher::Exact(serde_json::Value::Object(map))
}

fn parse_value_matcher_object(map: serde_json::Map<String, serde_json::Value>) -> ValueMatcher {
    if map.get("any").and_then(serde_json::Value::as_bool) == Some(true) {
        return ValueMatcher::Any;
    }
    if map.get("null").and_then(serde_json::Value::as_bool) == Some(true) {
        return ValueMatcher::Null;
    }
    if let Some(tag) = map.get("type").and_then(serde_json::Value::as_str) {
        match tag {
            "Any" => return ValueMatcher::Any,
            "Null" => return ValueMatcher::Null,
            "String" => {
                return map
                    .get("value")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| ValueMatcher::String(value.to_string()))
                    .unwrap_or(ValueMatcher::StringType);
            }
            "Number" => {
                return map
                    .get("value")
                    .and_then(serde_json::Value::as_i64)
                    .map(ValueMatcher::Number)
                    .unwrap_or(ValueMatcher::NumberType);
            }
            "Bool" => {
                return map
                    .get("value")
                    .and_then(serde_json::Value::as_bool)
                    .map(ValueMatcher::Bool)
                    .unwrap_or(ValueMatcher::BoolType);
            }
            "ArrayLen" => {
                return map
                    .get("value")
                    .and_then(serde_json::Value::as_u64)
                    .map(|value| ValueMatcher::ArrayLen(value as usize))
                    .unwrap_or(ValueMatcher::Any);
            }
            "ObjectContains" => {
                return map
                    .get("value")
                    .and_then(serde_json::Value::as_object)
                    .map(|value| ValueMatcher::ObjectContains(parse_matcher_map(value)))
                    .unwrap_or(ValueMatcher::Any);
            }
            "Exact" => {
                return map
                    .get("value")
                    .cloned()
                    .map(ValueMatcher::Exact)
                    .unwrap_or(ValueMatcher::Any);
            }
            _ => {}
        }
    }
    if let Some(value) = map.get("exact").cloned() {
        return ValueMatcher::Exact(value);
    }
    if let Some(value) = map.get("contains").and_then(serde_json::Value::as_object) {
        return ValueMatcher::ObjectContains(parse_matcher_map(value));
    }
    if let Some(value) = map
        .get("objectContains")
        .and_then(serde_json::Value::as_object)
    {
        return ValueMatcher::ObjectContains(parse_matcher_map(value));
    }
    if let Some(value) = map.get("len").and_then(serde_json::Value::as_u64) {
        return ValueMatcher::ArrayLen(value as usize);
    }
    if let Some(value) = map.get("arrayLen").and_then(serde_json::Value::as_u64) {
        return ValueMatcher::ArrayLen(value as usize);
    }
    ValueMatcher::Exact(serde_json::Value::Object(map))
}

fn parse_matcher_map(
    map: &serde_json::Map<String, serde_json::Value>,
) -> BTreeMap<String, ValueMatcher> {
    map.iter()
        .map(|(key, value)| (key.clone(), value.clone().into()))
        .collect()
}

impl From<serde_json::Value> for ValueMatcher {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Object(map) => parse_value_matcher_object(map),
            other => ValueMatcher::Exact(other),
        }
    }
}

fn number_matches_i64(actual: &serde_json::Number, expected: i64) -> bool {
    actual
        .as_i64()
        .map(|value| value == expected)
        .or_else(|| {
            actual
                .as_u64()
                .map(|value| expected >= 0 && value == expected as u64)
        })
        .unwrap_or(false)
}

fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<invalid-json>".to_string())
}

fn deserialize_present_value<'de, D>(deserializer: D) -> Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(serde_json::Value::deserialize(deserializer)?))
}

// --- JSON-RPC message types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    jsonrpc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<serde_json::Value>,
    #[serde(
        default,
        deserialize_with = "deserialize_present_value",
        skip_serializing_if = "Option::is_none"
    )]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

// --- Transcript ---

#[derive(Debug, Serialize)]
struct TranscriptEntry<'a> {
    direction: String,
    step_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actual_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actual_method: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actual_id: Option<&'a serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    match_result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mismatch_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a JsonRpcMessage>,
}

struct TranscriptWriter {
    writer: Box<dyn Write>,
}

impl TranscriptWriter {
    fn new(path: &PathBuf) -> io::Result<Self> {
        let file = File::create(path)?;
        Ok(Self {
            writer: Box::new(file),
        })
    }

    fn write_entry(&mut self, entry: &TranscriptEntry) -> io::Result<()> {
        let line = serde_json::to_string(entry)?;
        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;
        Ok(())
    }
}

// --- Framed I/O ---

struct FramedReader {
    reader: BufReader<io::Stdin>,
}

impl FramedReader {
    fn new() -> Self {
        Self {
            reader: BufReader::new(io::stdin()),
        }
    }

    fn read_message(&mut self) -> io::Result<Option<JsonRpcMessage>> {
        let mut content_length: Option<usize> = None;

        loop {
            let mut line = String::new();
            let bytes_read = self.reader.read_line(&mut line)?;
            if bytes_read == 0 {
                return Ok(None);
            }

            let line = line.trim();
            if line.is_empty() {
                break;
            }

            if let Some(val) = line.strip_prefix("Content-Length: ") {
                content_length = val.parse().ok();
            }
        }

        let length = content_length.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Missing Content-Length header")
        })?;

        let mut body = vec![0u8; length];
        self.reader.read_exact(&mut body)?;

        let msg: JsonRpcMessage = serde_json::from_slice(&body).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid JSON: {e}"))
        })?;

        Ok(Some(msg))
    }
}

struct FramedWriter {
    writer: io::Stdout,
}

impl FramedWriter {
    fn new() -> Self {
        Self {
            writer: io::stdout(),
        }
    }

    fn write_message(&mut self, msg: &JsonRpcMessage) -> io::Result<()> {
        let body = serde_json::to_string(msg)?;
        self.write_framed_body(body.as_bytes())
    }

    fn write_raw_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    fn write_raw_frame(&mut self, body: &str) -> io::Result<()> {
        self.write_framed_body(body.as_bytes())
    }

    fn write_json_with_declared_length(
        &mut self,
        value: &serde_json::Value,
        declared_length: usize,
    ) -> io::Result<()> {
        let body = serde_json::to_string(value)?;
        let header = format!("Content-Length: {declared_length}\r\n\r\n");
        self.writer.write_all(header.as_bytes())?;
        self.writer.write_all(body.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }

    fn write_header_only(&mut self, header: &str) -> io::Result<()> {
        self.writer.write_all(header.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }

    fn write_body_chunks(
        &mut self,
        header: &str,
        chunks: &[String],
        delay_millis: u64,
    ) -> io::Result<()> {
        self.writer.write_all(header.as_bytes())?;
        self.writer.flush()?;
        for (idx, chunk) in chunks.iter().enumerate() {
            self.writer.write_all(chunk.as_bytes())?;
            self.writer.flush()?;
            if idx + 1 != chunks.len() {
                thread::sleep(Duration::from_millis(delay_millis));
            }
        }
        Ok(())
    }

    fn write_frames_together(&mut self, messages: &[serde_json::Value]) -> io::Result<()> {
        let mut buffer = Vec::new();
        for value in messages {
            let body = serde_json::to_string(value)?;
            buffer.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
            buffer.extend_from_slice(body.as_bytes());
        }
        self.writer.write_all(&buffer)?;
        self.writer.flush()?;
        Ok(())
    }

    fn close_stdout(&mut self) -> io::Result<()> {
        self.writer.flush()?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = self.writer.as_raw_fd();
            let result = unsafe { libc::close(fd) };
            if result == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
        #[cfg(not(unix))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "CloseStdout is only supported on unix targets",
            ))
        }
    }

    fn write_framed_body(&mut self, body: &[u8]) -> io::Result<()> {
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.writer.write_all(header.as_bytes())?;
        self.writer.write_all(body)?;
        self.writer.flush()?;
        Ok(())
    }
}

// --- Helpers ---

fn next_request_id(counter: &mut u64) -> serde_json::Value {
    *counter += 1;
    serde_json::Value::Number((*counter).into())
}

fn message_category(msg: &JsonRpcMessage) -> &'static str {
    let is_method = msg.method.is_some();
    let is_id = msg.id.is_some();
    let is_result_or_error = msg.result.is_some() || msg.error.is_some();
    match (is_method, is_id, is_result_or_error) {
        (true, true, false) => "request",
        (true, false, false) => "notification",
        (false, true, true) => "response",
        _ => "other",
    }
}

fn response_matches(
    msg: &JsonRpcMessage,
    expected_id: &IdMatcher,
    expected_result: &Option<ValueMatcher>,
    expected_error: &Option<ErrorMatcher>,
) -> Result<(), String> {
    if expected_result.is_some() && expected_error.is_some() {
        return Err("response cannot require both result and error".to_string());
    }
    if !is_response(msg) {
        return Err("received non-response message instead of response".to_string());
    }

    if !msg.id.as_ref().is_some_and(|id| expected_id.matches(id)) {
        return Err(format!(
            "response id did not match expected {}",
            expected_id.summary()
        ));
    }

    match (&msg.result, &msg.error, expected_result, expected_error) {
        (Some(result), None, Some(matcher), None) => {
            if matcher.matches(Some(result)) {
                Ok(())
            } else {
                Err(format!(
                    "response result did not match expected {}",
                    matcher.summary()
                ))
            }
        }
        (None, Some(error), None, Some(matcher)) => {
            if matcher.matches(error) {
                Ok(())
            } else {
                Err(format!(
                    "response error did not match expected {}",
                    matcher.summary()
                ))
            }
        }
        (Some(_), None, None, Some(_)) => Err("expected error response but received result".into()),
        (None, Some(_), Some(_), None) => Err("expected result response but received error".into()),
        (Some(_), Some(_), _, _) => Err("response contained both result and error".into()),
        (None, None, _, _) => Err("response missing result/error".into()),
        _ => Ok(()),
    }
}

fn request_matches(
    msg: &JsonRpcMessage,
    expected_method: &str,
    expected_id: &IdMatcher,
    expected_params: &ValueMatcher,
) -> Result<(), String> {
    if !is_request(msg) {
        return Err("received non-request message instead of request".to_string());
    }
    if msg.method.as_deref() != Some(expected_method) {
        return Err(format!(
            "expected request method {expected_method:?} but received {:?}",
            msg.method
        ));
    }
    let actual_id = msg
        .id
        .as_ref()
        .ok_or_else(|| "request missing id".to_string())?;
    if !expected_id.matches(actual_id) {
        return Err(format!(
            "request id did not match expected {}",
            expected_id.summary()
        ));
    }
    if !expected_params.matches(msg.params.as_ref()) {
        return Err(format!(
            "request params did not match expected {}",
            expected_params.summary()
        ));
    }
    Ok(())
}

fn notification_matches(
    msg: &JsonRpcMessage,
    expected_method: &str,
    expected_params: &ValueMatcher,
) -> Result<(), String> {
    if !is_notification(msg) {
        return Err("received non-notification message instead of notification".to_string());
    }
    if msg.method.as_deref() != Some(expected_method) {
        return Err(format!(
            "expected notification method {expected_method:?} but received {:?}",
            msg.method
        ));
    }
    if !expected_params.matches(msg.params.as_ref()) {
        return Err(format!(
            "notification params did not match expected {}",
            expected_params.summary()
        ));
    }
    Ok(())
}

fn record_message_entry(
    transcript: &mut TranscriptWriter,
    direction: &str,
    step_index: usize,
    msg: &JsonRpcMessage,
) {
    transcript
        .write_entry(&TranscriptEntry {
            direction: direction.to_string(),
            step_index,
            event: None,
            expected_summary: None,
            actual_category: None,
            actual_method: None,
            actual_id: None,
            match_result: None,
            mismatch_reason: None,
            message: Some(msg),
        })
        .ok();
}

fn record_action_event(
    transcript: &mut TranscriptWriter,
    step_index: usize,
    event: &str,
    detail: &str,
) {
    transcript
        .write_entry(&TranscriptEntry {
            direction: "sent".to_string(),
            step_index,
            event: Some(event.to_string()),
            expected_summary: Some(detail.to_string()),
            actual_category: Some("stdout".to_string()),
            actual_method: None,
            actual_id: None,
            match_result: Some("raw".to_string()),
            mismatch_reason: None,
            message: None,
        })
        .ok();
}

fn record_allowed_entry(
    transcript: &mut TranscriptWriter,
    step_index: usize,
    msg: &JsonRpcMessage,
    reason: &str,
) {
    transcript
        .write_entry(&TranscriptEntry {
            direction: "recv".to_string(),
            step_index,
            event: Some("AllowedMessage".to_string()),
            expected_summary: Some(reason.to_string()),
            actual_category: Some(message_category(msg).to_string()),
            actual_method: msg.method.as_deref(),
            actual_id: msg.id.as_ref(),
            match_result: Some("allowed".to_string()),
            mismatch_reason: None,
            message: Some(msg),
        })
        .ok();
}

fn record_allow_step(
    transcript: &mut TranscriptWriter,
    step_index: usize,
    method: &str,
    kind: &str,
) {
    transcript
        .write_entry(&TranscriptEntry {
            direction: "sent".to_string(),
            step_index,
            event: Some(format!("Allow{kind}")),
            expected_summary: Some(method.to_string()),
            actual_category: Some("scenario".to_string()),
            actual_method: Some(method),
            actual_id: None,
            match_result: Some("allowlisted".to_string()),
            mismatch_reason: None,
            message: None,
        })
        .ok();
}

fn record_failure_entry(
    transcript: &mut TranscriptWriter,
    step_index: usize,
    expected_summary: String,
    actual: Option<&JsonRpcMessage>,
    mismatch_reason: String,
) {
    transcript
        .write_entry(&TranscriptEntry {
            direction: "diag".to_string(),
            step_index,
            event: Some("StepMismatch".to_string()),
            expected_summary: Some(expected_summary),
            actual_category: actual.map(message_category).map(str::to_string),
            actual_method: actual.and_then(|msg| msg.method.as_deref()),
            actual_id: actual.and_then(|msg| msg.id.as_ref()),
            match_result: Some("mismatch".to_string()),
            mismatch_reason: Some(mismatch_reason),
            message: actual,
        })
        .ok();
}

fn summarize_expect_request(method: &str, id: &IdMatcher, params: &ValueMatcher) -> String {
    format!(
        "request method={method:?} id={} params={}",
        id.summary(),
        params.summary()
    )
}

fn summarize_expect_notification(method: &str, params: &ValueMatcher) -> String {
    format!("notification method={method:?} params={}", params.summary())
}

fn summarize_expect_response(
    id: &IdMatcher,
    result: &Option<ValueMatcher>,
    error: &Option<ErrorMatcher>,
) -> String {
    let mut out = format!("response id={}", id.summary());
    if let Some(result) = result {
        out.push_str(&format!(" result={}", result.summary()));
    }
    if let Some(error) = error {
        out.push_str(&format!(" error={}", error.summary()));
    }
    out
}

fn read_until_request(
    reader: &mut FramedReader,
    transcript: &mut TranscriptWriter,
    step_idx: usize,
    expected_method: &str,
    expected_id: &IdMatcher,
    expected_params: &ValueMatcher,
    strict: bool,
    allowed_requests: &HashSet<String>,
) -> Result<Option<JsonRpcMessage>, StepFailure> {
    let expected_summary = summarize_expect_request(expected_method, expected_id, expected_params);
    loop {
        match reader.read_message() {
            Ok(Some(msg)) => {
                record_message_entry(transcript, "recv", step_idx, &msg);
                if msg
                    .method
                    .as_deref()
                    .is_some_and(|method| allowed_requests.contains(method))
                    && is_request(&msg)
                {
                    let reason = format!("allowed request {}", msg.method.as_deref().unwrap());
                    record_allowed_entry(transcript, step_idx, &msg, &reason);
                    continue;
                }
                match request_matches(&msg, expected_method, expected_id, expected_params) {
                    Ok(()) => return Ok(Some(msg)),
                    Err(reason) if strict => {
                        return Err(StepFailure::new(
                            step_idx,
                            expected_summary,
                            Some(msg),
                            reason,
                        ));
                    }
                    Err(_) => continue,
                }
            }
            Ok(None) => return Ok(None),
            Err(e) => {
                if strict {
                    return Err(StepFailure::new(
                        step_idx,
                        expected_summary,
                        None,
                        format!("read error: {e}"),
                    ));
                }
                return Ok(None);
            }
        }
    }
}

fn read_until_notification(
    reader: &mut FramedReader,
    transcript: &mut TranscriptWriter,
    step_idx: usize,
    expected_method: &str,
    expected_params: &ValueMatcher,
    strict: bool,
    allowed_notifications: &HashSet<String>,
) -> Result<Option<JsonRpcMessage>, StepFailure> {
    let expected_summary = summarize_expect_notification(expected_method, expected_params);
    loop {
        match reader.read_message() {
            Ok(Some(msg)) => {
                record_message_entry(transcript, "recv", step_idx, &msg);
                if msg
                    .method
                    .as_deref()
                    .is_some_and(|method| allowed_notifications.contains(method))
                    && is_notification(&msg)
                {
                    let reason = format!("allowed notification {}", msg.method.as_deref().unwrap());
                    record_allowed_entry(transcript, step_idx, &msg, &reason);
                    continue;
                }
                match notification_matches(&msg, expected_method, expected_params) {
                    Ok(()) => return Ok(Some(msg)),
                    Err(reason) if strict => {
                        return Err(StepFailure::new(
                            step_idx,
                            expected_summary,
                            Some(msg),
                            reason,
                        ));
                    }
                    Err(_) => continue,
                }
            }
            Ok(None) => return Ok(None),
            Err(e) => {
                if strict {
                    return Err(StepFailure::new(
                        step_idx,
                        expected_summary,
                        None,
                        format!("read error: {e}"),
                    ));
                }
                return Ok(None);
            }
        }
    }
}

fn read_until_response(
    reader: &mut FramedReader,
    transcript: &mut TranscriptWriter,
    step_idx: usize,
    expected_id: &IdMatcher,
    expected_result: &Option<ValueMatcher>,
    expected_error: &Option<ErrorMatcher>,
    strict: bool,
) -> Result<Option<JsonRpcMessage>, StepFailure> {
    let expected_summary = summarize_expect_response(expected_id, expected_result, expected_error);
    loop {
        match reader.read_message() {
            Ok(Some(msg)) => {
                record_message_entry(transcript, "recv", step_idx, &msg);
                match response_matches(&msg, expected_id, expected_result, expected_error) {
                    Ok(()) => return Ok(Some(msg)),
                    Err(reason) if strict => {
                        return Err(StepFailure::new(
                            step_idx,
                            expected_summary,
                            Some(msg),
                            reason,
                        ));
                    }
                    Err(_) => continue,
                }
            }
            Ok(None) => return Ok(None),
            Err(e) => {
                if strict {
                    return Err(StepFailure::new(
                        step_idx,
                        expected_summary,
                        None,
                        format!("read error: {e}"),
                    ));
                }
                return Ok(None);
            }
        }
    }
}

#[derive(Debug)]
struct StepFailure {
    step_index: usize,
    expected_summary: String,
    actual: Option<JsonRpcMessage>,
    mismatch_reason: String,
}

impl StepFailure {
    fn new(
        step_index: usize,
        expected_summary: String,
        actual: Option<JsonRpcMessage>,
        mismatch_reason: String,
    ) -> Self {
        Self {
            step_index,
            expected_summary,
            actual,
            mismatch_reason,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionControl {
    Continue,
    Exit(i32),
}

fn execute_actions(
    actions: &[Action],
    request_id: &Option<serde_json::Value>,
    writer: &mut FramedWriter,
    transcript: &mut TranscriptWriter,
    step_idx: usize,
    request_counter: &mut u64,
) -> ActionControl {
    for action in actions {
        match action {
            Action::RespondResult { result } => {
                let msg = JsonRpcMessage {
                    jsonrpc: Some("2.0".to_string()),
                    method: None,
                    params: None,
                    id: request_id.clone(),
                    result: Some(result.clone()),
                    error: None,
                };
                writer.write_message(&msg).unwrap_or_else(|e| {
                    panic!("Failed to send response: {e}");
                });
                record_message_entry(transcript, "sent", step_idx, &msg);
            }
            Action::RespondError {
                code,
                message,
                data,
            } => {
                let msg = JsonRpcMessage {
                    jsonrpc: Some("2.0".to_string()),
                    method: None,
                    params: None,
                    id: request_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: *code,
                        message: message.clone(),
                        data: data.clone(),
                    }),
                };
                writer.write_message(&msg).unwrap_or_else(|e| {
                    panic!("Failed to send error response: {e}");
                });
                record_message_entry(transcript, "sent", step_idx, &msg);
            }
            Action::SendNotification { method, params } => {
                let msg = JsonRpcMessage {
                    jsonrpc: Some("2.0".to_string()),
                    method: Some(method.clone()),
                    params: Some(params.clone()),
                    id: None,
                    result: None,
                    error: None,
                };
                writer.write_message(&msg).unwrap_or_else(|e| {
                    panic!("Failed to send notification {method}: {e}");
                });
                record_message_entry(transcript, "sent", step_idx, &msg);
            }
            Action::SendRequest { method, params } => {
                let id = next_request_id(request_counter);
                let msg = JsonRpcMessage {
                    jsonrpc: Some("2.0".to_string()),
                    method: Some(method.clone()),
                    params: Some(params.clone()),
                    id: Some(id),
                    result: None,
                    error: None,
                };
                writer.write_message(&msg).unwrap_or_else(|e| {
                    panic!("Failed to send request {method}: {e}");
                });
                record_message_entry(transcript, "sent", step_idx, &msg);
            }
            Action::SendRawBytes { bytes_base64 } => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(bytes_base64.as_bytes())
                    .unwrap_or_else(|e| panic!("Invalid base64 raw bytes: {e}"));
                writer.write_raw_bytes(&bytes).unwrap_or_else(|e| {
                    panic!("Failed to send raw bytes: {e}");
                });
                record_action_event(transcript, step_idx, "SendRawBytes", "raw bytes");
            }
            Action::SendRawFrame { body } => {
                writer.write_raw_frame(body).unwrap_or_else(|e| {
                    panic!("Failed to send raw frame: {e}");
                });
                record_action_event(transcript, step_idx, "SendRawFrame", body);
            }
            Action::SendJsonWithDeclaredLength {
                value,
                declared_length,
            } => {
                writer
                    .write_json_with_declared_length(value, *declared_length)
                    .unwrap_or_else(|e| panic!("Failed to send json with declared length: {e}"));
                record_action_event(
                    transcript,
                    step_idx,
                    "SendJsonWithDeclaredLength",
                    &compact_json(value),
                );
            }
            Action::SendHeaderOnly { header } => {
                writer.write_header_only(header).unwrap_or_else(|e| {
                    panic!("Failed to send header-only output: {e}");
                });
                record_action_event(transcript, step_idx, "SendHeaderOnly", header);
            }
            Action::SendBodyChunks {
                header,
                chunks,
                delay_millis,
            } => {
                writer
                    .write_body_chunks(header, chunks, *delay_millis)
                    .unwrap_or_else(|e| panic!("Failed to send body chunks: {e}"));
                record_action_event(transcript, step_idx, "SendBodyChunks", header);
            }
            Action::SendFramesTogether { messages } => {
                writer.write_frames_together(messages).unwrap_or_else(|e| {
                    panic!("Failed to send grouped frames: {e}");
                });
                record_action_event(
                    transcript,
                    step_idx,
                    "SendFramesTogether",
                    "multiple frames",
                );
            }
            Action::CloseStdout => {
                writer.close_stdout().unwrap_or_else(|e| {
                    panic!("Failed to close stdout: {e}");
                });
                record_action_event(transcript, step_idx, "CloseStdout", "stdout closed");
            }
            Action::Exit { code } => {
                return ActionControl::Exit(*code);
            }
        }
    }

    ActionControl::Continue
}

fn write_failure_diagnostics(transcript: &mut TranscriptWriter, failure: &StepFailure) {
    record_failure_entry(
        transcript,
        failure.step_index,
        failure.expected_summary.clone(),
        failure.actual.as_ref(),
        failure.mismatch_reason.clone(),
    );
}

fn is_request(msg: &JsonRpcMessage) -> bool {
    msg.id.is_some() && msg.method.is_some() && msg.result.is_none() && msg.error.is_none()
}

fn is_notification(msg: &JsonRpcMessage) -> bool {
    msg.id.is_none() && msg.method.is_some()
}

fn is_response(msg: &JsonRpcMessage) -> bool {
    msg.method.is_none() && msg.id.is_some() && (msg.result.is_some() || msg.error.is_some())
}

// --- Main ---

fn main() {
    let scenario_path =
        std::env::var("CODEGG_FAKE_LSP_SCENARIO").expect("CODEGG_FAKE_LSP_SCENARIO not set");

    let transcript_path =
        std::env::var("CODEGG_FAKE_LSP_TRANSCRIPT").expect("CODEGG_FAKE_LSP_TRANSCRIPT not set");

    let scenario_file = File::open(&scenario_path)
        .unwrap_or_else(|e| panic!("Failed to open scenario file {scenario_path}: {e}"));
    let scenario: Scenario = serde_json::from_reader(scenario_file)
        .unwrap_or_else(|e| panic!("Failed to parse scenario: {e}"));

    let mut transcript = TranscriptWriter::new(&PathBuf::from(&transcript_path))
        .unwrap_or_else(|e| panic!("Failed to create transcript file: {e}"));

    let mut reader = FramedReader::new();
    let mut writer = FramedWriter::new();
    let mut request_counter: u64 = 0;
    let mut allowed_requests: HashSet<String> = HashSet::new();
    let mut allowed_notifications: HashSet<String> = HashSet::new();
    let mut exit_code: i32 = match &scenario.exit {
        ExitConfig::ExitCode { code } => *code,
    };

    eprintln!("[fake-lsp] scenario: {}", scenario.name);
    eprintln!("[fake-lsp] steps: {}", scenario.steps.len());

    for (step_idx, step) in scenario.steps.iter().enumerate() {
        eprintln!("[fake-lsp] step {step_idx}: {step:?}");

        match step {
            Step::ExpectRequest {
                method,
                id,
                params,
                then,
            } => {
                let msg = match read_until_request(
                    &mut reader,
                    &mut transcript,
                    step_idx,
                    method,
                    id,
                    params,
                    scenario.strict,
                    &allowed_requests,
                ) {
                    Ok(msg) => msg,
                    Err(failure) => {
                        eprintln!(
                            "[fake-lsp] strict failure at step {step_idx}: {}",
                            failure.mismatch_reason
                        );
                        write_failure_diagnostics(&mut transcript, &failure);
                        exit_code = 1;
                        break;
                    }
                };

                if let Some(msg) = msg {
                    match execute_actions(
                        then,
                        &msg.id,
                        &mut writer,
                        &mut transcript,
                        step_idx,
                        &mut request_counter,
                    ) {
                        ActionControl::Continue => {}
                        ActionControl::Exit(code) => {
                            exit_code = code;
                            break;
                        }
                    }
                } else if scenario.strict {
                    eprintln!("[fake-lsp] EOF waiting for request {method} in strict mode");
                    exit_code = 1;
                    break;
                }
            }
            Step::ExpectNotification {
                method,
                params,
                then,
            } => {
                let msg = match read_until_notification(
                    &mut reader,
                    &mut transcript,
                    step_idx,
                    method,
                    params,
                    scenario.strict,
                    &allowed_notifications,
                ) {
                    Ok(msg) => msg,
                    Err(failure) => {
                        eprintln!(
                            "[fake-lsp] strict failure at step {step_idx}: {}",
                            failure.mismatch_reason
                        );
                        write_failure_diagnostics(&mut transcript, &failure);
                        exit_code = 1;
                        break;
                    }
                };

                if let Some(_msg) = msg {
                    match execute_actions(
                        then,
                        &None,
                        &mut writer,
                        &mut transcript,
                        step_idx,
                        &mut request_counter,
                    ) {
                        ActionControl::Continue => {}
                        ActionControl::Exit(code) => {
                            exit_code = code;
                            break;
                        }
                    }
                } else if scenario.strict {
                    eprintln!("[fake-lsp] EOF waiting for notification {method} in strict mode");
                    exit_code = 1;
                    break;
                }
            }
            Step::ExpectResponse {
                id,
                result,
                error,
                then,
            } => {
                let msg = match read_until_response(
                    &mut reader,
                    &mut transcript,
                    step_idx,
                    id,
                    result,
                    error,
                    scenario.strict,
                ) {
                    Ok(msg) => msg,
                    Err(failure) => {
                        eprintln!(
                            "[fake-lsp] strict failure at step {step_idx}: {}",
                            failure.mismatch_reason
                        );
                        write_failure_diagnostics(&mut transcript, &failure);
                        exit_code = 1;
                        break;
                    }
                };

                if let Some(_msg) = msg {
                    match execute_actions(
                        then,
                        &None,
                        &mut writer,
                        &mut transcript,
                        step_idx,
                        &mut request_counter,
                    ) {
                        ActionControl::Continue => {}
                        ActionControl::Exit(code) => {
                            exit_code = code;
                            break;
                        }
                    }
                } else if scenario.strict {
                    eprintln!(
                        "[fake-lsp] EOF waiting for response id={} in strict mode",
                        id.summary()
                    );
                    exit_code = 1;
                    break;
                }
            }
            Step::AllowNotification { method } => {
                allowed_notifications.insert(method.clone());
                record_allow_step(&mut transcript, step_idx, method, "Notification");
            }
            Step::AllowRequest { method } => {
                allowed_requests.insert(method.clone());
                record_allow_step(&mut transcript, step_idx, method, "Request");
            }
            Step::SendNotification { method, params } => {
                let msg = JsonRpcMessage {
                    jsonrpc: Some("2.0".to_string()),
                    method: Some(method.clone()),
                    params: Some(params.clone()),
                    id: None,
                    result: None,
                    error: None,
                };
                writer.write_message(&msg).unwrap_or_else(|e| {
                    panic!("Failed to send notification {method}: {e}");
                });
                record_message_entry(&mut transcript, "sent", step_idx, &msg);
            }
            Step::SendRequest { method, params } => {
                let id = next_request_id(&mut request_counter);
                let msg = JsonRpcMessage {
                    jsonrpc: Some("2.0".to_string()),
                    method: Some(method.clone()),
                    params: Some(params.clone()),
                    id: Some(id),
                    result: None,
                    error: None,
                };
                writer.write_message(&msg).unwrap_or_else(|e| {
                    panic!("Failed to send request {method}: {e}");
                });
                record_message_entry(&mut transcript, "sent", step_idx, &msg);
            }
            Step::Delay { millis } => {
                thread::sleep(Duration::from_millis(*millis));
            }
            Step::Exit { code } => {
                exit_code = *code;
                break;
            }
        }
    }

    eprintln!("[fake-lsp] exiting with code {exit_code}");
    std::process::exit(exit_code);
}
