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
use codegg_core::tool_program::{CallRequest, CompletedCall, InterpreterCheckpoint, ProgramValue};
use codegg_protocol::projection::dto::{ToolProgramCallPage, ToolProgramCallSummary};
use codegg_protocol::projection::limits::{
    MAX_PROJECTION_CALL_PAGE_SIZE, MAX_PROJECTION_TOOL_PROGRAM_CALLS,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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

const MAX_JOURNAL_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct JournalFile {
    version: u16,
    program_id: String,
    reservations: Vec<JournalReservation>,
    completed: Vec<CompletedCall>,
    checkpoint: Option<InterpreterCheckpoint>,
    /// Original absolute deadline, persisted for restart authority (C-23).
    deadline_millis: Option<i64>,
    /// Divergence records: sequence -> observed output digest that differs
    /// from the durable record (C-22).
    #[serde(default)]
    divergences: Vec<JournalDivergence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalReservation {
    sequence: u32,
    request: CallRequest,
    state: String,
    reserved_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalDivergence {
    sequence: u32,
    observed_output_digest: String,
    diverged_at: i64,
}

/// M014-C19: Cross-process lock guard for journal operations.
enum LockGuard {
    #[cfg(unix)]
    Unix { _file: std::fs::File },
    #[cfg(not(unix))]
    None,
}

#[derive(Clone)]
pub struct ToolProgramLedger {
    base_dir: PathBuf,
    /// M014-C19: Lock directory for cross-process journal locking.
    /// Each program gets a lock file in this directory; the lock is
    /// acquired via `flock` (Unix) for true cross-process safety.
    lock_dir: PathBuf,
}

impl ToolProgramLedger {
    pub fn new(workspace_root: &Path) -> Self {
        let base_dir = workspace_root.join(".codegg").join("tool_program_calls");
        let lock_dir = base_dir.join("locks");
        Self { base_dir, lock_dir }
    }

    /// M014-C19: Acquire a cross-process lock for the given program.
    /// Uses `flock` on Unix for true cross-process safety; falls back
    /// to no lock on platforms without `flock` (single-process only).
    fn acquire_lock(&self, program_id: &str) -> Result<Option<LockGuard>, ToolProgramLedgerError> {
        validate_program_id(program_id)?;
        std::fs::create_dir_all(&self.lock_dir)?;
        let lock_path = self.lock_dir.join(format!("{}.lock", program_id));

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            use std::os::unix::io::AsRawFd;
            let file = std::fs::OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(&lock_path)?;
            nix::fcntl::flock(file.as_raw_fd(), nix::fcntl::FlockArg::LockExclusive)
                .map_err(|e| ToolProgramLedgerError::Io(std::io::Error::from(e)))?;
            return Ok(Some(LockGuard::Unix { _file: file }));
        }

        #[cfg(not(unix))]
        {
            Ok(Some(LockGuard::None))
        }
    }

    /// Run the given read-modify-write cycle under the per-program lock so
    /// concurrent writers cannot lose reservations or completions.
    fn mutate_journal<F, R>(&self, program_id: &str, f: F) -> Result<R, ToolProgramLedgerError>
    where
        F: FnOnce(&mut JournalFile) -> Result<R, ToolProgramLedgerError>,
    {
        let _guard = self.acquire_lock(program_id)?;
        let mut journal = self.read_journal(program_id)?;
        let result = f(&mut journal)?;
        self.write_journal(program_id, &journal)?;
        Ok(result)
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

    /// Persist a call reservation before dispatch. A conflicting durable
    /// identity fails closed instead of executing an ambiguous replay.
    pub fn reserve_call(
        &self,
        program_id: &str,
        sequence: u32,
        request: &CallRequest,
    ) -> Result<(), ToolProgramLedgerError> {
        validate_program_id(program_id)?;
        self.mutate_journal(program_id, |journal| {
            if let Some(completed) = journal.completed.iter().find(|c| c.sequence == sequence) {
                if completed.request.tool_name != request.tool_name
                    || completed.request.input != request.input
                {
                    return Err(ToolProgramLedgerError::InvalidLedger(
                        "completed call diverges from requested replay".into(),
                    ));
                }
                return Ok(());
            }
            if let Some(existing) = journal
                .reservations
                .iter()
                .find(|reservation| reservation.sequence == sequence)
            {
                if existing.request.tool_name != request.tool_name
                    || existing.request.input != request.input
                {
                    return Err(ToolProgramLedgerError::InvalidLedger(
                        "reserved call diverges from requested replay".into(),
                    ));
                }
                return Ok(());
            }
            journal.reservations.push(JournalReservation {
                sequence,
                request: request.clone(),
                state: "reserved".into(),
                reserved_at: Utc::now().timestamp_millis(),
            });
            Ok(())
        })
    }

    /// Persist a typed completion before the interpreter advances.
    pub fn persist_call_completion(
        &self,
        program_id: &str,
        completed: &CompletedCall,
    ) -> Result<(), ToolProgramLedgerError> {
        validate_program_id(program_id)?;
        self.mutate_journal(program_id, |journal| {
            if let Some(existing) = journal
                .completed
                .iter()
                .find(|call| call.sequence == completed.sequence)
            {
                if existing.request.tool_name != completed.request.tool_name
                    || existing.request.input != completed.request.input
                {
                    return Err(ToolProgramLedgerError::InvalidLedger(
                        "completed call identity conflict".into(),
                    ));
                }
                return Ok(());
            }
            journal
                .reservations
                .retain(|r| r.sequence != completed.sequence);
            journal.completed.push(completed.clone());
            journal.completed.sort_by_key(|call| call.sequence);
            Ok(())
        })
    }

    /// Persist the latest interpreter checkpoint after a durable call
    /// completion or explicit checkpoint instruction.
    pub fn persist_checkpoint(
        &self,
        program_id: &str,
        checkpoint: &InterpreterCheckpoint,
    ) -> Result<(), ToolProgramLedgerError> {
        validate_program_id(program_id)?;
        self.mutate_journal(program_id, |journal| {
            journal.checkpoint = Some(checkpoint.clone());
            Ok(())
        })
    }

    /// Load completed calls for restart replay. The public redacted ledger is
    /// intentionally not used for this operation.
    pub fn load_completed_calls(
        &self,
        program_id: &str,
    ) -> Result<HashMap<u32, CompletedCall>, ToolProgramLedgerError> {
        validate_program_id(program_id)?;
        Ok(self
            .read_journal(program_id)?
            .completed
            .into_iter()
            .map(|call| (call.sequence, call))
            .collect())
    }

    /// M014-C11: Load the latest valid checkpoint for restart replay.
    /// Returns `None` if no checkpoint exists.
    pub fn load_latest_checkpoint(&self, program_id: &str) -> Option<InterpreterCheckpoint> {
        self.read_journal(program_id)
            .ok()
            .and_then(|j| j.checkpoint)
    }

    /// Check if a call has been durably completed (C-20).
    pub fn is_call_completed(&self, program_id: &str, sequence: u32) -> bool {
        self.load_completed_calls(program_id)
            .map(|calls| calls.contains_key(&sequence))
            .unwrap_or(false)
    }

    /// Record the original absolute deadline for restart authority (C-23).
    pub fn record_program_deadline(
        &self,
        program_id: &str,
        deadline_millis: i64,
    ) -> Result<(), ToolProgramLedgerError> {
        validate_program_id(program_id)?;
        self.mutate_journal(program_id, |journal| {
            journal.deadline_millis = Some(deadline_millis);
            Ok(())
        })
    }

    /// Retrieve the original absolute deadline (C-23).
    pub fn get_program_deadline(&self, program_id: &str) -> Option<i64> {
        self.read_journal(program_id)
            .ok()
            .and_then(|j| j.deadline_millis)
    }

    /// Record a replay divergence for inspection (C-22).
    pub fn record_divergence(
        &self,
        program_id: &str,
        sequence: u32,
        observed_output_digest: &str,
    ) -> Result<(), ToolProgramLedgerError> {
        validate_program_id(program_id)?;
        self.mutate_journal(program_id, |journal| {
            journal.divergences.push(JournalDivergence {
                sequence,
                observed_output_digest: observed_output_digest.to_string(),
                diverged_at: Utc::now().timestamp_millis(),
            });
            Ok(())
        })
    }

    /// Check if a divergence has been recorded for a call (C-22).
    pub fn has_divergence(&self, program_id: &str, sequence: u32) -> bool {
        self.read_journal(program_id)
            .map(|j| j.divergences.iter().any(|d| d.sequence == sequence))
            .unwrap_or(false)
    }

    /// Get the input digest for a reserved/completed call (C-21).
    pub fn get_call_input_digest(&self, program_id: &str, sequence: u32) -> Option<String> {
        let journal = self.read_journal(program_id).ok()?;
        let input_str = if let Some(reservation) =
            journal.reservations.iter().find(|r| r.sequence == sequence)
        {
            serde_json::to_string(&reservation.request.input).ok()?
        } else if let Some(completed) = journal.completed.iter().find(|c| c.sequence == sequence) {
            serde_json::to_string(&completed.request.input).ok()?
        } else {
            return None;
        };
        Some(format!("sha256:{:x}", Sha256::digest(input_str.as_bytes())))
    }

    /// Get the output digest for a completed call (C-21).
    pub fn get_call_output_digest(&self, program_id: &str, sequence: u32) -> Option<String> {
        let journal = self.read_journal(program_id).ok()?;
        journal
            .completed
            .iter()
            .find(|c| c.sequence == sequence)
            .and_then(|call| {
                let output_str = serde_json::to_string(&call.result.output).ok()?;
                Some(format!(
                    "sha256:{:x}",
                    Sha256::digest(output_str.as_bytes())
                ))
            })
    }

    fn path(&self, program_id: &str) -> PathBuf {
        self.base_dir.join(format!("{program_id}.json"))
    }

    fn journal_path(&self, program_id: &str) -> PathBuf {
        self.base_dir.join(format!("{program_id}.journal.json"))
    }

    fn read_journal(&self, program_id: &str) -> Result<JournalFile, ToolProgramLedgerError> {
        let path = self.journal_path(program_id);
        if !path.exists() {
            return Ok(JournalFile {
                version: 1,
                program_id: program_id.to_string(),
                ..JournalFile::default()
            });
        }
        let metadata = path.symlink_metadata()?;
        if metadata.file_type().is_symlink() || metadata.len() > MAX_JOURNAL_BYTES {
            return Err(ToolProgramLedgerError::InvalidLedger(
                "journal path is invalid or oversized".into(),
            ));
        }
        let bytes = std::fs::read(path)?;
        let journal: JournalFile = serde_json::from_slice(&bytes)
            .map_err(|error| ToolProgramLedgerError::InvalidLedger(error.to_string()))?;
        if journal.program_id != program_id || journal.version != 1 {
            return Err(ToolProgramLedgerError::InvalidLedger(
                "journal identity or version mismatch".into(),
            ));
        }
        Ok(journal)
    }

    fn write_journal(
        &self,
        program_id: &str,
        journal: &JournalFile,
    ) -> Result<(), ToolProgramLedgerError> {
        let bytes = serde_json::to_vec(journal)
            .map_err(|error| ToolProgramLedgerError::InvalidLedger(error.to_string()))?;
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(ToolProgramLedgerError::Oversized);
        }
        std::fs::create_dir_all(&self.base_dir)?;
        let target = self.journal_path(program_id);
        let temporary = self.base_dir.join(format!(
            ".{program_id}.{}.journal.tmp",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&temporary, bytes)?;
        if let Err(error) = std::fs::rename(&temporary, &target) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.into());
        }
        Ok(())
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
                success: true,
            },
            replay_fingerprint: None,
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
