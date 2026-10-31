//! The discoverable skill catalogue exposed to frontends.
//!
//! The catalogue is **metadata only**: a name, a description, the namespace
//! the skill was discovered in, and the content digest. It never carries a
//! skill body, a resource body, or a filesystem path, so a renderer can draw
//! the list without learning where the daemon's files live — and the
//! response stays bounded whatever the skill packages contain.

use serde::{Deserialize, Serialize};

/// Maximum number of skills one catalogue response returns.
///
/// The daemon clamps a client-requested `max_entries` to this value, so a
/// client can narrow the bound but never widen it.
pub const MAX_SKILL_CATALOGUE_ENTRIES: usize = 256;

/// One request for a project's skill catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCatalogueRequestDto {
    /// The project whose roots are searched for skills.
    ///
    /// The daemon resolves the project's root from its own durable project
    /// binding; this request carries no filesystem path.
    pub project_id: String,
    /// An optional client-requested entry cap.
    ///
    /// The daemon clamps this to [`MAX_SKILL_CATALOGUE_ENTRIES`]. `None`
    /// means "the daemon's cap".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_entries: Option<usize>,
}

/// The skills the daemon discovered for one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCatalogueDto {
    /// The project the catalogue was built for.
    pub project_id: String,
    /// The discovered skills, capped at the daemon's entry bound.
    pub skills: Vec<SkillSummaryDto>,
    /// How many skills were discovered before the cap was applied.
    pub total: usize,
    /// Whether `skills` was shortened by the cap.
    pub truncated: bool,
}

/// One discovered skill, described without its body or its location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSummaryDto {
    /// The skill's name.
    pub name: String,
    /// The skill's one-line description.
    pub description: String,
    /// The namespace the skill was discovered in.
    pub source: SkillSourceDto,
    /// The digest of the skill's content.
    pub content_digest: String,
    /// How many additional resources the skill package declares.
    pub resource_count: usize,
}

/// The namespace a skill was discovered in.
///
/// The wire shape is deliberately coarser than the discovery layer's own
/// vendor taxonomy: a frontend renders a namespace, not a vendor matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSourceDto {
    /// A skill found under the project's own roots.
    Project,
    /// A skill found under a user-level (global) root.
    Global,
    /// A skill contributed by an installed plugin.
    Plugin,
    /// A skill found under a user-configured extra root.
    Configured,
    /// A compatibility skill shipped with CodeGG itself.
    Native,
}

/// Maximum number of proposals one listing returns.
pub const MAX_SKILL_PROMOTION_LIST: usize = 64;

/// What a promotion request asks the daemon to do.
///
/// The workflow has five human steps and one model step. The daemon owns
/// every one of them, so the request names an **action** and the daemon
/// performs the durable effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillPromotionActionDto {
    /// Stage a request: the human asked for a draft from a habit.
    ///
    /// The daemon resolves the habit candidate, builds the bounded draft
    /// prompt, and records the request against the canonical workspace.
    Begin,
    /// List the workspace's proposals.
    List,
    /// Read one proposal.
    Get,
    /// Record that the human previewed the proposal's current revision.
    Preview,
    /// Reject a proposal that is awaiting review.
    Reject,
    /// Publish a proposal to one scope.
    Publish,
}

/// One request against the daemon's promotion workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPromotionRequestDto {
    /// The canonical workspace the workflow acts in.
    ///
    /// This is the **namespace**: the daemon's durable record is keyed on
    /// it, so the human's request and the daemon tool's proposal resolve to
    /// the same record.
    pub workspace_id: String,
    /// The session the workflow acts in.
    pub session_id: String,
    /// What to do.
    pub action: SkillPromotionActionDto,
    /// The habit a `Begin` request is derived from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub habit_id: Option<String>,
    /// The proposal a `Get`, `Preview`, `Reject` or `Publish` request names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    /// The scope a `Publish` request publishes to.
    ///
    /// `None` means the project scope. The value is a closed enum, never a
    /// filesystem path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_scope: Option<SkillTargetScopeDto>,
}

/// The scope a proposal is published to.
///
/// The value is a closed enum: a publication never names a filesystem path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTargetScopeDto {
    /// The project's own skill root, under the checkout.
    Project,
    /// The user-level skill root, outside every checkout.
    Global,
}

/// One proposal, as the daemon reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillProposalSummaryDto {
    /// The proposal's identity.
    pub proposal_id: String,
    /// The proposal's name.
    pub name: String,
    /// The proposal's description.
    pub description: String,
    /// The proposal's status, in the workflow's own vocabulary.
    pub status: String,
    /// The proposal's current revision.
    pub revision: u64,
    /// The digest of the proposal's body.
    pub content_digest: String,
}

/// One diagnostic a proposal carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillProposalDiagnosticDto {
    /// The severity, as the workflow's own name.
    pub severity: String,
    /// Why the diagnostic was raised.
    pub reason: String,
    /// Where it was raised, when the workflow named a place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

/// One proposal in full, as the daemon reports it.
///
/// The **list** carries summaries only: a proposal's body is the model's own
/// `SKILL.md` draft, so a listing does not carry it. The **detail** does,
/// because the human reads it before approving it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillProposalDetailDto {
    pub proposal_id: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub revision: u64,
    pub content_digest: String,
    pub habit_id: String,
    pub habit_fingerprint: String,
    pub candidate_revision: u64,
    pub target_scope: SkillTargetScopeDto,
    pub skill_markdown: String,
    pub diagnostics: Vec<SkillProposalDiagnosticDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_relative_path: Option<String>,
}

/// The daemon's answer to a promotion request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPromotionResultDto {
    /// The request's identity, when the action produced one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// The bounded draft prompt, when the action was `Begin`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_prompt: Option<String>,
    /// The proposals, when the action was `List` or `Get`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposals: Option<Vec<SkillProposalSummaryDto>>,
    /// The published proposal's relative path, when the action was
    /// `Publish`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_relative_path: Option<String>,
    /// The proposal in full, when the action was `Get` or `Preview`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<SkillProposalDetailDto>,
    /// Whether the action changed the durable record.
    pub changed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_catalogue_request_ignores_a_client_supplied_path() {
        let json = serde_json::json!({
            "project_id": "project-1",
            "path": "/etc",
            "skills_root": "/etc",
        });
        let request: SkillCatalogueRequestDto =
            serde_json::from_value(json).expect("the request decodes without a path field");
        assert_eq!(request.project_id, "project-1");
        assert_eq!(request.max_entries, None);
        let rendered = serde_json::to_value(&request).expect("the request re-renders");
        assert!(
            rendered.get("path").is_none(),
            "a client-supplied path must not survive into the request"
        );
        assert!(rendered.get("skills_root").is_none());
    }

    #[test]
    fn skill_catalogue_carries_metadata_but_no_body_or_location() {
        let catalogue = SkillCatalogueDto {
            project_id: "project-1".into(),
            skills: vec![SkillSummaryDto {
                name: "testing-ci".into(),
                description: "Select focused verification.".into(),
                source: SkillSourceDto::Project,
                content_digest: "sha256:abc".into(),
                resource_count: 2,
            }],
            total: 1,
            truncated: false,
        };
        let json = serde_json::to_value(&catalogue).expect("the catalogue renders");
        let entry = json
            .get("skills")
            .and_then(|skills| skills.get(0))
            .expect("the first skill is present");
        assert_eq!(
            entry.get("name").and_then(|value| value.as_str()),
            Some("testing-ci")
        );
        assert_eq!(
            entry.get("source").and_then(|value| value.as_str()),
            Some("project")
        );
        assert!(
            entry.get("body").is_none(),
            "the catalogue must not carry a skill body"
        );
        assert!(
            entry.get("source_path").is_none(),
            "the catalogue must not carry a filesystem path"
        );
        let decoded: SkillCatalogueDto =
            serde_json::from_value(json).expect("the catalogue decodes back");
        assert_eq!(decoded, catalogue);
    }
}
