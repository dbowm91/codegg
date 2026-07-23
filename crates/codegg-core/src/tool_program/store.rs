//! Durable Tool Program store trait and implementations.
//!
//! The [`ToolProgramStore`] is the single source of truth for program
//! lifecycle records. It owns program creation, state transitions,
//! call recording, checkpoint persistence, and result storage.
//!
//! # Invariants
//!
//! - Transactional create of program record plus manifest/reference
//!   metadata before job submission.
//! - Unique constraints for program ID, call ID, `(program_id,
//!   sequence)`, and normalized replay key where applicable.
//! - Compare-and-set or expected-state transitions.
//! - Bounded query indexes by session, turn, job, state, and updated
//!   time.
//! - Retention never deletes source/IR/calls required by active or
//!   recoverable work.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;

use super::{
    ProgramCallId, ProgramCallRecord, ProgramCallState, ProgramCheckpoint, ProgramLanguage,
    ProgramResult, ToolProgramId, ToolProgramRecord, ToolProgramState,
};
use crate::error::StorageError;

/// Errors from program store operations.
#[derive(Debug, Error)]
pub enum ProgramStoreError {
    #[error("storage failure: {0}")]
    Storage(#[from] StorageError),

    #[error("program '{0}' not found")]
    NotFound(String),

    #[error("call '{0}' not found")]
    CallNotFound(String),

    #[error("invalid transition for program '{program}': {from:?} -> {to:?}")]
    InvalidTransition {
        program: String,
        from: ToolProgramState,
        to: ToolProgramState,
    },

    #[error("invalid call transition for call '{call}': {from:?} -> {to:?}")]
    InvalidCallTransition {
        call: String,
        from: ProgramCallState,
        to: ProgramCallState,
    },

    #[error("program '{0}' is already terminal")]
    AlreadyTerminal(String),

    #[error("call '{0}' is already terminal")]
    CallAlreadyTerminal(String),

    #[error("duplicate submission key '{0}'")]
    DuplicateSubmission(String),

    #[error("serialization failure: {0}")]
    Serialization(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("call sequence overflow for program '{0}'")]
    SequenceOverflow(String),
}

/// Query parameters for listing programs.
#[derive(Debug, Clone, Default)]
pub struct ProgramStoreQuery {
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub states: Vec<ToolProgramState>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Compact summary for list queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramSummary {
    pub program_id: ToolProgramId,
    pub state: ToolProgramState,
    pub language: ProgramLanguage,
    pub submission_key: String,
    pub job_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub terminal_at: Option<DateTime<Utc>>,
}

/// Storage trait for the Tool Program domain.
#[async_trait]
pub trait ToolProgramStore: Send + Sync {
    /// Create a new program record. Fails on duplicate submission key.
    async fn create_program(
        &self,
        record: ToolProgramRecord,
    ) -> Result<ToolProgramRecord, ProgramStoreError>;

    /// Fetch a program by ID.
    async fn get_program(
        &self,
        id: &ToolProgramId,
    ) -> Result<Option<ToolProgramRecord>, ProgramStoreError>;

    /// List programs matching the query.
    async fn list_programs(
        &self,
        query: ProgramStoreQuery,
    ) -> Result<Vec<ProgramSummary>, ProgramStoreError>;

    /// Transition program state via compare-and-set.
    async fn transition_program(
        &self,
        id: &ToolProgramId,
        expected: ToolProgramState,
        to: ToolProgramState,
    ) -> Result<ToolProgramRecord, ProgramStoreError>;

    /// Link a scheduler job to the program.
    async fn set_job_id(
        &self,
        id: &ToolProgramId,
        job_id: &str,
    ) -> Result<ToolProgramRecord, ProgramStoreError>;

    /// Persist the compiled IR reference.
    async fn set_ir_ref(
        &self,
        id: &ToolProgramId,
        ir_ref: super::ProgramIrRef,
    ) -> Result<ToolProgramRecord, ProgramStoreError>;

    /// Record a new call in the call ledger. Returns the assigned
    /// call record with its sequence number.
    async fn reserve_call(
        &self,
        program_id: &ToolProgramId,
        record: ProgramCallRecord,
    ) -> Result<ProgramCallRecord, ProgramStoreError>;

    /// Transition a call via compare-and-set.
    async fn transition_call(
        &self,
        call_id: &ProgramCallId,
        expected: ProgramCallState,
        to: ProgramCallState,
    ) -> Result<ProgramCallRecord, ProgramStoreError>;

    /// Fetch a call by ID.
    async fn get_call(
        &self,
        call_id: &ProgramCallId,
    ) -> Result<Option<ProgramCallRecord>, ProgramStoreError>;

    /// List all calls for a program, ordered by sequence.
    async fn list_calls(
        &self,
        program_id: &ToolProgramId,
    ) -> Result<Vec<ProgramCallRecord>, ProgramStoreError>;

    /// Persist a checkpoint.
    async fn set_checkpoint(
        &self,
        id: &ToolProgramId,
        checkpoint: ProgramCheckpoint,
    ) -> Result<(), ProgramStoreError>;

    /// Record the terminal result.
    async fn set_result(
        &self,
        id: &ToolProgramId,
        result: ProgramResult,
    ) -> Result<ToolProgramRecord, ProgramStoreError>;

    /// Load all non-terminal programs (for restart recovery).
    async fn list_non_terminal(&self) -> Result<Vec<ToolProgramRecord>, ProgramStoreError>;

    /// Load programs by submission key (for idempotency).
    async fn get_by_submission_key(
        &self,
        submission_key: &str,
    ) -> Result<Option<ToolProgramRecord>, ProgramStoreError>;
}

// ─── In-memory implementation ─────────────────────────────────────

struct Inner {
    programs: std::collections::HashMap<String, ToolProgramRecord>,
    calls: std::collections::HashMap<String, ProgramCallRecord>,
    /// program_id -> ordered call IDs
    calls_by_program: std::collections::HashMap<String, Vec<String>>,
    /// submission_key -> program_id
    by_submission_key: std::collections::HashMap<String, String>,
}

/// In-memory tool program store for tests.
pub struct InMemoryToolProgramStore {
    inner: AsyncMutex<Inner>,
}

impl InMemoryToolProgramStore {
    pub fn new() -> Self {
        Self {
            inner: AsyncMutex::new(Inner {
                programs: std::collections::HashMap::new(),
                calls: std::collections::HashMap::new(),
                calls_by_program: std::collections::HashMap::new(),
                by_submission_key: std::collections::HashMap::new(),
            }),
        }
    }
}

impl Default for InMemoryToolProgramStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolProgramStore for InMemoryToolProgramStore {
    async fn create_program(
        &self,
        record: ToolProgramRecord,
    ) -> Result<ToolProgramRecord, ProgramStoreError> {
        let mut inner = self.inner.lock().await;
        if inner.programs.contains_key(record.program_id.as_str()) {
            return Err(ProgramStoreError::Conflict(format!(
                "program {} already exists",
                record.program_id
            )));
        }
        if inner.by_submission_key.contains_key(&record.submission_key) {
            return Err(ProgramStoreError::DuplicateSubmission(
                record.submission_key.clone(),
            ));
        }
        let id = record.program_id.as_str().to_string();
        let sk = record.submission_key.clone();
        inner.programs.insert(id.clone(), record.clone());
        inner.by_submission_key.insert(sk, id);
        Ok(record)
    }

    async fn get_program(
        &self,
        id: &ToolProgramId,
    ) -> Result<Option<ToolProgramRecord>, ProgramStoreError> {
        let inner = self.inner.lock().await;
        Ok(inner.programs.get(id.as_str()).cloned())
    }

    async fn list_programs(
        &self,
        query: ProgramStoreQuery,
    ) -> Result<Vec<ProgramSummary>, ProgramStoreError> {
        let inner = self.inner.lock().await;
        let mut results: Vec<ProgramSummary> = inner
            .programs
            .values()
            .filter(|p| {
                query
                    .workspace_id
                    .as_ref()
                    .map(|w| &p.workspace_id == w)
                    .unwrap_or(true)
            })
            .filter(|p| {
                query
                    .session_id
                    .as_ref()
                    .map(|s| p.session_id.as_ref() == Some(s))
                    .unwrap_or(true)
            })
            .filter(|p| query.states.is_empty() || query.states.contains(&p.state))
            .map(|p| ProgramSummary {
                program_id: p.program_id.clone(),
                state: p.state,
                language: p.language,
                submission_key: p.submission_key.clone(),
                job_id: p.job_id.clone(),
                created_at: p.created_at,
                updated_at: p.updated_at,
                terminal_at: p.terminal_at,
            })
            .collect();
        results.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        let offset = query.offset.unwrap_or(0) as usize;
        let limit = query.limit.unwrap_or(100) as usize;
        results = results.into_iter().skip(offset).take(limit).collect();
        Ok(results)
    }

    async fn transition_program(
        &self,
        id: &ToolProgramId,
        expected: ToolProgramState,
        to: ToolProgramState,
    ) -> Result<ToolProgramRecord, ProgramStoreError> {
        let mut inner = self.inner.lock().await;
        let record = inner
            .programs
            .get_mut(id.as_str())
            .ok_or_else(|| ProgramStoreError::NotFound(id.as_str().to_string()))?;

        if record.state != expected {
            return Err(ProgramStoreError::InvalidTransition {
                program: id.as_str().to_string(),
                from: record.state,
                to,
            });
        }

        super::validate_program_transition(expected, to).map_err(|(from, to)| {
            ProgramStoreError::InvalidTransition {
                program: id.as_str().to_string(),
                from,
                to,
            }
        })?;

        record.state = to;
        record.updated_at = Utc::now();
        if to.is_terminal() {
            record.terminal_at = Some(record.updated_at);
        }

        Ok(record.clone())
    }

    async fn set_job_id(
        &self,
        id: &ToolProgramId,
        job_id: &str,
    ) -> Result<ToolProgramRecord, ProgramStoreError> {
        let mut inner = self.inner.lock().await;
        let record = inner
            .programs
            .get_mut(id.as_str())
            .ok_or_else(|| ProgramStoreError::NotFound(id.as_str().to_string()))?;
        record.job_id = Some(job_id.to_string());
        record.updated_at = Utc::now();
        Ok(record.clone())
    }

    async fn set_ir_ref(
        &self,
        id: &ToolProgramId,
        ir_ref: super::ProgramIrRef,
    ) -> Result<ToolProgramRecord, ProgramStoreError> {
        let mut inner = self.inner.lock().await;
        let record = inner
            .programs
            .get_mut(id.as_str())
            .ok_or_else(|| ProgramStoreError::NotFound(id.as_str().to_string()))?;
        record.ir_ref = Some(ir_ref);
        record.updated_at = Utc::now();
        Ok(record.clone())
    }

    async fn reserve_call(
        &self,
        program_id: &ToolProgramId,
        mut record: ProgramCallRecord,
    ) -> Result<ProgramCallRecord, ProgramStoreError> {
        let mut inner = self.inner.lock().await;

        if !inner.programs.contains_key(program_id.as_str()) {
            return Err(ProgramStoreError::NotFound(program_id.as_str().to_string()));
        }

        let call_ids = inner
            .calls_by_program
            .entry(program_id.as_str().to_string())
            .or_default();

        let sequence = call_ids.len() as u32;
        record.sequence = sequence;
        call_ids.push(record.call_id.as_str().to_string());

        inner
            .calls
            .insert(record.call_id.as_str().to_string(), record.clone());

        Ok(record)
    }

    async fn transition_call(
        &self,
        call_id: &ProgramCallId,
        expected: ProgramCallState,
        to: ProgramCallState,
    ) -> Result<ProgramCallRecord, ProgramStoreError> {
        let mut inner = self.inner.lock().await;
        let record = inner
            .calls
            .get_mut(call_id.as_str())
            .ok_or_else(|| ProgramStoreError::CallNotFound(call_id.as_str().to_string()))?;

        if record.state != expected {
            return Err(ProgramStoreError::InvalidCallTransition {
                call: call_id.as_str().to_string(),
                from: record.state,
                to,
            });
        }

        if expected.is_terminal() {
            return Err(ProgramStoreError::CallAlreadyTerminal(
                call_id.as_str().to_string(),
            ));
        }

        record.state = to;
        record.updated_at = Utc::now();
        if to.is_terminal() {
            record.terminal_at = Some(record.updated_at);
        }
        record.attempts += 1;

        Ok(record.clone())
    }

    async fn get_call(
        &self,
        call_id: &ProgramCallId,
    ) -> Result<Option<ProgramCallRecord>, ProgramStoreError> {
        let inner = self.inner.lock().await;
        Ok(inner.calls.get(call_id.as_str()).cloned())
    }

    async fn list_calls(
        &self,
        program_id: &ToolProgramId,
    ) -> Result<Vec<ProgramCallRecord>, ProgramStoreError> {
        let inner = self.inner.lock().await;
        let call_ids = inner
            .calls_by_program
            .get(program_id.as_str())
            .cloned()
            .unwrap_or_default();
        let calls: Vec<ProgramCallRecord> = call_ids
            .iter()
            .filter_map(|id| inner.calls.get(id).cloned())
            .collect();
        Ok(calls)
    }

    async fn set_checkpoint(
        &self,
        id: &ToolProgramId,
        checkpoint: ProgramCheckpoint,
    ) -> Result<(), ProgramStoreError> {
        let mut inner = self.inner.lock().await;
        let record = inner
            .programs
            .get_mut(id.as_str())
            .ok_or_else(|| ProgramStoreError::NotFound(id.as_str().to_string()))?;
        record.checkpoint = Some(checkpoint);
        record.updated_at = Utc::now();
        Ok(())
    }

    async fn set_result(
        &self,
        id: &ToolProgramId,
        result: ProgramResult,
    ) -> Result<ToolProgramRecord, ProgramStoreError> {
        let mut inner = self.inner.lock().await;
        let record = inner
            .programs
            .get_mut(id.as_str())
            .ok_or_else(|| ProgramStoreError::NotFound(id.as_str().to_string()))?;
        record.result = Some(result);
        record.updated_at = Utc::now();
        if record.state.is_terminal() {
            record.terminal_at = Some(record.updated_at);
        }
        Ok(record.clone())
    }

    async fn list_non_terminal(&self) -> Result<Vec<ToolProgramRecord>, ProgramStoreError> {
        let inner = self.inner.lock().await;
        Ok(inner
            .programs
            .values()
            .filter(|p| !p.state.is_terminal())
            .cloned()
            .collect())
    }

    async fn get_by_submission_key(
        &self,
        submission_key: &str,
    ) -> Result<Option<ToolProgramRecord>, ProgramStoreError> {
        let inner = self.inner.lock().await;
        Ok(inner
            .by_submission_key
            .get(submission_key)
            .and_then(|id| inner.programs.get(id))
            .cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_program::*;

    fn test_record(submission_key: &str) -> ToolProgramRecord {
        ToolProgramRecord {
            program_id: ToolProgramId::new_unchecked(uuid::Uuid::new_v4().to_string()),
            workspace_id: "w1".to_string(),
            session_id: Some("s1".to_string()),
            turn_id: None,
            language: ProgramLanguage::RestrictedPython,
            state: ToolProgramState::Submitted,
            source_ref: ProgramSourceRef {
                digest: "abc123".to_string(),
                byte_length: 100,
                schema_version: 1,
                content_location: "store:src".to_string(),
            },
            ir_ref: None,
            manifest: ProgramCapabilityManifest {
                manifest_version: 1,
                tools: std::collections::HashMap::new(),
                max_concurrent_calls: 1,
                max_total_calls: 100,
                authority_digest: "auth1".to_string(),
                allow_mutations: false,
                resource_limits: ProgramLimitsSnapshot::default(),
            },
            checkpoint: None,
            result: None,
            job_id: None,
            submission_key: submission_key.to_string(),
            labels: std::collections::HashMap::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            terminal_at: None,
        }
    }

    fn test_call_record() -> ProgramCallRecord {
        ProgramCallRecord {
            call_id: ProgramCallId::new_unchecked(uuid::Uuid::new_v4().to_string()),
            sequence: 0,
            tool_name: "read".to_string(),
            tool_contract_hash: "hash1".to_string(),
            normalized_input_hash: "input1".to_string(),
            state: ProgramCallState::Reserved,
            attempts: 0,
            child_job_id: None,
            child_run_id: None,
            result_artifacts: vec![],
            result_projection: None,
            failure_class: None,
            error_message: None,
            replay_disposition: ReplayDisposition::Replay,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            terminal_at: None,
        }
    }

    #[tokio::test]
    async fn create_and_get_program() {
        let store = InMemoryToolProgramStore::new();
        let record = test_record("key1");
        let id = record.program_id.clone();
        store.create_program(record).await.unwrap();
        let got = store.get_program(&id).await.unwrap();
        assert!(got.is_some());
        assert_eq!(got.unwrap().state, ToolProgramState::Submitted);
    }

    #[tokio::test]
    async fn duplicate_submission_key_rejected() {
        let store = InMemoryToolProgramStore::new();
        let r1 = test_record("key1");
        let r2 = test_record("key1");
        store.create_program(r1).await.unwrap();
        let result = store.create_program(r2).await;
        assert!(matches!(
            result,
            Err(ProgramStoreError::DuplicateSubmission(_))
        ));
    }

    #[tokio::test]
    async fn program_transition_valid() {
        let store = InMemoryToolProgramStore::new();
        let record = test_record("key1");
        let id = record.program_id.clone();
        store.create_program(record).await.unwrap();
        let updated = store
            .transition_program(&id, ToolProgramState::Submitted, ToolProgramState::Queued)
            .await
            .unwrap();
        assert_eq!(updated.state, ToolProgramState::Queued);
    }

    #[tokio::test]
    async fn program_transition_invalid() {
        let store = InMemoryToolProgramStore::new();
        let record = test_record("key1");
        let id = record.program_id.clone();
        store.create_program(record).await.unwrap();
        let result = store
            .transition_program(
                &id,
                ToolProgramState::Submitted,
                ToolProgramState::Completed,
            )
            .await;
        assert!(matches!(
            result,
            Err(ProgramStoreError::InvalidTransition { .. })
        ));
    }

    #[tokio::test]
    async fn reserve_and_list_calls() {
        let store = InMemoryToolProgramStore::new();
        let record = test_record("key1");
        let pid = record.program_id.clone();
        store.create_program(record).await.unwrap();

        let call1 = test_call_record();
        let call2 = test_call_record();
        let c1 = store.reserve_call(&pid, call1).await.unwrap();
        let c2 = store.reserve_call(&pid, call2).await.unwrap();
        assert_eq!(c1.sequence, 0);
        assert_eq!(c2.sequence, 1);

        let calls = store.list_calls(&pid).await.unwrap();
        assert_eq!(calls.len(), 2);
    }

    #[tokio::test]
    async fn call_transition() {
        let store = InMemoryToolProgramStore::new();
        let record = test_record("key1");
        let pid = record.program_id.clone();
        store.create_program(record).await.unwrap();

        let call = test_call_record();
        let cid = call.call_id.clone();
        store.reserve_call(&pid, call).await.unwrap();

        let updated = store
            .transition_call(&cid, ProgramCallState::Reserved, ProgramCallState::Running)
            .await
            .unwrap();
        assert_eq!(updated.state, ProgramCallState::Running);
        assert_eq!(updated.attempts, 1);
    }

    #[tokio::test]
    async fn terminal_state_blocks_transition() {
        let store = InMemoryToolProgramStore::new();
        let record = test_record("key1");
        let pid = record.program_id.clone();
        store.create_program(record).await.unwrap();

        let call = test_call_record();
        let cid = call.call_id.clone();
        store.reserve_call(&pid, call).await.unwrap();

        store
            .transition_call(&cid, ProgramCallState::Reserved, ProgramCallState::Running)
            .await
            .unwrap();
        store
            .transition_call(&cid, ProgramCallState::Running, ProgramCallState::Completed)
            .await
            .unwrap();

        let result = store
            .transition_call(&cid, ProgramCallState::Completed, ProgramCallState::Failed)
            .await;
        assert!(matches!(
            result,
            Err(ProgramStoreError::CallAlreadyTerminal(_))
        ));
    }

    #[tokio::test]
    async fn list_non_terminal() {
        let store = InMemoryToolProgramStore::new();
        let r1 = test_record("key1");
        let r2 = test_record("key2");
        let id2 = r2.program_id.clone();
        store.create_program(r1).await.unwrap();
        store.create_program(r2).await.unwrap();

        // Transition r2 to terminal through the correct chain
        store
            .transition_program(&id2, ToolProgramState::Submitted, ToolProgramState::Queued)
            .await
            .unwrap();
        store
            .transition_program(&id2, ToolProgramState::Queued, ToolProgramState::Running)
            .await
            .unwrap();
        store
            .transition_program(&id2, ToolProgramState::Running, ToolProgramState::Completed)
            .await
            .unwrap();

        let non_terminal = store.list_non_terminal().await.unwrap();
        assert_eq!(non_terminal.len(), 1);
        assert_eq!(non_terminal[0].state, ToolProgramState::Submitted);
    }

    #[tokio::test]
    async fn get_by_submission_key() {
        let store = InMemoryToolProgramStore::new();
        let record = test_record("unique-key");
        let id = record.program_id.clone();
        store.create_program(record).await.unwrap();
        let got = store.get_by_submission_key("unique-key").await.unwrap();
        assert!(got.is_some());
        assert_eq!(got.unwrap().program_id, id);
    }

    #[tokio::test]
    async fn set_checkpoint() {
        let store = InMemoryToolProgramStore::new();
        let record = test_record("key1");
        let id = record.program_id.clone();
        store.create_program(record).await.unwrap();

        let checkpoint = ProgramCheckpoint {
            ir_version: 1,
            ir_hash: "ir_hash".to_string(),
            instruction_cursor: 42,
            loop_frames: vec![],
            completed_call_cursor: 3,
            remaining_steps: 9999,
            remaining_time_ms: 290_000,
            local_values: std::collections::HashMap::new(),
        };
        store.set_checkpoint(&id, checkpoint).await.unwrap();
        let got = store.get_program(&id).await.unwrap().unwrap();
        assert!(got.checkpoint.is_some());
        assert_eq!(got.checkpoint.unwrap().instruction_cursor, 42);
    }

    #[tokio::test]
    async fn set_result_and_terminal_at() {
        let store = InMemoryToolProgramStore::new();
        let record = test_record("key1");
        let id = record.program_id.clone();
        store.create_program(record).await.unwrap();

        // Move to terminal through the correct chain
        store
            .transition_program(&id, ToolProgramState::Submitted, ToolProgramState::Queued)
            .await
            .unwrap();
        store
            .transition_program(&id, ToolProgramState::Queued, ToolProgramState::Running)
            .await
            .unwrap();
        store
            .transition_program(&id, ToolProgramState::Running, ToolProgramState::Completed)
            .await
            .unwrap();

        let result = ProgramResult {
            terminal_type: super::super::ProgramTerminalType::Success,
            schema_version: 1,
            value: None,
            artifacts: vec![],
            has_partial_results: false,
            failure_summary: None,
            budget_usage: super::super::ProgramBudgetUsage {
                steps_used: 10,
                elapsed_ms: 500,
                peak_memory_mb: 64,
                total_calls: 1,
                artifact_bytes: 0,
            },
            recorded_at: Utc::now(),
        };
        let updated = store.set_result(&id, result).await.unwrap();
        assert!(updated.terminal_at.is_some());
    }

    #[tokio::test]
    async fn list_programs_filters() {
        let store = InMemoryToolProgramStore::new();
        let mut r1 = test_record("key1");
        r1.workspace_id = "w1".to_string();
        let mut r2 = test_record("key2");
        r2.workspace_id = "w2".to_string();
        store.create_program(r1).await.unwrap();
        store.create_program(r2).await.unwrap();

        let query = ProgramStoreQuery {
            workspace_id: Some("w1".to_string()),
            ..Default::default()
        };
        let results = store.list_programs(query).await.unwrap();
        assert_eq!(results.len(), 1);
    }
}
