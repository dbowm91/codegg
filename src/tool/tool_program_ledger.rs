//! Bounded, workspace-local call ledger for Tool Program inspection.
//!
//! The interpreter keeps replay state in memory while a job runs. This
//! module persists only redacted call summaries at terminal boundaries so
//! the daemon's inspection API remains useful after the executor returns or
//! the process restarts. Raw arguments and result bodies never enter this
//! ledger.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use codegg_core::tool_program::{CompletedCall, ProgramValue};
use codegg_protocol::projection::dto::{ToolProgramCallPage, ToolProgramCallSummary};
use codegg_protocol::projection::limits::{
    MAX_PROJECTION_CALL_PAGE_SIZE, MAX_PROJECTION_TOOL_PROGRAM_CALLS,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_LEDGER_BYTES: u64 = 1024 * 1024;
const MAX_PROGRAM_ID_BYTES: usize = 128;

#[derive(Debug, Error)]
pub enum ToolProgramLedgerError {
    #[error("invalid tool-program identity: {0}")]
    InvalidProgramId(String),
    #[error("tool-program ledger I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("tool-program ledger is too large")]
    Oversized,
    #[error("tool-program ledger is invalid: {0}")]
    InvalidLedger(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LedgerFile {
    program_id: String,
    calls: Vec<ToolProgramCallSummary>,
}

pub struct ToolProgramLedger {
    base_dir: PathBuf,
}

impl ToolProgramLedger {
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            base_dir: workspace_root.join(".codegg").join("tool_program_calls"),
        }
    }

    /// Persist the completed-call view atomically, replacing an earlier
    /// attempt for the same logical program identity.
    pub fn persist_completed_calls(
        &self,
        program_id: &str,
        calls: &HashMap<u32, CompletedCall>,
    ) -> Result<(), ToolProgramLedgerError> {
        validate_program_id(program_id)?;
        let now = Utc::now().timestamp_millis();
        let mut ordered: Vec<&CompletedCall> = calls.values().collect();
        ordered.sort_by_key(|call| call.sequence);
        let summaries = ordered
            .into_iter()
            .take(MAX_PROJECTION_TOOL_PROGRAM_CALLS)
            .map(|call| {
                let mut summary = ToolProgramCallSummary {
                    call_index: call.sequence,
                    tool_name: call.request.tool_name.clone(),
                    arguments_summary: json_shape(&call.request.input),
                    result_summary: format!(
                        "shape={} artifacts={}",
                        program_value_shape(&call.result.output),
                        call.result.artifacts.len()
                    ),
                    success: true,
                    duration_ms: None,
                    started_at: now,
                    completed_at: Some(now),
                };
                summary.normalise();
                summary
            })
            .collect();
        self.write_file(&LedgerFile {
            program_id: program_id.to_string(),
            calls: summaries,
        })
    }

    /// Read one bounded page. Missing ledgers are represented as an empty
    /// page, which is the correct result for a finite program with no calls.
    pub fn read_page(
        &self,
        program_id: &str,
        offset: u32,
    ) -> Result<ToolProgramCallPage, ToolProgramLedgerError> {
        validate_program_id(program_id)?;
        let path = self.path(program_id);
        let calls = if !path.exists() {
            Vec::new()
        } else {
            let metadata = path.symlink_metadata()?;
            if metadata.file_type().is_symlink() {
                return Err(ToolProgramLedgerError::InvalidLedger(
                    "ledger path is a symlink".into(),
                ));
            }
            if metadata.len() > MAX_LEDGER_BYTES {
                return Err(ToolProgramLedgerError::Oversized);
            }
            let bytes = std::fs::read(path)?;
            let file: LedgerFile = serde_json::from_slice(&bytes)
                .map_err(|error| ToolProgramLedgerError::InvalidLedger(error.to_string()))?;
            if file.program_id != program_id {
                return Err(ToolProgramLedgerError::InvalidLedger(
                    "program identity mismatch".into(),
                ));
            }
            file.calls
        };
        let start = offset as usize;
        let end = start
            .saturating_add(MAX_PROJECTION_CALL_PAGE_SIZE)
            .min(calls.len());
        let page_calls = if start >= calls.len() {
            Vec::new()
        } else {
            calls[start..end].to_vec()
        };
        Ok(ToolProgramCallPage {
            program_id: program_id.to_string(),
            offset,
            total_calls: calls.len() as u32,
            has_more: end < calls.len(),
            calls: page_calls,
        })
    }

    fn path(&self, program_id: &str) -> PathBuf {
        self.base_dir.join(format!("{program_id}.json"))
    }

    fn write_file(&self, file: &LedgerFile) -> Result<(), ToolProgramLedgerError> {
        let bytes = serde_json::to_vec(file)
            .map_err(|error| ToolProgramLedgerError::InvalidLedger(error.to_string()))?;
        if bytes.len() as u64 > MAX_LEDGER_BYTES {
            return Err(ToolProgramLedgerError::Oversized);
        }
        std::fs::create_dir_all(&self.base_dir)?;
        let target = self.path(&file.program_id);
        let temporary =
            self.base_dir
                .join(format!(".{}.{}.tmp", file.program_id, uuid::Uuid::new_v4()));
        std::fs::write(&temporary, bytes)?;
        if let Err(error) = std::fs::rename(&temporary, &target) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.into());
        }
        Ok(())
    }
}

fn validate_program_id(program_id: &str) -> Result<(), ToolProgramLedgerError> {
    if program_id.is_empty()
        || program_id.len() > MAX_PROGRAM_ID_BYTES
        || !program_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ToolProgramLedgerError::InvalidProgramId(
            program_id.to_string(),
        ));
    }
    Ok(())
}

fn json_shape(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(_) => "bool".into(),
        serde_json::Value::Number(_) => "number".into(),
        serde_json::Value::String(value) => format!("string(len={})", value.len()),
        serde_json::Value::Array(values) => format!("array(len={})", values.len()),
        serde_json::Value::Object(values) => format!("object(fields={})", values.len()),
    }
}

fn program_value_shape(value: &ProgramValue) -> String {
    match value {
        ProgramValue::None => "null".into(),
        ProgramValue::Bool(_) => "bool".into(),
        ProgramValue::Int(_) | ProgramValue::Float(_) => "number".into(),
        ProgramValue::String(value) => format!("string(len={})", value.len()),
        ProgramValue::List(values) => format!("array(len={})", values.len()),
        ProgramValue::Dict(values) => format!("object(fields={})", values.len()),
        ProgramValue::ToolResult(value) => json_shape(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::tool_program::{CallRequest, CallResult};

    #[test]
    fn persists_redacted_bounded_call_page() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ToolProgramLedger::new(temp.path());
        let call = CompletedCall {
            sequence: 2,
            request: CallRequest {
                tool_name: "read".into(),
                input: serde_json::json!({"secret": "do-not-persist"}),
                call_id: None,
            },
            result: CallResult {
                output: ProgramValue::ToolResult(serde_json::json!({"secret": "hidden"})),
                artifacts: vec!["artifact-1".into()],
            },
        };
        let calls = HashMap::from([(2, call)]);
        ledger.persist_completed_calls("tp-test", &calls).unwrap();
        let page = ledger.read_page("tp-test", 0).unwrap();
        assert_eq!(page.total_calls, 1);
        assert_eq!(page.calls[0].tool_name, "read");
        assert!(!page.calls[0].arguments_summary.contains("do-not-persist"));
        assert!(!page.calls[0].result_summary.contains("hidden"));
    }

    #[test]
    fn rejects_path_traversal_identity() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ToolProgramLedger::new(temp.path());
        assert!(matches!(
            ledger.read_page("../secret", 0),
            Err(ToolProgramLedgerError::InvalidProgramId(_))
        ));
    }
}
