use serde::{Deserialize, Serialize};
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
        then: Vec<Action>,
    },
    ExpectNotification {
        method: String,
        #[serde(default)]
        then: Vec<Action>,
    },
    ExpectResponse {
        id: serde_json::Value,
        #[serde(default)]
        then: Vec<Action>,
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
    ExitNow {
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
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ExitConfig {
    ExitCode { code: i32 },
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
    #[serde(skip_serializing_if = "Option::is_none")]
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
    message: &'a JsonRpcMessage,
    step_index: usize,
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

        // Read headers
        loop {
            let mut line = String::new();
            let bytes_read = self.reader.read_line(&mut line)?;
            if bytes_read == 0 {
                // EOF
                return Ok(None);
            }

            let line = line.trim();
            if line.is_empty() {
                break;
            }

            if let Some(val) = line.strip_prefix("Content-Length: ") {
                content_length = val.parse().ok();
            }
            // Ignore other headers
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
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.writer.write_all(header.as_bytes())?;
        self.writer.write_all(body.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }
}

// --- Helpers ---

fn next_request_id(counter: &mut u64) -> serde_json::Value {
    *counter += 1;
    serde_json::Value::Number((*counter).into())
}

// --- Main ---

fn main() {
    let scenario_path =
        std::env::var("CODEGG_FAKE_LSP_SCENARIO").expect("CODEGG_FAKE_LSP_SCENARIO not set");

    let transcript_path =
        std::env::var("CODEGG_FAKE_LSP_TRANSCRIPT").expect("CODEGG_FAKE_LSP_TRANSCRIPT not set");

    let scenario_file = File::open(&scenario_path)
        .unwrap_or_else(|e| panic!("Failed to open scenario file {scenario_path}: {e}"));
    let scenario: Scenario =
        serde_json::from_reader(scenario_file).unwrap_or_else(|e| panic!("Failed to parse scenario: {e}"));

    let mut transcript = TranscriptWriter::new(&PathBuf::from(&transcript_path))
        .unwrap_or_else(|e| panic!("Failed to create transcript file: {e}"));

    let mut reader = FramedReader::new();
    let mut writer = FramedWriter::new();
    let mut request_counter: u64 = 0;
    let mut exit_code: i32 = match &scenario.exit {
        ExitConfig::ExitCode { code } => *code,
    };

    eprintln!("[fake-lsp] scenario: {}", scenario.name);
    eprintln!("[fake-lsp] steps: {}", scenario.steps.len());

    for (step_idx, step) in scenario.steps.iter().enumerate() {
        eprintln!("[fake-lsp] step {step_idx}: {step:?}");

        match step {
            Step::ExpectRequest { method, then } => {
                let msg = read_until_request(&mut reader, &mut transcript, step_idx, method);
                match msg {
                    Some(m) => {
                        execute_actions(
                            then,
                            &m.id,
                            &mut writer,
                            &mut transcript,
                            step_idx,
                            &mut request_counter,
                        );
                    }
                    None => {
                        if scenario.strict {
                            eprintln!(
                                "[fake-lsp] EOF waiting for request {method} in strict mode"
                            );
                            exit_code = 1;
                            break;
                        }
                        eprintln!(
                            "[fake-lsp] EOF waiting for request {method}, non-strict continuing"
                        );
                    }
                }
            }
            Step::ExpectNotification { method, then } => {
                let msg =
                    read_until_notification(&mut reader, &mut transcript, step_idx, method);
                if msg.is_none() && scenario.strict {
                    eprintln!(
                        "[fake-lsp] EOF waiting for notification {method} in strict mode"
                    );
                    exit_code = 1;
                    break;
                }
                // then actions for notifications don't need a request id
                execute_actions(
                    then,
                    &None,
                    &mut writer,
                    &mut transcript,
                    step_idx,
                    &mut request_counter,
                );
            }
            Step::ExpectResponse { id, then } => {
                let msg = read_until_response(&mut reader, &mut transcript, step_idx, id);
                if msg.is_none() && scenario.strict {
                    eprintln!("[fake-lsp] EOF waiting for response id={id} in strict mode");
                    exit_code = 1;
                    break;
                }
                if msg.is_some() {
                    execute_actions(
                        then,
                        &None,
                        &mut writer,
                        &mut transcript,
                        step_idx,
                        &mut request_counter,
                    );
                }
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
                transcript
                    .write_entry(&TranscriptEntry {
                        direction: "sent".to_string(),
                        message: &msg,
                        step_index: step_idx,
                    })
                    .ok();
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
                transcript
                    .write_entry(&TranscriptEntry {
                        direction: "sent".to_string(),
                        message: &msg,
                        step_index: step_idx,
                    })
                    .ok();
                // We don't wait for response here; the caller can use ExpectResponse step
            }
            Step::Delay { millis } => {
                thread::sleep(Duration::from_millis(*millis));
            }
            Step::ExitNow { code } => {
                exit_code = *code;
                break;
            }
        }
    }

    eprintln!("[fake-lsp] exiting with code {exit_code}");
    std::process::exit(exit_code);
}

fn read_until_request(
    reader: &mut FramedReader,
    transcript: &mut TranscriptWriter,
    step_idx: usize,
    expected_method: &str,
) -> Option<JsonRpcMessage> {
    loop {
        match reader.read_message() {
            Ok(Some(msg)) => {
                let is_request = msg.id.is_some() && msg.method.is_some();
                if is_request && msg.method.as_deref() == Some(expected_method) {
                    transcript
                        .write_entry(&TranscriptEntry {
                            direction: "recv".to_string(),
                            message: &msg,
                            step_index: step_idx,
                        })
                        .ok();
                    return Some(msg);
                }
                // Not the expected message — record it and keep reading
                transcript
                    .write_entry(&TranscriptEntry {
                        direction: "recv".to_string(),
                        message: &msg,
                        step_index: step_idx,
                    })
                    .ok();
            }
            Ok(None) => return None, // EOF
            Err(e) => {
                eprintln!("[fake-lsp] read error: {e}");
                return None;
            }
        }
    }
}

fn read_until_notification(
    reader: &mut FramedReader,
    transcript: &mut TranscriptWriter,
    step_idx: usize,
    expected_method: &str,
) -> Option<JsonRpcMessage> {
    loop {
        match reader.read_message() {
            Ok(Some(msg)) => {
                let is_notification = msg.id.is_none() && msg.method.is_some();
                if is_notification && msg.method.as_deref() == Some(expected_method) {
                    transcript
                        .write_entry(&TranscriptEntry {
                            direction: "recv".to_string(),
                            message: &msg,
                            step_index: step_idx,
                        })
                        .ok();
                    return Some(msg);
                }
                transcript
                    .write_entry(&TranscriptEntry {
                        direction: "recv".to_string(),
                        message: &msg,
                        step_index: step_idx,
                    })
                    .ok();
            }
            Ok(None) => return None,
            Err(e) => {
                eprintln!("[fake-lsp] read error: {e}");
                return None;
            }
        }
    }
}

fn read_until_response(
    reader: &mut FramedReader,
    transcript: &mut TranscriptWriter,
    step_idx: usize,
    expected_id: &serde_json::Value,
) -> Option<JsonRpcMessage> {
    loop {
        match reader.read_message() {
            Ok(Some(msg)) => {
                let is_response = msg.result.is_some() || msg.error.is_some();
                if is_response && msg.id.as_ref() == Some(expected_id) {
                    transcript
                        .write_entry(&TranscriptEntry {
                            direction: "recv".to_string(),
                            message: &msg,
                            step_index: step_idx,
                        })
                        .ok();
                    return Some(msg);
                }
                transcript
                    .write_entry(&TranscriptEntry {
                        direction: "recv".to_string(),
                        message: &msg,
                        step_index: step_idx,
                    })
                    .ok();
            }
            Ok(None) => return None,
            Err(e) => {
                eprintln!("[fake-lsp] read error: {e}");
                return None;
            }
        }
    }
}

fn execute_actions(
    actions: &[Action],
    request_id: &Option<serde_json::Value>,
    writer: &mut FramedWriter,
    transcript: &mut TranscriptWriter,
    step_idx: usize,
    request_counter: &mut u64,
) {
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
                transcript
                    .write_entry(&TranscriptEntry {
                        direction: "sent".to_string(),
                        message: &msg,
                        step_index: step_idx,
                    })
                    .ok();
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
                transcript
                    .write_entry(&TranscriptEntry {
                        direction: "sent".to_string(),
                        message: &msg,
                        step_index: step_idx,
                    })
                    .ok();
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
                transcript
                    .write_entry(&TranscriptEntry {
                        direction: "sent".to_string(),
                        message: &msg,
                        step_index: step_idx,
                    })
                    .ok();
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
                transcript
                    .write_entry(&TranscriptEntry {
                        direction: "sent".to_string(),
                        message: &msg,
                        step_index: step_idx,
                    })
                    .ok();
            }
        }
    }
}