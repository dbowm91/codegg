//! Durable, typed terminal results for Tool Programs.
//!
//! The scheduler, foreground tool, background notification service, and
//! inspection API all read the same bounded record. Human-readable executor
//! summaries are presentation only and are never parsed for semantics.
//!
//! M012-F07: Result records now carry bounded artifact handles and verify
//! their stored digest on every load. Corrupt or mismatched records fail
//! closed and remain diagnostically inspectable.

use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::Utc;
use codegg_core::tool_program::ProgramResult;
use serde::{Deserialize, Serialize};
use sha2::Digest;
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
    #[error("result digest mismatch: stored={stored} computed={computed}")]
    DigestMismatch { stored: String, computed: String },
    #[error("result version mismatch")]
    VersionMismatch,
    #[error("canonical artifact error: {0}")]
    Artifact(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalArtifactRef {
    pub handle: String,
    pub digest: String,
    pub byte_length: usize,
}

pub async fn persist_program_artifact(
    store: std::sync::Arc<dyn crate::context::ContextArtifactStore>,
    session_id: &str,
    call_id: &str,
    tool_name: &str,
    content: &[u8],
) -> Result<CanonicalArtifactRef, ToolProgramResultError> {
    let redacted_content = String::from_utf8_lossy(content).into_owned();
    let handle = crate::context::ContextHandle::build_tool(session_id, 0, call_id)
        .map_err(|error| ToolProgramResultError::Artifact(error.to_string()))?;
    let content_hash = crate::context::compute_content_hash(&redacted_content);
    let artifact = crate::context::ContextArtifact {
        handle: handle.clone(),
        session_id: session_id.to_string(),
        turn_index: 0,
        tool_call_id: Some(call_id.to_string()),
        tool_name: Some(tool_name.to_string()),
        kind: crate::context::ArtifactKind::ToolResult,
        created_at_ms: Utc::now().timestamp_millis(),
        content_hash: content_hash.clone(),
        redacted_content,
        raw_bytes_len: content.len(),
        estimated_tokens: crate::context::estimate_tokens(&String::from_utf8_lossy(content)),
    };
    store
        .put(artifact)
        .await
        .map_err(|error| ToolProgramResultError::Artifact(error.to_string()))?;
    Ok(CanonicalArtifactRef {
        handle,
        digest: format!("sha256:{content_hash}"),
        byte_length: content.len(),
    })
}

pub async fn resolve_program_artifact(
    store: std::sync::Arc<dyn crate::context::ContextArtifactStore>,
    reference: &CanonicalArtifactRef,
) -> Result<crate::context::ContextArtifact, ToolProgramResultError> {
    let artifact = store
        .get(&reference.handle)
        .await
        .map_err(|error| ToolProgramResultError::Artifact(error.to_string()))?
        .ok_or_else(|| ToolProgramResultError::Artifact("artifact is missing".into()))?;
    let computed = format!(
        "sha256:{}",
        crate::context::compute_content_hash(&artifact.redacted_content)
    );
    if computed != reference.digest
        || artifact.raw_bytes_len != reference.byte_length
        || format!("sha256:{}", artifact.content_hash) != reference.digest
    {
        return Err(ToolProgramResultError::Artifact(
            "artifact content digest or length mismatch".into(),
        ));
    }
    Ok(artifact)
}

/// Bounded artifact handle for a tool call or child job result.
///
/// M012-F07: Replaces the unconditional empty `program_artifacts` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramArtifactHandle {
    /// Tool name that produced this artifact (for call artifacts).
    pub tool_name: Option<String>,
    /// Bounded display preview (first ~200 chars).
    pub preview: String,
    /// Whether the call succeeded.
    pub success: bool,
    /// Artifact handle (ctx:// URI) for the full output content.
    pub artifact_id: Option<String>,
    /// Content digest for integrity verification.
    pub digest: Option<String>,
    #[serde(default)]
    pub absence_reason: Option<String>,
}

/// Bounded child job artifact handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildArtifactHandle {
    /// Child job ID.
    pub job_id: String,
    #[serde(default)]
    pub attempt_id: Option<String>,
    /// Child run ID, if available.
    pub run_id: Option<String>,
    /// Child terminal status.
    pub status: String,
    /// Artifact handle for the child's output.
    pub artifact_id: Option<String>,
    /// Content digest.
    pub digest: Option<String>,
    #[serde(default)]
    pub absence_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramResultRecord {
    pub schema_version: u16,
    pub program_id: String,
    pub attempt_id: String,
    pub selected_backend: String,
    pub result: ProgramResult,
    /// Bounded call artifact handles (M012-F07).
    #[serde(default)]
    pub call_artifacts: Vec<ProgramArtifactHandle>,
    /// Bounded child job artifact handles (M012-F07).
    #[serde(default)]
    pub child_artifacts: Vec<ChildArtifactHandle>,
    /// Output artifact handle, if the output was spilled to an artifact.
    #[serde(default)]
    pub output_artifact: Option<String>,
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
        call_artifacts: Vec<ProgramArtifactHandle>,
        child_artifacts: Vec<ChildArtifactHandle>,
        output_artifact: Option<String>,
    ) -> Result<ProgramResultRecord, ToolProgramResultError> {
        validate_identity(program_id)?;
        if attempt_id.is_empty() || attempt_id.len() > MAX_PROGRAM_ID_BYTES {
            return Err(ToolProgramResultError::InvalidIdentity);
        }
        if let Some(error) = result.error_message.as_mut() {
            error.truncate(4096);
        }

        let mut record = ProgramResultRecord {
            schema_version: 2,
            program_id: program_id.to_string(),
            attempt_id: attempt_id.to_string(),
            selected_backend: selected_backend.into(),
            result,
            call_artifacts,
            child_artifacts,
            output_artifact,
            result_digest: String::new(),
            recorded_at: Utc::now().timestamp_millis(),
        };
        // M013-H1: Compute digest over the complete semantic record
        // excluding the digest field itself. Every field that consumers
        // depend on (result, call_artifacts, child_artifacts,
        // output_artifact, selected_backend, schema identity) is part
        // of the signed payload so any tampering causes load-time
        // DigestMismatch.
        record.result_digest = compute_full_record_digest(&record)?;
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
        let metadata = match path.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() || metadata.len() > MAX_RESULT_BYTES as u64 {
            return Err(ToolProgramResultError::Oversized);
        }
        let file = match open_result_file(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_RESULT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_RESULT_BYTES {
            return Err(ToolProgramResultError::Oversized);
        }
        let record: ProgramResultRecord = serde_json::from_slice(&bytes)?;
        if record.program_id != program_id {
            return Err(ToolProgramResultError::IdentityMismatch);
        }
        // M012-F07: Verify schema version.
        if record.schema_version != 2 {
            return Err(ToolProgramResultError::VersionMismatch);
        }
        // M013-H1: Recompute digest over the full semantic record and
        // reject mismatch. The full-record digest covers every field a
        // consumer reads (result, call_artifacts, child_artifacts,
        // output_artifact, selected_backend, identities), so any tamper
        // — including appending a forged artifact — fails closed.
        let computed = compute_full_record_digest(&record)?;
        if computed != record.result_digest {
            return Err(ToolProgramResultError::DigestMismatch {
                stored: record.result_digest.clone(),
                computed,
            });
        }
        Ok(Some(record))
    }

    fn path(&self, program_id: &str) -> PathBuf {
        self.base_dir.join(format!("{program_id}.json"))
    }
}

#[cfg(unix)]
fn open_result_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_result_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

pub fn result_to_json(record: &ProgramResultRecord) -> serde_json::Value {
    let status = serde_json::to_value(record.result.status).unwrap_or_else(|_| "failed".into());
    let failure_class = record
        .result
        .failure_class
        .as_ref()
        .and_then(|class| serde_json::to_value(class).ok());
    let call_artifacts: Vec<serde_json::Value> = record
        .call_artifacts
        .iter()
        .map(|a| {
            serde_json::json!({
                "tool_name": a.tool_name,
                "success": a.success,
                "artifact_handle": a.artifact_id,
                "preview": a.preview,
                "digest": a.digest,
                "absence_reason": a.absence_reason,
            })
        })
        .collect();
    let child_artifacts: Vec<serde_json::Value> = record
        .child_artifacts
        .iter()
        .map(|a| {
            serde_json::json!({
                "job_id": a.job_id,
                "attempt_id": a.attempt_id,
                "run_id": a.run_id,
                "status": a.status,
                "artifact_handle": a.artifact_id,
                "digest": a.digest,
                "absence_reason": a.absence_reason,
            })
        })
        .collect();
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
        "program_artifacts": call_artifacts,
        "child_artifacts": child_artifacts,
        "output_artifact": record.output_artifact,
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

/// M013-H1: Compute the SHA-256 digest over every semantic field of the
/// result record (schema version, identities, backend, terminal result,
/// call artifacts, child artifacts, output artifact). The digest field
/// itself is excluded by serialising a canonical projection that
/// contains only the security-relevant fields.
fn compute_full_record_digest(
    record: &ProgramResultRecord,
) -> Result<String, ToolProgramResultError> {
    let projection = serde_json::json!({
        "schema_version": record.schema_version,
        "program_id": record.program_id,
        "attempt_id": record.attempt_id,
        "selected_backend": record.selected_backend,
        "result": record.result,
        "call_artifacts": record.call_artifacts,
        "child_artifacts": record.child_artifacts,
        "output_artifact": record.output_artifact,
    });
    let bytes = serde_json::to_vec(&projection)?;
    Ok(format!("sha256:{:x}", Sha256::digest(&bytes)))
}

use sha2::Sha256;

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
            calls_completed: 3,
            calls_total: 3,
            iterations_used: 1,
        };
        let record = store
            .persist(
                "tp-test",
                "attempt-1",
                "native",
                result,
                vec![],
                vec![],
                None,
            )
            .unwrap();
        let loaded = store.load("tp-test").unwrap().unwrap();
        assert_eq!(loaded.result.steps_used, 7);
        assert_eq!(loaded.result.calls_total, 3);
        assert_eq!(loaded.result_digest, record.result_digest);
    }
}
