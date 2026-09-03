//! Deterministic, privacy-bounded workflow habit observation.
//!
//! Habit candidates deliberately live beside, but independently from, text
//! memories.  They contain only an allowlisted structural workflow and the
//! minimum bounded provenance needed to establish repeated occurrences.

use super::project_namespace;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub const WORKFLOW_VERSION: u16 = 1;
pub const DEFAULT_READY_OCCURRENCES: u32 = 3;
pub const MIN_READY_SESSIONS: u32 = 2;
pub const MAX_WORKFLOW_ACTIONS: usize = 32;
pub const MAX_VARIANT_BYTES: usize = 64;
pub const MAX_ID_BYTES: usize = 128;
pub const MAX_RETAINED_SESSIONS: usize = 64;
pub const MAX_RETAINED_OCCURRENCES: usize = 128;
pub const MAX_CANDIDATES: usize = 128;
pub const MAX_FILE_BYTES: u64 = 256 * 1024;

const HABIT_FILE_VERSION: u16 = 1;
const HABIT_FILE_DOMAIN: &[u8] = b"codegg-habit-v1\0";
const OCCURRENCE_DOMAIN: &[u8] = b"codegg-habit-occurrence-v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowActionKind {
    FileRead,
    Search,
    Edit,
    Patch,
    Test,
    Lint,
    Build,
    Format,
    GitRead,
    GitWrite,
    LspRead,
    LspRefactor,
    SkillActivate,
    Delegate,
    DeterministicValidate,
    ShellExec,
}

impl WorkflowActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FileRead => "read",
            Self::Search => "search",
            Self::Edit => "edit",
            Self::Patch => "patch",
            Self::Test => "test",
            Self::Lint => "lint",
            Self::Build => "build",
            Self::Format => "format",
            Self::GitRead => "git_read",
            Self::GitWrite => "git_write",
            Self::LspRead => "lsp_read",
            Self::LspRefactor => "lsp_refactor",
            Self::SkillActivate => "skill_activate",
            Self::Delegate => "delegate",
            Self::DeterministicValidate => "deterministic_validate",
            Self::ShellExec => "shell_exec",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEffectClass {
    ReadOnly,
    ReadValidate,
    SafeRepeat,
    Mutating,
    ProcessExec,
}

impl WorkflowEffectClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::ReadValidate => "read_validate",
            Self::SafeRepeat => "safe_repeat",
            Self::Mutating => "mutating",
            Self::ProcessExec => "process_exec",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowAction {
    pub kind: WorkflowActionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    pub effect: WorkflowEffectClass,
}

impl WorkflowAction {
    pub fn new(
        kind: WorkflowActionKind,
        variant: Option<String>,
        effect: WorkflowEffectClass,
    ) -> Self {
        Self {
            kind,
            variant,
            effect,
        }
    }

    pub fn label(&self) -> String {
        match self.variant.as_deref() {
            Some(variant) => format!("{}:{}", self.kind.as_str(), variant),
            None => self.kind.as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowOutcome {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowObservation {
    pub project_namespace: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub root_or_run_id: Option<String>,
    pub action: WorkflowAction,
    pub outcome: WorkflowOutcome,
    pub occurred_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowOccurrence {
    pub project_namespace: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub root_or_run_id: Option<String>,
    pub actions: Vec<WorkflowAction>,
    pub outcome: WorkflowOutcome,
    pub occurred_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedWorkflow {
    pub version: u16,
    pub project_namespace: String,
    pub fingerprint: String,
    pub actions: Vec<WorkflowAction>,
}

pub fn normalize_workflow(
    project_namespace: &str,
    actions: &[WorkflowAction],
) -> Option<NormalizedWorkflow> {
    if !is_safe_namespace(project_namespace) {
        return None;
    }

    let mut normalized = Vec::with_capacity(actions.len().min(MAX_WORKFLOW_ACTIONS));
    for action in actions {
        if let Some(previous) = normalized.last() {
            if previous == action {
                continue;
            }
        }
        if action.variant.as_deref().is_some_and(|variant| {
            variant.is_empty()
                || variant.len() > MAX_VARIANT_BYTES
                || variant.chars().any(|c| c.is_control() || c == '|')
        }) {
            continue;
        }
        if normalized.len() == MAX_WORKFLOW_ACTIONS {
            break;
        }
        normalized.push(action.clone());
    }

    let distinct = normalized.iter().collect::<HashSet<_>>().len();
    if distinct < 2 {
        return None;
    }

    let mut input = Vec::new();
    input.extend_from_slice(HABIT_FILE_DOMAIN);
    input.extend_from_slice(project_namespace.as_bytes());
    input.push(b'\n');
    for action in &normalized {
        input.extend_from_slice(action.kind.as_str().as_bytes());
        input.push(b'|');
        if let Some(variant) = &action.variant {
            input.extend_from_slice(variant.as_bytes());
        }
        input.push(b'|');
        input.extend_from_slice(action.effect.as_str().as_bytes());
        input.push(b'\n');
    }
    let mut hasher = Sha256::new();
    hasher.update(input);
    let fingerprint = format!("v{WORKFLOW_VERSION}-{:x}", hasher.finalize());

    Some(NormalizedWorkflow {
        version: WORKFLOW_VERSION,
        project_namespace: project_namespace.to_string(),
        fingerprint,
        actions: normalized,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HabitId(String);

impl HabitId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value.is_empty() || value.len() > MAX_ID_BYTES || value.chars().any(char::is_control) {
            None
        } else {
            Some(Self(value.to_string()))
        }
    }
}

impl Default for HabitId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HabitCandidateStatus {
    Observing,
    Ready,
    Dismissed,
    Promoted,
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedSkillRef {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HabitCandidate {
    pub id: HabitId,
    pub project_namespace: String,
    pub workflow_version: u16,
    pub workflow_fingerprint: String,
    pub actions: Vec<WorkflowAction>,
    pub successful_occurrences: u32,
    pub distinct_sessions: u32,
    pub recent_session_ids: Vec<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    pub status: HabitCandidateStatus,
    /// Monotonic revision used by promotion requests to detect changes made
    /// after the user reviewed the candidate.
    #[serde(default = "default_revision")]
    pub revision: u64,
    #[serde(default)]
    pub related_memory_ids: Vec<String>,
    #[serde(default)]
    pub promoted_skill: Option<PublishedSkillRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    occurrence_ids: Vec<String>,
    #[serde(default)]
    pub superseded_by: Option<HabitId>,
}

impl HabitCandidate {
    pub fn summary(&self) -> String {
        self.actions
            .iter()
            .map(WorkflowAction::label)
            .collect::<Vec<_>>()
            .join(" -> ")
    }

    pub fn is_ready(&self) -> bool {
        self.status == HabitCandidateStatus::Ready
    }

    fn validate(&self, namespace: &str) -> io::Result<()> {
        if self.project_namespace != namespace
            || self.workflow_version != WORKFLOW_VERSION
            || self.workflow_fingerprint.len() > MAX_ID_BYTES
            || self.actions.is_empty()
            || self.actions.len() > MAX_WORKFLOW_ACTIONS
            || self.recent_session_ids.len() > MAX_RETAINED_SESSIONS
            || self.occurrence_ids.len() > MAX_RETAINED_OCCURRENCES
        {
            return Err(invalid_data("habit candidate exceeds bounds"));
        }
        if self.id.0.len() > MAX_ID_BYTES
            || self
                .recent_session_ids
                .iter()
                .any(|id| id.is_empty() || id.len() > MAX_ID_BYTES)
            || self.occurrence_ids.iter().any(|id| id.len() > MAX_ID_BYTES)
            || self.actions.iter().any(|action| {
                action
                    .variant
                    .as_deref()
                    .is_some_and(|variant| variant.is_empty() || variant.len() > MAX_VARIANT_BYTES)
            })
        {
            return Err(invalid_data("habit candidate contains an unsafe field"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct HabitFile {
    version: u16,
    candidates: Vec<HabitCandidate>,
}

pub struct HabitStore {
    root: PathBuf,
}

impl HabitStore {
    pub fn new() -> io::Result<Self> {
        let root = dirs::config_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no config dir"))?
            .join("codegg")
            .join("memory");
        Self::with_root(root)
    }

    pub fn with_root(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("habits").join("project"))?;
        Ok(Self { root })
    }

    pub fn project_path(&self, project_identity: &str) -> PathBuf {
        self.root.join("habits").join("project").join(format!(
            "{}.json",
            project_namespace(project_identity).trim_start_matches("project/")
        ))
    }

    pub fn load(&self, project_identity: &str) -> io::Result<Vec<HabitCandidate>> {
        let namespace = project_namespace(project_identity);
        self.load_namespace(&namespace)
    }

    pub fn reload(&self, project_identity: &str) -> io::Result<Vec<HabitCandidate>> {
        self.load(project_identity)
    }

    pub fn list(
        &self,
        project_identity: &str,
        status: Option<HabitCandidateStatus>,
        limit: usize,
    ) -> io::Result<Vec<HabitCandidate>> {
        let status = status.as_ref();
        Ok(self
            .load(project_identity)?
            .into_iter()
            .filter(|candidate| status.map_or(true, |wanted| &candidate.status == wanted))
            .take(limit.min(MAX_CANDIDATES))
            .collect())
    }

    pub fn observe(&self, occurrence: WorkflowOccurrence) -> io::Result<Option<HabitCandidate>> {
        if occurrence.outcome != WorkflowOutcome::Succeeded
            || occurrence.session_id.is_empty()
            || occurrence.session_id.len() > MAX_ID_BYTES
        {
            return Ok(None);
        }
        let Some(workflow) = normalize_workflow(&occurrence.project_namespace, &occurrence.actions)
        else {
            return Ok(None);
        };
        let occurrence_id = occurrence_id(&occurrence, &workflow);
        let namespace = workflow.project_namespace.clone();
        let path = self.path_for_namespace(&namespace)?;
        let lock_path = path.with_extension("json.lock");
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .open(lock_path)?;
        flock_lock(&lock)?;
        let result = (|| {
            let mut candidates = self.load_namespace_unlocked(&namespace)?;
            let now = occurrence.occurred_at;
            let candidate = if let Some(candidate) = candidates.iter_mut().find(|candidate| {
                candidate.workflow_fingerprint == workflow.fingerprint
                    && candidate.workflow_version == workflow.version
            }) {
                if matches!(
                    candidate.status,
                    HabitCandidateStatus::Dismissed
                        | HabitCandidateStatus::Promoted
                        | HabitCandidateStatus::Superseded
                ) {
                    return Ok(None);
                }
                if occurrence_id
                    .as_ref()
                    .is_some_and(|id| candidate.occurrence_ids.contains(id))
                {
                    return Ok(Some(candidate.clone()));
                }
                candidate.successful_occurrences =
                    candidate.successful_occurrences.saturating_add(1);
                candidate.revision = candidate.revision.saturating_add(1);
                if !candidate
                    .recent_session_ids
                    .contains(&occurrence.session_id)
                    && candidate.recent_session_ids.len() < MAX_RETAINED_SESSIONS
                {
                    candidate
                        .recent_session_ids
                        .push(occurrence.session_id.clone());
                    candidate.distinct_sessions = candidate.distinct_sessions.saturating_add(1);
                }
                if let Some(id) = occurrence_id {
                    if candidate.occurrence_ids.len() == MAX_RETAINED_OCCURRENCES {
                        candidate.occurrence_ids.remove(0);
                    }
                    candidate.occurrence_ids.push(id);
                }
                candidate.last_seen = candidate.last_seen.max(now);
                if candidate.successful_occurrences >= DEFAULT_READY_OCCURRENCES
                    && candidate.distinct_sessions >= MIN_READY_SESSIONS
                {
                    candidate.status = HabitCandidateStatus::Ready;
                }
                candidate.clone()
            } else {
                let candidate = HabitCandidate {
                    id: HabitId::new(),
                    project_namespace: namespace.clone(),
                    workflow_version: workflow.version,
                    workflow_fingerprint: workflow.fingerprint,
                    actions: workflow.actions,
                    successful_occurrences: 1,
                    distinct_sessions: 1,
                    recent_session_ids: vec![occurrence.session_id],
                    first_seen: now,
                    last_seen: now,
                    status: HabitCandidateStatus::Observing,
                    revision: 1,
                    related_memory_ids: Vec::new(),
                    promoted_skill: None,
                    occurrence_ids: occurrence_id.into_iter().collect(),
                    superseded_by: None,
                };
                candidates.push(candidate.clone());
                candidate
            };
            if !prune_observing(&mut candidates) {
                return Err(invalid_data(
                    "habit candidate capacity contains no observing record",
                ));
            }
            self.save_namespace_unlocked(&namespace, &candidates)?;
            Ok(Some(candidate))
        })();
        let _ = flock_unlock(&lock);
        result
    }

    pub fn dismiss(&self, project_identity: &str, id: &HabitId) -> io::Result<bool> {
        self.transition(project_identity, id, |candidate| {
            if matches!(
                candidate.status,
                HabitCandidateStatus::Observing | HabitCandidateStatus::Ready
            ) {
                candidate.status = HabitCandidateStatus::Dismissed;
                candidate.revision = candidate.revision.saturating_add(1);
                true
            } else {
                false
            }
        })
    }

    pub fn mark_promoted(
        &self,
        project_identity: &str,
        id: &HabitId,
        skill: PublishedSkillRef,
    ) -> io::Result<bool> {
        self.transition(project_identity, id, |candidate| {
            if candidate.status == HabitCandidateStatus::Ready {
                candidate.status = HabitCandidateStatus::Promoted;
                candidate.revision = candidate.revision.saturating_add(1);
                candidate.promoted_skill = Some(skill.clone());
                true
            } else {
                false
            }
        })
    }

    pub fn mark_superseded(
        &self,
        project_identity: &str,
        id: &HabitId,
        replacement: Option<HabitId>,
    ) -> io::Result<bool> {
        self.transition(project_identity, id, |candidate| {
            if candidate.status != HabitCandidateStatus::Promoted {
                candidate.status = HabitCandidateStatus::Superseded;
                candidate.revision = candidate.revision.saturating_add(1);
                candidate.superseded_by = replacement.clone();
                true
            } else {
                false
            }
        })
    }

    fn transition<F>(&self, project_identity: &str, id: &HabitId, transition: F) -> io::Result<bool>
    where
        F: FnOnce(&mut HabitCandidate) -> bool,
    {
        let namespace = project_namespace(project_identity);
        let path = self.path_for_namespace(&namespace)?;
        let lock_path = path.with_extension("json.lock");
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .open(lock_path)?;
        flock_lock(&lock)?;
        let result = (|| {
            let mut candidates = self.load_namespace_unlocked(&namespace)?;
            let Some(candidate) = candidates.iter_mut().find(|candidate| &candidate.id == id)
            else {
                return Ok(false);
            };
            if !transition(candidate) {
                return Ok(false);
            }
            self.save_namespace_unlocked(&namespace, &candidates)?;
            Ok(true)
        })();
        let _ = flock_unlock(&lock);
        result
    }

    fn path_for_namespace(&self, namespace: &str) -> io::Result<PathBuf> {
        if !is_safe_namespace(namespace) {
            return Err(invalid_data("unsafe habit namespace"));
        }
        let Some(name) = namespace.strip_prefix("project/") else {
            return Err(invalid_data("habit namespace is not project-scoped"));
        };
        if name.len() != 64 || !name.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid_data("invalid project namespace"));
        }
        Ok(self
            .root
            .join("habits")
            .join("project")
            .join(format!("{name}.json")))
    }

    fn load_namespace(&self, namespace: &str) -> io::Result<Vec<HabitCandidate>> {
        let path = self.path_for_namespace(namespace)?;
        self.load_path(&path, namespace)
    }

    fn load_namespace_unlocked(&self, namespace: &str) -> io::Result<Vec<HabitCandidate>> {
        let path = self.path_for_namespace(namespace)?;
        self.load_path(&path, namespace)
    }

    fn load_path(&self, path: &Path, namespace: &str) -> io::Result<Vec<HabitCandidate>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let metadata = fs::metadata(path)?;
        if metadata.len() > MAX_FILE_BYTES {
            return Err(invalid_data("habit file exceeds size bound"));
        }
        let mut content = Vec::with_capacity(metadata.len() as usize);
        File::open(path)?
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut content)?;
        if content.len() as u64 > MAX_FILE_BYTES {
            return Err(invalid_data("habit file exceeds size bound"));
        }
        let file: HabitFile = serde_json::from_slice(&content)
            .map_err(|error| invalid_data(&format!("malformed habit file: {error}")))?;
        if file.version != HABIT_FILE_VERSION || file.candidates.len() > MAX_CANDIDATES {
            return Err(invalid_data("unsupported or oversized habit file"));
        }
        for candidate in &file.candidates {
            candidate.validate(namespace)?;
        }
        Ok(file.candidates)
    }

    fn save_namespace_unlocked(
        &self,
        namespace: &str,
        candidates: &[HabitCandidate],
    ) -> io::Result<()> {
        if candidates.len() > MAX_CANDIDATES {
            return Err(invalid_data("too many habit candidates"));
        }
        for candidate in candidates {
            candidate.validate(namespace)?;
        }
        let path = self.path_for_namespace(namespace)?;
        let temp_path = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&HabitFile {
            version: HABIT_FILE_VERSION,
            candidates: candidates.to_vec(),
        })
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(invalid_data("serialized habit file exceeds size bound"));
        }
        let mut file = File::create(&temp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, &path)?;
        Ok(())
    }
}

fn occurrence_id(occurrence: &WorkflowOccurrence, workflow: &NormalizedWorkflow) -> Option<String> {
    let turn = occurrence.turn_id.as_deref()?;
    let mut hasher = Sha256::new();
    hasher.update(OCCURRENCE_DOMAIN);
    hasher.update(workflow.project_namespace.as_bytes());
    hasher.update(b"\0");
    hasher.update(occurrence.session_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(turn.as_bytes());
    if let Some(run) = &occurrence.root_or_run_id {
        hasher.update(b"\0");
        hasher.update(run.as_bytes());
    }
    Some(format!("{:x}", hasher.finalize()))
}

fn prune_observing(candidates: &mut Vec<HabitCandidate>) -> bool {
    while candidates.len() > MAX_CANDIDATES {
        let Some(index) = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.status == HabitCandidateStatus::Observing)
            .min_by_key(|(_, candidate)| candidate.last_seen)
            .map(|(index, _)| index)
        else {
            return false;
        };
        candidates.remove(index);
    }
    true
}

fn is_safe_namespace(namespace: &str) -> bool {
    let mut parts = namespace.split('/');
    matches!(parts.next(), Some("project"))
        && parts
            .next()
            .is_some_and(|part| !part.is_empty() && !part.contains(['/', '\\', '.']))
        && parts.next().is_none()
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn default_revision() -> u64 {
    1
}

#[cfg(unix)]
fn flock_lock(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    #[allow(unsafe_code)]
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn flock_unlock(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    #[allow(unsafe_code)]
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn flock_lock(_file: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn flock_unlock(_file: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn action(kind: WorkflowActionKind) -> WorkflowAction {
        WorkflowAction::new(kind, None, WorkflowEffectClass::ReadOnly)
    }

    fn occurrence(
        namespace: &str,
        session: &str,
        turn: &str,
        actions: Vec<WorkflowAction>,
    ) -> WorkflowOccurrence {
        WorkflowOccurrence {
            project_namespace: namespace.to_string(),
            session_id: session.to_string(),
            turn_id: Some(turn.to_string()),
            root_or_run_id: None,
            actions,
            outcome: WorkflowOutcome::Succeeded,
            occurred_at: 100,
        }
    }

    #[test]
    fn normalization_is_bounded_deterministic_and_project_scoped() {
        let first = normalize_workflow(
            "project/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &[
                action(WorkflowActionKind::FileRead),
                action(WorkflowActionKind::FileRead),
                action(WorkflowActionKind::Edit),
            ],
        )
        .unwrap();
        let same = normalize_workflow(
            &first.project_namespace,
            &[
                action(WorkflowActionKind::FileRead),
                action(WorkflowActionKind::Edit),
            ],
        )
        .unwrap();
        let other = normalize_workflow(
            "project/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            &first.actions,
        )
        .unwrap();
        assert_eq!(first.fingerprint, same.fingerprint);
        assert_ne!(first.fingerprint, other.fingerprint);
        assert_eq!(first.actions.len(), 2);
    }

    #[test]
    fn one_session_cannot_be_ready_and_duplicate_turn_is_idempotent() {
        let dir = tempdir().unwrap();
        let store = HabitStore::with_root(dir.path()).unwrap();
        let identity = "/workspace/project";
        let namespace = project_namespace(identity);
        let workflow = vec![
            action(WorkflowActionKind::FileRead),
            action(WorkflowActionKind::Edit),
        ];
        assert_eq!(
            store
                .observe(occurrence(&namespace, "s1", "t1", workflow.clone()))
                .unwrap()
                .unwrap()
                .successful_occurrences,
            1
        );
        assert_eq!(
            store
                .observe(occurrence(&namespace, "s1", "t1", workflow.clone()))
                .unwrap()
                .unwrap()
                .successful_occurrences,
            1
        );
        assert_eq!(
            store
                .observe(occurrence(&namespace, "s1", "t2", workflow.clone()))
                .unwrap()
                .unwrap()
                .successful_occurrences,
            2
        );
        let candidate = store
            .observe(occurrence(&namespace, "s2", "t3", workflow))
            .unwrap()
            .unwrap();
        assert_eq!(candidate.successful_occurrences, 3);
        assert_eq!(candidate.distinct_sessions, 2);
        assert!(candidate.is_ready());
    }

    #[test]
    fn failed_and_dismissed_occurrences_do_not_build_confidence() {
        let dir = tempdir().unwrap();
        let store = HabitStore::with_root(dir.path()).unwrap();
        let identity = "/workspace/project";
        let namespace = project_namespace(identity);
        let mut failed = occurrence(
            &namespace,
            "s1",
            "t1",
            vec![
                action(WorkflowActionKind::FileRead),
                action(WorkflowActionKind::Edit),
            ],
        );
        failed.outcome = WorkflowOutcome::Failed;
        assert!(store.observe(failed).unwrap().is_none());
        let candidate = store
            .observe(occurrence(
                &namespace,
                "s1",
                "t2",
                vec![
                    action(WorkflowActionKind::FileRead),
                    action(WorkflowActionKind::Edit),
                ],
            ))
            .unwrap()
            .unwrap();
        assert!(!candidate.is_ready());
        assert!(store.dismiss(identity, &candidate.id).unwrap());
        assert!(store
            .observe(occurrence(
                &namespace,
                "s2",
                "t3",
                vec![
                    action(WorkflowActionKind::FileRead),
                    action(WorkflowActionKind::Edit)
                ]
            ))
            .unwrap()
            .is_none());
        assert_eq!(
            store.list(identity, None, 10).unwrap()[0].status,
            HabitCandidateStatus::Dismissed
        );
    }

    #[test]
    fn malformed_and_oversized_files_fail_bounded() {
        let dir = tempdir().unwrap();
        let store = HabitStore::with_root(dir.path()).unwrap();
        let path = store.project_path("/workspace/project");
        fs::write(&path, b"not-json").unwrap();
        assert_eq!(
            store.load("/workspace/project").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::write(&path, vec![b'x'; (MAX_FILE_BYTES + 1) as usize]).unwrap();
        assert_eq!(
            store.load("/workspace/project").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn concurrent_observers_leave_a_valid_complete_file() {
        let dir = tempdir().unwrap();
        let identity = "/workspace/concurrent";
        let namespace = project_namespace(identity);
        let workflow = vec![
            action(WorkflowActionKind::FileRead),
            action(WorkflowActionKind::Edit),
        ];
        let mut threads = Vec::new();
        for index in 0..8 {
            let root = dir.path().to_path_buf();
            let namespace = namespace.clone();
            let workflow = workflow.clone();
            threads.push(std::thread::spawn(move || {
                let store = HabitStore::with_root(root).unwrap();
                store
                    .observe(occurrence(
                        &namespace,
                        &format!("session-{index}"),
                        &format!("turn-{index}"),
                        workflow,
                    ))
                    .unwrap();
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        let store = HabitStore::with_root(dir.path()).unwrap();
        let candidates = store.load(identity).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].successful_occurrences, 8);
        let bytes = fs::read(store.project_path(identity)).unwrap();
        assert!(serde_json::from_slice::<HabitFile>(&bytes).is_ok());
    }
}
