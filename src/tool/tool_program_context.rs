//! Construction and hashing of immutable Tool Program submission context.

use sha2::{Digest, Sha256};

pub fn stable_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn to_core_context(
    context: Option<&crate::tool::backend::ToolExecutionContext>,
    workspace_id: &str,
    program_id: &str,
) -> codegg_core::jobs::ToolProgramExecutionContext {
    let invocation_key = context
        .and_then(|context| context.invocation_key.clone())
        .unwrap_or_else(|| format!("tool-program:{program_id}"));
    let session_id = context.and_then(|context| context.session_id.clone());
    let agent_id = context.and_then(|context| context.agent_id.clone());
    codegg_core::jobs::ToolProgramExecutionContext {
        schema_version: 1,
        workspace_path_policy_id: format!("workspace:{workspace_id}"),
        session_id: session_id.clone(),
        turn_id: context.and_then(|context| context.turn_id.clone()),
        agent_id: agent_id.clone(),
        parent_job_id: context.and_then(|context| context.parent_job_id.clone()),
        parent_attempt_id: context.and_then(|context| context.parent_attempt_id.clone()),
        parent_call_id: context.and_then(|context| context.invocation_key.clone()),
        principal_ref: Some(format!("program:{}", program_id)),
        authority_ref: Some(stable_digest(&format!(
            "program:{}:{}:{}",
            program_id,
            workspace_id,
            session_id.as_deref().unwrap_or("anon")
        ))),
        permission_mode: context.and_then(|context| context.permission_mode.clone()),
        policy_revision: Some(format!(
            "policy:{}:{}",
            workspace_id,
            agent_id.as_deref().unwrap_or("no-agent")
        )),
        provider_connection_id: context.and_then(|context| context.provider_name.clone()),
        provider_model: None,
        backend_policy: context
            .and_then(|context| context.backend_policy.clone())
            .unwrap_or_else(|| "native_only".into()),
        correlation_id: stable_digest(&invocation_key),
    }
}

pub fn authority_digest(
    context: &codegg_core::jobs::ToolProgramExecutionContext,
    allowed_tools: &[String],
    source_digest: &str,
) -> String {
    let material = serde_json::json!({
        "context": context,
        "allowed_tools": allowed_tools,
        "source_digest": source_digest,
    });
    stable_digest(&material.to_string())
}

/// M012-B: Build a real authority grant from the execution context.
///
/// This function constructs a `ToolAuthorityGrant` from the durable
/// execution context, allowed tools, and source digest. The grant is
/// verified by the Tool Broker on every nested call.
pub fn build_authority_grant(
    exec_ctx: Option<&codegg_core::jobs::ToolProgramExecutionContext>,
    workspace_id: &str,
    program_id: &str,
    allowed_tools: &[String],
    source_digest: &str,
) -> codegg_core::jobs::ToolAuthorityGrant {
    let now = chrono::Utc::now().timestamp_millis();
    let manifest_digest = format!(
        "sha256:{}",
        stable_digest(
            &serde_json::to_string(&serde_json::json!({
                "allowed_tools": allowed_tools,
                "source_digest": source_digest,
            }))
            .unwrap_or_default()
        )
    );
    let decision_digest = format!(
        "sha256:grant:{}:{}:{}:{}",
        workspace_id, program_id, source_digest, now
    );

    let (session_id, agent_id, turn_id, permission_mode, policy_revision) = match exec_ctx {
        Some(ctx) => (
            ctx.session_id.clone(),
            ctx.agent_id.clone(),
            ctx.turn_id.clone(),
            ctx.permission_mode.clone(),
            ctx.policy_revision
                .clone()
                .unwrap_or_else(|| "tool-program-v1".into()),
        ),
        None => (None, None, None, None, "tool-program-v1".into()),
    };

    codegg_core::jobs::ToolAuthorityGrant {
        schema_version: 1,
        grant_id: format!("grant:{}:{}", program_id, now),
        principal_ref: exec_ctx
            .and_then(|ctx| ctx.principal_ref.clone())
            .unwrap_or_else(|| format!("program:{}", program_id)),
        workspace_id: workspace_id.to_string(),
        workspace_path_policy_id: exec_ctx
            .map(|ctx| ctx.workspace_path_policy_id.clone())
            .unwrap_or_else(|| format!("workspace:{}", workspace_id)),
        session_id,
        agent_id,
        turn_id,
        permission_mode,
        policy_revision,
        allowed_caller_class: "program".into(),
        allowed_effect_class: "read_only".into(),
        manifest_digest,
        issued_at: now,
        expires_at: None,
        revoked_at: None,
        decision_digest,
    }
}
