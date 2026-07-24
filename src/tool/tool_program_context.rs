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
    codegg_core::jobs::ToolProgramExecutionContext {
        schema_version: 1,
        workspace_path_policy_id: format!("workspace:{workspace_id}"),
        session_id: context.and_then(|context| context.session_id.clone()),
        turn_id: context.and_then(|context| context.turn_id.clone()),
        agent_id: context.and_then(|context| context.agent_id.clone()),
        parent_job_id: context.and_then(|context| context.parent_job_id.clone()),
        parent_attempt_id: context.and_then(|context| context.parent_attempt_id.clone()),
        parent_call_id: context.and_then(|context| context.invocation_key.clone()),
        principal_ref: Some("local-agent".into()),
        authority_ref: Some(stable_digest("local-agent-authority-v1")),
        permission_mode: context.and_then(|context| context.permission_mode.clone()),
        policy_revision: Some("tool-policy-v1".into()),
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
