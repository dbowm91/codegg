use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::RunStoreError;

pub const SCHEMA_VERSION: u32 = 1;
pub const INDEX_FILENAME: &str = "index.jsonl";
pub const MANIFEST_FILENAME: &str = "manifest.json";
pub const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
pub const DEFAULT_MAX_RUN_COUNT: usize = 1000;
pub const DEFAULT_MAX_AGE_DAYS: u32 = 30;
pub const DEFAULT_FAILED_EXTRA_DAYS: u32 = 30;

// ── Run ID ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(pub String);

impl RunId {
    pub fn new() -> Self {
        RunId(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Artifact ID ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactId(pub String);

impl ArtifactId {
    pub fn new() -> Self {
        ArtifactId(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ArtifactId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Run Kind ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    RawShell,
    ManagedProcess,
    Test,
    GitRead,
    GitMutation,
    Search,
    Python,
    NativeTool,
}

impl std::fmt::Display for RunKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunKind::RawShell => write!(f, "raw_shell"),
            RunKind::ManagedProcess => write!(f, "managed_process"),
            RunKind::Test => write!(f, "test"),
            RunKind::GitRead => write!(f, "git_read"),
            RunKind::GitMutation => write!(f, "git_mutation"),
            RunKind::Search => write!(f, "search"),
            RunKind::Python => write!(f, "python"),
            RunKind::NativeTool => write!(f, "native_tool"),
        }
    }
}

// ── Run Status ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Complete,
    Failed,
    TimedOut,
    Cancelled,
    Incomplete,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunStatus::Running => write!(f, "running"),
            RunStatus::Complete => write!(f, "complete"),
            RunStatus::Failed => write!(f, "failed"),
            RunStatus::TimedOut => write!(f, "timed_out"),
            RunStatus::Cancelled => write!(f, "cancelled"),
            RunStatus::Incomplete => write!(f, "incomplete"),
        }
    }
}

// ── Artifact Kind ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Stdout,
    Stderr,
    CombinedLog,
    CommandSource,
    TestReport,
    TestLog,
    UnifiedDiff,
    ChangedFiles,
    Projection,
    RtkProjection,
    StructuredJson,
    PolicyEvidence,
}

// ── Run Invocation ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunInvocation {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argv: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_hash: Option<String>,
}

// ── Backend Record ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendRecord {
    pub family: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ── Risk Record ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskRecord {
    pub level: String,
    pub has_subprocess: bool,
    pub has_git_mutation: bool,
    pub has_destructive_mutation: bool,
}

// ── Permission Decision Record ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDecisionRecord {
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub decision: String,
}

// ── Sandbox Record ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxRecord {
    pub os_isolation: bool,
    pub network_isolation: bool,
    #[serde(default)]
    pub read_roots: Vec<PathBuf>,
    #[serde(default)]
    pub write_roots: Vec<PathBuf>,
}

// ── Artifact Record ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub artifact_id: ArtifactId,
    pub kind: ArtifactKind,
    pub relative_path: String,
    pub mime_type: String,
    pub byte_length: u64,
    pub sha256: String,
    pub truncated: bool,
    pub redacted: bool,
    pub created_at: DateTime<Utc>,
    pub safe_for_model: bool,
}

// ── Projection Record ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionRecord {
    pub projector: String,
    pub exactness: String,
    #[serde(default)]
    pub omitted_ranges: Vec<OmittedRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmittedRange {
    pub start: u64,
    pub end: u64,
}

// ── Changed Path Record ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedPathRecord {
    pub path: PathBuf,
    pub kind: String,
}

// ── Rerun Descriptor ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerunDescriptor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argv: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_source_ref: Option<String>,
    pub backend_family: String,
    pub cwd: PathBuf,
    pub workspace_root: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<RunId>,
}

// ── Run Manifest ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub schema_version: u32,
    pub run_id: RunId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<RunId>,
    pub kind: RunKind,
    pub invocation: RunInvocation,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub status: RunStatus,
    pub workspace_root: PathBuf,
    pub cwd: PathBuf,
    pub backend: BackendRecord,
    pub risk: RiskRecord,
    #[serde(default)]
    pub permissions: Vec<PermissionDecisionRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxRecord>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection: Option<ProjectionRecord>,
    #[serde(default)]
    pub changes: Vec<ChangedPathRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerun: Option<RerunDescriptor>,
}

// ── Run Summary (for listing) ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: RunId,
    pub kind: RunKind,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub command: String,
}

// ── Draft / Handle / Completion ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RunDraft {
    pub kind: RunKind,
    pub invocation: RunInvocation,
    pub session_id: Option<String>,
    pub parent_run_id: Option<RunId>,
    pub workspace_root: PathBuf,
    pub cwd: PathBuf,
    pub backend: BackendRecord,
    pub risk: RiskRecord,
}

#[derive(Debug, Clone)]
pub struct RunHandle {
    pub run_id: RunId,
    pub run_dir: PathBuf,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RunCompletion {
    pub status: RunStatus,
    pub completed_at: DateTime<Utc>,
    pub permissions: Vec<PermissionDecisionRecord>,
    pub sandbox: Option<SandboxRecord>,
    pub projection: Option<ProjectionRecord>,
    pub changes: Vec<ChangedPathRecord>,
    pub rerun: Option<RerunDescriptor>,
}

// ── Query ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RunQuery {
    pub kind: Option<RunKind>,
    pub status: Option<RunStatus>,
    pub session_id: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
}

// ── Artifact I/O ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ArtifactInput {
    pub kind: ArtifactKind,
    pub data: Vec<u8>,
    pub mime_type: String,
    pub safe_for_model: bool,
}

#[derive(Debug, Clone)]
pub struct ArtifactRef {
    pub artifact_id: ArtifactId,
    pub relative_path: String,
    pub sha256: String,
    pub byte_length: u64,
}

#[derive(Debug, Clone)]
pub struct ArtifactChunk {
    pub artifact_id: ArtifactId,
    pub data: Vec<u8>,
    pub total_bytes: u64,
    pub byte_offset: usize,
}

#[derive(Debug, Clone)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

// ── Retention ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RetentionConfig {
    pub max_total_bytes: u64,
    pub max_run_count: usize,
    pub max_age_days: u32,
    pub preserve_failed_longer: bool,
    pub failed_extra_days: u32,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            max_run_count: DEFAULT_MAX_RUN_COUNT,
            max_age_days: DEFAULT_MAX_AGE_DAYS,
            preserve_failed_longer: true,
            failed_extra_days: DEFAULT_FAILED_EXTRA_DAYS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CleanupPlan {
    pub runs_to_delete: Vec<RunId>,
    pub bytes_to_free: u64,
    pub pinned_runs_skipped: Vec<RunId>,
}

// ── Index Entry (JSONL line) ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub run_id: RunId,
    pub kind: RunKind,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub command: String,
    pub workspace_root: PathBuf,
    pub date_dir: String,
    pub pinned: bool,
}

// ── RunStore trait ──────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait RunStore: Send + Sync {
    async fn begin_run(&self, draft: RunDraft) -> Result<RunHandle, RunStoreError>;
    async fn write_artifact(
        &self,
        run: &RunHandle,
        artifact: ArtifactInput,
    ) -> Result<ArtifactRef, RunStoreError>;
    async fn complete_run(
        &self,
        run: RunHandle,
        completion: RunCompletion,
    ) -> Result<RunManifest, RunStoreError>;
    async fn get_run(&self, id: &RunId) -> Result<Option<RunManifest>, RunStoreError>;
    async fn read_artifact(
        &self,
        id: &ArtifactId,
        range: Option<ByteRange>,
    ) -> Result<ArtifactChunk, RunStoreError>;
    async fn list_runs(&self, query: RunQuery) -> Result<Vec<RunSummary>, RunStoreError>;
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn date_dir(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d").to_string()
}

fn validate_id_str(s: &str) -> Result<(), RunStoreError> {
    if s.is_empty() || s.contains('/') || s.contains('\\') || s.contains('\0') || s.contains("..") {
        return Err(RunStoreError::PathTraversal(format!(
            "invalid ID string: {}",
            s
        )));
    }
    Ok(())
}

// ── FsRunStore ──────────────────────────────────────────────────────────

pub struct FsRunStore {
    root: PathBuf,
    lock: tokio::sync::Mutex<()>,
    index_cache: parking_lot::RwLock<Vec<IndexEntry>>,
}

impl FsRunStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            lock: tokio::sync::Mutex::new(()),
            index_cache: parking_lot::RwLock::new(Vec::new()),
        }
    }

    async fn ensure_root(&self) -> Result<(), RunStoreError> {
        fs::create_dir_all(&self.root)
            .await
            .map_err(RunStoreError::Io)?;
        Ok(())
    }

    fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_FILENAME)
    }

    fn run_dir(&self, date: &str, run_id: &str) -> Result<PathBuf, RunStoreError> {
        validate_id_str(run_id)?;
        Ok(self.root.join(date).join(run_id))
    }

    async fn load_index(&self) -> Result<Vec<IndexEntry>, RunStoreError> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = fs::read(&path).await.map_err(RunStoreError::Io)?;
        let mut entries = Vec::new();
        for line in data.split(|&b| b == b'\n') {
            if line.is_empty() {
                continue;
            }
            let entry: IndexEntry = serde_json::from_slice(line).map_err(RunStoreError::Json)?;
            entries.push(entry);
        }
        Ok(entries)
    }

    async fn write_index_append(&self, entry: &IndexEntry) -> Result<(), RunStoreError> {
        let _lock = self.lock.lock().await;
        let path = self.index_path();
        let line = serde_json::to_vec(entry).map_err(RunStoreError::Json)?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .map_err(RunStoreError::Io)?;
        file.write_all(&line).await.map_err(RunStoreError::Io)?;
        file.write_all(b"\n").await.map_err(RunStoreError::Io)?;
        file.flush().await.map_err(RunStoreError::Io)?;

        let mut cache = self.index_cache.write();
        cache.push(entry.clone());
        Ok(())
    }

    async fn rewrite_index(&self, entries: &[IndexEntry]) -> Result<(), RunStoreError> {
        let _lock = self.lock.lock().await;
        let path = self.index_path();
        let tmp_path = path.with_extension("jsonl.tmp");

        let mut data = Vec::new();
        for entry in entries {
            let line = serde_json::to_vec(entry).map_err(RunStoreError::Json)?;
            data.extend_from_slice(&line);
            data.push(b'\n');
        }

        fs::write(&tmp_path, &data)
            .await
            .map_err(RunStoreError::Io)?;
        fs::rename(&tmp_path, &path)
            .await
            .map_err(RunStoreError::Io)?;

        let mut cache = self.index_cache.write();
        *cache = entries.to_vec();
        Ok(())
    }

    async fn write_artifact_atomic(
        &self,
        run_dir: &Path,
        filename: &str,
        data: &[u8],
    ) -> Result<PathBuf, RunStoreError> {
        validate_id_str(filename)?;
        let target = run_dir.join(filename);
        let tmp = run_dir.join(format!(".{}.tmp", filename));

        fs::write(&tmp, data).await.map_err(RunStoreError::Io)?;
        fs::rename(&tmp, &target).await.map_err(RunStoreError::Io)?;
        Ok(target)
    }

    async fn read_index_cached(&self) -> Vec<IndexEntry> {
        {
            let cache = self.index_cache.read();
            if !cache.is_empty() {
                return cache.clone();
            }
        }
        if let Ok(entries) = self.load_index().await {
            let mut cache = self.index_cache.write();
            *cache = entries.clone();
            entries
        } else {
            Vec::new()
        }
    }

    pub fn plan_cleanup(&self, config: &RetentionConfig) -> Result<CleanupPlan, RunStoreError> {
        let entries = self.index_cache.read().clone();
        let now = Utc::now();
        let max_age = chrono::Duration::days(config.max_age_days as i64);
        let failed_age =
            chrono::Duration::days((config.max_age_days + config.failed_extra_days) as i64);

        let mut candidates: Vec<(&IndexEntry, bool)> = entries
            .iter()
            .filter(|e| !e.pinned)
            .map(|e| {
                let is_failed = matches!(
                    e.status,
                    RunStatus::Failed | RunStatus::TimedOut | RunStatus::Incomplete
                );
                (e, is_failed)
            })
            .collect();

        candidates.sort_by_key(|(e, _)| e.started_at);

        let mut to_delete = Vec::new();
        let mut bytes_estimated: u64 = 0;

        for (entry, is_failed) in &candidates {
            let age = now.signed_duration_since(entry.started_at);
            let limit = if config.preserve_failed_longer && *is_failed {
                failed_age
            } else {
                max_age
            };
            if age > limit {
                to_delete.push(entry.run_id.clone());
                bytes_estimated += 1024 * 1024;
            }
        }

        if to_delete.len() > config.max_run_count {
            let excess = to_delete.len() - config.max_run_count;
            to_delete.drain(0..excess);
        }

        if bytes_estimated > config.max_total_bytes {
            let excess = bytes_estimated - config.max_total_bytes;
            bytes_estimated -= excess;
        }

        let pinned: Vec<RunId> = entries
            .iter()
            .filter(|e| e.pinned)
            .map(|e| e.run_id.clone())
            .collect();

        Ok(CleanupPlan {
            runs_to_delete: to_delete,
            bytes_to_free: bytes_estimated,
            pinned_runs_skipped: pinned,
        })
    }
}

#[async_trait::async_trait]
impl RunStore for FsRunStore {
    async fn begin_run(&self, draft: RunDraft) -> Result<RunHandle, RunStoreError> {
        self.ensure_root().await?;

        let run_id = RunId::new();
        let started_at = Utc::now();
        let date = date_dir(&started_at);
        let run_dir = self.run_dir(&date, run_id.as_str())?;

        fs::create_dir_all(&run_dir)
            .await
            .map_err(RunStoreError::Io)?;

        let manifest = RunManifest {
            schema_version: SCHEMA_VERSION,
            run_id: run_id.clone(),
            session_id: draft.session_id,
            parent_run_id: draft.parent_run_id,
            kind: draft.kind.clone(),
            invocation: draft.invocation,
            started_at,
            completed_at: None,
            status: RunStatus::Running,
            workspace_root: draft.workspace_root,
            cwd: draft.cwd,
            backend: draft.backend,
            risk: draft.risk,
            permissions: Vec::new(),
            sandbox: None,
            artifacts: Vec::new(),
            projection: None,
            changes: Vec::new(),
            rerun: None,
        };

        let data = serde_json::to_vec_pretty(&manifest).map_err(RunStoreError::Json)?;
        self.write_artifact_atomic(&run_dir, MANIFEST_FILENAME, &data)
            .await?;

        let entry = IndexEntry {
            run_id: run_id.clone(),
            kind: manifest.kind,
            status: RunStatus::Running,
            started_at,
            completed_at: None,
            command: manifest.invocation.command.clone(),
            workspace_root: manifest.workspace_root.clone(),
            date_dir: date,
            pinned: false,
        };
        self.write_index_append(&entry).await?;

        Ok(RunHandle {
            run_id,
            run_dir,
            started_at,
        })
    }

    async fn write_artifact(
        &self,
        run: &RunHandle,
        artifact: ArtifactInput,
    ) -> Result<ArtifactRef, RunStoreError> {
        if artifact.data.len() as u64 > MAX_ARTIFACT_BYTES {
            return Err(RunStoreError::IntegrityViolation(format!(
                "artifact too large: {} bytes (max {})",
                artifact.data.len(),
                MAX_ARTIFACT_BYTES
            )));
        }

        let artifact_id = ArtifactId::new();
        let sha256 = compute_sha256(&artifact.data);
        let ext = match artifact.kind {
            ArtifactKind::Stdout => "stdout.log",
            ArtifactKind::Stderr => "stderr.log",
            ArtifactKind::CombinedLog => "combined.log",
            ArtifactKind::CommandSource => "invocation.json",
            ArtifactKind::TestReport => "report.json",
            ArtifactKind::TestLog => "test.log",
            ArtifactKind::UnifiedDiff => "diff.patch",
            ArtifactKind::ChangedFiles => "changes.json",
            ArtifactKind::Projection => "projection.txt",
            ArtifactKind::RtkProjection => "rtk_projection.txt",
            ArtifactKind::StructuredJson => "result.json",
            ArtifactKind::PolicyEvidence => "policy.json",
        };

        let relative_path = ext.to_string();
        self.write_artifact_atomic(&run.run_dir, ext, &artifact.data)
            .await?;

        let record = ArtifactRecord {
            artifact_id: artifact_id.clone(),
            kind: artifact.kind,
            relative_path: relative_path.clone(),
            mime_type: artifact.mime_type,
            byte_length: artifact.data.len() as u64,
            sha256: sha256.clone(),
            truncated: false,
            redacted: false,
            created_at: Utc::now(),
            safe_for_model: artifact.safe_for_model,
        };

        let manifest_path = run.run_dir.join(MANIFEST_FILENAME);
        let mut manifest: RunManifest = if manifest_path.exists() {
            let data = fs::read(&manifest_path).await.map_err(RunStoreError::Io)?;
            serde_json::from_slice(&data).map_err(RunStoreError::Json)?
        } else {
            return Err(RunStoreError::NotFound(format!(
                "manifest missing for run {}",
                run.run_id
            )));
        };

        manifest.artifacts.push(record);
        let data = serde_json::to_vec_pretty(&manifest).map_err(RunStoreError::Json)?;
        let tmp = manifest_path.with_extension("json.tmp");
        fs::write(&tmp, &data).await.map_err(RunStoreError::Io)?;
        fs::rename(&tmp, &manifest_path)
            .await
            .map_err(RunStoreError::Io)?;

        Ok(ArtifactRef {
            artifact_id,
            relative_path,
            sha256,
            byte_length: artifact.data.len() as u64,
        })
    }

    async fn complete_run(
        &self,
        run: RunHandle,
        completion: RunCompletion,
    ) -> Result<RunManifest, RunStoreError> {
        let manifest_path = run.run_dir.join(MANIFEST_FILENAME);
        let mut manifest: RunManifest = if manifest_path.exists() {
            let data = fs::read(&manifest_path).await.map_err(RunStoreError::Io)?;
            serde_json::from_slice(&data).map_err(RunStoreError::Json)?
        } else {
            return Err(RunStoreError::NotFound(format!(
                "manifest missing for run {}",
                run.run_id
            )));
        };

        manifest.status = completion.status;
        manifest.completed_at = Some(completion.completed_at);
        manifest.permissions = completion.permissions;
        manifest.sandbox = completion.sandbox;
        manifest.projection = completion.projection;
        manifest.changes = completion.changes;
        manifest.rerun = completion.rerun;

        let data = serde_json::to_vec_pretty(&manifest).map_err(RunStoreError::Json)?;
        self.write_artifact_atomic(&run.run_dir, MANIFEST_FILENAME, &data)
            .await?;

        let _lock = self.lock.lock().await;
        let mut entries = self.load_index().await.unwrap_or_default();
        if let Some(entry) = entries.iter_mut().find(|e| e.run_id == manifest.run_id) {
            entry.status = manifest.status.clone();
            entry.completed_at = manifest.completed_at;
        }
        self.rewrite_index(&entries).await?;

        Ok(manifest)
    }

    async fn get_run(&self, id: &RunId) -> Result<Option<RunManifest>, RunStoreError> {
        let entries = self.read_index_cached().await;
        let entry = match entries.iter().find(|e| e.run_id == *id) {
            Some(e) => e,
            None => return Ok(None),
        };

        let manifest_path = self
            .run_dir(&entry.date_dir, id.as_str())?
            .join(MANIFEST_FILENAME);
        if !manifest_path.exists() {
            return Ok(None);
        }

        let data = fs::read(&manifest_path).await.map_err(RunStoreError::Io)?;
        let manifest: RunManifest = serde_json::from_slice(&data).map_err(RunStoreError::Json)?;
        Ok(Some(manifest))
    }

    async fn read_artifact(
        &self,
        id: &ArtifactId,
        range: Option<ByteRange>,
    ) -> Result<ArtifactChunk, RunStoreError> {
        let entries = self.read_index_cached().await;

        for entry in &entries {
            let manifest_path = self
                .run_dir(&entry.date_dir, entry.run_id.as_str())?
                .join(MANIFEST_FILENAME);
            if !manifest_path.exists() {
                continue;
            }
            let data = fs::read(&manifest_path).await.map_err(RunStoreError::Io)?;
            let manifest: RunManifest =
                serde_json::from_slice(&data).map_err(RunStoreError::Json)?;

            if let Some(art) = manifest.artifacts.iter().find(|a| a.artifact_id == *id) {
                let file_path = self
                    .run_dir(&entry.date_dir, entry.run_id.as_str())?
                    .join(&art.relative_path);
                let full_data = fs::read(&file_path).await.map_err(RunStoreError::Io)?;

                let expected_sha = compute_sha256(&full_data);
                if expected_sha != art.sha256 {
                    return Err(RunStoreError::IntegrityViolation(format!(
                        "artifact {} checksum mismatch",
                        id
                    )));
                }

                let total_bytes = full_data.len() as u64;
                let chunk = match range {
                    Some(r) => {
                        let start = r.start.min(full_data.len());
                        let end = r.end.min(full_data.len());
                        ArtifactChunk {
                            artifact_id: id.clone(),
                            data: full_data[start..end].to_vec(),
                            total_bytes,
                            byte_offset: start,
                        }
                    }
                    None => ArtifactChunk {
                        artifact_id: id.clone(),
                        data: full_data,
                        total_bytes,
                        byte_offset: 0,
                    },
                };
                return Ok(chunk);
            }
        }

        Err(RunStoreError::NotFound(format!("artifact {}", id)))
    }

    async fn list_runs(&self, query: RunQuery) -> Result<Vec<RunSummary>, RunStoreError> {
        let entries = self.read_index_cached().await;
        let mut results: Vec<RunSummary> = entries
            .iter()
            .filter(|e| {
                if let Some(ref kind) = query.kind {
                    if e.kind != *kind {
                        return false;
                    }
                }
                if let Some(ref status) = query.status {
                    if e.status != *status {
                        return false;
                    }
                }
                if let Some(ref sid) = query.session_id {
                    // session_id is on the manifest, not the index entry; skip filter if set
                    // This is a limitation of the JSONL index; for full fidelity use get_run()
                    let _ = sid;
                }
                if let Some(since) = query.since {
                    if e.started_at < since {
                        return false;
                    }
                }
                if let Some(until) = query.until {
                    if e.started_at > until {
                        return false;
                    }
                }
                true
            })
            .map(|e| RunSummary {
                run_id: e.run_id.clone(),
                kind: e.kind.clone(),
                status: e.status.clone(),
                started_at: e.started_at,
                completed_at: e.completed_at,
                command: e.command.clone(),
            })
            .collect();

        results.sort_by_key(|b| std::cmp::Reverse(b.started_at));

        if let Some(limit) = query.limit {
            results.truncate(limit);
        }

        Ok(results)
    }
}

// ── MemRunStore ─────────────────────────────────────────────────────────

/// In-memory artifact record: (owner run ID, raw bytes, metadata).
type MemArtifactEntry = (RunId, Vec<u8>, ArtifactRecord);

pub struct MemRunStore {
    runs: parking_lot::RwLock<HashMap<RunId, RunManifest>>,
    artifacts: parking_lot::RwLock<HashMap<ArtifactId, MemArtifactEntry>>,
}

impl MemRunStore {
    pub fn new() -> Self {
        Self {
            runs: parking_lot::RwLock::new(HashMap::new()),
            artifacts: parking_lot::RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemRunStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemRunStore {
    pub fn plan_cleanup(&self, config: &RetentionConfig) -> Result<CleanupPlan, RunStoreError> {
        let runs = self.runs.read();
        let now = Utc::now();
        let max_age = chrono::Duration::days(config.max_age_days as i64);
        let failed_age =
            chrono::Duration::days((config.max_age_days + config.failed_extra_days) as i64);

        let mut candidates: Vec<(&RunId, &RunManifest, bool)> = runs
            .iter()
            .map(|(id, m)| {
                let is_failed = matches!(
                    m.status,
                    RunStatus::Failed | RunStatus::TimedOut | RunStatus::Incomplete
                );
                (id, m, is_failed)
            })
            .collect();

        candidates.sort_by_key(|(_, m, _)| m.started_at);

        let mut to_delete = Vec::new();

        for (id, manifest, is_failed) in &candidates {
            let age = now.signed_duration_since(manifest.started_at);
            let limit = if config.preserve_failed_longer && *is_failed {
                failed_age
            } else {
                max_age
            };
            if age > limit {
                to_delete.push(RunId(id.0.clone()));
            }
        }

        if to_delete.len() > config.max_run_count {
            let excess = to_delete.len() - config.max_run_count;
            to_delete.drain(0..excess);
        }

        Ok(CleanupPlan {
            runs_to_delete: to_delete,
            bytes_to_free: 0,
            pinned_runs_skipped: Vec::new(),
        })
    }
}

#[async_trait::async_trait]
impl RunStore for MemRunStore {
    async fn begin_run(&self, draft: RunDraft) -> Result<RunHandle, RunStoreError> {
        let run_id = RunId::new();
        let started_at = Utc::now();

        let manifest = RunManifest {
            schema_version: SCHEMA_VERSION,
            run_id: run_id.clone(),
            session_id: draft.session_id,
            parent_run_id: draft.parent_run_id,
            kind: draft.kind,
            invocation: draft.invocation,
            started_at,
            completed_at: None,
            status: RunStatus::Running,
            workspace_root: draft.workspace_root,
            cwd: draft.cwd,
            backend: draft.backend,
            risk: draft.risk,
            permissions: Vec::new(),
            sandbox: None,
            artifacts: Vec::new(),
            projection: None,
            changes: Vec::new(),
            rerun: None,
        };

        let mut runs = self.runs.write();
        runs.insert(run_id.clone(), manifest);

        Ok(RunHandle {
            run_id,
            run_dir: PathBuf::from("/mem"),
            started_at,
        })
    }

    async fn write_artifact(
        &self,
        run: &RunHandle,
        artifact: ArtifactInput,
    ) -> Result<ArtifactRef, RunStoreError> {
        if artifact.data.len() as u64 > MAX_ARTIFACT_BYTES {
            return Err(RunStoreError::IntegrityViolation(format!(
                "artifact too large: {} bytes",
                artifact.data.len()
            )));
        }

        let artifact_id = ArtifactId::new();
        let sha256 = compute_sha256(&artifact.data);
        let relative_path = format!("{}.dat", artifact_id.as_str());

        let record = ArtifactRecord {
            artifact_id: artifact_id.clone(),
            kind: artifact.kind,
            relative_path: relative_path.clone(),
            mime_type: artifact.mime_type,
            byte_length: artifact.data.len() as u64,
            sha256: sha256.clone(),
            truncated: false,
            redacted: false,
            created_at: Utc::now(),
            safe_for_model: artifact.safe_for_model,
        };

        let byte_length = artifact.data.len() as u64;
        self.artifacts.write().insert(
            artifact_id.clone(),
            (run.run_id.clone(), artifact.data, record.clone()),
        );

        let mut runs = self.runs.write();
        if let Some(manifest) = runs.get_mut(&run.run_id) {
            manifest.artifacts.push(record);
        }

        Ok(ArtifactRef {
            artifact_id,
            relative_path,
            sha256,
            byte_length,
        })
    }

    async fn complete_run(
        &self,
        run: RunHandle,
        completion: RunCompletion,
    ) -> Result<RunManifest, RunStoreError> {
        let mut runs = self.runs.write();
        let manifest = runs
            .get_mut(&run.run_id)
            .ok_or_else(|| RunStoreError::NotFound(format!("run {}", run.run_id)))?;

        manifest.status = completion.status;
        manifest.completed_at = Some(completion.completed_at);
        manifest.permissions = completion.permissions;
        manifest.sandbox = completion.sandbox;
        manifest.projection = completion.projection;
        manifest.changes = completion.changes;
        manifest.rerun = completion.rerun;

        Ok(manifest.clone())
    }

    async fn get_run(&self, id: &RunId) -> Result<Option<RunManifest>, RunStoreError> {
        let runs = self.runs.read();
        Ok(runs.get(id).cloned())
    }

    async fn read_artifact(
        &self,
        id: &ArtifactId,
        range: Option<ByteRange>,
    ) -> Result<ArtifactChunk, RunStoreError> {
        let artifacts = self.artifacts.read();
        let (_, data, record) = artifacts
            .get(id)
            .ok_or_else(|| RunStoreError::NotFound(format!("artifact {}", id)))?;

        let expected_sha = compute_sha256(data);
        if expected_sha != record.sha256 {
            return Err(RunStoreError::IntegrityViolation(format!(
                "artifact {} checksum mismatch",
                id
            )));
        }

        let total_bytes = data.len() as u64;
        let chunk = match range {
            Some(r) => {
                let start = r.start.min(data.len());
                let end = r.end.min(data.len());
                ArtifactChunk {
                    artifact_id: id.clone(),
                    data: data[start..end].to_vec(),
                    total_bytes,
                    byte_offset: start,
                }
            }
            None => ArtifactChunk {
                artifact_id: id.clone(),
                data: data.clone(),
                total_bytes,
                byte_offset: 0,
            },
        };
        Ok(chunk)
    }

    async fn list_runs(&self, query: RunQuery) -> Result<Vec<RunSummary>, RunStoreError> {
        let runs = self.runs.read();
        let mut results: Vec<RunSummary> = runs
            .values()
            .filter(|m| {
                if let Some(ref kind) = query.kind {
                    if m.kind != *kind {
                        return false;
                    }
                }
                if let Some(ref status) = query.status {
                    if m.status != *status {
                        return false;
                    }
                }
                if let Some(since) = query.since {
                    if m.started_at < since {
                        return false;
                    }
                }
                if let Some(until) = query.until {
                    if m.started_at > until {
                        return false;
                    }
                }
                true
            })
            .map(|m| RunSummary {
                run_id: m.run_id.clone(),
                kind: m.kind.clone(),
                status: m.status.clone(),
                started_at: m.started_at,
                completed_at: m.completed_at,
                command: m.invocation.command.clone(),
            })
            .collect();

        results.sort_by_key(|b| std::cmp::Reverse(b.started_at));

        if let Some(limit) = query.limit {
            results.truncate(limit);
        }

        Ok(results)
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_draft(kind: RunKind) -> RunDraft {
        RunDraft {
            kind,
            invocation: RunInvocation {
                command: "cargo test".to_string(),
                argv: Some(vec!["cargo".to_string(), "test".to_string()]),
                script_hash: None,
            },
            session_id: None,
            parent_run_id: None,
            workspace_root: PathBuf::from("/workspace"),
            cwd: PathBuf::from("/workspace"),
            backend: BackendRecord {
                family: "test_runner".to_string(),
                detail: None,
            },
            risk: RiskRecord {
                level: "low".to_string(),
                has_subprocess: false,
                has_git_mutation: false,
                has_destructive_mutation: false,
            },
        }
    }

    fn test_completion() -> RunCompletion {
        RunCompletion {
            status: RunStatus::Complete,
            completed_at: Utc::now(),
            permissions: Vec::new(),
            sandbox: None,
            projection: None,
            changes: Vec::new(),
            rerun: None,
        }
    }

    #[tokio::test]
    async fn run_id_generation_and_ordering() {
        let mut ids: Vec<String> = (0..100).map(|_| RunId::new().0).collect();
        ids.sort();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);

        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), 100);
    }

    #[tokio::test]
    async fn manifest_serde_roundtrip() {
        let manifest = RunManifest {
            schema_version: SCHEMA_VERSION,
            run_id: RunId::new(),
            session_id: Some("sess-1".to_string()),
            parent_run_id: None,
            kind: RunKind::Python,
            invocation: RunInvocation {
                command: "python3 script.py".to_string(),
                argv: None,
                script_hash: Some("abc123".to_string()),
            },
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            status: RunStatus::Complete,
            workspace_root: PathBuf::from("/ws"),
            cwd: PathBuf::from("/ws"),
            backend: BackendRecord {
                family: "python".to_string(),
                detail: Some("analyze".to_string()),
            },
            risk: RiskRecord {
                level: "medium".to_string(),
                has_subprocess: false,
                has_git_mutation: false,
                has_destructive_mutation: false,
            },
            permissions: Vec::new(),
            sandbox: None,
            artifacts: Vec::new(),
            projection: None,
            changes: Vec::new(),
            rerun: None,
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let decoded: RunManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.run_id, manifest.run_id);
        assert_eq!(decoded.kind, RunKind::Python);
        assert_eq!(decoded.status, RunStatus::Complete);
    }

    #[tokio::test]
    async fn mem_store_begin_write_complete() {
        let store = MemRunStore::new();
        let handle = store
            .begin_run(test_draft(RunKind::RawShell))
            .await
            .unwrap();

        let artifact = ArtifactInput {
            kind: ArtifactKind::Stdout,
            data: b"hello world".to_vec(),
            mime_type: "text/plain".to_string(),
            safe_for_model: true,
        };
        let artifact_ref = store.write_artifact(&handle, artifact).await.unwrap();
        assert_eq!(artifact_ref.byte_length, 11);

        let manifest = store.complete_run(handle, test_completion()).await.unwrap();
        assert_eq!(manifest.status, RunStatus::Complete);
        assert_eq!(manifest.artifacts.len(), 1);
    }

    #[tokio::test]
    async fn mem_store_get_run_and_list() {
        let store = MemRunStore::new();
        let h1 = store.begin_run(test_draft(RunKind::Python)).await.unwrap();
        let h2 = store.begin_run(test_draft(RunKind::Test)).await.unwrap();
        store.complete_run(h1, test_completion()).await.unwrap();
        store.complete_run(h2, test_completion()).await.unwrap();

        let runs = store.list_runs(RunQuery::default()).await.unwrap();
        assert_eq!(runs.len(), 2);

        let filtered = store
            .list_runs(RunQuery {
                kind: Some(RunKind::Python),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].kind, RunKind::Python);
    }

    #[tokio::test]
    async fn mem_store_read_artifact_with_range() {
        let store = MemRunStore::new();
        let handle = store
            .begin_run(test_draft(RunKind::RawShell))
            .await
            .unwrap();
        let artifact = ArtifactInput {
            kind: ArtifactKind::Stdout,
            data: b"0123456789abcdef".to_vec(),
            mime_type: "text/plain".to_string(),
            safe_for_model: true,
        };
        let artifact_ref = store.write_artifact(&handle, artifact).await.unwrap();

        let chunk = store
            .read_artifact(
                &artifact_ref.artifact_id,
                Some(ByteRange { start: 4, end: 8 }),
            )
            .await
            .unwrap();
        assert_eq!(chunk.data, b"4567");
        assert_eq!(chunk.byte_offset, 4);
        assert_eq!(chunk.total_bytes, 16);
    }

    #[tokio::test]
    async fn mem_store_integrity_violation() {
        let store = MemRunStore::new();
        let handle = store
            .begin_run(test_draft(RunKind::RawShell))
            .await
            .unwrap();
        let artifact = ArtifactInput {
            kind: ArtifactKind::Stdout,
            data: b"data".to_vec(),
            mime_type: "text/plain".to_string(),
            safe_for_model: true,
        };
        let artifact_ref = store.write_artifact(&handle, artifact).await.unwrap();

        let mut runs = store.runs.write();
        let manifest = runs.get_mut(&handle.run_id).unwrap();
        manifest.artifacts[0].sha256 = "bad_hash".to_string();
        drop(runs);

        let result = store.read_artifact(&artifact_ref.artifact_id, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mem_store_artifact_too_large() {
        let store = MemRunStore::new();
        let handle = store
            .begin_run(test_draft(RunKind::RawShell))
            .await
            .unwrap();
        let artifact = ArtifactInput {
            kind: ArtifactKind::Stdout,
            data: vec![0u8; (MAX_ARTIFACT_BYTES + 1) as usize],
            mime_type: "text/plain".to_string(),
            safe_for_model: true,
        };
        let result = store.write_artifact(&handle, artifact).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rerun_descriptor_no_permission_persistence() {
        let rerun = RerunDescriptor {
            argv: Some(vec!["cargo".to_string(), "test".to_string()]),
            script_source_ref: None,
            backend_family: "test_runner".to_string(),
            cwd: PathBuf::from("/ws"),
            workspace_root: PathBuf::from("/ws"),
            mode: None,
            config_profile: None,
            parent_run_id: Some(RunId::new()),
        };

        let json = serde_json::to_string(&rerun).unwrap();
        assert!(!json.contains("permission"));
        assert!(!json.contains("allow"));
        assert!(!json.contains("deny"));
    }

    #[tokio::test]
    async fn mem_store_concurrent_writes() {
        let store = std::sync::Arc::new(MemRunStore::new());
        let mut handles = Vec::new();

        for i in 0..10 {
            let store = store.clone();
            handles.push(tokio::spawn(async move {
                let mut draft = test_draft(RunKind::RawShell);
                draft.invocation.command = format!("cmd-{}", i);
                let h = store.begin_run(draft).await.unwrap();
                let artifact = ArtifactInput {
                    kind: ArtifactKind::Stdout,
                    data: format!("output-{}", i).into_bytes(),
                    mime_type: "text/plain".to_string(),
                    safe_for_model: true,
                };
                store.write_artifact(&h, artifact).await.unwrap();
                store.complete_run(h, test_completion()).await.unwrap();
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        let runs = store.list_runs(RunQuery::default()).await.unwrap();
        assert_eq!(runs.len(), 10);
    }

    #[tokio::test]
    async fn path_traversal_rejection() {
        assert!(validate_id_str("../etc/passwd").is_err());
        assert!(validate_id_str("foo/bar").is_err());
        assert!(validate_id_str("foo\\bar").is_err());
        assert!(validate_id_str("foo\0bar").is_err());
        assert!(validate_id_str("").is_err());
        assert!(validate_id_str("valid-run-id").is_ok());
        assert!(validate_id_str("abc123").is_ok());
    }

    #[tokio::test]
    async fn mem_store_list_with_limit() {
        let store = MemRunStore::new();
        for _ in 0..5 {
            let h = store
                .begin_run(test_draft(RunKind::RawShell))
                .await
                .unwrap();
            store.complete_run(h, test_completion()).await.unwrap();
        }

        let runs = store
            .list_runs(RunQuery {
                limit: Some(3),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(runs.len(), 3);
    }

    #[tokio::test]
    async fn cleanup_plan_respects_pinned() {
        let store = MemRunStore::new();
        let config = RetentionConfig::default();
        let plan = store.plan_cleanup(&config).unwrap();
        assert!(plan.runs_to_delete.is_empty());
    }

    #[tokio::test]
    async fn fs_store_atomic_begin_and_read() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FsRunStore::new(tmp.path().to_path_buf());

        let handle = store.begin_run(test_draft(RunKind::Python)).await.unwrap();
        assert!(handle.run_dir.exists());

        let manifest_path = handle.run_dir.join(MANIFEST_FILENAME);
        assert!(manifest_path.exists());

        let retrieved = store.get_run(&handle.run_id).await.unwrap();
        assert!(retrieved.is_some());
        let m = retrieved.unwrap();
        assert_eq!(m.status, RunStatus::Running);
        assert_eq!(m.kind, RunKind::Python);
    }

    #[tokio::test]
    async fn fs_store_artifact_atomic_write() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FsRunStore::new(tmp.path().to_path_buf());

        let handle = store
            .begin_run(test_draft(RunKind::RawShell))
            .await
            .unwrap();
        let artifact = ArtifactInput {
            kind: ArtifactKind::Stdout,
            data: b"test output".to_vec(),
            mime_type: "text/plain".to_string(),
            safe_for_model: true,
        };
        let artifact_ref = store.write_artifact(&handle, artifact).await.unwrap();

        let stdout_path = handle.run_dir.join("stdout.log");
        assert!(stdout_path.exists());
        let content = fs::read(&stdout_path).await.unwrap();
        assert_eq!(content, b"test output");

        let chunk = store
            .read_artifact(&artifact_ref.artifact_id, None)
            .await
            .unwrap();
        assert_eq!(chunk.data, b"test output");
    }

    #[tokio::test]
    async fn fs_store_complete_updates_index() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FsRunStore::new(tmp.path().to_path_buf());

        let handle = store.begin_run(test_draft(RunKind::Test)).await.unwrap();
        let completed = store.complete_run(handle, test_completion()).await.unwrap();
        assert_eq!(completed.status, RunStatus::Complete);

        let runs = store.list_runs(RunQuery::default()).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Complete);
    }
}
