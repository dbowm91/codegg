//! Authority narrowing and projection adapters owned by authorization.

use super::{
    is_local_owner_broad, now_millis, AuthorizationDecision, AuthorizationError,
    AuthorizationRequest, AuthorizationService, Capability, CapabilitySet, OperationDescriptor,
    PolicyKind, ProjectRole, ScopeKind,
};
use crate::identity::ProjectId;
use crate::projection_replay::context::{
    BoundedProjectResolver, ProjectionCapability, ProjectionCapabilitySet,
};
use crate::provider_connections::ProviderScope;
use crate::team::PrincipalStatus;
use crate::transport_auth::AuthenticatedPrincipal;

/// Intersect `child` with `parent`: the effective authority of a
/// delegated agent, tool call, or sub-job.
///
/// Authority can only narrow. [`child_escalates`] detects the negative
/// case; [`authorize_child_delegation`] enforces it.
pub fn narrow_authority(parent: &CapabilitySet, child: &CapabilitySet) -> CapabilitySet {
    parent
        .iter()
        .filter(|capability| child.has(*capability))
        .collect()
}

/// `true` when `child` requests any capability outside `parent`.
pub fn child_escalates(parent: &CapabilitySet, child: &CapabilitySet) -> bool {
    child.iter().any(|capability| !parent.has(capability))
}

/// Enforce delegation narrowing: return the narrowed set, or fail closed
/// when the child requests authority beyond the parent decision.
pub fn authorize_child_delegation(
    parent: &CapabilitySet,
    child: &CapabilitySet,
) -> Result<CapabilitySet, AuthorizationError> {
    if child_escalates(parent, child) {
        return Err(AuthorizationError::Denied {
            operation: "child_delegation",
            capability: "narrowed_parent_authority",
        });
    }
    Ok(narrow_authority(parent, child))
}

/// Check that `principal` may use a provider connection with `scope`.
///
/// - Personal connections are owner-only.
/// - Project connections require `capability` on the scoped project.
/// - Deployment connections require the local-owner broad policy or an
///   Owner grant in at least one project (conservative: deployment scope
///   is daemon-wide).
pub async fn authorize_provider_use(
    service: &AuthorizationService,
    principal: &AuthenticatedPrincipal,
    scope: &ProviderScope,
    capability: Capability,
    correlation_id: &str,
) -> Result<AuthorizationDecision, AuthorizationError> {
    if is_local_owner_broad(principal) {
        return Ok(AuthorizationDecision {
            principal_id: principal.principal_id().clone(),
            operation: "provider_connection_use".to_owned(),
            capability: Some(capability.as_str().to_owned()),
            project_id: None,
            membership_revision: None,
            policy: PolicyKind::LocalOwnerBroad,
            decision_id: uuid::Uuid::new_v4().to_string(),
            correlation_id: correlation_id.to_owned(),
            reason: "local-owner broad local policy".to_owned(),
            decided_at_ms: now_millis(),
        });
    }
    match scope {
        ProviderScope::Personal { owner } => {
            if owner != principal.principal_id() {
                return Err(AuthorizationError::Denied {
                    operation: "provider_connection_use",
                    capability: capability.as_str(),
                });
            }
            // Ownership is itself the grant; the principal must still be active.
            let record = service
                .team
                .get_principal(principal.principal_id())
                .await?
                .ok_or(AuthorizationError::PrincipalNotActive)?;
            if record.status != PrincipalStatus::Active {
                return Err(AuthorizationError::DisabledPrincipal);
            }
            Ok(AuthorizationDecision {
                principal_id: principal.principal_id().clone(),
                operation: "provider_connection_use".to_owned(),
                capability: Some(capability.as_str().to_owned()),
                project_id: None,
                membership_revision: Some(record.revision),
                policy: PolicyKind::TeamMembership,
                decision_id: uuid::Uuid::new_v4().to_string(),
                correlation_id: correlation_id.to_owned(),
                reason: "personal connection owner".to_owned(),
                decided_at_ms: now_millis(),
            })
        }
        ProviderScope::Project { project_id } => {
            let descriptor = OperationDescriptor::new(
                "provider_connection_use",
                ScopeKind::DirectProject,
                Some(capability),
            );
            let request = AuthorizationRequest::new(
                principal.clone(),
                descriptor,
                Some(project_id.clone()),
                correlation_id,
            );
            service.authorize(&request).await
        }
        ProviderScope::Deployment { .. } => {
            let memberships = service
                .team
                .list_memberships_for_principal(principal.principal_id())
                .await?;
            let owner_grant = memberships.iter().find(|membership| {
                membership.role == ProjectRole::Owner
                    && membership.state == crate::team::MembershipState::Active
            });
            match owner_grant {
                Some(membership) => Ok(AuthorizationDecision {
                    principal_id: principal.principal_id().clone(),
                    operation: "provider_connection_use".to_owned(),
                    capability: Some(capability.as_str().to_owned()),
                    project_id: Some(membership.project_id.clone()),
                    membership_revision: Some(membership.revision),
                    policy: PolicyKind::TeamMembership,
                    decision_id: uuid::Uuid::new_v4().to_string(),
                    correlation_id: correlation_id.to_owned(),
                    reason: "deployment scope via owner grant".to_owned(),
                    decided_at_ms: now_millis(),
                }),
                None => Err(AuthorizationError::Denied {
                    operation: "provider_connection_use",
                    capability: capability.as_str(),
                }),
            }
        }
    }
}

/// Map team capabilities onto projection-layer capabilities.
///
/// `project.observe`/`session.observe` open the corresponding projection
/// streams; read capabilities open artifact/tool/diff reads; local
/// diagnostics stay local-only. The mapping is conservative: unknown or
/// empty team sets yield an empty projection set.
pub fn team_capabilities_to_projection(caps: &CapabilitySet) -> ProjectionCapabilitySet {
    let mut out = Vec::new();
    if caps.has(Capability::ProjectObserve) {
        out.push(ProjectionCapability::ObservePublicProjection);
    }
    if caps.has(Capability::SessionObserve) || caps.has(Capability::SessionRead) {
        out.push(ProjectionCapability::ObserveSessionProjection);
    }
    if caps.has(Capability::ProjectObserve) || caps.has(Capability::SessionRead) {
        out.push(ProjectionCapability::ObserveClientLocal);
    }
    if caps.has(Capability::SessionRead) {
        out.push(ProjectionCapability::ReadRunArtifact);
        out.push(ProjectionCapability::ReadToolOutput);
    }
    if caps.has(Capability::FileRead) || caps.has(Capability::GitRead) {
        out.push(ProjectionCapability::ReadDiffOrLog);
    }
    ProjectionCapabilitySet::from_iter(out)
}

/// Bounded project resolver for the projects `principal` may read.
///
/// LocalOwner callers should keep the allow-all resolver; team
/// principals receive exactly the projects where they hold
/// `project.read`, so projection `authorize_scope` enforces the same
/// membership grants as the daemon boundary.
pub async fn bounded_resolver_for_principal(
    service: &AuthorizationService,
    principal: &AuthenticatedPrincipal,
    candidates: &[ProjectId],
) -> BoundedProjectResolver {
    let mut allowed = Vec::new();
    for project in candidates {
        let descriptor = OperationDescriptor::new(
            "projection_scope",
            ScopeKind::DirectProject,
            Some(Capability::ProjectRead),
        );
        let request = AuthorizationRequest::new(
            principal.clone(),
            descriptor,
            Some(project.clone()),
            "projection-resolver",
        );
        if service.authorize(&request).await.is_ok() {
            allowed.push(project.as_str().to_owned());
        }
    }
    BoundedProjectResolver::new(allowed)
}

/// Copy audit decision provenance from a gate-enforced decision (M004).
///
/// The daemon boundary calls this with the [`AuthorizationDecision`] it
/// just enforced, so the resulting
/// [`crate::audit::AuditDecisionProvenance`] carries the real decision
/// linkage into the append-only audit store. Instrumentation (M005) MUST
/// use this bridge rather than fabricating provenance: request payloads
/// supply locators but never authority.
pub fn audit_provenance(decision: &AuthorizationDecision) -> crate::audit::AuditDecisionProvenance {
    crate::audit::AuditDecisionProvenance::new(
        decision.decision_id.clone(),
        decision.correlation_id.clone(),
        decision.policy.as_str().to_owned(),
        decision.project_id.clone(),
    )
}

/// Filter `candidates` to the projects where `principal` holds
/// `project.read`.
///
/// Enumeration responses (project/session/job/schedule lists) must pass
/// through this filter so unauthorized callers cannot infer the
/// existence of projects they may not observe. LocalOwner broad policy
/// observes everything.
pub async fn visible_projects(
    service: &AuthorizationService,
    principal: &AuthenticatedPrincipal,
    candidates: &[ProjectId],
) -> Vec<ProjectId> {
    if is_local_owner_broad(principal) {
        return candidates.to_vec();
    }
    let mut visible = Vec::new();
    for project in candidates {
        if service
            .team
            .has_capability(project, principal.principal_id(), Capability::ProjectRead)
            .await
            .unwrap_or(false)
        {
            visible.push(project.clone());
        }
    }
    visible
}

/// Privacy-preserving denial shape for single-project reads.
///
/// Returns the same `(code, message)` the catalog returns for a
/// genuinely absent project, so a denial is indistinguishable from
/// non-existence.
pub fn denial_as_not_found() -> (&'static str, String) {
    ("project_not_found", "project not found".to_owned())
}
