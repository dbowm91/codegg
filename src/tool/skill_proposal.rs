//! Model-facing proposal submission with host-owned authorization.
//!
//! M007-I made the daemon the **one** writer of the promotion store. The
//! tool therefore writes through the daemon's session storage, keyed on the
//! canonical `WorkspaceId`, and consumes the staged request in the **same**
//! transaction that records the proposal. There is no second writer and no
//! coordination protocol to get wrong.
//!
//! The tool never publishes. Publication is a separate, human-authorized
//! daemon action.

use crate::skills::promotion::{
    build_validated_proposal, collision_diagnostics, habit_is_promotable, proposal_to_payload,
    request_from_payload, PromotionRequestId, SkillProposalSubmission,
};
use crate::tool::{
    StructuredToolResult, Tool, ToolCallerPolicy, ToolCategory, ToolEffectClass,
    ToolExecutionContext, ToolRetryPolicy,
};
use async_trait::async_trait;
use codegg_core::session::skill_promotion::{
    SkillPromotionKind, SkillPromotionRecord, SkillPromotionStore,
};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct SkillProposalInput {
    action: String,
    promotion_request_id: String,
    habit_id: String,
    name: String,
    description: String,
    skill_markdown: String,
}

/// The model-facing proposal tool.
///
/// The tool is **rooted**: it carries the daemon's session pool and the
/// canonical workspace identity, so the proposal it records lands in the
/// same record the human's staged request occupies. An unrooted tool has no
/// store to write and refuses.
pub struct SkillProposalTool {
    store: Option<SkillPromotionStore>,
    workspace_id: Option<String>,
    workspace_root: Option<PathBuf>,
}

impl Default for SkillProposalTool {
    fn default() -> Self {
        Self {
            store: None,
            workspace_id: None,
            workspace_root: None,
        }
    }
}

impl SkillProposalTool {
    /// The rooted tool: it writes into the daemon's own promotion store.
    pub fn new(
        pool: sqlx::SqlitePool,
        workspace_id: impl Into<String>,
        workspace_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            store: Some(SkillPromotionStore::new(pool)),
            workspace_id: Some(workspace_id.into()),
            workspace_root: Some(workspace_root.into()),
        }
    }
}

#[async_trait]
impl Tool for SkillProposalTool {
    fn name(&self) -> &str {
        "skill_proposal"
    }

    fn description(&self) -> &str {
        "Submit one user-authorized, validated portable SKILL.md proposal for preview; never installs or writes a skill."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["submit"] },
                "promotion_request_id": { "type": "string" },
                "habit_id": { "type": "string" },
                "name": { "type": "string" },
                "description": { "type": "string" },
                "skill_markdown": { "type": "string" }
            },
            "required": [
                "action", "promotion_request_id", "habit_id",
                "name", "description", "skill_markdown"
            ],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::SafeMutating
    }

    /// M002: proposal submission is specialist; deferred for ordinary turns.
    fn defer_loading(&self) -> bool {
        crate::tool::disclosure::is_deferred_by_default(self.name())
    }

    fn contract(
        &self,
        tool_name: &str,
        input_schema: serde_json::Value,
    ) -> crate::tool::ToolContract {
        let mut contract = crate::tool::ToolContract::legacy(tool_name, input_schema);
        contract.caller_policy = ToolCallerPolicy::DirectOnly;
        contract.effect_class = ToolEffectClass::NonIdempotent;
        contract.retry_policy = ToolRetryPolicy::none();
        contract.implementation_id = "codegg/skill-proposal".to_string();
        contract.output_schema = Some(serde_json::json!({
            "type": "object",
            "required": ["status", "proposal_id", "content_digest"]
        }));
        contract
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, crate::error::ToolError> {
        Err(crate::error::ToolError::Execution(
            "skill_proposal requires the broker's session and workspace context".to_string(),
        ))
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, crate::error::ToolError> {
        let ctx = ctx.ok_or_else(|| {
            crate::error::ToolError::Execution(
                "skill_proposal requires the broker's session and workspace context".to_string(),
            )
        })?;
        let parsed: SkillProposalInput = serde_json::from_value(input).map_err(|error| {
            crate::error::ToolError::Execution(format!("invalid input: {error}"))
        })?;
        if parsed.action != "submit" {
            return Err(crate::error::ToolError::Execution(
                "skill_proposal only supports action=submit".to_string(),
            ));
        }
        let store = self.store.as_ref().ok_or_else(|| {
            crate::error::ToolError::Execution(
                "skill_proposal requires the daemon's session storage".to_string(),
            )
        })?;
        let workspace_id = self.workspace_id.as_deref().ok_or_else(|| {
            crate::error::ToolError::Execution(
                "skill_proposal requires a canonical workspace".to_string(),
            )
        })?;
        let workspace_root = self.workspace_root.as_deref().ok_or_else(|| {
            crate::error::ToolError::Execution(
                "skill_proposal requires a canonical workspace root".to_string(),
            )
        })?;
        let request_id =
            PromotionRequestId::parse(&parsed.promotion_request_id).ok_or_else(|| {
                crate::error::ToolError::Execution("invalid promotion request ID".into())
            })?;
        let habit_id = codegg_core::memory::habit::HabitId::parse(&parsed.habit_id)
            .ok_or_else(|| crate::error::ToolError::Execution("invalid habit ID".into()))?;
        let session_id = ctx.session_id.ok_or_else(|| {
            crate::error::ToolError::Execution("promotion submission requires a session".into())
        })?;

        // The staged request is the **same record** the human's request
        // occupies: both sides name the canonical workspace, so the old
        // "promotion request not found" divergence cannot arise.
        let record = store
            .get(workspace_id, SkillPromotionKind::Request, request_id.as_str())
            .await
            .map_err(|error| {
                crate::error::ToolError::Execution(format!("promotion store unavailable: {error}"))
            })?
            .ok_or_else(|| {
                crate::error::ToolError::Execution(
                    "the promotion request was not found for this workspace".into(),
                )
            })?;
        let request = request_from_payload(&record.payload)
            .map_err(|error| {
                crate::error::ToolError::Execution(format!("the promotion request is unreadable: {error}"))
            })?;
        if request.session_id != session_id
            || request.habit_id != habit_id
            || request.workspace_id != workspace_id
        {
            return Err(crate::error::ToolError::Execution(
                "the promotion request scope does not match".into(),
            ));
        }

        // The habit candidate is re-validated against the daemon's own view:
        // a candidate that changed since the request invalidates the
        // approval, and the human must start a new request.
        let candidate = self.load_habit_candidate(workspace_root, &habit_id)?;
        let Some(candidate) = candidate else {
            return Err(crate::error::ToolError::Execution(
                "habit candidate no longer exists".into(),
            ));
        };
        if !habit_is_promotable(&candidate)
            || candidate.revision != request.candidate_revision
            || candidate.workflow_fingerprint != request.habit_fingerprint
        {
            return Err(crate::error::ToolError::Execution(
                "habit candidate changed; start a new promotion request".into(),
            ));
        }

        let now = chrono::Utc::now().timestamp_millis();
        let mut proposal = build_validated_proposal(
            &request,
            &SkillProposalSubmission {
                request_id: &request_id,
                habit_id: &habit_id,
                supplied_name: &parsed.name,
                supplied_description: &parsed.description,
                skill_markdown: &parsed.skill_markdown,
                now,
            },
        )
        .map_err(|error| crate::error::ToolError::Execution(error))?;

        // The collision advisory is computed **before** the record is
        // written, so the proposal's own diagnostics are complete in the one
        // transaction that consumes the request.
        if proposal.status == crate::skills::promotion::SkillProposalStatus::Validated {
            let collisions = self.collision_diagnostics(workspace_root, &proposal.name);
            proposal
                .diagnostics
                .extend(collisions);
            proposal
                .diagnostics
                .truncate(crate::skills::promotion::MAX_DIAGNOSTICS);
        }

        let payload = proposal_to_payload(&proposal).map_err(|error| {
            crate::error::ToolError::Execution(format!("failed to encode proposal: {error}"))
        })?;
        let proposal_record = SkillPromotionRecord {
            id: proposal.id.as_str().to_owned(),
            workspace_id: workspace_id.to_owned(),
            session_id: session_id.clone(),
            kind: SkillPromotionKind::Proposal,
            payload,
            payload_sha256: String::new(),
            status: proposal.status.as_str().to_owned(),
            revision: i64::try_from(proposal.revision).unwrap_or(i64::MAX),
            created_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: None,
        };
        // The consume and the insert are **one** transaction: a request is
        // never consumed without its proposal.
        store
            .consume_request_into_proposal(workspace_id, request_id.as_str(), proposal_record, now)
            .await
            .map_err(|error| crate::error::ToolError::Execution(error.to_string()))?;

        let value = serde_json::to_value(&proposal).map_err(|error| {
            crate::error::ToolError::Execution(format!("failed to encode proposal: {error}"))
        })?;
        let output = serde_json::to_string_pretty(&value).map_err(|error| {
            crate::error::ToolError::Execution(format!("failed to encode proposal: {error}"))
        })?;
        Ok(StructuredToolResult::with_value(output, value, true, None))
    }
}

impl SkillProposalTool {
    /// The habit candidate the daemon owns, for one workspace.
    fn load_habit_candidate(
        &self,
        workspace_root: &std::path::Path,
        habit_id: &codegg_core::memory::habit::HabitId,
    ) -> Result<Option<codegg_core::memory::habit::HabitCandidate>, crate::error::ToolError> {
        let identity = workspace_root.to_string_lossy().to_string();
        let habit_id = habit_id.as_str().to_owned();
        let store = codegg_core::memory::habit::HabitStore::new().map_err(|error| {
            crate::error::ToolError::Execution(format!("habit store unavailable: {error}"))
        })?;
        let candidates = store.load(&identity).map_err(|error| {
            crate::error::ToolError::Execution(format!("habit store unavailable: {error}"))
        })?;
        Ok(candidates
            .into_iter()
            .find(|candidate| candidate.id.as_str() == habit_id))
    }

    /// The advisory the proposal carries when the workspace already has a
    /// skill with the same normalized name.
    fn collision_diagnostics(
        &self,
        workspace_root: &std::path::Path,
        name: &str,
    ) -> Vec<crate::skills::promotion::BoundedDiagnostic> {
        let config = crate::skills::AssetDiscoveryConfig::default();
        let global_roots: Vec<std::path::PathBuf> =
            crate::agent::asset_context::default_global_discovery_root()
                .into_iter()
                .collect();
        let registry = crate::skills::AssetRegistry::build(&config, workspace_root, &global_roots);
        collision_diagnostics(&registry, &name.to_lowercase())
    }
}
