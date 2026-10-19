//! Deterministic event summary producer.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InputEvent {
    pub id: String,
    pub occurred_at: String,
    pub kind: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub insertions: Option<u64>,
    #[serde(default)]
    pub deletions: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub schema: String,
    pub source_revision: String,
    pub events: Vec<Value>,
    pub totals: Value,
    pub by_kind: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryError(pub String);

impl std::fmt::Display for SummaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SummaryError {}

fn line_error(line: usize, message: impl std::fmt::Display) -> SummaryError {
    SummaryError(format!("line {line}: {message}"))
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.as_bytes().get(1).is_some_and(|byte| *byte == b':')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

fn required_string<'a>(obj: &'a Map<String, Value>, key: &str, line: usize) -> Result<&'a str, SummaryError> {
    obj.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| line_error(line, format!("{key} must be a string")))
}

fn required_count(obj: &Map<String, Value>, key: &str, line: usize) -> Result<u64, SummaryError> {
    obj.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| line_error(line, format!("{key} must be a nonnegative integer")))
}

/// Parse, validate, normalize, deduplicate, sort, and aggregate JSONL events.
pub fn summarize(source_revision: &str, jsonl: &str) -> Result<Summary, SummaryError> {
    if source_revision.is_empty() {
        return Err(SummaryError("source revision must not be empty".into()));
    }

    let mut unique: BTreeMap<String, (Value, usize)> = BTreeMap::new();
    for (index, text) in jsonl.lines().enumerate() {
        let line = index + 1;
        let raw: Value = serde_json::from_str(text)
            .map_err(|error| line_error(line, format!("invalid JSON: {error}")))?;
        let obj = raw.as_object().ok_or_else(|| line_error(line, "event must be a JSON object"))?;
        for key in obj.keys() {
            if !["id", "occurred_at", "kind", "path", "insertions", "deletions"].contains(&key.as_str()) {
                return Err(line_error(line, format!("unknown field {key:?}")));
            }
        }
        let id = required_string(obj, "id", line)?;
        if id.is_empty() {
            return Err(line_error(line, "id must not be empty"));
        }
        let timestamp = required_string(obj, "occurred_at", line)?;
        let parsed = DateTime::parse_from_rfc3339(timestamp)
            .map_err(|error| line_error(line, format!("invalid occurred_at timestamp: {error}")))?;
        let occurred_at = parsed.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Secs, true);
        let kind = required_string(obj, "kind", line)?;

        let mut event = Map::new();
        event.insert("id".into(), json!(id));
        event.insert("occurred_at".into(), json!(occurred_at));
        event.insert("kind".into(), json!(kind));
        match kind {
            "file_changed" => {
                let path = required_string(obj, "path", line)?;
                if !safe_path(path) {
                    return Err(line_error(line, format!("unsafe or non-normalized path {path:?}")));
                }
                let insertions = required_count(obj, "insertions", line)?;
                let deletions = required_count(obj, "deletions", line)?;
                event.insert("path".into(), json!(path));
                event.insert("insertions".into(), json!(insertions));
                event.insert("deletions".into(), json!(deletions));
            }
            "test_passed" | "test_failed" => {
                for key in ["path", "insertions", "deletions"] {
                    if obj.contains_key(key) {
                        return Err(line_error(line, format!("test event must not contain {key}")));
                    }
                }
            }
            _ => return Err(line_error(line, format!("invalid event kind {kind:?}"))),
        }

        let value = Value::Object(event);
        if let Some((previous, first_line)) = unique.get(id) {
            if previous != &value {
                return Err(line_error(line, format!("duplicate id {id:?} conflicts with line {first_line}")));
            }
        } else {
            unique.insert(id.to_owned(), (value, line));
        }
    }

    let mut events: Vec<Value> = unique.into_values().map(|(value, _)| value).collect();
    events.sort_by(|left, right| {
        left["occurred_at"].as_str().cmp(&right["occurred_at"].as_str())
            .then_with(|| left["id"].as_str().cmp(&right["id"].as_str()))
    });

    let mut files_changed = 0u64;
    let mut insertions = 0u64;
    let mut deletions = 0u64;
    let mut file_changed = 0u64;
    let mut test_passed = 0u64;
    let mut test_failed = 0u64;
    for event in &events {
        match event["kind"].as_str().expect("validated kind") {
            "file_changed" => {
                files_changed = files_changed.checked_add(1).ok_or_else(|| SummaryError("files_changed overflow".into()))?;
                file_changed = file_changed.checked_add(1).ok_or_else(|| SummaryError("file_changed overflow".into()))?;
                insertions = insertions.checked_add(event["insertions"].as_u64().unwrap()).ok_or_else(|| SummaryError("insertions overflow".into()))?;
                deletions = deletions.checked_add(event["deletions"].as_u64().unwrap()).ok_or_else(|| SummaryError("deletions overflow".into()))?;
            }
            "test_passed" => test_passed = test_passed.checked_add(1).ok_or_else(|| SummaryError("test_passed overflow".into()))?,
            "test_failed" => test_failed = test_failed.checked_add(1).ok_or_else(|| SummaryError("test_failed overflow".into()))?,
            _ => unreachable!(),
        }
    }
    let event_count = u64::try_from(events.len()).map_err(|_| SummaryError("events overflow".into()))?;
    Ok(Summary {
        schema: "codegg.project-summary.v1".into(),
        source_revision: source_revision.into(),
        events,
        totals: json!({"events": event_count, "files_changed": files_changed, "insertions": insertions, "deletions": deletions}),
        by_kind: json!({"file_changed": file_changed, "test_passed": test_passed, "test_failed": test_failed}),
    })
}
