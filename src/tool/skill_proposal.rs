//! Model-facing proposal submission with host-owned authorization.

use crate::skills::promotion::{collision_diagnostics, PromotionRequestId, SkillPromotionStore};
use crate::tool::{
    StructuredToolResult, Tool, ToolCallerPolicy, ToolCategory, ToolEffectClass,
    ToolExecutionContext, ToolRetryPolicy,
};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SkillProposalInput {
    action: String,
    promotion_request_id: String,
    habit_id: String,
    name: String,
    description: String,
    skill_markdown: String,
}

#[derive(Default)]
pub struct SkillProposalTool;

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
        let request_id =
            PromotionRequestId::parse(&parsed.promotion_request_id).ok_or_else(|| {
                crate::error::ToolError::Execution("invalid promotion request ID".into())
            })?;
        let habit_id = codegg_core::memory::habit::HabitId::parse(&parsed.habit_id)
            .ok_or_else(|| crate::error::ToolError::Execution("invalid habit ID".into()))?;
        let session_id = ctx.session_id.ok_or_else(|| {
            crate::error::ToolError::Execution("promotion submission requires a session".into())
        })?;
        let project_identity = ctx.cwd.to_string_lossy().to_string();
        let store = SkillPromotionStore::new().map_err(|error| {
            crate::error::ToolError::Execution(format!("promotion store unavailable: {error}"))
        })?;
        let now = chrono::Utc::now().timestamp_millis();
        let mut proposal = store
            .submit(crate::skills::promotion::SkillProposalSubmission {
                project_identity: &project_identity,
                session_id: &session_id,
                request_id: &request_id,
                habit_id: &habit_id,
                supplied_name: &parsed.name,
                supplied_description: &parsed.description,
                skill_markdown: &parsed.skill_markdown,
                now,
            })
            .map_err(|error| crate::error::ToolError::Execution(error.to_string()))?;

        if proposal.status == crate::skills::promotion::SkillProposalStatus::Validated {
            let config = crate::skills::AssetDiscoveryConfig::default();
            let mut global_roots = Vec::new();
            if let Some(root) = crate::agent::asset_context::default_global_skills_root() {
                global_roots.push(root);
            }
            let registry = crate::skills::AssetRegistry::build(&config, &ctx.cwd, &global_roots);
            let collisions = collision_diagnostics(&registry, &proposal.name.to_lowercase());
            if !collisions.is_empty() {
                store
                    .append_diagnostics(&project_identity, &proposal.id, collisions)
                    .map_err(|error| crate::error::ToolError::Execution(error.to_string()))?;
                proposal.diagnostics = store
                    .get_proposal(&project_identity, &proposal.id)
                    .map_err(|error| crate::error::ToolError::Execution(error.to_string()))?
                    .map(|proposal| proposal.diagnostics)
                    .unwrap_or_default();
            }
        }

        let value = serde_json::to_value(&proposal).map_err(|error| {
            crate::error::ToolError::Execution(format!("failed to encode proposal: {error}"))
        })?;
        let output = serde_json::to_string_pretty(&value).map_err(|error| {
            crate::error::ToolError::Execution(format!("failed to encode proposal: {error}"))
        })?;
        Ok(StructuredToolResult::with_value(output, value, true, None))
    }
}
