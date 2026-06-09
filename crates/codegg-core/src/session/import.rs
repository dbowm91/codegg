//! Import/export types and functions for sessions.

use serde::Deserialize;

use crate::error::StorageError;

#[derive(Deserialize)]
pub struct SessionImport {
    #[serde(rename = "session")]
    pub session: SessionImportData,
    #[serde(default)]
    pub messages: Vec<MessageImport>,
    #[serde(default)]
    pub parts: Vec<PartImport>,
    #[serde(default)]
    pub todos: Vec<TodoImport>,
}

#[derive(Deserialize)]
pub struct SessionImportData {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Deserialize)]
pub struct MessageImport {
    #[serde(default)]
    pub id: Option<String>,
    pub data: serde_json::Value,
    #[serde(rename = "time_created", default)]
    pub time_created: Option<i64>,
    #[serde(rename = "time_updated", default)]
    pub time_updated: Option<i64>,
}

#[derive(Deserialize)]
pub struct PartImport {
    #[serde(rename = "message_id", default)]
    pub message_id: Option<String>,
    pub data: serde_json::Value,
    #[serde(rename = "time_created", default)]
    pub time_created: Option<i64>,
    #[serde(rename = "time_updated", default)]
    pub time_updated: Option<i64>,
}

#[derive(Deserialize)]
pub struct TodoImport {
    pub content: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub priority: String,
    #[serde(rename = "time_created", default)]
    pub time_created: Option<i64>,
    #[serde(rename = "time_updated", default)]
    pub time_updated: Option<i64>,
}

const MAX_IMPORT_MESSAGES: usize = 100_000;
const MAX_IMPORT_PARTS: usize = 500_000;
const MAX_TOTAL_IMPORT_BYTES: usize = 500 * 1024 * 1024;

pub fn validate_import_size(data: &serde_json::Value) -> Result<usize, StorageError> {
    let msg_count = data
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let part_count = data
        .get("parts")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let total_bytes = serde_json::to_vec(data).map(|v| v.len()).unwrap_or(0);

    if msg_count > MAX_IMPORT_MESSAGES {
        return Err(StorageError::Import(format!(
            "import validation failed: too many messages ({} > {})",
            msg_count, MAX_IMPORT_MESSAGES
        )));
    }
    if part_count > MAX_IMPORT_PARTS {
        return Err(StorageError::Import(format!(
            "import validation failed: too many parts ({} > {})",
            part_count, MAX_IMPORT_PARTS
        )));
    }
    if total_bytes > MAX_TOTAL_IMPORT_BYTES {
        return Err(StorageError::Import(format!(
            "import validation failed: import size ({} bytes) exceeds limit ({} bytes)",
            total_bytes, MAX_TOTAL_IMPORT_BYTES
        )));
    }

    Ok(total_bytes)
}

pub fn redact_for_export(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(mut obj) => {
            let should_redact_tool_input =
                if let Some(serde_json::Value::String(type_str)) = obj.get("type") {
                    type_str == "tool_call"
                } else {
                    false
                };

            if should_redact_tool_input {
                if obj.get("input").is_some() {
                    obj.insert("input".to_string(), serde_json::json!("[REDACTED]"));
                }
                if let Some(output) = obj.get("output") {
                    if !output.is_null() {
                        obj.insert("output".to_string(), serde_json::json!("[REDACTED]"));
                    }
                }
                if let Some(serde_json::Value::String(name)) = obj.get("name") {
                    let should_redact = name == "bash"
                        || name == "write"
                        || name == "read"
                        || name == "edit"
                        || name == "replace"
                        || name == "multiedit"
                        || name == "terminal"
                        || name == "git"
                        || name == "webfetch"
                        || name == "apply_patch";

                    if should_redact {
                        let keys_to_redact = [
                            "command",
                            "path",
                            "content",
                            "text",
                            "pattern",
                            "replacement",
                            "old_string",
                            "new_string",
                            "url",
                            "patch",
                        ];
                        for k in keys_to_redact {
                            if let Some(serde_json::Value::Object(input_obj)) = obj.get("input") {
                                if let Some(val) = input_obj.get(k) {
                                    if !val.is_null() && !val.as_str().unwrap_or("").is_empty() {
                                        if let Some(serde_json::Value::Object(inp)) =
                                            obj.get_mut("input")
                                        {
                                            inp.insert(
                                                k.to_string(),
                                                serde_json::json!("[REDACTED]"),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for (_, v) in obj.iter_mut() {
                *v = redact_for_export(std::mem::take(v));
            }
            serde_json::Value::Object(obj)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(redact_for_export).collect())
        }
        other => other,
    }
}
