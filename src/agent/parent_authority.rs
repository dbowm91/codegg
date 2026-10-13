//! Host-attested parent tool authority for descendant agent turns.
//!
//! M003 (agent-tool-runtime-reconciliation): a subagent's eligible tool
//! names *and* typed capabilities are a subset of its parent turn's
//! **effective** eligible names/effects. The existing
//! [`ResolvedToolSurface`](super::tool_surface::ResolvedToolSurface)
//! intersection API accepts an optional capability ceiling, but every
//! production child builder passed `None`, and a capability bit alone
//! cannot express an exact-name restriction (two tools may share
//! `FilesystemRead` or `PluginExecute`).
//!
//! This module is the single owner of the descendant ceiling:
//!
//! - [`ParentToolAuthoritySnapshot`] is minted **only** by host code from
//!   the parent turn's already-resolved effective surface
//!   ([`AgentLoop::mint_delegation_ceiling`]), never deserialized from
//!   model tool arguments. Model input JSON has no field for it, and the
//!   static guard (`scripts/check_descendant_tool_ceiling.py`) rejects any
//!   read of a model-arg authority field.
//! - The snapshot travels parent → child through [`TaskTool`](crate::tool::task::TaskTool)
//!   execution context (host side) into
//!   [`SubAgentRequest`](super::worker::SubAgentRequest) and, for
//!   scheduler-owned runs, through the additive `parent_authority` JSON
//!   string on `JobPayload::Subagent`/`SubagentRun` (decoded host side;
//!   corrupt or absent-on-model-path payloads fail closed).
//! - The worker validates the snapshot (integrity fingerprint, expiry,
//!   workspace scope) and enforces it twice: exact-name filtering of the
//!   child registry plus
//!   [`ResolvedToolSurface::resolve_with_parent_authority`], and broker
//!   admission via `BrokerInvocationContext.allowed_tools` (the manifest
//!   check) for every child tool call.
//! - Nested children re-narrow through [`narrow_for_child`](ParentToolAuthoritySnapshot::narrow_for_child),
//!   so the ancestry upper bound is retained transitively without ever
//!   broadening.
//!
//! Explicit root / no-parent system delegations (research evidence,
//! security review, user-initiated collaboration tasks, legacy scheduler
//! payloads without the additive field) carry `None`. That is a distinct,
//! permitted host-owned delegation kind — never an unrestricted default:
//! the worker applies the existing denied/filtered registry behavior for
//! those paths, and the model-driven `task` tool path fails closed when
//! no snapshot is available.

use super::tool_surface::{AgentCapabilitySet, ResolvedToolSurface};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Bounded time-to-live for one delegation snapshot.
///
/// Child turns are short-lived; a stale snapshot must fail closed rather
/// than authorize indefinitely. Revocation narrows *future* delegations
/// immediately because the ceiling is re-minted from live policy at each
/// `task` invocation — the snapshot itself stays immutable.
pub const AUTHORITY_TTL_MS: i64 = 24 * 60 * 60 * 1000;

/// Bounded deny reasons emitted at the descendant boundary.
///
/// Fingerprints only — never policy payloads, credentials, paths, or
/// tool arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentDenyReason {
    /// The canonical name is outside the pinned parent eligible set.
    ParentCeiling,
    /// The pinned MCP generation no longer matches the live catalog.
    StaleGeneration,
    /// The tool contract effect exceeds the parent max-effect envelope.
    ForbiddenEffect,
    /// Fingerprint mismatch, scope mismatch, expiry, or undecodable state.
    InvalidParentReceipt,
    /// No snapshot is available on the model-driven delegation path.
    MissingAuthority,
}

impl ParentDenyReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ParentCeiling => "ParentCeiling",
            Self::StaleGeneration => "StaleGeneration",
            Self::ForbiddenEffect => "ForbiddenEffect",
            Self::InvalidParentReceipt => "InvalidParentReceipt",
            Self::MissingAuthority => "MissingAuthority",
        }
    }
}

impl std::fmt::Display for ParentDenyReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Immutable, host-minted ceiling for one direct delegation.
///
/// All fields are minted from daemon/loop-owned state. The struct is
/// `Serialize`/`Deserialize` only for host-persisted transport (scheduler
/// job payload JSON, in-memory spawner channels) — it is never populated
/// from model tool-call arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParentToolAuthoritySnapshot {
    /// Session that owns the parent turn.
    pub parent_session_id: String,
    /// Exact daemon-owned turn identity, when available.
    pub parent_turn_id: Option<String>,
    /// Durable run identity of the parent, when scheduler-owned.
    pub parent_run_id: Option<String>,
    /// Acting agent name that produced the parent surface.
    pub parent_agent: String,
    /// Workspace scope the ceiling is bound to (workspace root path).
    pub workspace_scope: String,
    /// Exact eligible canonical tool names (parent-effective).
    pub eligible_tools: BTreeSet<String>,
    /// Typed capability ceiling (parent-effective).
    pub capabilities: AgentCapabilitySet,
    /// Fingerprint of the parent resolved surface (diagnostics + binding).
    pub surface_fingerprint: String,
    /// Pinned MCP catalog generation (`McpService::catalog_fingerprint`;
    /// empty when no service backs the parent turn).
    pub mcp_generation: String,
    /// Deterministic plugin activation revision at mint time.
    pub plugin_activation_rev: String,
    /// Permission policy revision at mint time (`config:{hash}` scheme).
    pub policy_revision: String,
    /// Maximum admitted effect class (wire spelling, e.g. `non_idempotent`).
    pub max_effect_class: String,
    /// Mint time (millis since epoch).
    pub issued_at_ms: i64,
    /// Hard expiry (millis since epoch; `issued_at_ms + AUTHORITY_TTL_MS`).
    pub expires_at_ms: i64,
    /// Integrity fingerprint over every field above. Recomputed on every
    /// validation; any host-external mutation fails closed.
    pub snapshot_fingerprint: String,
}

/// Inputs for [`ParentToolAuthoritySnapshot::mint`]. Every input must come
/// from host-owned state (resolved parent surface, daemon identities,
/// live generations, current policy revision, wall clock).
pub struct ParentAuthorityMintInput {
    pub parent_session_id: String,
    pub parent_turn_id: Option<String>,
    pub parent_run_id: Option<String>,
    pub parent_agent: String,
    pub workspace_scope: String,
    pub surface: ResolvedToolSurface,
    pub mcp_generation: String,
    pub plugin_activation_rev: String,
    pub policy_revision: String,
    pub max_effect_class: String,
    pub now_ms: i64,
}

impl ParentToolAuthoritySnapshot {
    /// Host-only constructor. Takes the parent turn's already-resolved
    /// effective surface and binds it to the delegation scope.
    pub fn mint(input: ParentAuthorityMintInput) -> Self {
        let eligible_tools = input
            .surface
            .tools
            .iter()
            .map(|tool| tool.canonical_name.clone())
            .collect::<BTreeSet<_>>();
        let mut snapshot = Self {
            parent_session_id: input.parent_session_id,
            parent_turn_id: input.parent_turn_id,
            parent_run_id: input.parent_run_id,
            parent_agent: input.parent_agent,
            workspace_scope: input.workspace_scope,
            eligible_tools,
            capabilities: input.surface.capabilities,
            surface_fingerprint: input.surface.fingerprint,
            mcp_generation: input.mcp_generation,
            plugin_activation_rev: input.plugin_activation_rev,
            policy_revision: input.policy_revision,
            max_effect_class: input.max_effect_class,
            issued_at_ms: input.now_ms,
            expires_at_ms: input.now_ms.saturating_add(AUTHORITY_TTL_MS),
            snapshot_fingerprint: String::new(),
        };
        snapshot.snapshot_fingerprint = snapshot.compute_fingerprint();
        snapshot
    }

    fn fingerprint_material(&self) -> String {
        let mut material = String::new();
        material.push_str(&self.parent_session_id);
        material.push('\0');
        material.push_str(self.parent_turn_id.as_deref().unwrap_or(""));
        material.push('\0');
        material.push_str(self.parent_run_id.as_deref().unwrap_or(""));
        material.push('\0');
        material.push_str(&self.parent_agent);
        material.push('\0');
        material.push_str(&self.workspace_scope);
        material.push('\0');
        for name in &self.eligible_tools {
            material.push_str(name);
            material.push('\0');
        }
        material.push_str(&format!("caps:{:?}", self.capabilities.capabilities()));
        material.push('\0');
        material.push_str(&self.surface_fingerprint);
        material.push('\0');
        material.push_str(&self.mcp_generation);
        material.push('\0');
        material.push_str(&self.plugin_activation_rev);
        material.push('\0');
        material.push_str(&self.policy_revision);
        material.push('\0');
        material.push_str(&self.max_effect_class);
        material.push('\0');
        material.push_str(&self.issued_at_ms.to_string());
        material.push('\0');
        material.push_str(&self.expires_at_ms.to_string());
        material
    }

    fn compute_fingerprint(&self) -> String {
        format!(
            "sha256:{:x}",
            Sha256::digest(self.fingerprint_material().as_bytes())
        )
    }

    /// Integrity check only (no scope or time evaluation).
    pub fn verify_integrity(&self) -> bool {
        !self.snapshot_fingerprint.is_empty()
            && self.compute_fingerprint() == self.snapshot_fingerprint
    }

    /// Full delegation-time validation: integrity, expiry, workspace scope.
    /// The policy revision is advisory (revocation narrows future mints;
    /// an in-flight snapshot stays immutable) and is reported through
    /// diagnostics, not enforced here.
    pub fn validate(&self, now_ms: i64, workspace_scope: &str) -> Result<(), ParentDenyReason> {
        if !self.verify_integrity() {
            return Err(ParentDenyReason::InvalidParentReceipt);
        }
        if now_ms > self.expires_at_ms {
            return Err(ParentDenyReason::InvalidParentReceipt);
        }
        if self.workspace_scope != workspace_scope {
            return Err(ParentDenyReason::InvalidParentReceipt);
        }
        Ok(())
    }

    /// Invocation-time check for one canonical tool name plus its contract
    /// effect class (wire spelling, e.g. `read_only`). Exact-name
    /// membership and the max-effect envelope are enforced independently:
    /// a tool sharing a capability bit with an eligible tool is still
    /// denied when its name is absent, and an eligible name with a widened
    /// contract effect is still denied.
    pub fn check_tool(
        &self,
        canonical_name: &str,
        contract_effect: &str,
        now_ms: i64,
        workspace_scope: &str,
    ) -> Result<(), ParentDenyReason> {
        self.validate(now_ms, workspace_scope)?;
        if !self.eligible_tools.contains(canonical_name) {
            return Err(ParentDenyReason::ParentCeiling);
        }
        let ceiling =
            crate::tool::contract::ToolEffectClass::from_authority_str(&self.max_effect_class)
                .ok_or(ParentDenyReason::ForbiddenEffect)?;
        let effect = crate::tool::contract::ToolEffectClass::from_authority_str(contract_effect)
            .ok_or(ParentDenyReason::ForbiddenEffect)?;
        if !effect.is_within_ceiling(ceiling) {
            return Err(ParentDenyReason::ForbiddenEffect);
        }
        Ok(())
    }

    /// Narrow this snapshot to a directly delegated child whose effective
    /// surface has been resolved under it. Used by the worker for nested
    /// delegation: the child-effective set can only shrink, so the full
    /// ancestry upper bound is retained transitively. Scope, generations,
    /// policy, envelope, and expiry are inherited unchanged.
    pub fn narrow_for_child(&self, child_surface: &ResolvedToolSurface) -> Self {
        let child_names = child_surface
            .tools
            .iter()
            .map(|tool| tool.canonical_name.clone())
            .collect::<BTreeSet<_>>();
        let eligible_tools = self
            .eligible_tools
            .intersection(&child_names)
            .cloned()
            .collect::<BTreeSet<_>>();
        let capabilities = self.capabilities.intersect(child_surface.capabilities);
        let mut narrowed = Self {
            parent_session_id: self.parent_session_id.clone(),
            parent_turn_id: self.parent_turn_id.clone(),
            parent_run_id: self.parent_run_id.clone(),
            parent_agent: self.parent_agent.clone(),
            workspace_scope: self.workspace_scope.clone(),
            eligible_tools,
            capabilities,
            surface_fingerprint: child_surface.fingerprint.clone(),
            mcp_generation: self.mcp_generation.clone(),
            plugin_activation_rev: self.plugin_activation_rev.clone(),
            policy_revision: self.policy_revision.clone(),
            max_effect_class: self.max_effect_class.clone(),
            issued_at_ms: self.issued_at_ms,
            expires_at_ms: self.expires_at_ms,
            snapshot_fingerprint: String::new(),
        };
        narrowed.snapshot_fingerprint = narrowed.compute_fingerprint();
        narrowed
    }

    /// Sorted eligible names for broker `allowed_tools` binding and bounded
    /// diagnostics. The broker manifest check denies any child tool call
    /// whose canonical name is absent here — including tools injected
    /// after child launch.
    pub fn allowed_tools_sorted(&self) -> Vec<String> {
        self.eligible_tools.iter().cloned().collect()
    }
}

/// Generation-freshness check for the pinned MCP catalog generation
/// against the live catalog at child-build time.
///
/// A pinned non-empty generation that disagrees with a live non-empty
/// one fails closed ([`ParentDenyReason::StaleGeneration`]): the tool
/// set the parent saw is no longer the tool set present. An empty
/// pinned generation (no service backed the parent turn) proceeds —
/// exact-name filtering still removes any tool the parent never saw —
/// as does an empty live generation (service momentarily unreadable;
/// the pinned adapters fail closed individually at invocation).
pub fn check_generation_fresh(pinned: &str, live: &str) -> Result<(), ParentDenyReason> {
    if !pinned.is_empty() && !live.is_empty() && pinned != live {
        return Err(ParentDenyReason::StaleGeneration);
    }
    Ok(())
}

/// Encode a snapshot for host-persisted transport (scheduler job payload).
/// Encoding failures fail closed at the call site.
pub fn encode_parent_authority(snapshot: &ParentToolAuthoritySnapshot) -> Result<String, String> {
    serde_json::to_string(snapshot)
        .map_err(|error| format!("parent authority encode failed: {error}"))
}

/// Decode host-persisted transport. Any failure (corrupt, tampered,
/// wrong shape) fails closed at the call site — never a silent `None`.
pub fn decode_parent_authority(
    encoded: &str,
) -> Result<ParentToolAuthoritySnapshot, ParentDenyReason> {
    serde_json::from_str(encoded).map_err(|_| ParentDenyReason::InvalidParentReceipt)
}

#[cfg(test)]
mod tests {
    use super::super::tool_surface::{ResolvedToolSurface, ToolOmissionReason};
    use super::*;
    use crate::provider::ToolDefinition;
    use std::collections::BTreeSet;

    fn def(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.into(),
            description: name.into(),
            parameters: serde_json::json!({"type":"object"}),
            defer_loading: None,
        }
    }

    fn parent_surface() -> ResolvedToolSurface {
        ResolvedToolSurface::resolve(
            [def("read"), def("write"), def("git_query")],
            &BTreeSet::new(),
            &BTreeSet::new(),
            false,
            false,
            None,
        )
        .unwrap()
    }

    fn mint_parent() -> ParentToolAuthoritySnapshot {
        ParentToolAuthoritySnapshot::mint(ParentAuthorityMintInput {
            parent_session_id: "session-1".into(),
            parent_turn_id: Some("turn-1".into()),
            parent_run_id: None,
            parent_agent: "build".into(),
            workspace_scope: "/workspace".into(),
            surface: parent_surface(),
            mcp_generation: "mcp-gen-1".into(),
            plugin_activation_rev: "plugins-a".into(),
            policy_revision: "config:1".into(),
            max_effect_class: "non_idempotent".into(),
            now_ms: 1_000,
        })
    }

    #[test]
    fn mint_validate_roundtrip() {
        let snapshot = mint_parent();
        assert!(snapshot.verify_integrity());
        assert!(snapshot.validate(2_000, "/workspace").is_ok());
        assert_eq!(snapshot.eligible_tools.len(), 3);
    }

    #[test]
    fn tampered_snapshot_fails_closed() {
        let mut snapshot = mint_parent();
        snapshot.eligible_tools.insert("task".into());
        assert!(!snapshot.verify_integrity());
        assert_eq!(
            snapshot.validate(2_000, "/workspace"),
            Err(ParentDenyReason::InvalidParentReceipt)
        );
        // Forged fingerprint field is equally rejected.
        let mut forged = mint_parent();
        forged.snapshot_fingerprint = "sha256:0".into();
        assert!(!forged.verify_integrity());
    }

    #[test]
    fn expired_or_wrong_workspace_fails_closed() {
        let snapshot = mint_parent();
        assert_eq!(
            snapshot.validate(snapshot.expires_at_ms + 1, "/workspace"),
            Err(ParentDenyReason::InvalidParentReceipt)
        );
        assert_eq!(
            snapshot.validate(2_000, "/other"),
            Err(ParentDenyReason::InvalidParentReceipt)
        );
    }

    #[test]
    fn exact_name_and_effect_enforced_independently() {
        let snapshot = mint_parent();
        // Eligible name + envelope: allowed.
        assert!(snapshot
            .check_tool("read", "read_only", 2_000, "/workspace")
            .is_ok());
        // Ineligible name even though its capability bit is set: denied.
        assert_eq!(
            snapshot.check_tool("write_file", "non_idempotent", 2_000, "/workspace"),
            Err(ParentDenyReason::ParentCeiling)
        );
        // Eligible name with a widened contract effect: denied.
        let mut narrow = mint_parent();
        narrow.max_effect_class = "read_only".into();
        narrow.snapshot_fingerprint = narrow.compute_fingerprint();
        assert_eq!(
            narrow.check_tool("write", "non_idempotent", 2_000, "/workspace"),
            Err(ParentDenyReason::ForbiddenEffect)
        );
        // Unknown effect spelling fails closed, never widens.
        assert_eq!(
            snapshot.check_tool("read", "bogus", 2_000, "/workspace"),
            Err(ParentDenyReason::ForbiddenEffect)
        );
    }

    #[test]
    fn narrow_for_child_only_shrinks() {
        let parent = mint_parent();
        let child_surface = ResolvedToolSurface::resolve(
            [def("read"), def("write"), def("task")],
            &BTreeSet::new(),
            &BTreeSet::new(),
            false,
            true,
            None,
        )
        .unwrap();
        let narrowed = parent.narrow_for_child(&child_surface);
        assert!(narrowed.verify_integrity());
        // `task` was never parent-eligible, so it cannot be resurrected.
        assert!(!narrowed.eligible_tools.contains("task"));
        assert!(narrowed.eligible_tools.contains("read"));
        assert!(narrowed.eligible_tools.contains("write"));
        // Scope, envelope, and expiry are inherited unchanged.
        assert_eq!(narrowed.workspace_scope, parent.workspace_scope);
        assert_eq!(narrowed.max_effect_class, parent.max_effect_class);
        assert_eq!(narrowed.expires_at_ms, parent.expires_at_ms);
        // Double narrowing is monotonic.
        let again = narrowed.narrow_for_child(&child_surface);
        assert_eq!(again.eligible_tools, narrowed.eligible_tools);
    }

    #[test]
    fn generation_freshness_matrix() {
        use super::check_generation_fresh;
        assert!(check_generation_fresh("a", "a").is_ok());
        assert_eq!(
            check_generation_fresh("a", "b"),
            Err(ParentDenyReason::StaleGeneration)
        );
        // Empty pinned (no service at mint): exact-name filtering below
        // still removes unseen tools, so launch proceeds.
        assert!(check_generation_fresh("", "b").is_ok());
        // Empty live (momentarily unreadable): pinned adapters fail
        // closed individually at invocation.
        assert!(check_generation_fresh("a", "").is_ok());
        assert!(check_generation_fresh("", "").is_ok());
    }

    #[test]
    fn encode_decode_roundtrip_and_corrupt_fails_closed() {
        let snapshot = mint_parent();
        let encoded = encode_parent_authority(&snapshot).unwrap();
        let decoded = decode_parent_authority(&encoded).unwrap();
        assert_eq!(decoded, snapshot);
        assert_eq!(
            decode_parent_authority("{not json"),
            Err(ParentDenyReason::InvalidParentReceipt)
        );
        // Legacy payloads without the field decode as None at the
        // `Option<String>` layer, not here — documented in task payloads.
    }

    #[test]
    fn omission_reason_parent_ceiling_exists() {
        assert_eq!(
            ToolOmissionReason::ParentCeiling,
            ToolOmissionReason::ParentCeiling
        );
    }
}
