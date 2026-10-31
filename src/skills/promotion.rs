//! User-authorized, pre-publication skill promotion state.
//!
//! This module owns the workflow's **domain**: the identity types, the
//! bounded context, the draft prompt and the proposal validation. It writes
//! nothing.
//!
//! M007-I moved the durable record out of this module and into the daemon's
//! session storage (`codegg_core::session::skill_promotion`). The workflow
//! used to be two writers of one JSON file, held together by an `flock`
//! sidecar; the daemon is now the **one** writer, and the record is keyed on
//! the canonical `WorkspaceId` rather than on a digest of a path string. The
//! sidecar is retired with the second writer.

use super::parser::{validate_portable_document, ValidatedSkillDocument};
use super::{AssetDiscoveryConfig, AssetRegistry, Diagnostic, Severity, SourceKind};
use codegg_core::memory::habit::{HabitCandidate, HabitCandidateStatus, HabitId, WorkflowAction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_NAME_BYTES: usize = 128;
pub const MAX_DESCRIPTION_BYTES: usize = 2048;
pub const MAX_MARKDOWN_BYTES: u64 = 256 * 1024;
pub const MAX_DIAGNOSTICS: usize = 32;
pub const MAX_DIAGNOSTIC_BYTES: usize = 512;
pub const MAX_MEMORY_SUMMARY_BYTES: usize = 512;
pub const MAX_RELATED_MEMORIES: usize = 16;
/// How long a staged request stays usable before it expires.
pub const PROMOTION_REQUEST_TTL_MS: i64 = 15 * 60 * 1000;

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

/// One staged request: the human asked for a draft from a ready habit.
///
/// The record is keyed on the canonical `WorkspaceId`, so the human's staged
/// request and the model tool's proposal resolve to **the same** record. The
/// old namespace was a digest of a path string, derived differently on each
/// side, and the disagreement surfaced as "promotion request not found".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionDraftRequest {
    pub id: PromotionRequestId,
    pub session_id: String,
    /// The canonical workspace the request belongs to.
    pub workspace_id: String,
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

impl SkillProposalStatus {
    /// The status' own name, which is also the stored record's status.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Validated => "validated",
            Self::Rejected => "rejected",
            Self::Published => "published",
            Self::Superseded => "superseded",
        }
    }

    /// The status a stored name names.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(Self::Draft),
            "validated" => Some(Self::Validated),
            "rejected" => Some(Self::Rejected),
            "published" => Some(Self::Published),
            "superseded" => Some(Self::Superseded),
            _ => None,
        }
    }
}

/// Durable, root-relative provenance for a published proposal.  The
/// destination is intentionally represented as a scope plus relative path;
/// machine-specific absolute paths are not part of proposal state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedSkillRef {
    pub proposal_id: SkillProposalId,
    pub target_scope: SkillTargetScope,
    pub normalized_name: String,
    pub relative_path: String,
    pub content_digest: String,
    pub published_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    pub(crate) fn error(reason: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            reason: reason.into().chars().take(MAX_DIAGNOSTIC_BYTES).collect(),
            location: Some("skill proposal".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillProposal {
    pub id: SkillProposalId,
    pub promotion_request_id: PromotionRequestId,
    pub habit_id: HabitId,
    pub habit_fingerprint: String,
    pub candidate_revision: u64,
    /// The canonical workspace the proposal belongs to.
    pub workspace_id: String,
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
    #[serde(default)]
    pub publication: Option<PublishedSkillRef>,
    #[serde(default)]
    pub previewed_revision: Option<u64>,
}

/// What one proposal's summary carries, without its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillProposalSummary {
    pub name: String,
    pub description: String,
    pub status: String,
    pub revision: u64,
    pub content_digest: String,
}

/// The model's own submission, against a staged request.
#[derive(Debug, Clone, Copy)]
pub struct SkillProposalSubmission<'a> {
    pub request_id: &'a PromotionRequestId,
    pub habit_id: &'a HabitId,
    pub supplied_name: &'a str,
    pub supplied_description: &'a str,
    pub skill_markdown: &'a str,
    pub now: i64,
}

/// The workflow's own serialized proposal, as the durable record holds it.
pub fn proposal_to_payload(proposal: &SkillProposal) -> Result<String, String> {
    serde_json::to_string(proposal).map_err(|error| format!("failed to encode proposal: {error}"))
}

/// A durable proposal record's payload, decoded.
pub fn proposal_from_payload(payload: &str) -> Result<SkillProposal, String> {
    serde_json::from_str(payload).map_err(|error| format!("malformed proposal record: {error}"))
}

/// The workflow's own serialized request, as the durable record holds it.
pub fn request_to_payload(request: &PromotionDraftRequest) -> Result<String, String> {
    serde_json::to_string(request)
        .map_err(|error| format!("failed to encode promotion request: {error}"))
}

/// A durable request record's payload, decoded.
pub fn request_from_payload(payload: &str) -> Result<PromotionDraftRequest, String> {
    serde_json::from_str(payload)
        .map_err(|error| format!("malformed promotion request record: {error}"))
}

/// A durable proposal record's payload, summarized without its body.
pub fn proposal_summary_from_payload(payload: &str) -> Result<SkillProposalSummary, String> {
    let proposal = proposal_from_payload(payload)?;
    Ok(SkillProposalSummary {
        name: proposal.name,
        description: proposal.description,
        status: proposal.status.as_str().to_owned(),
        revision: proposal.revision,
        content_digest: proposal.content_digest,
    })
}

/// The bounded context a staged request carries.
pub fn draft_context(
    candidate: &HabitCandidate,
    existing_skill_names: Vec<String>,
    related_memories: Vec<BoundedMemoryRef>,
) -> Result<HabitPromotionContext, String> {
    bounded_context(candidate, existing_skill_names, related_memories).map_err(|error| {
        error
            .to_string()
            .replace("invalid data: ", "")
            .replace("promotion request ", "")
    })
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

/// Validate the model's draft against the portable-document rules and build
/// the proposal it produces.
///
/// The function is **pure**: it reads no store and writes none. The caller
/// owns the atomic consume-and-record step.
pub fn build_validated_proposal(
    request: &PromotionDraftRequest,
    submission: &SkillProposalSubmission<'_>,
) -> Result<SkillProposal, String> {
    if submission.skill_markdown.len() as u64 > MAX_MARKDOWN_BYTES {
        return Err("proposal exceeds skill file size bound".to_owned());
    }
    let parsed = validate_portable_document(
        submission.skill_markdown,
        &AssetDiscoveryConfig::default(),
    );
    let (name, description, mut diagnostics, status) = match parsed {
        Ok(document) => {
            let mut diagnostics = document
                .diagnostics
                .iter()
                .map(BoundedDiagnostic::from_diagnostic)
                .collect::<Vec<_>>();
            diagnostics.extend(generated_restriction_diagnostics(
                submission.skill_markdown,
                &document,
            ));
            if normalize_input_name(submission.supplied_name)
                != Some(document.normalized_name.clone())
                || submission.supplied_description.trim() != document.description.trim()
            {
                diagnostics.push(BoundedDiagnostic::error(
                    "submitted name/description must match portable frontmatter",
                ));
            }
            let status = if diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error)
            {
                SkillProposalStatus::Rejected
            } else {
                SkillProposalStatus::Validated
            };
            (document.name, document.description, diagnostics, status)
        }
        Err(diagnostic) => (
            bounded_text(submission.supplied_name, MAX_NAME_BYTES),
            bounded_text(submission.supplied_description, MAX_DESCRIPTION_BYTES),
            vec![BoundedDiagnostic::from_diagnostic(&diagnostic)],
            SkillProposalStatus::Rejected,
        ),
    };
    diagnostics.truncate(MAX_DIAGNOSTICS);
    Ok(SkillProposal {
        id: SkillProposalId::new(),
        promotion_request_id: request.id.clone(),
        habit_id: request.habit_id.clone(),
        habit_fingerprint: request.habit_fingerprint.clone(),
        candidate_revision: request.candidate_revision,
        workspace_id: request.workspace_id.clone(),
        target_scope: request.context.target_scope_hint,
        name: bounded_text(&name, MAX_NAME_BYTES),
        description: bounded_text(&description, MAX_DESCRIPTION_BYTES),
        skill_markdown: submission.skill_markdown.to_string(),
        content_digest: compute_content_digest(submission.skill_markdown),
        status,
        diagnostics,
        created_at: submission.now,
        updated_at: submission.now,
        revision: 1,
        publication: None,
        previewed_revision: None,
    })
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

/// Re-run the generated-content restrictions at publication time.  This is
/// deliberately separate from proposal submission so changes to parser or
/// restriction policy cannot be bypassed by an old `Validated` record.
pub(crate) fn publication_restriction_diagnostics(
    source: &str,
    document: &ValidatedSkillDocument,
) -> Vec<BoundedDiagnostic> {
    generated_restriction_diagnostics(source, document)
}

fn bounded_context(
    candidate: &HabitCandidate,
    mut existing_skill_names: Vec<String>,
    related_memories: Vec<BoundedMemoryRef>,
) -> Result<HabitPromotionContext, String> {
    existing_skill_names.truncate(32);
    existing_skill_names
        .retain(|name| name.len() <= MAX_NAME_BYTES && !name.chars().any(char::is_control));
    if related_memories.len() > MAX_RELATED_MEMORIES {
        return Err("too many related memories".to_owned());
    }
    for memory in &related_memories {
        if memory.id.is_empty()
            || memory.id.len() > MAX_NAME_BYTES
            || memory.id.chars().any(char::is_control)
        {
            return Err("invalid related memory reference".to_owned());
        }
    }
    let related_memories = related_memories
        .into_iter()
        .map(|memory| BoundedMemoryRef {
            id: memory.id.chars().take(MAX_NAME_BYTES).collect(),
            summary: memory
                .summary
                .chars()
                .take(MAX_MEMORY_SUMMARY_BYTES)
                .collect(),
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

/// The habit candidate is promotable only when it is **ready**.
///
/// The check is part of the domain, so the daemon's request handler and the
/// model-facing tool agree on it without either re-deriving the rule.
pub fn habit_is_promotable(candidate: &HabitCandidate) -> bool {
    candidate.status == HabitCandidateStatus::Ready
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
        SourceKind::CursorProject => "cursor-project",
        SourceKind::GeminiProject => "gemini-project",
        SourceKind::CopilotProject => "copilot-project",
        SourceKind::CopilotGlobal => "copilot-global",
        SourceKind::PiProject => "pi-project",
        SourceKind::PiGlobal => "pi-global",
        SourceKind::RooProject => "roo-project",
        SourceKind::RooGlobal => "roo-global",
        SourceKind::FactoryProject => "factory-project",
        SourceKind::FactoryGlobal => "factory-global",
        SourceKind::ClineProject => "cline-project",
        SourceKind::ClineGlobal => "cline-global",
        SourceKind::Plugin => "plugin",
        SourceKind::Configured => "configured",
    }
}
