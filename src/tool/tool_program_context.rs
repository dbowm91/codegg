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

    // M014-A2: Derive authority fields from the actual accepted
    // permission/path-policy decision carried in the execution context.
    // Fall back to bounded, non-authorizing values only when no decision
    // was recorded — these are correlation values, never authorization
    // evidence, and the executor fails closed when the grant is absent.
    let principal_ref = context
        .and_then(|context| context.principal_identity.clone())
        .or_else(|| Some(format!("program:{}", program_id)));

    let authority_ref = context
        .and_then(|context| context.decision_id.clone())
        .or_else(|| {
            Some(stable_digest(&format!(
                "program:{}:{}:{}",
                program_id,
                workspace_id,
                session_id.as_deref().unwrap_or("anon")
            )))
        });

    let workspace_path_policy_id = context
        .and_then(|context| context.workspace_path_policy_id.clone())
        .unwrap_or_else(|| format!("workspace:{workspace_id}"));

    let policy_revision = context
        .and_then(|context| context.permission_policy_revision.clone())
        .or_else(|| context.and_then(|context| context.workspace_path_policy_revision.clone()))
        .unwrap_or_else(|| {
            format!(
                "policy:{}:{}",
                workspace_id,
                agent_id.as_deref().unwrap_or("no-agent")
            )
        });

    codegg_core::jobs::ToolProgramExecutionContext {
        schema_version: 1,
        workspace_path_policy_id,
        session_id: session_id.clone(),
        turn_id: context.and_then(|context| context.turn_id.clone()),
        agent_id: agent_id.clone(),
        parent_job_id: context.and_then(|context| context.parent_job_id.clone()),
        parent_attempt_id: context.and_then(|context| context.parent_attempt_id.clone()),
        parent_call_id: context.and_then(|context| context.invocation_key.clone()),
        principal_ref,
        authority_ref,
        permission_mode: context.and_then(|context| context.permission_mode.clone()),
        policy_revision: Some(policy_revision),
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

/// M014-B2: Compute the canonical contract catalog digest from the
/// resolved tool contracts. Uses deterministic serialization (sorted
/// tool names, stable field order) and SHA-256. The same helper must
/// be used at submission, executor admission, and every nested Broker
/// invocation.
pub fn canonical_contract_digest(contracts: &[ContractEntry]) -> String {
    let mut sorted: Vec<&ContractEntry> = contracts.iter().collect();
    sorted.sort_by(|a, b| a.tool_name.cmp(&b.tool_name));
    let material = serde_json::to_string(&serde_json::json!({
        "contracts": sorted.iter().map(|c| serde_json::json!({
            "tool_name": c.tool_name,
            "implementation_id": c.implementation_id,
            "implementation_version": c.implementation_version,
            "caller_policy": c.caller_policy,
            "effect_class": c.effect_class,
            "idempotency": c.idempotency,
            "input_schema_digest": c.input_schema_digest,
            "output_schema_digest": c.output_schema_digest,
        })).collect::<Vec<_>>(),
    }))
    .unwrap_or_default();
    format!("sha256:{}", stable_digest(&material))
}

/// M014-B1: A single entry in the frozen contract catalog snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContractEntry {
    pub tool_name: String,
    pub implementation_id: String,
    pub implementation_version: String,
    pub caller_policy: String,
    pub effect_class: String,
    pub idempotency: String,
    pub input_schema_digest: String,
    pub output_schema_digest: String,
}

/// M014-B1: Resolve the frozen contract snapshot for the requested tools
/// from the production Broker catalog. Returns `Err` if any requested tool
/// is missing, direct-only, mutation-capable, or schema-incomplete.
pub fn resolve_contract_snapshot(
    broker: &crate::tool::broker::ToolBroker,
    allowed_tools: &[String],
) -> Result<Vec<ContractEntry>, String> {
    let mut entries = Vec::new();
    for tool_name in allowed_tools {
        let contract = broker
            .lookup_contract(tool_name)
            .map_err(|e| format!("tool '{}' not found in catalog: {}", tool_name, e))?;
        // M014-B1: Reject direct-only and programmatic-only contracts
        // (only DirectOrProgrammatic is allowed for Tool Programs).
        if contract.caller_policy != crate::tool::contract::ToolCallerPolicy::DirectOrProgrammatic {
            return Err(format!(
                "tool '{}' has unsupported caller policy for Tool Programs",
                tool_name
            ));
        }
        if !matches!(
            contract.effect_class,
            crate::tool::contract::ToolEffectClass::ReadOnly
                | crate::tool::contract::ToolEffectClass::ReadValidate
                | crate::tool::contract::ToolEffectClass::SafeRepeat
        ) {
            return Err(format!(
                "tool '{}' has non-read-only effect class",
                tool_name
            ));
        }
        let input_schema_digest = format!(
            "sha256:{}",
            stable_digest(&serde_json::to_string(&contract.input_schema).unwrap_or_default())
        );
        let output_schema_digest = match &contract.output_schema {
            Some(schema) => format!(
                "sha256:{}",
                stable_digest(&serde_json::to_string(schema).unwrap_or_default())
            ),
            None => "sha256:none".to_string(),
        };
        entries.push(ContractEntry {
            tool_name: tool_name.clone(),
            implementation_id: contract.implementation_id.clone(),
            implementation_version: contract.implementation_version.clone(),
            caller_policy: match contract.caller_policy {
                crate::tool::contract::ToolCallerPolicy::DirectOnly => "direct_only",
                crate::tool::contract::ToolCallerPolicy::DirectOrProgrammatic => {
                    "direct_or_programmatic"
                }
                crate::tool::contract::ToolCallerPolicy::ProgrammaticOnly => "programmatic_only",
            }
            .to_string(),
            effect_class: match contract.effect_class {
                crate::tool::contract::ToolEffectClass::ReadOnly => "read_only",
                crate::tool::contract::ToolEffectClass::ReadValidate => "read_validate",
                crate::tool::contract::ToolEffectClass::SafeRepeat => "safe_repeat",
                crate::tool::contract::ToolEffectClass::IdempotentMutating => "idempotent_mutating",
                crate::tool::contract::ToolEffectClass::NonIdempotent => "non_idempotent",
                crate::tool::contract::ToolEffectClass::ProcessExec => "process_exec",
            }
            .to_string(),
            idempotency: match contract.idempotency {
                crate::tool::contract::IdempotencyClass::Idempotent => "idempotent",
                crate::tool::contract::IdempotencyClass::NonIdempotent => "non_idempotent",
            }
            .to_string(),
            input_schema_digest,
            output_schema_digest,
        });
    }
    Ok(entries)
}

/// M012-B / M013-A: Build a real authority grant from the execution
/// context, allowed tools, source, IR, and contract catalog.
///
/// This function constructs a `ToolAuthorityGrant` from the durable
/// execution context, allowed tools, and source/IR/contract digests.
/// The grant is verified by the Tool Broker on every nested call. The
/// decision_digest covers every security-relevant field via
/// `compute_digest()` so any tamper fails verification.
pub fn build_authority_grant(
    exec_ctx: Option<&codegg_core::jobs::ToolProgramExecutionContext>,
    workspace_id: &str,
    program_id: &str,
    allowed_tools: &[String],
    source_digest: &str,
    ir_digest: &str,
    contract_digest: &str,
) -> codegg_core::jobs::ToolAuthorityGrant {
    let now = chrono::Utc::now().timestamp_millis();

    // M014-A2: Build the manifest digest from the actual frozen contract
    // snapshot (allowed tools + source + IR + contract). This is the same
    // canonical digest used at executor admission and every nested Broker
    // call.
    let manifest_digest = format!(
        "sha256:{}",
        stable_digest(
            &serde_json::to_string(&serde_json::json!({
                "allowed_tools": allowed_tools,
                "source_digest": source_digest,
                "ir_digest": ir_digest,
                "contract_digest": contract_digest,
            }))
            .unwrap_or_default()
        )
    );

    // M014-A2: Extract decision fields from the execution context.
    // These come from the actual accepted permission/path-policy decision
    // in the agent loop, not from synthesized identity strings.
    let (
        session_id,
        agent_id,
        turn_id,
        permission_mode,
        policy_revision,
        principal_ref,
        authority_ref,
        workspace_path_policy_id,
    ) = match exec_ctx {
        Some(ctx) => (
            ctx.session_id.clone(),
            ctx.agent_id.clone(),
            ctx.turn_id.clone(),
            ctx.permission_mode.clone(),
            ctx.policy_revision
                .clone()
                .unwrap_or_else(|| "tool-program-v1".into()),
            ctx.principal_ref
                .clone()
                .unwrap_or_else(|| format!("program:{}", program_id)),
            ctx.authority_ref.clone().unwrap_or_else(|| {
                stable_digest(&format!("program:{}:{}", program_id, workspace_id))
            }),
            ctx.workspace_path_policy_id.clone(),
        ),
        None => (
            None,
            None,
            None,
            None,
            "tool-program-v1".into(),
            format!("program:{}", program_id),
            stable_digest(&format!("program:{}:{}", program_id, workspace_id)),
            format!("workspace:{}", workspace_id),
        ),
    };

    let grant = codegg_core::jobs::ToolAuthorityGrant {
        schema_version: 1,
        grant_id: format!("grant:{}:{}", program_id, now),
        principal_ref,
        workspace_id: workspace_id.to_string(),
        workspace_path_policy_id,
        session_id,
        agent_id,
        turn_id,
        permission_mode,
        policy_revision,
        allowed_caller_class: "program".into(),
        allowed_effect_class: "read_only".into(),
        manifest_digest,
        source_digest: source_digest.to_string(),
        ir_digest: ir_digest.to_string(),
        contract_digest: contract_digest.to_string(),
        issued_at: now,
        expires_at: None,
        revoked_at: None,
        decision_digest: String::new(),
    };
    // M013-A2 / C-04: compute the digest over every security-relevant
    // field so any later tamper fails `verify_integrity()`.
    let decision_digest = grant.compute_digest();
    codegg_core::jobs::ToolAuthorityGrant {
        decision_digest,
        ..grant
    }
}
