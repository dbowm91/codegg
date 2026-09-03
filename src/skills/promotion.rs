//! User-authorized, pre-publication skill promotion state.
//!
//! This module owns the short-lived initiation capability and the durable
//! proposal record. It deliberately has no publisher and never writes a
//! skill root.

use super::parser::{validate_portable_document, ValidatedSkillDocument};
use super::{AssetDiscoveryConfig, AssetRegistry, Diagnostic, Severity, SourceKind};
use codegg_core::memory::habit::{HabitCandidate, HabitCandidateStatus, HabitId, WorkflowAction};
use codegg_core::memory::project_namespace;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub const MAX_REQUESTS: usize = 64;
pub const MAX_PROPOSALS: usize = 64;
pub const MAX_NAME_BYTES: usize = 128;
pub const MAX_DESCRIPTION_BYTES: usize = 2048;
pub const MAX_MARKDOWN_BYTES: u64 = 256 * 1024;
pub const MAX_DIAGNOSTICS: usize = 32;
pub const MAX_DIAGNOSTIC_BYTES: usize = 512;
pub const MAX_MEMORY_SUMMARY_BYTES: usize = 512;
pub const MAX_RELATED_MEMORIES: usize = 16;
pub const REQUEST_TTL_MS: i64 = 15 * 60 * 1000;
const FILE_VERSION: u16 = 1;
/// Upper bound for one serialized promotion file: 64 proposals at the 256 KiB
/// skill bound each, plus request/provenance overhead.
const MAX_PROMOTION_FILE_BYTES: u64 = (MAX_PROPOSALS as u64) * MAX_MARKDOWN_BYTES + 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PromotionRequestId(String);

impl PromotionRequestId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value.is_empty() || value.len() > MAX_NAME_BYTES || value.chars().any(char::is_control) {
            None
        } else {
            Some(Self(value.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for PromotionRequestId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SkillProposalId(String);

impl SkillProposalId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value.is_empty() || value.len() > MAX_NAME_BYTES || value.chars().any(char::is_control) {
            None
        } else {
            Some(Self(value.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SkillProposalId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTargetScope {
    Project,
    Global,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundedMemoryRef {
    pub id: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HabitPromotionContext {
    pub habit_id: HabitId,
    pub workflow_fingerprint: String,
    pub action_skeleton: Vec<WorkflowAction>,
    pub successful_occurrences: u32,
    pub distinct_sessions: u32,
    pub related_memories: Vec<BoundedMemoryRef>,
    pub existing_skill_names: Vec<String>,
    pub target_scope_hint: SkillTargetScope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionDraftRequest {
    pub id: PromotionRequestId,
    pub session_id: String,
    pub project_namespace: String,
    pub habit_id: HabitId,
    pub habit_fingerprint: String,
    pub candidate_revision: u64,
    pub context: HabitPromotionContext,
    pub created_at: i64,
    pub expires_at: i64,
    pub consumed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillProposalStatus {
    Draft,
    Validated,
    Rejected,
    Published,
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundedDiagnostic {
    pub severity: Severity,
    pub reason: String,
    pub location: Option<String>,
}

impl BoundedDiagnostic {
    fn from_diagnostic(diagnostic: &Diagnostic) -> Self {
        Self {
            severity: diagnostic.severity,
            reason: diagnostic
                .reason
                .chars()
                .take(MAX_DIAGNOSTIC_BYTES)
                .collect(),
            location: diagnostic
                .location
                .as_deref()
                .map(|location| location.chars().take(MAX_DIAGNOSTIC_BYTES).collect()),
        }
    }

    fn error(reason: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            reason: reason.into().chars().take(MAX_DIAGNOSTIC_BYTES).collect(),
            location: Some("skill proposal".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillProposal {
    pub id: SkillProposalId,
    pub promotion_request_id: PromotionRequestId,
    pub habit_id: HabitId,
    pub habit_fingerprint: String,
    pub candidate_revision: u64,
    pub project_namespace: String,
    pub target_scope: SkillTargetScope,
    pub name: String,
    pub description: String,
    pub skill_markdown: String,
    pub content_digest: String,
    pub status: SkillProposalStatus,
    pub diagnostics: Vec<BoundedDiagnostic>,
    pub created_at: i64,
    pub updated_at: i64,
    pub revision: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct PromotionFile {
    version: u16,
    requests: Vec<PromotionDraftRequest>,
    proposals: Vec<SkillProposal>,
}

pub struct SkillPromotionStore {
    root: PathBuf,
    habit_root: PathBuf,
}

impl SkillPromotionStore {
    pub fn new() -> io::Result<Self> {
        let root = dirs::config_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no config dir"))?
            .join("codegg")
            .join("memory")
            .join("skill-promotions");
        Self::with_roots(
            root,
            dirs::config_dir()
                .map(|d| d.join("codegg").join("memory"))
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no config dir"))?,
        )
    }

    pub fn with_root(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        // Production layout nests promotions under the memory root
        // (`.../memory/skill-promotions`). Tests use sibling temp dirs.
        // Handle both so a caller passing either layout resolves habits.
        let habit_root = match (root.file_name().and_then(|n| n.to_str()), root.parent()) {
            (Some("skill-promotions"), Some(parent))
                if parent.file_name().and_then(|n| n.to_str()) == Some("memory") =>
            {
                parent.to_path_buf()
            }
            (_, Some(parent)) => parent.join("memory"),
            (_, None) => root.join("memory"),
        };
        Self::with_roots(root, habit_root)
    }

    pub fn with_roots(
        root: impl Into<PathBuf>,
        habit_root: impl Into<PathBuf>,
    ) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            habit_root: habit_root.into(),
        })
    }

    fn habit_store(&self) -> io::Result<codegg_core::memory::habit::HabitStore> {
        codegg_core::memory::habit::HabitStore::with_root(self.habit_root.clone())
    }

    pub fn begin_request(
        &self,
        project_identity: &str,
        session_id: &str,
        habit_id: &HabitId,
        existing_skill_names: Vec<String>,
        related_memories: Vec<BoundedMemoryRef>,
        now: i64,
    ) -> io::Result<PromotionDraftRequest> {
        if session_id.is_empty() || session_id.len() > MAX_NAME_BYTES {
            return Err(invalid_data("invalid session id"));
        }
        let habit_store = self.habit_store()?;
        let namespace = project_namespace(project_identity);
        let candidate = habit_store
            .load(project_identity)?
            .into_iter()
            .find(|candidate| &candidate.id == habit_id)
            .ok_or_else(|| invalid_data("habit candidate not found"))?;
        if candidate.status != HabitCandidateStatus::Ready {
            return Err(invalid_data("habit candidate is not ready for promotion"));
        }
        let context = bounded_context(&candidate, existing_skill_names, related_memories)?;
        let request = PromotionDraftRequest {
            id: PromotionRequestId::new(),
            session_id: session_id.to_string(),
            project_namespace: namespace.clone(),
            habit_id: candidate.id,
            habit_fingerprint: candidate.workflow_fingerprint,
            candidate_revision: candidate.revision,
            context,
            created_at: now,
            expires_at: now.saturating_add(REQUEST_TTL_MS),
            consumed: false,
        };
        self.with_locked_file(&namespace, |file| {
            prune_expired(file, now);
            if file.requests.len() >= MAX_REQUESTS {
                return Err(invalid_data("promotion request capacity reached"));
            }
            file.requests.push(request.clone());
            Ok(request.clone())
        })
    }

    pub fn list_proposals(
        &self,
        project_identity: &str,
        limit: usize,
    ) -> io::Result<Vec<SkillProposal>> {
        let namespace = project_namespace(project_identity);
        let file = self.load_namespace(&namespace)?;
        Ok(file
            .proposals
            .into_iter()
            .rev()
            .take(limit.min(MAX_PROPOSALS))
            .collect())
    }

    pub fn get_proposal(
        &self,
        project_identity: &str,
        id: &SkillProposalId,
    ) -> io::Result<Option<SkillProposal>> {
        let namespace = project_namespace(project_identity);
        Ok(self
            .load_namespace(&namespace)?
            .proposals
            .into_iter()
            .find(|proposal| &proposal.id == id))
    }

    pub fn reject_proposal(
        &self,
        project_identity: &str,
        id: &SkillProposalId,
        now: i64,
    ) -> io::Result<bool> {
        let namespace = project_namespace(project_identity);
        self.with_locked_file(&namespace, |file| {
            let Some(proposal) = file
                .proposals
                .iter_mut()
                .find(|proposal| &proposal.id == id)
            else {
                return Ok(false);
            };
            if proposal.status != SkillProposalStatus::Validated {
                return Ok(false);
            }
            proposal.status = SkillProposalStatus::Rejected;
            proposal.updated_at = now;
            proposal.revision = proposal.revision.saturating_add(1);
            Ok(true)
        })
    }

    pub fn append_diagnostics(
        &self,
        project_identity: &str,
        id: &SkillProposalId,
        diagnostics: Vec<BoundedDiagnostic>,
    ) -> io::Result<bool> {
        let namespace = project_namespace(project_identity);
        self.with_locked_file(&namespace, |file| {
            let Some(proposal) = file
                .proposals
                .iter_mut()
                .find(|proposal| &proposal.id == id)
            else {
                return Ok(false);
            };
            proposal.diagnostics.extend(diagnostics);
            proposal.diagnostics.truncate(MAX_DIAGNOSTICS);
            proposal.updated_at = chrono::Utc::now().timestamp_millis();
            proposal.revision = proposal.revision.saturating_add(1);
            Ok(true)
        })
    }

    /// Validate and persist one proposal, consuming the initiation record in
    /// the same locked transaction. No filesystem skill root is touched.
    pub fn submit(
        &self,
        project_identity: &str,
        session_id: &str,
        request_id: &PromotionRequestId,
        habit_id: &HabitId,
        supplied_name: &str,
        supplied_description: &str,
        skill_markdown: &str,
        now: i64,
    ) -> io::Result<SkillProposal> {
        if skill_markdown.len() as u64 > MAX_MARKDOWN_BYTES {
            return Err(invalid_data("proposal exceeds skill file size bound"));
        }
        let parsed = validate_portable_document(skill_markdown, &AssetDiscoveryConfig::default());
        let (name, description, mut diagnostics, status) = match parsed {
            Ok(document) => {
                let mut diagnostics = document
                    .diagnostics
                    .iter()
                    .map(BoundedDiagnostic::from_diagnostic)
                    .collect::<Vec<_>>();
                diagnostics.extend(generated_restriction_diagnostics(skill_markdown, &document));
                if normalize_input_name(supplied_name) != Some(document.normalized_name.clone())
                    || supplied_description.trim() != document.description.trim()
                {
                    diagnostics.push(BoundedDiagnostic::error(
                        "submitted name/description must match portable frontmatter",
                    ));
                }
                let status = if diagnostics.iter().any(|d| d.severity == Severity::Error) {
                    SkillProposalStatus::Rejected
                } else {
                    SkillProposalStatus::Validated
                };
                (document.name, document.description, diagnostics, status)
            }
            Err(diagnostic) => (
                bounded_text(supplied_name, MAX_NAME_BYTES),
                bounded_text(supplied_description, MAX_DESCRIPTION_BYTES),
                vec![BoundedDiagnostic::from_diagnostic(&diagnostic)],
                SkillProposalStatus::Rejected,
            ),
        };
        diagnostics.truncate(MAX_DIAGNOSTICS);
        let namespace = project_namespace(project_identity);
        self.with_locked_file(&namespace, |file| {
            prune_expired(file, now);
            let request = file
                .requests
                .iter_mut()
                .find(|request| request.id == *request_id)
                .ok_or_else(|| invalid_data("promotion request not found"))?;
            if request.consumed || request.expires_at < now {
                return Err(invalid_data(
                    "promotion request is expired or already consumed",
                ));
            }
            if request.session_id != session_id
                || request.project_namespace != namespace
                || request.habit_id != *habit_id
            {
                return Err(invalid_data("promotion request scope does not match"));
            }
            let candidates = self.habit_store()?.load(project_identity)?;
            let candidate = candidates
                .iter()
                .find(|candidate| candidate.id == *habit_id)
                .ok_or_else(|| invalid_data("habit candidate no longer exists"))?;
            if candidate.status != HabitCandidateStatus::Ready
                || candidate.revision != request.candidate_revision
                || candidate.workflow_fingerprint != request.habit_fingerprint
            {
                return Err(invalid_data(
                    "habit candidate changed; start a new promotion request",
                ));
            }
            if file.proposals.len() >= MAX_PROPOSALS {
                return Err(invalid_data("skill proposal capacity reached"));
            }
            let proposal = SkillProposal {
                id: SkillProposalId::new(),
                promotion_request_id: request.id.clone(),
                habit_id: request.habit_id.clone(),
                habit_fingerprint: request.habit_fingerprint.clone(),
                candidate_revision: request.candidate_revision,
                project_namespace: namespace.clone(),
                target_scope: request.context.target_scope_hint,
                name: bounded_text(&name, MAX_NAME_BYTES),
                description: bounded_text(&description, MAX_DESCRIPTION_BYTES),
                skill_markdown: skill_markdown.to_string(),
                content_digest: compute_content_digest(skill_markdown),
                status,
                diagnostics,
                created_at: now,
                updated_at: now,
                revision: 1,
            };
            request.consumed = true;
            file.proposals.push(proposal.clone());
            Ok(proposal)
        })
    }

    fn path_for_namespace(&self, namespace: &str) -> io::Result<PathBuf> {
        let suffix = namespace
            .strip_prefix("project/")
            .filter(|suffix| {
                suffix.len() == 64 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            .ok_or_else(|| invalid_data("invalid project namespace"))?;
        Ok(self.root.join(format!("{suffix}.json")))
    }

    fn load_namespace(&self, namespace: &str) -> io::Result<PromotionFile> {
        let path = self.path_for_namespace(namespace)?;
        if !path.exists() {
            return Ok(empty_file());
        }
        let metadata = fs::metadata(&path)?;
        if metadata.len() > MAX_PROMOTION_FILE_BYTES {
            return Err(invalid_data("promotion file exceeds size bound"));
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_PROMOTION_FILE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        let file: PromotionFile = serde_json::from_slice(&bytes)
            .map_err(|error| invalid_data(&format!("malformed promotion file: {error}")))?;
        validate_file(&file)?;
        Ok(file)
    }

    fn with_locked_file<T>(
        &self,
        namespace: &str,
        operation: impl FnOnce(&mut PromotionFile) -> io::Result<T>,
    ) -> io::Result<T> {
        let path = self.path_for_namespace(namespace)?;
        let lock_path = path.with_extension("json.lock");
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .open(lock_path)?;
        flock_lock(&lock)?;
        let result = (|| {
            let mut file = self.load_namespace(namespace)?;
            let result = operation(&mut file)?;
            save_file(&path, &file)?;
            Ok(result)
        })();
        let _ = flock_unlock(&lock);
        result
    }
}

pub fn compute_content_digest(markdown: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"codegg-skill-proposal-v1\0");
    hasher.update(markdown.replace("\r\n", "\n").as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn build_draft_prompt(request: &PromotionDraftRequest) -> String {
    let context = &request.context;
    let actions = context
        .action_skeleton
        .iter()
        .map(WorkflowAction::label)
        .collect::<Vec<_>>()
        .join(" -> ");
    let existing = if context.existing_skill_names.is_empty() {
        "(none listed)".to_string()
    } else {
        context.existing_skill_names.join(", ")
    };
    let memories = if context.related_memories.is_empty() {
        "(none)".to_string()
    } else {
        context
            .related_memories
            .iter()
            .take(MAX_RELATED_MEMORIES)
            .map(|m| {
                format!(
                    "- [{}] {}",
                    m.id.chars().take(24).collect::<String>(),
                    m.summary
                        .chars()
                        .take(MAX_MEMORY_SUMMARY_BYTES)
                        .collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "[CodeGG explicit skill-promotion request]\nRequest ID: {}\nHabit ID: {}\n\nThe user explicitly asked you to draft one portable SKILL.md proposal from this ready workflow evidence. Submit exactly one proposal with the skill_proposal tool using action=submit and this request ID. Do not write files, activate skills, grant permissions, add scripts/resources, or use allowed-tools.\n\nWorkflow fingerprint: {}\nActions: {}\nSuccessful occurrences: {}\nDistinct sessions: {}\nExisting skill names (bounded advisory list): {}\nTarget scope hint: project\nRelated memory summaries (bounded, explicitly selected only):\n{}\n\nDraft concise reusable instructions with frontmatter name and description, followed by Markdown body. The host will validate and preview it; it is not installed yet.",
        request.id.as_str(),
        context.habit_id.as_str(),
        context.workflow_fingerprint,
        actions,
        context.successful_occurrences,
        context.distinct_sessions,
        existing,
        memories,
    )
}

pub fn collision_diagnostics(
    registry: &AssetRegistry,
    normalized_name: &str,
) -> Vec<BoundedDiagnostic> {
    registry
        .get(normalized_name)
        .map(|skill| {
            vec![BoundedDiagnostic {
                severity: Severity::Warning,
                reason: "existing effective skill with same normalized name".to_string(),
                location: Some(format!(
                    "{}:{}",
                    source_kind_label(skill.source_kind),
                    skill.source_path.display()
                )),
            }]
        })
        .unwrap_or_default()
}

fn generated_restriction_diagnostics(
    source: &str,
    document: &ValidatedSkillDocument,
) -> Vec<BoundedDiagnostic> {
    let mut diagnostics = Vec::new();
    let fields = codegg_config::parse_yaml::<std::collections::HashMap<String, serde_json::Value>>(
        "skill proposal restrictions",
        document.frontmatter_raw.as_bytes(),
    );
    if let Ok(fields) = fields {
        for key in fields.keys() {
            if key == "allowed-tools" {
                diagnostics.push(BoundedDiagnostic::error(
                    "generated proposals cannot contain allowed-tools",
                ));
            } else if !matches!(
                key.as_str(),
                "name" | "description" | "license" | "metadata"
            ) {
                diagnostics.push(BoundedDiagnostic::error(format!(
                    "generated proposals reject unsupported frontmatter field '{key}'"
                )));
            }
        }
    }
    if document.metadata.contains_key("allowed-tools") {
        diagnostics.push(BoundedDiagnostic::error(
            "generated proposals cannot contain allowed-tools",
        ));
    }
    // M002 accepts a single SKILL.md with no sidecar package. Reject explicit
    // sidecar declarations (path-like references), not ordinary prose that
    // merely mentions plugins or MCP concepts.
    let lowered = source.to_ascii_lowercase();
    for marker in ["scripts/", "resources/", "package.json"] {
        if lowered.contains(marker) {
            diagnostics.push(BoundedDiagnostic::error(format!(
                "generated proposals cannot declare bundled {marker}"
            )));
        }
    }
    // YAML-like declarations embedded in the body would indicate an
    // executable/resource/plugin payload rather than plain instructions.
    for marker in ["mcp:", "plugin:", "mcp_servers:", "allowed-tools:"] {
        if lowered.contains(marker) {
            diagnostics.push(BoundedDiagnostic::error(format!(
                "generated proposals cannot declare bundled {marker}"
            )));
        }
    }
    diagnostics
}

fn bounded_context(
    candidate: &HabitCandidate,
    mut existing_skill_names: Vec<String>,
    related_memories: Vec<BoundedMemoryRef>,
) -> io::Result<HabitPromotionContext> {
    existing_skill_names.truncate(32);
    existing_skill_names
        .retain(|name| name.len() <= MAX_NAME_BYTES && !name.chars().any(char::is_control));
    if related_memories.len() > MAX_RELATED_MEMORIES {
        return Err(invalid_data("too many related memories"));
    }
    for m in &related_memories {
        if m.id.is_empty() || m.id.len() > MAX_NAME_BYTES || m.id.chars().any(char::is_control) {
            return Err(invalid_data("invalid related memory reference"));
        }
    }
    let related_memories = related_memories
        .into_iter()
        .map(|m| BoundedMemoryRef {
            id: m.id.chars().take(MAX_NAME_BYTES).collect(),
            summary: m.summary.chars().take(MAX_MEMORY_SUMMARY_BYTES).collect(),
        })
        .collect();
    Ok(HabitPromotionContext {
        habit_id: candidate.id.clone(),
        workflow_fingerprint: bounded_text(&candidate.workflow_fingerprint, MAX_NAME_BYTES),
        action_skeleton: candidate.actions.iter().take(32).cloned().collect(),
        successful_occurrences: candidate.successful_occurrences,
        distinct_sessions: candidate.distinct_sessions,
        related_memories,
        existing_skill_names,
        target_scope_hint: SkillTargetScope::Project,
    })
}

fn normalize_input_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        None
    } else {
        Some(name.to_lowercase())
    }
}

fn bounded_text(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn source_kind_label(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::CodeGGProject => "codegg-project",
        SourceKind::CodeGGGlobal => "codegg-global",
        SourceKind::CodeGGNativeCompat => "codegg-native",
        SourceKind::AgentsProject => "agents-project",
        SourceKind::AgentsGlobal => "agents-global",
        SourceKind::OpenCodeProject => "opencode-project",
        SourceKind::OpenCodeGlobal => "opencode-global",
        SourceKind::ClaudeProject => "claude-project",
        SourceKind::ClaudeGlobal => "claude-global",
        SourceKind::Plugin => "plugin",
    }
}

fn empty_file() -> PromotionFile {
    PromotionFile {
        version: FILE_VERSION,
        requests: Vec::new(),
        proposals: Vec::new(),
    }
}

fn prune_expired(file: &mut PromotionFile, now: i64) {
    file.requests
        .retain(|request| request.expires_at >= now && !request.consumed);
}

fn validate_file(file: &PromotionFile) -> io::Result<()> {
    if file.version != FILE_VERSION
        || file.requests.len() > MAX_REQUESTS
        || file.proposals.len() > MAX_PROPOSALS
        || file.proposals.iter().any(|proposal| {
            proposal.skill_markdown.len() as u64 > MAX_MARKDOWN_BYTES
                || proposal.diagnostics.len() > MAX_DIAGNOSTICS
        })
    {
        return Err(invalid_data("unsupported or oversized promotion file"));
    }
    Ok(())
}

fn save_file(path: &Path, file: &PromotionFile) -> io::Result<()> {
    validate_file(file)?;
    let bytes = serde_json::to_vec_pretty(file)
        .map_err(|error| invalid_data(&format!("failed to serialize promotion file: {error}")))?;
    if bytes.len() as u64 > MAX_PROMOTION_FILE_BYTES {
        return Err(invalid_data("serialized promotion file exceeds size bound"));
    }
    let temp = path.with_extension("json.tmp");
    let mut output = File::create(&temp)?;
    output.write_all(&bytes)?;
    output.sync_all()?;
    drop(output);
    fs::rename(temp, path)
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
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
