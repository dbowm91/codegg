//! Immutable authorization state carried from the agent decision boundary to
//! the execution owner.

use crate::permission::{approval::ExecutionPolicySnapshot, PermissionDecisionReceipt};

/// A bounded summary of command preflight already evaluated for this request.
/// `Some` means the check was run; an empty summary is an affirmative clean
/// result. It is advisory and never grants or denies resource authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPreflightResult {
    /// True only when the configured preflight caller policy requires refusal.
    /// Findings by themselves remain advisory and never confer authority.
    pub blocked: bool,
    pub summary: Option<String>,
}

/// Authorization facts accepted for one logical tool invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionAuthorizationContext {
    pub snapshot: ExecutionPolicySnapshot,
    pub receipt: PermissionDecisionReceipt,
    pub command_preflight: Option<CommandPreflightResult>,
    /// Host-authorized durable run scratch. Never accepted from tool arguments.
    pub scratch_scope: Option<crate::scratch::AgentScratchScope>,
}
