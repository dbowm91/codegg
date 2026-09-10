//! Daemon authorization and originating-principal attribution (M003).
//!
//! M001 owns the durable principal/membership/capability domain in
//! [`crate::team`]. M002 owns transport authentication in
//! [`crate::transport_auth`]. This module owns the M003 decision layer:
//! mapping every native [`CoreRequest`] to a scope plus semantic
//! [`Capability`](crate::team::Capability), evaluating that capability
//! server-side against [`TeamStore`](crate::team::TeamStore) state, and
//! propagating the immutable originating principal into durable
//! attribution records.
//!
//! ## Design notes
//!
//! - Authorization occurs server-side. [`AuthorizationRequest`] is built
//!   only from the transport-bound [`AuthenticatedPrincipal`] plus request
//!   locators (project/session/job ids). Request DTOs supply locators but
//!   never capabilities: there is no constructor that accepts a
//!   caller-supplied principal, role, or capability.
//! - [`ProjectRole`](crate::team::ProjectRole) expansion stays in
//!   [`crate::team`]. Handler code asks for semantic capabilities through
//!   [`operation_descriptor`]; role interpretation never appears at call
//!   sites.
//! - [`LOCAL_OWNER broad policy`](is_local_owner_broad): the personal-local
//!   owner resolves through this same API and receives a broad local
//!   policy. It is an explicit policy composition, not a bypass: the
//!   decision is still constructed, still carries a policy marker, and
//!   still binds a correlation/decision id for attribution.
//! - Requests whose required capability has no resolvable project scope
//!   fail closed for team principals ([`AuthorizationError::MissingScope`]).
//!   Local-owner broad policy is the only path that authorizes without a
//!   project binding.
//! - Project enumeration itself is protected: [`visible_projects`] filters
//!   listings to `project.read` grants, and [`denial_as_not_found`] maps a
//!   single-project denial onto the same `project_not_found` shape as a
//!   genuinely absent project so unauthorized callers cannot infer
//!   existence.
//! - Effective agent/tool authority can only narrow. [`narrow_authority`]
//!   intersects parent and child capability sets; any child capability
//!   outside the parent is an escalation and fails closed.
//! - Pre-M003 durable records carry no canonical principal. They are
//!   attributed explicitly through [`OriginAttribution::legacy_local`],
//!   never by silently fabricating a team identity.
//!
//! ## Transport contract (M004/M005 own persistence of these decisions)
//!
//! Decisions are returned to the daemon boundary for enforcement and
//! attribution capture. The append-only audit store (M004) and its
//! instrumentation (M005) consume [`AuthorizationDecision`] and
//! [`OriginAttribution`]; this module never writes audit events itself.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(test)]
use sqlx::SqlitePool;

use crate::error::StorageError;
use crate::identity::{PrincipalId, ProjectId};
#[cfg(test)]
use crate::projection_replay::context::ProjectionCapability;
#[cfg(test)]
use crate::provider_connections::ProviderScope;
use crate::team::{PrincipalStatus, TeamStore, LOCAL_OWNER_PRINCIPAL_ID};
use crate::transport_auth::AuthenticatedPrincipal;

mod attribution;
mod authority;
mod policy;
pub(super) use attribution::now_millis;
pub use attribution::{OriginAttribution, OriginAttributionStore};
pub use authority::{
    audit_provenance, authorize_child_delegation, authorize_provider_use,
    bounded_resolver_for_principal, child_escalates, denial_as_not_found, narrow_authority,
    team_capabilities_to_projection, visible_projects,
};
pub use policy::{
    operation_capability_matrix, operation_descriptor, OperationDescriptor, ScopeKind,
};

/// Re-export so callers do not need a second import for principal kinds.
pub use crate::team::ProjectRole;
/// Re-exported capability vocabulary evaluated by the service.
pub use crate::team::{Capability, CapabilitySet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyKind {
    /// Personal-local owner resolved over a trusted local transport (or
    /// the bootstrap compatibility seam). Broad local policy, evaluated
    /// through this same API and carrying a decision id for attribution.
    LocalOwnerBroad,
    /// Team membership evaluated against current [`TeamStore`] state.
    TeamMembership,
}

impl PolicyKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalOwnerBroad => "local_owner_broad",
            Self::TeamMembership => "team_membership",
        }
    }
}

/// Server-side authorization request.
///
/// Constructed by the daemon transport from the connection-bound
/// [`AuthenticatedPrincipal`] plus request locators. There is deliberately
/// no constructor that accepts a caller-supplied principal, role, or
/// capability, so a spoofed payload principal cannot influence the
/// decision.
#[derive(Debug, Clone)]
pub struct AuthorizationRequest {
    principal: AuthenticatedPrincipal,
    operation: &'static str,
    scope_kind: ScopeKind,
    capability: Option<Capability>,
    project: Option<ProjectId>,
    correlation_id: String,
}

impl AuthorizationRequest {
    pub fn new(
        principal: AuthenticatedPrincipal,
        descriptor: OperationDescriptor,
        project: Option<ProjectId>,
        correlation_id: impl Into<String>,
    ) -> Self {
        Self {
            principal,
            operation: descriptor.operation,
            scope_kind: descriptor.scope_kind,
            capability: descriptor.capability,
            project,
            correlation_id: correlation_id.into(),
        }
    }

    pub fn principal(&self) -> &AuthenticatedPrincipal {
        &self.principal
    }

    pub fn principal_id(&self) -> &PrincipalId {
        self.principal.principal_id()
    }

    pub fn operation(&self) -> &'static str {
        self.operation
    }

    pub fn scope_kind(&self) -> ScopeKind {
        self.scope_kind
    }

    pub fn capability(&self) -> Option<Capability> {
        self.capability
    }

    pub fn project(&self) -> Option<&ProjectId> {
        self.project.as_ref()
    }

    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }
}

/// Successful server-side authorization decision.
///
/// The daemon enforces the decision before side effects and captures its
/// context (decision id, membership revision, policy) with the resulting
/// turn, run, or job attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationDecision {
    pub principal_id: PrincipalId,
    pub operation: String,
    pub capability: Option<String>,
    pub project_id: Option<ProjectId>,
    /// Membership revision observed at decision time. Callers bind
    /// long-lived operations to this revision so a concurrent revocation
    /// cannot be masked by a stale allow.
    pub membership_revision: Option<u64>,
    pub policy: PolicyKind,
    pub decision_id: String,
    pub correlation_id: String,
    pub reason: String,
    pub decided_at_ms: i64,
}

/// Structured authorization failure.
///
/// Denial codes and messages are typed for older-client compatibility
/// (`CoreResponse::Error { code, message }`) and contain no secret
/// material and no project-existence signal beyond the operation and
/// capability names (which the caller already supplied).
#[derive(Debug, Error)]
pub enum AuthorizationError {
    #[error("not authorized for {capability} on {operation}")]
    Denied {
        operation: &'static str,
        capability: &'static str,
    },
    #[error("operation {operation} requires a project scope")]
    MissingScope { operation: &'static str },
    #[error("operation {operation} has an ambiguous project scope")]
    AmbiguousScope { operation: &'static str },
    #[error("principal is disabled")]
    DisabledPrincipal,
    #[error("principal is not active")]
    PrincipalNotActive,
    #[error("authorization store is unavailable")]
    Unavailable(#[from] StorageError),
    #[error("team error: {0}")]
    Team(#[from] crate::team::TeamError),
}

impl AuthorizationError {
    /// Stable wire code for `CoreResponse::Error`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Denied { .. } => "authorization_denied",
            Self::MissingScope { .. } => "authorization_scope_required",
            Self::AmbiguousScope { .. } => "authorization_scope_ambiguous",
            Self::DisabledPrincipal | Self::PrincipalNotActive => {
                "authorization_principal_inactive"
            }
            Self::Unavailable(_) | Self::Team(_) => "authorization_unavailable",
        }
    }

    /// `true` for policy denials as opposed to infrastructure failures.
    pub fn is_denial(&self) -> bool {
        matches!(
            self,
            Self::Denied { .. }
                | Self::MissingScope { .. }
                | Self::AmbiguousScope { .. }
                | Self::DisabledPrincipal
                | Self::PrincipalNotActive
        )
    }
}

/// `true` when `principal` is the deterministic local owner.
///
/// LocalOwner principals are evaluated through the same
/// [`AuthorizationService`] API under the broad local policy; this
/// helper only names the composition so call sites stay readable.
pub fn is_local_owner_broad(principal: &AuthenticatedPrincipal) -> bool {
    principal.principal_id().as_str() == LOCAL_OWNER_PRINCIPAL_ID
}

/// Centralized daemon authorization service over M001 team state.
///
/// The service is the single authority that expands roles to
/// capabilities at request time. Handler code asks for semantic
/// capabilities via [`operation_descriptor`]; role interpretation never
/// appears at call sites.
#[derive(Clone)]
pub struct AuthorizationService {
    team: TeamStore,
}

impl AuthorizationService {
    pub fn new(team: TeamStore) -> Self {
        Self { team }
    }

    pub fn team(&self) -> &TeamStore {
        &self.team
    }

    /// Evaluate `request` against current team state.
    ///
    /// LocalOwner principals receive the broad local policy through this
    /// same entry point. Every other principal must be `Active`; global
    /// operations then allow, while project-scoped operations require a
    /// resolved project plus a current membership grant. Unknown
    /// principals, non-active memberships, and missing scopes all fail
    /// closed.
    pub async fn authorize(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<AuthorizationDecision, AuthorizationError> {
        let principal = request.principal();
        if is_local_owner_broad(principal) {
            return Ok(self.allow(
                request,
                None,
                PolicyKind::LocalOwnerBroad,
                "local-owner broad local policy",
            ));
        }
        let record = self
            .team
            .get_principal(request.principal_id())
            .await?
            .ok_or(AuthorizationError::PrincipalNotActive)?;
        if record.status != PrincipalStatus::Active {
            return Err(AuthorizationError::DisabledPrincipal);
        }
        let Some(capability) = request.capability() else {
            return Ok(self.allow(
                request,
                None,
                PolicyKind::TeamMembership,
                "global operation",
            ));
        };
        let Some(project) = request.project() else {
            return Err(AuthorizationError::MissingScope {
                operation: request.operation(),
            });
        };
        let membership = self
            .team
            .get_membership(project, request.principal_id())
            .await?;
        let Some(membership) = membership else {
            return Err(AuthorizationError::Denied {
                operation: request.operation(),
                capability: capability.as_str(),
            });
        };
        if !membership.has_capability(capability) {
            return Err(AuthorizationError::Denied {
                operation: request.operation(),
                capability: capability.as_str(),
            });
        }
        Ok(self.allow(
            request,
            Some(membership.revision),
            PolicyKind::TeamMembership,
            "team membership grant",
        ))
    }

    /// Authorize a bounded enumeration (listing) operation.
    ///
    /// Enumeration rows are privacy-filtered after the read (see
    /// [`visible_projects`]), so no project scope is required up front.
    /// The principal must still be active: unknown or disabled
    /// principals fail closed. LocalOwner uses the broad local policy.
    pub async fn authorize_enumeration(
        &self,
        principal: &AuthenticatedPrincipal,
        operation: &'static str,
        correlation_id: &str,
    ) -> Result<AuthorizationDecision, AuthorizationError> {
        if is_local_owner_broad(principal) {
            return Ok(AuthorizationDecision {
                principal_id: principal.principal_id().clone(),
                operation: operation.to_owned(),
                capability: None,
                project_id: None,
                membership_revision: None,
                policy: PolicyKind::LocalOwnerBroad,
                decision_id: uuid::Uuid::new_v4().to_string(),
                correlation_id: correlation_id.to_owned(),
                reason: "local-owner broad local policy".to_owned(),
                decided_at_ms: now_millis(),
            });
        }
        let record = self
            .team
            .get_principal(principal.principal_id())
            .await?
            .ok_or(AuthorizationError::PrincipalNotActive)?;
        if record.status != PrincipalStatus::Active {
            return Err(AuthorizationError::DisabledPrincipal);
        }
        Ok(AuthorizationDecision {
            principal_id: principal.principal_id().clone(),
            operation: operation.to_owned(),
            capability: None,
            project_id: None,
            membership_revision: None,
            policy: PolicyKind::TeamMembership,
            decision_id: uuid::Uuid::new_v4().to_string(),
            correlation_id: correlation_id.to_owned(),
            reason: "enumeration; rows filtered post-read".to_owned(),
            decided_at_ms: now_millis(),
        })
    }

    fn allow(
        &self,
        request: &AuthorizationRequest,
        membership_revision: Option<u64>,
        policy: PolicyKind,
        reason: &str,
    ) -> AuthorizationDecision {
        AuthorizationDecision {
            principal_id: request.principal_id().clone(),
            operation: request.operation().to_owned(),
            capability: request
                .capability()
                .map(Capability::as_str)
                .map(str::to_owned),
            project_id: request.project().cloned(),
            membership_revision,
            policy,
            decision_id: uuid::Uuid::new_v4().to_string(),
            correlation_id: request.correlation_id().to_owned(),
            reason: reason.to_owned(),
            decided_at_ms: now_millis(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::{MembershipState, PrincipalKind, ProjectRole};

    async fn test_service() -> (TeamStore, AuthorizationService) {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate test store");
        let team = TeamStore::new(pool);
        let service = AuthorizationService::new(team.clone());
        (team, service)
    }

    fn local_owner_request(
        operation: &'static str,
        capability: Option<Capability>,
        project: Option<ProjectId>,
    ) -> AuthorizationRequest {
        let principal = AuthenticatedPrincipal::local_owner("client-local");
        AuthorizationRequest::new(
            principal,
            OperationDescriptor::new(operation, ScopeKind::DirectProject, capability),
            project,
            "corr-1",
        )
    }

    fn remote_request(
        principal: AuthenticatedPrincipal,
        operation: &'static str,
        capability: Option<Capability>,
        project: Option<ProjectId>,
    ) -> AuthorizationRequest {
        AuthorizationRequest::new(
            principal,
            OperationDescriptor::new(operation, ScopeKind::DirectProject, capability),
            project,
            "corr-remote",
        )
    }

    async fn human_with_token(
        team: &TeamStore,
        name: &str,
        client: &str,
    ) -> AuthenticatedPrincipal {
        use crate::transport_auth::PersonalTokenStore;
        let record = team
            .create_principal(PrincipalKind::Human, name)
            .await
            .unwrap();
        let tokens = PersonalTokenStore::with_team(team.pool().clone(), team.clone());
        let (plaintext, _) = tokens
            .create_personal_token(&record.id, "device", None)
            .await
            .unwrap();
        tokens.verify_for_client(&plaintext, client).await.unwrap()
    }

    #[test]
    fn operation_matrix_has_no_duplicate_operations() {
        let matrix = operation_capability_matrix();
        let mut names: Vec<&str> = matrix.iter().map(|(op, _, _)| op.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            matrix.len(),
            "matrix operations must be unique"
        );
        // The matrix must cover the full native surface (exhaustive match
        // guarantees classification; this pins the breadth for reviewers).
        assert!(
            matrix.len() >= 130,
            "matrix must cover the native surface, got {}",
            matrix.len()
        );
    }

    #[test]
    fn operation_matrix_spot_checks_match_plan() {
        let find = |operation: &str| {
            operation_capability_matrix()
                .into_iter()
                .find(|(op, _, _)| op == operation)
                .unwrap_or_else(|| panic!("missing matrix row for {operation}"))
        };
        assert_eq!(
            find("turn_submit"),
            (
                "turn_submit".to_owned(),
                "via_session".to_owned(),
                "agent.invoke".to_owned()
            )
        );
        assert_eq!(
            find("project_list"),
            (
                "project_list".to_owned(),
                "enumeration".to_owned(),
                "project.read".to_owned()
            )
        );
        assert_eq!(
            find("project_register"),
            (
                "project_register".to_owned(),
                "global".to_owned(),
                "none".to_owned()
            )
        );
        assert_eq!(
            find("session_create"),
            (
                "session_create".to_owned(),
                "direct_project".to_owned(),
                "session.create".to_owned()
            )
        );
        assert_eq!(
            find("job_submit"),
            (
                "job_submit".to_owned(),
                "via_session".to_owned(),
                "job.submit".to_owned()
            )
        );
        assert_eq!(
            find("projection_artifact_read"),
            (
                "projection_artifact_read".to_owned(),
                "direct_project".to_owned(),
                "project.observe".to_owned()
            )
        );
        assert_eq!(
            find("lsp_preview_apply"),
            (
                "lsp_preview_apply".to_owned(),
                "opaque".to_owned(),
                "file.modify".to_owned()
            )
        );
        assert_eq!(
            find("initialize"),
            (
                "initialize".to_owned(),
                "global".to_owned(),
                "none".to_owned()
            )
        );
    }

    #[test]
    fn every_representative_request_maps_to_a_named_operation() {
        for request in policy::representative_requests() {
            let descriptor = operation_descriptor(&request);
            assert!(!descriptor.operation.is_empty());
            assert!(!descriptor.scope_kind.as_str().is_empty());
        }
        // Representative set and matrix stay in lockstep.
        assert_eq!(
            policy::representative_requests().len(),
            operation_capability_matrix().len()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn role_allow_deny_matrix_matches_m001_expansion() {
        let (team, service) = test_service().await;
        let project = ProjectId::new();
        let cases = [
            (ProjectRole::Viewer, Capability::ProjectRead, true),
            (ProjectRole::Viewer, Capability::GitWrite, false),
            (ProjectRole::Viewer, Capability::SessionCreate, false),
            (ProjectRole::Contributor, Capability::SessionCreate, true),
            (ProjectRole::Contributor, Capability::AgentInvoke, true),
            (ProjectRole::Contributor, Capability::MemberManage, false),
            (
                ProjectRole::Contributor,
                Capability::ProjectConfigure,
                false,
            ),
            (ProjectRole::Maintainer, Capability::ProjectConfigure, true),
            (ProjectRole::Maintainer, Capability::AuditRead, true),
            (ProjectRole::Maintainer, Capability::MemberManage, false),
            (ProjectRole::Maintainer, Capability::NodeTarget, false),
            (ProjectRole::Owner, Capability::MemberManage, true),
            (ProjectRole::Owner, Capability::NodeTarget, true),
        ];
        for (role, capability, allowed) in cases {
            let name = format!("{role:?}-probe");
            let principal = team
                .create_principal(PrincipalKind::Human, &name)
                .await
                .unwrap();
            team.create_membership(&project, &principal.id, role)
                .await
                .unwrap();
            let bound = AuthenticatedPrincipal::internal_test(&principal, "client-x");
            let request = remote_request(bound, "probe", Some(capability), Some(project.clone()));
            let outcome = service.authorize(&request).await.is_ok();
            assert_eq!(outcome, allowed, "{role:?} vs {}", capability.as_str());
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn local_owner_uses_same_api_with_broad_policy() {
        let (_, service) = test_service().await;
        // No membership rows exist at all; LocalOwner still authorizes.
        let request = local_owner_request(
            "turn_submit",
            Some(Capability::AgentInvoke),
            Some(ProjectId::new()),
        );
        let decision = service.authorize(&request).await.unwrap();
        assert_eq!(decision.policy, PolicyKind::LocalOwnerBroad);
        assert!(!decision.decision_id.is_empty());
        // And without any project scope as well.
        let request = local_owner_request("lsp_preview_apply", Some(Capability::FileModify), None);
        let decision = service.authorize(&request).await.unwrap();
        assert_eq!(decision.policy, PolicyKind::LocalOwnerBroad);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_scope_fails_closed_for_team_principals() {
        let (team, service) = test_service().await;
        let principal = human_with_token(&team, "Ada", "client-ada").await;
        let request = remote_request(
            principal,
            "session_create",
            Some(Capability::SessionCreate),
            None,
        );
        let error = service.authorize(&request).await.unwrap_err();
        assert!(matches!(error, AuthorizationError::MissingScope { .. }));
        assert_eq!(error.code(), "authorization_scope_required");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn enumeration_requires_active_principal_but_no_scope() {
        let (team, service) = test_service().await;
        let active = human_with_token(&team, "Ada", "client-ada").await;
        let decision = service
            .authorize_enumeration(&active, "project_list", "corr-enum")
            .await
            .unwrap();
        assert_eq!(decision.policy, PolicyKind::TeamMembership);
        let record = team
            .create_principal(PrincipalKind::Human, "Mallory")
            .await
            .unwrap();
        team.set_principal_status(&record.id, record.revision, PrincipalStatus::Disabled)
            .await
            .unwrap();
        let disabled = AuthenticatedPrincipal::internal_test(&record, "client-mallory");
        assert!(service
            .authorize_enumeration(&disabled, "project_list", "corr-enum")
            .await
            .is_err());
        let local = AuthenticatedPrincipal::local_owner("client-local");
        let decision = service
            .authorize_enumeration(&local, "project_list", "corr-enum")
            .await
            .unwrap();
        assert_eq!(decision.policy, PolicyKind::LocalOwnerBroad);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn spoofed_payload_project_cannot_grant_authority() {
        let (team, service) = test_service().await;
        let victim_project = ProjectId::new();
        // The DTO names the victim project, but the bound principal is an
        // outsider with no membership: the decision must deny.
        let attacker = human_with_token(&team, "Mallory", "client-mallory").await;
        let request = remote_request(
            attacker,
            "turn_submit",
            Some(Capability::AgentInvoke),
            Some(victim_project),
        );
        let error = service.authorize(&request).await.unwrap_err();
        assert!(matches!(error, AuthorizationError::Denied { .. }));
        assert_eq!(error.code(), "authorization_denied");
        // The denial carries no secret and no existence signal.
        let rendered = format!("{error:?}");
        for forbidden in ["secret", "token", "bearer", "digest", "password"] {
            assert!(!rendered.to_ascii_lowercase().contains(forbidden));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn membership_removal_race_fails_new_authorization() {
        let (team, service) = test_service().await;
        let project = ProjectId::new();
        let record = team
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        let membership = team
            .create_membership(&project, &record.id, ProjectRole::Owner)
            .await
            .unwrap();
        let bound = AuthenticatedPrincipal::internal_test(&record, "client-ada");
        let request = remote_request(
            bound.clone(),
            "project_archive",
            Some(Capability::ProjectConfigure),
            Some(project.clone()),
        );
        let decision = service.authorize(&request).await.unwrap();
        assert_eq!(decision.membership_revision, Some(membership.revision));
        // Revoke, then re-authorization with the same (now stale) context fails.
        team.revoke_membership(&project, &record.id, membership.revision)
            .await
            .unwrap();
        let error = service.authorize(&request).await.unwrap_err();
        assert!(matches!(error, AuthorizationError::Denied { .. }));
        // A stale writer holding the pre-revocation revision cannot restore
        // authority through the membership store either.
        let stale = team
            .update_membership(
                &project,
                &record.id,
                membership.revision,
                Some(ProjectRole::Owner),
                Some(MembershipState::Active),
            )
            .await;
        assert!(stale.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disabled_principal_cannot_authorize() {
        let (team, service) = test_service().await;
        let project = ProjectId::new();
        let record = team
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        team.create_membership(&project, &record.id, ProjectRole::Owner)
            .await
            .unwrap();
        team.set_principal_status(&record.id, record.revision, PrincipalStatus::Disabled)
            .await
            .unwrap();
        let bound = AuthenticatedPrincipal::internal_test(&record, "client-ada");
        let request = remote_request(
            bound,
            "session_create",
            Some(Capability::SessionCreate),
            Some(project),
        );
        assert!(service.authorize(&request).await.is_err());
    }

    #[test]
    fn child_authority_narrows_and_escalation_fails() {
        let parent = ProjectRole::Contributor.capabilities();
        let child_ok = CapabilitySet::from_caps([Capability::FileRead, Capability::GitRead]);
        assert!(!child_escalates(&parent, &child_ok));
        let narrowed = authorize_child_delegation(&parent, &child_ok).unwrap();
        assert_eq!(narrowed, child_ok);
        let child_evil = CapabilitySet::from_caps([Capability::FileRead, Capability::MemberManage]);
        assert!(child_escalates(&parent, &child_evil));
        let error = authorize_child_delegation(&parent, &child_evil).unwrap_err();
        assert_eq!(error.code(), "authorization_denied");
        // Narrowing is an intersection even for benign overlap.
        let partial = CapabilitySet::from_caps([Capability::GitWrite, Capability::NodeTarget]);
        let narrowed = narrow_authority(&parent, &partial);
        assert!(narrowed.has(Capability::GitWrite));
        assert!(!narrowed.has(Capability::NodeTarget));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn provider_scope_checks_enforce_ownership() {
        let (team, service) = test_service().await;
        let project = ProjectId::new();
        let alice = team
            .create_principal(PrincipalKind::Human, "Alice")
            .await
            .unwrap();
        let bob = team
            .create_principal(PrincipalKind::Human, "Bob")
            .await
            .unwrap();
        team.create_membership(&project, &alice.id, ProjectRole::Contributor)
            .await
            .unwrap();
        let alice_bound = AuthenticatedPrincipal::internal_test(&alice, "client-alice");
        let bob_bound = AuthenticatedPrincipal::internal_test(&bob, "client-bob");

        // Personal scope is owner-only.
        let personal = ProviderScope::Personal {
            owner: alice.id.clone(),
        };
        assert!(authorize_provider_use(
            &service,
            &alice_bound,
            &personal,
            Capability::AgentInvoke,
            "c1"
        )
        .await
        .is_ok());
        assert!(authorize_provider_use(
            &service,
            &bob_bound,
            &personal,
            Capability::AgentInvoke,
            "c1"
        )
        .await
        .is_err());
        // Project scope follows membership grants.
        let project_scope = ProviderScope::Project {
            project_id: project.clone(),
        };
        assert!(authorize_provider_use(
            &service,
            &alice_bound,
            &project_scope,
            Capability::AgentInvoke,
            "c1"
        )
        .await
        .is_ok());
        assert!(authorize_provider_use(
            &service,
            &bob_bound,
            &project_scope,
            Capability::AgentInvoke,
            "c1"
        )
        .await
        .is_err());
        // Deployment scope needs an Owner grant somewhere (Bob has none).
        let deployment = ProviderScope::deployment("deployment-fixture").expect("deployment scope");
        assert!(authorize_provider_use(
            &service,
            &bob_bound,
            &deployment,
            Capability::AgentInvoke,
            "c1"
        )
        .await
        .is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deployment_scope_owner_grant_allows() {
        let (team, service) = test_service().await;
        let project = ProjectId::new();
        let owner = team
            .create_principal(PrincipalKind::Human, "Owner")
            .await
            .unwrap();
        team.create_membership(&project, &owner.id, ProjectRole::Owner)
            .await
            .unwrap();
        let bound = AuthenticatedPrincipal::internal_test(&owner, "client-owner");
        let deployment = ProviderScope::deployment("deployment-fixture").expect("deployment scope");
        let decision =
            authorize_provider_use(&service, &bound, &deployment, Capability::AgentInvoke, "c1")
                .await
                .unwrap();
        assert_eq!(decision.policy, PolicyKind::TeamMembership);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn enumeration_is_privacy_filtered() {
        let (team, service) = test_service().await;
        let project_a = ProjectId::new();
        let project_b = ProjectId::new();
        let alice = team
            .create_principal(PrincipalKind::Human, "Alice")
            .await
            .unwrap();
        team.create_membership(&project_a, &alice.id, ProjectRole::Viewer)
            .await
            .unwrap();
        let bound = AuthenticatedPrincipal::internal_test(&alice, "client-alice");
        let visible =
            visible_projects(&service, &bound, &[project_a.clone(), project_b.clone()]).await;
        assert_eq!(visible, vec![project_a]);
        // LocalOwner observes everything through the broad policy.
        let local = AuthenticatedPrincipal::local_owner("client-local");
        let visible = visible_projects(&service, &local, std::slice::from_ref(&project_b)).await;
        assert_eq!(visible, vec![project_b.clone()]);
        // Denial-as-not-found carries no existence signal.
        let (code, message) = denial_as_not_found();
        assert_eq!(code, "project_not_found");
        assert!(!message.contains(project_b.as_str()));
    }

    #[test]
    fn team_capabilities_map_conservatively_onto_projection() {
        let viewer = ProjectRole::Viewer.capabilities();
        let set = team_capabilities_to_projection(&viewer);
        assert!(set.has(ProjectionCapability::ObservePublicProjection));
        assert!(set.has(ProjectionCapability::ObserveSessionProjection));
        assert!(set.has(ProjectionCapability::ReadRunArtifact));
        assert!(!set.has(ProjectionCapability::AdminBypass));
        let empty = team_capabilities_to_projection(&CapabilitySet::new());
        assert!(empty.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn origin_attribution_round_trip_and_first_write_wins() {
        let pool = SqlitePool::connect("sqlite::memory:").await.expect("pool");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        let store = OriginAttributionStore::new(pool);
        let principal = AuthenticatedPrincipal::local_owner("client-1");
        let decision = AuthorizationDecision {
            principal_id: principal.principal_id().clone(),
            operation: "session_create".to_owned(),
            capability: Some("session.create".to_owned()),
            project_id: None,
            membership_revision: None,
            policy: PolicyKind::LocalOwnerBroad,
            decision_id: "decision-1".to_owned(),
            correlation_id: "corr-1".to_owned(),
            reason: "test".to_owned(),
            decided_at_ms: 1,
        };
        let attribution = OriginAttribution::from_authority(&principal, &decision);
        assert!(!attribution.is_legacy());
        let stored = store
            .record("session", "session-1", &attribution)
            .await
            .unwrap();
        assert_eq!(stored, attribution);
        // A concurrent second writer cannot rewrite the origin.
        let mut other = attribution.clone();
        other.decision_id = "decision-2".to_owned();
        let stored_again = store.record("session", "session-1", &other).await.unwrap();
        assert_eq!(stored_again.decision_id, "decision-1");
        assert!(store.get("session", "nope").await.unwrap().is_none());
        assert!(store.record("bogus", "x", &attribution).await.is_err());
    }

    #[test]
    fn legacy_attribution_is_explicit_never_fabricated() {
        let legacy = OriginAttribution::legacy_local("corr-legacy");
        assert!(legacy.is_legacy());
        assert_eq!(legacy.origin_principal.as_str(), "legacy-local");
        let json = serde_json::to_string(&legacy).expect("serialize legacy");
        assert!(json.contains("legacy-local"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn attribution_survives_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("m003-attribution.db");
        let url = format!("sqlite:{}?mode=rwc", path.display());
        let pool = SqlitePool::connect(&url).await.expect("connect");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        let store = OriginAttributionStore::new(pool.clone());
        let principal = AuthenticatedPrincipal::local_owner("client-1");
        let decision = AuthorizationDecision {
            principal_id: principal.principal_id().clone(),
            operation: "job_submit".to_owned(),
            capability: Some("job.submit".to_owned()),
            project_id: None,
            membership_revision: None,
            policy: PolicyKind::LocalOwnerBroad,
            decision_id: "decision-restart".to_owned(),
            correlation_id: "corr-restart".to_owned(),
            reason: "test".to_owned(),
            decided_at_ms: 1,
        };
        let attribution = OriginAttribution::from_authority(&principal, &decision);
        store.record("job", "job-1", &attribution).await.unwrap();
        pool.close().await;
        let pool2 = SqlitePool::connect(&url).await.expect("reconnect");
        crate::session::schema::migrate(&pool2)
            .await
            .expect("remigrate");
        let store2 = OriginAttributionStore::new(pool2.clone());
        let reloaded = store2.get("job", "job-1").await.unwrap().unwrap();
        assert_eq!(reloaded, attribution);
        pool2.close().await;
    }

    #[test]
    fn authorization_errors_carry_no_secrets() {
        let denied = AuthorizationError::Denied {
            operation: "turn_submit",
            capability: "agent.invoke",
        };
        assert_eq!(denied.code(), "authorization_denied");
        let rendered = format!("{denied} {denied:?}");
        for forbidden in [
            "secret",
            "token",
            "bearer",
            "digest",
            "password",
            "credential",
        ] {
            assert!(
                !rendered.to_ascii_lowercase().contains(forbidden),
                "{forbidden} leaked"
            );
        }
        assert!(AuthorizationError::MissingScope { operation: "x" }.is_denial());
        assert!(
            !AuthorizationError::Unavailable(StorageError::Database("db".to_owned())).is_denial()
        );
    }
}
