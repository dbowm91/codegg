//! Immutable authorization state carried from the agent decision boundary to
//! the execution owner.

use crate::permission::PermissionLevel;
use crate::permission::{approval::ExecutionPolicySnapshot, PermissionDecisionReceipt};
use codegg_core::approval::SandboxProfile;

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

/// Specialized command routes are currently qualified only for an explicitly
/// accepted FullHost snapshot. Constrained requests must use a route that
/// carries the exact accepted snapshot into its enforcement owner; Test and
/// Python job executors do not yet do so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecializedExecutionRoute {
    BashIntent,
    Test,
    Python,
}

/// Validate the immutable accepted authority before dispatching a specialized
/// subprocess route. This is deliberately independent of argv, classifier
/// labels, and the calling tool's name.
pub fn admit_full_host_specialized_route(
    authorization: Option<&ExecutionAuthorizationContext>,
    route: SpecializedExecutionRoute,
) -> Result<(), SpecializedRouteUnavailable> {
    let snapshot = accepted_execution_snapshot(authorization)?;
    if snapshot.sandbox_profile() != SandboxProfile::FullHost {
        return Err(SpecializedRouteUnavailable::ConstrainedBackendUnavailable {
            route,
            profile: snapshot.sandbox_profile(),
        });
    }
    Ok(())
}

/// Validate the accepted receipt and return its immutable policy snapshot.
/// Specialized routes may then apply a narrower, route-specific profile
/// ceiling before passing this snapshot to the scheduler and process owner.
pub fn accepted_execution_snapshot(
    authorization: Option<&ExecutionAuthorizationContext>,
) -> Result<&ExecutionPolicySnapshot, SpecializedRouteUnavailable> {
    let Some(authorization) = authorization else {
        return Err(SpecializedRouteUnavailable::MissingAuthorization);
    };
    let snapshot = &authorization.snapshot;
    let receipt = &authorization.receipt;
    if receipt.outcome != PermissionLevel::Allow
        || receipt.decision_id.trim().is_empty()
        || receipt.policy_revision.as_deref() != snapshot.policy_revision()
    {
        return Err(SpecializedRouteUnavailable::InvalidAuthorization);
    }
    Ok(snapshot)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecializedRouteUnavailable {
    MissingAuthorization,
    InvalidAuthorization,
    ConstrainedBackendUnavailable {
        route: SpecializedExecutionRoute,
        profile: SandboxProfile,
    },
}

impl std::fmt::Display for SpecializedRouteUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingAuthorization => f.write_str("accepted execution authorization is missing"),
            Self::InvalidAuthorization => f.write_str("accepted execution authorization is invalid"),
            Self::ConstrainedBackendUnavailable { route, profile } => match route {
                SpecializedExecutionRoute::Test => write!(
                    f,
                    "typed Test cannot enforce accepted {profile:?} profile; use managed Bash for an equivalent test command"
                ),
                SpecializedExecutionRoute::Python => write!(
                    f,
                    "typed Python cannot enforce accepted {profile:?} profile; no equivalent constrained route is qualified"
                ),
                SpecializedExecutionRoute::BashIntent => write!(
                    f,
                    "active Bash intent route cannot enforce accepted {profile:?} profile; use the managed shell route where equivalent"
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::PermissionDecisionReceipt;
    use codegg_core::approval::{ApprovalMode, ExecutionPolicySnapshot};

    fn accepted(profile: SandboxProfile, matching_revision: bool) -> ExecutionAuthorizationContext {
        let revision = Some("policy-r1".to_owned());
        ExecutionAuthorizationContext {
            snapshot: ExecutionPolicySnapshot::capture(
                ApprovalMode::Interactive,
                profile,
                Some("principal".into()),
                Some("session".into()),
                None,
                revision.clone(),
                None,
            ),
            receipt: PermissionDecisionReceipt {
                decision_id: "decision-1".into(),
                outcome: PermissionLevel::Allow,
                source: "fixture".into(),
                issued_at: 1,
                policy_revision: matching_revision.then_some(revision).flatten(),
            },
            command_preflight: None,
        }
    }

    #[test]
    fn specialized_admission_requires_matching_accepted_full_host_authority() {
        let full_host = accepted(SandboxProfile::FullHost, true);
        assert!(admit_full_host_specialized_route(
            Some(&full_host),
            SpecializedExecutionRoute::Test
        )
        .is_ok());

        let constrained = accepted(SandboxProfile::WorkspaceWrite, true);
        assert!(matches!(
            admit_full_host_specialized_route(
                Some(&constrained),
                SpecializedExecutionRoute::Python
            ),
            Err(SpecializedRouteUnavailable::ConstrainedBackendUnavailable { .. })
        ));
        assert!(matches!(
            admit_full_host_specialized_route(None, SpecializedExecutionRoute::Test),
            Err(SpecializedRouteUnavailable::MissingAuthorization)
        ));
        let stale = accepted(SandboxProfile::FullHost, false);
        assert!(matches!(
            admit_full_host_specialized_route(Some(&stale), SpecializedExecutionRoute::Test),
            Err(SpecializedRouteUnavailable::InvalidAuthorization)
        ));
        let mut denied = accepted(SandboxProfile::FullHost, true);
        denied.receipt.outcome = PermissionLevel::Deny;
        assert!(matches!(
            admit_full_host_specialized_route(Some(&denied), SpecializedExecutionRoute::Test),
            Err(SpecializedRouteUnavailable::InvalidAuthorization)
        ));
    }
}
