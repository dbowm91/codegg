//! Construction and hashing of immutable Tool Program submission context.

use sha2::{Digest, Sha256};

pub fn stable_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn to_core_context(
    context: Option<&crate::tool::backend::ToolExecutionContext>,
    workspace_id: &str,
    program_id: &str,
) -> Result<codegg_core::jobs::ToolProgramExecutionContext, String> {
    let context = context.ok_or_else(|| "accepted permission decision is missing".to_string())?;
    let required = |value: &Option<String>, name: &str| {
        value
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .ok_or_else(|| format!("accepted permission decision is missing {name}"))
    };
    let decision_id = required(&context.decision_id, "decision_id")?;
    if context.decision_outcome.as_deref() != Some("allowed") {
        return Err("permission decision outcome is not allowed".into());
    }
    let principal_ref = required(&context.principal_identity, "principal_identity")?;
    let workspace_path_policy_id = required(
        &context.workspace_path_policy_id,
        "workspace_path_policy_id",
    )?;
    let path_policy_revision = required(
        &context.workspace_path_policy_revision,
        "workspace_path_policy_revision",
    )?;
    let policy_revision = required(
        &context.permission_policy_revision,
        "permission_policy_revision",
    )?;
    let caller_class = required(&context.caller_class, "caller_class")?;
    let maximum_effect_class = required(&context.max_effect_class, "max_effect_class")?;
    let issued_at = context
        .decision_issued_at
        .ok_or_else(|| "accepted permission decision is missing issued_at".to_string())?;
    let now = chrono::Utc::now().timestamp_millis();
    if issued_at > now + 30_000 {
        return Err("permission decision was issued in the future".into());
    }
    if context
        .decision_expires_at
        .is_some_and(|expiry| expiry <= now)
    {
        return Err("permission decision is expired".into());
    }
    if context.decision_revoked_at.is_some() {
        return Err("permission decision is revoked".into());
    }
    let invocation_key = context
        .invocation_key
        .clone()
        .unwrap_or_else(|| format!("tool-program:{program_id}"));
    let session_id = context.session_id.clone();
    let agent_id = context.agent_id.clone();

    Ok(codegg_core::jobs::ToolProgramExecutionContext {
        schema_version: 1,
        workspace_path_policy_id,
        session_id: session_id.clone(),
        turn_id: context.turn_id.clone(),
        agent_id: agent_id.clone(),
        parent_job_id: context.parent_job_id.clone(),
        parent_attempt_id: context.parent_attempt_id.clone(),
        parent_call_id: context.invocation_key.clone(),
        principal_ref: Some(principal_ref),
        authority_ref: Some(decision_id),
        permission_mode: context.permission_mode.clone(),
        policy_revision: Some(policy_revision),
        path_policy_revision: Some(path_policy_revision),
        decision_outcome: context.decision_outcome.clone(),
        caller_class: Some(caller_class),
        maximum_effect_class: Some(maximum_effect_class),
        decision_issued_at: Some(issued_at),
        decision_expires_at: context.decision_expires_at,
        decision_revoked_at: context.decision_revoked_at,
        contract_snapshot_json: String::new(),
        provider_connection_id: context.provider_name.clone(),
        provider_model: None,
        backend_policy: context
            .backend_policy
            .clone()
            .unwrap_or_else(|| "native_only".into()),
        correlation_id: stable_digest(&invocation_key),
    })
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
pub fn canonical_contract_json(contracts: &[ContractEntry]) -> Result<String, String> {
    let mut sorted: Vec<&ContractEntry> = contracts.iter().collect();
    sorted.sort_by(|a, b| a.tool_name.cmp(&b.tool_name));
    if sorted
        .windows(2)
        .any(|pair| pair[0].tool_name == pair[1].tool_name)
    {
        return Err("duplicate tool name in contract snapshot".into());
    }
    serde_json::to_string(&serde_json::json!({
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
    .map_err(|error| format!("contract snapshot serialization failed: {error}"))
}

pub fn canonical_contract_digest(contracts: &[ContractEntry]) -> Result<String, String> {
    let material = canonical_contract_json(contracts)?;
    Ok(format!("sha256:{}", stable_digest(&material)))
}

/// M014-B1: A single entry in the frozen contract catalog snapshot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

pub fn contract_entry(
    contract: &crate::tool::contract::ToolContract,
) -> Result<ContractEntry, String> {
    let input_schema = serde_json::to_string(&contract.input_schema)
        .map_err(|error| format!("input schema serialization failed: {error}"))?;
    let output_schema = contract
        .output_schema
        .as_ref()
        .ok_or_else(|| format!("tool '{}' has no output schema", contract.name))?;
    let output_schema = serde_json::to_string(output_schema)
        .map_err(|error| format!("output schema serialization failed: {error}"))?;
    Ok(ContractEntry {
        tool_name: contract.name.clone(),
        implementation_id: contract.implementation_id.clone(),
        implementation_version: contract.implementation_version.clone(),
        caller_policy: match contract.caller_policy {
            crate::tool::contract::ToolCallerPolicy::DirectOnly => "direct_only",
            crate::tool::contract::ToolCallerPolicy::DirectOrProgrammatic => {
                "direct_or_programmatic"
            }
            crate::tool::contract::ToolCallerPolicy::ProgrammaticOnly => "programmatic_only",
        }
        .into(),
        effect_class: match contract.effect_class {
            crate::tool::contract::ToolEffectClass::ReadOnly => "read_only",
            crate::tool::contract::ToolEffectClass::ReadValidate => "read_validate",
            crate::tool::contract::ToolEffectClass::SafeRepeat => "safe_repeat",
            crate::tool::contract::ToolEffectClass::IdempotentMutating => "idempotent_mutating",
            crate::tool::contract::ToolEffectClass::NonIdempotent => "non_idempotent",
            crate::tool::contract::ToolEffectClass::ProcessExec => "process_exec",
        }
        .into(),
        idempotency: match contract.idempotency {
            crate::tool::contract::IdempotencyClass::Idempotent => "idempotent",
            crate::tool::contract::IdempotencyClass::NonIdempotent => "non_idempotent",
        }
        .into(),
        input_schema_digest: format!("sha256:{}", stable_digest(&input_schema)),
        output_schema_digest: format!("sha256:{}", stable_digest(&output_schema)),
    })
}

/// M014-B1: Resolve the frozen contract snapshot for the requested tools
/// from the production Broker catalog. Returns `Err` if any requested tool
/// is missing, direct-only, mutation-capable, or schema-incomplete.
pub fn resolve_contract_snapshot(
    broker: &crate::tool::broker::ToolBroker,
    allowed_tools: &[String],
) -> Result<Vec<ContractEntry>, String> {
    if allowed_tools.is_empty() {
        return Err("Tool Programs require at least one frozen runtime contract".into());
    }
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
        entries.push(contract_entry(contract)?);
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
    _program_id: &str,
    allowed_tools: &[String],
    source_digest: &str,
    ir_digest: &str,
    contract_digest: &str,
) -> Result<codegg_core::jobs::ToolAuthorityGrant, String> {
    let exec_ctx = exec_ctx.ok_or_else(|| "accepted permission decision is missing".to_string())?;
    if exec_ctx.decision_outcome.as_deref() != Some("allowed") {
        return Err("accepted permission decision is missing".into());
    }
    let required = |value: &Option<String>, name: &str| {
        value
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .ok_or_else(|| format!("accepted permission decision is missing {name}"))
    };
    let grant_id = required(&exec_ctx.authority_ref, "decision identity")?;
    let principal_ref = required(&exec_ctx.principal_ref, "principal identity")?;
    let policy_revision = required(&exec_ctx.policy_revision, "permission policy revision")?;
    let issued_at = exec_ctx
        .decision_issued_at
        .ok_or_else(|| "accepted permission decision is missing issued_at".to_string())?;

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
    let grant = codegg_core::jobs::ToolAuthorityGrant {
        schema_version: 1,
        grant_id,
        principal_ref,
        workspace_id: workspace_id.to_string(),
        workspace_path_policy_id: exec_ctx.workspace_path_policy_id.clone(),
        session_id: exec_ctx.session_id.clone(),
        agent_id: exec_ctx.agent_id.clone(),
        turn_id: exec_ctx.turn_id.clone(),
        permission_mode: exec_ctx.permission_mode.clone(),
        policy_revision,
        allowed_caller_class: "program".into(),
        allowed_effect_class: "read_only".into(),
        manifest_digest,
        source_digest: source_digest.to_string(),
        ir_digest: ir_digest.to_string(),
        contract_digest: contract_digest.to_string(),
        contract_snapshot_json: exec_ctx.contract_snapshot_json.clone(),
        issued_at,
        expires_at: exec_ctx.decision_expires_at,
        revoked_at: exec_ctx.decision_revoked_at,
        decision_digest: String::new(),
    };
    // M013-A2 / C-04: compute the digest over every security-relevant
    // field so any later tamper fails `verify_integrity()`.
    let decision_digest = grant.compute_digest();
    Ok(codegg_core::jobs::ToolAuthorityGrant {
        decision_digest,
        ..grant
    })
}
