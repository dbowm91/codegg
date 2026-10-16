//! M008 fixture starter: deterministic event summary producer.

use serde::{Deserialize, Serialize};

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
    pub events: Vec<serde_json::Value>,
    pub totals: serde_json::Value,
    pub by_kind: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryError(pub String);

impl std::fmt::Display for SummaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SummaryError {}

/// Parse, validate, normalize, deduplicate, sort, and aggregate JSONL events.
pub fn summarize(source_revision: &str, jsonl: &str) -> Result<Summary, SummaryError> {
    let _ = (source_revision, jsonl);
    Err(SummaryError("TODO: implement summary producer".into()))
}
