//! Durable, typed terminal results for Tool Programs.
//!
//! The scheduler, foreground tool, background notification service, and
//! inspection API all read the same bounded record. Human-readable executor
//! summaries are presentation only and are never parsed for semantics.

use std::path::{Path, PathBuf};

use chrono::Utc;
use codegg_core::tool_program::ProgramResult;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_RESULT_BYTES: usize = 2 * 1024 * 1024;
const MAX_PROGRAM_ID_BYTES: usize = 128;

#[derive(Debug, Error)]
pub enum ToolProgramResultError {
    #[error("invalid tool-program identity")]
    InvalidIdentity,
    #[error("result is oversized")]
    Oversized,
    #[error("result I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("result JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("result identity mismatch")]
    IdentityMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramResultRecord {
    pub schema_version: u16,
    pub program_id: String,
    pub attempt_id: String,
    pub selected_backend: String,
    pub result: ProgramResult,
    pub result_digest: String,
    pub recorded_at: i64,
}

#[derive(Debug, Clone)]
pub struct ToolProgramResultStore {
    base_dir: PathBuf,
}

impl ToolProgramResultStore {
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            base_dir: workspace_root.join(".codegg").join("tool_program_results"),
        }
    }

    pub fn persist(
        &self,
        program_id: &str,
        attempt_id: &str,
        selected_backend: impl Into<String>,
        mut result: ProgramResult,
    ) -> Result<ProgramResultRecord, ToolProgramResultError> {
        validate_identity(program_id)?;
        if attempt_id.is_empty() || attempt_id.len() > MAX_PROGRAM_ID_BYTES {
            return Err(ToolProgramResultError::InvalidIdentity);
        }
        if let Some(error) = result.error_message.as_mut() {
            error.truncate(4096);
        }
        let mut record = ProgramResultRecord {
            schema_version: 1,
            program_id: program_id.to_string(),
            attempt_id: attempt_id.to_string(),
            selected_backend: selected_backend.into(),
            result,
            result_digest: String::new(),
            recorded_at: Utc::now().timestamp_millis(),
        };
        let digest_input = serde_json::to_vec(&record.result)?;
        record.result_digest = format!("{:x}", sha2::Sha256::digest(&digest_input));
        let bytes = serde_json::to_vec(&record)?;
        if bytes.len() > MAX_RESULT_BYTES {
            return Err(ToolProgramResultError::Oversized);
        }
        std::fs::create_dir_all(&self.base_dir)?;
        let target = self.path(program_id);
        let temporary = self
            .base_dir
            .join(format!(".{program_id}.{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&temporary, bytes)?;
        if let Err(error) = std::fs::rename(&temporary, &target) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.into());
        }
        Ok(record)
    }

    pub fn load(
        &self,
        program_id: &str,
    ) -> Result<Option<ProgramResultRecord>, ToolProgramResultError> {
        validate_identity(program_id)?;
        let path = self.path(program_id);
        if !path.exists() {
            return Ok(None);
        }
        let metadata = path.symlink_metadata()?;
        if metadata.file_type().is_symlink() || metadata.len() as usize > MAX_RESULT_BYTES {
            return Err(ToolProgramResultError::Oversized);
        }
        let record: ProgramResultRecord = serde_json::from_slice(&std::fs::read(path)?)?;
        if record.program_id != program_id {
            return Err(ToolProgramResultError::IdentityMismatch);
        }
        Ok(Some(record))
    }

    fn path(&self, program_id: &str) -> PathBuf {
        self.base_dir.join(format!("{program_id}.json"))
    }
}

pub fn result_to_json(record: &ProgramResultRecord) -> serde_json::Value {
    let status = serde_json::to_value(record.result.status).unwrap_or_else(|_| "failed".into());
    let failure_class = record
        .result
        .failure_class
        .as_ref()
        .and_then(|class| serde_json::to_value(class).ok());
    let mut value = serde_json::json!({
        "status": status,
        "program_id": record.program_id,
        "steps_used": record.result.steps_used,
        "calls_completed": record.result.calls_completed,
        "calls_total": record.result.calls_total,
        "iterations_used": record.result.iterations_used,
        "bytes_used": record.result.bytes_used,
        "selected_backend": record.selected_backend,
        "result_digest": record.result_digest,
        "program_artifacts": [],
    });
    if let Some(output) = &record.result.output {
        value["output"] = output.to_json();
    }
    if let Some(error) = &record.result.error_message {
        value["error"] = error.clone().into();
    }
    if let Some(failure_class) = failure_class {
        value["failure_class"] = failure_class;
    }
    value["success"] = serde_json::Value::Bool(matches!(
        record.result.status,
        codegg_core::tool_program::ProgramStatus::Completed
    ));
    value
}

fn validate_identity(identity: &str) -> Result<(), ToolProgramResultError> {
    if identity.is_empty()
        || identity.len() > MAX_PROGRAM_ID_BYTES
        || !identity
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ToolProgramResultError::InvalidIdentity);
    }
    Ok(())
}

use sha2::Digest;

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::tool_program::{ProgramResult, ProgramStatus};

    #[test]
    fn typed_result_round_trips_without_summary_parsing() {
        let temp = tempfile::tempdir().unwrap();
        let store = ToolProgramResultStore::new(temp.path());
        let result = ProgramResult {
            status: ProgramStatus::Completed,
            output: None,
            error_message: None,
            failure_class: None,
            steps_used: 7,
            bytes_used: 2,
            calls_total: 3,
            calls_completed: 3,
            iterations_used: 1,
        };
        let record = store
            .persist("tp-test", "attempt-1", "native", result)
            .unwrap();
        let loaded = store.load("tp-test").unwrap().unwrap();
        assert_eq!(loaded.result.steps_used, 7);
        assert_eq!(loaded.result.calls_total, 3);
        assert_eq!(loaded.result_digest, record.result_digest);
    }
}
