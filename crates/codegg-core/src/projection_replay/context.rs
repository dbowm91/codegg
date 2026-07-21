//! Projection access context and policy input seam (M3).
//!
//! The projection subsystem must enforce visibility, redaction, and
//! artifact access for later multi-user observation. This module
//! defines the transport-derived access context that downstream
//! policy engines consume without changing projection DTOs or
//! replay storage.
//!
//! The context is constructed by the daemon (transport/server/socket
//! authority). Request DTOs may name a desired project / session /
//! handle but can never name capabilities. Capabilities are
//! **semantic**, not role-named, so a future team/role authorization
//! model can implement [`ProjectionAccessPolicy`] without altering
//! the projection surface.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Transport class that produced a projection request.
///
/// The policy engine MAY use this to choose conservative defaults
/// even when capability negotiation succeeds. The class is set by
/// the daemon transport, never by the request payload.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionTransportClass {
    /// Local single-user transport (TUI/CLI in-process, stdio, or
    /// local socket owned by the same OS user as the daemon).
    #[default]
    Local,
    /// Authenticated remote transport (HTTP/WebSocket with verified
    /// session). Capability set is bound to the verified principal.
    AuthenticatedRemote,
    /// Reserved for in-process tests and synthetic harnesses. The
    /// policy engine treats this as trusted but emits an explicit
    /// marker so diagnostics make the seam visible.
    InternalTest,
}

impl ProjectionTransportClass {
    /// Returns `true` for classes that should be considered trusted
    /// for diagnostics. The classification is the same as the
    /// `is_trusted` semantics used by capability gating: `Local` and
    /// `InternalTest` are trusted; `AuthenticatedRemote` is
    /// cap-bound.
    pub fn is_trusted_for_diagnostics(self) -> bool {
        matches!(
            self,
            ProjectionTransportClass::Local | ProjectionTransportClass::InternalTest
        )
    }
}

/// Stable opaque principal identifier.
///
/// For a local single-user daemon this is a synthesized
/// `"local-user"` value; for authenticated remotes it is the
/// verified principal id from the auth layer. The string is opaque
/// from the projection layer's point of view and MUST NOT be parsed
/// as a path, role, or capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectionPrincipalId(pub String);

impl ProjectionPrincipalId {
    /// Construct a new principal id. The caller is responsible for
    /// ensuring the value was produced by the daemon transport
    /// authority, not by a request payload field.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Local single-user principal identifier.
    pub fn local() -> Self {
        Self("local-user".to_string())
    }

    /// Internal-test principal identifier.
    pub fn internal_test() -> Self {
        Self("internal-test".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable daemon-issued client identity (one per transport
/// connection). The transport layer assigns this on accept; it is
/// never derived from the request payload.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectionClientId(pub String);

impl ProjectionClientId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Semantic projection capabilities.
///
/// These are the verbs the policy engine understands; a future
/// team/role layer translates named roles into this enum without
/// changing the projection surface. Adding a new variant is
/// backward compatible; consumers should ignore unknown
/// capabilities.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionCapability {
    /// Observe public projection events for an authorized project
    /// stream.
    ObservePublicProjection,
    /// Observe session projection events (default for the bound
    /// session only).
    ObserveSessionProjection,
    /// Receive client-local events owned by the calling client.
    ObserveClientLocal,
    /// Read a bounded run artifact via an authorized handle.
    ReadRunArtifact,
    /// Read a bounded tool output via an authorized handle.
    ReadToolOutput,
    /// Read a bounded diff/log excerpt via an authorized handle.
    ReadDiffOrLog,
    /// View bounded operational diagnostics.
    ViewOperationalDiagnostics,
    /// Admin: bypass redaction for harness-internal checks. The
    /// policy engine MUST refuse to grant this capability to any
    /// non-`InternalTest` transport, and it MUST NOT serialize
    /// `AdminBypass` results through the public transport.
    AdminBypass,
}

impl ProjectionCapability {
    /// `true` if the capability is restricted to harness-internal
    /// transports and cannot be granted to remote/local users.
    pub fn is_internal_only(self) -> bool {
        matches!(self, ProjectionCapability::AdminBypass)
    }
}

/// Capability set carried on a [`ProjectionAccessContext`].
///
/// The set is constructed by the daemon transport. The policy
/// engine is the only consumer; this struct is not exposed as a
/// request field.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ProjectionCapabilitySet {
    capabilities: Vec<ProjectionCapability>,
}

impl ProjectionCapabilitySet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a capability set from a fixed list. Duplicates are
    /// removed and the list is sorted for deterministic hashing.
    pub fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = ProjectionCapability>,
    {
        let mut caps: Vec<ProjectionCapability> = iter.into_iter().collect();
        caps.sort_by_key(|c| *c as u8);
        caps.dedup();
        Self { capabilities: caps }
    }

    /// Build the local single-user capability set. Local users
    /// receive every public / session / client-local / read /
    /// diagnostics capability; admin bypass is intentionally absent.
    pub fn local_user() -> Self {
        Self::from_iter([
            ProjectionCapability::ObservePublicProjection,
            ProjectionCapability::ObserveSessionProjection,
            ProjectionCapability::ObserveClientLocal,
            ProjectionCapability::ReadRunArtifact,
            ProjectionCapability::ReadToolOutput,
            ProjectionCapability::ReadDiffOrLog,
            ProjectionCapability::ViewOperationalDiagnostics,
        ])
    }

    /// Build the harness-internal test capability set. Only used by
    /// the in-process test transport; production daemon never
    /// exposes this.
    pub fn internal_test() -> Self {
        Self::from_iter([
            ProjectionCapability::ObservePublicProjection,
            ProjectionCapability::ObserveSessionProjection,
            ProjectionCapability::ObserveClientLocal,
            ProjectionCapability::ReadRunArtifact,
            ProjectionCapability::ReadToolOutput,
            ProjectionCapability::ReadDiffOrLog,
            ProjectionCapability::ViewOperationalDiagnostics,
            ProjectionCapability::AdminBypass,
        ])
    }

    pub fn has(&self, cap: ProjectionCapability) -> bool {
        self.capabilities.contains(&cap)
    }

    pub fn iter(&self) -> impl Iterator<Item = ProjectionCapability> + '_ {
        self.capabilities.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.capabilities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }
}

/// Bounded policy resolver over the projects a principal can read.
///
/// The default implementation rejects unknown projects. The M3
/// production resolver is supplied by the daemon constructor and
/// must never be replaced by request payload fields.
pub trait ProjectionProjectResolver: Send + Sync {
    fn is_allowed(&self, project_id: &str) -> bool;
    fn is_session_allowed(&self, project_id: &str, session_id: &str) -> bool;
}

#[derive(Debug, Clone, Default)]
pub struct AllowAllProjectResolver;

impl ProjectionProjectResolver for AllowAllProjectResolver {
    fn is_allowed(&self, _project_id: &str) -> bool {
        true
    }
    fn is_session_allowed(&self, _project_id: &str, _session_id: &str) -> bool {
        true
    }
}

#[derive(Debug, Clone, Default)]
pub struct BoundedProjectResolver {
    projects: Vec<String>,
}

impl BoundedProjectResolver {
    pub fn new<I, S>(projects: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            projects: projects.into_iter().map(Into::into).collect(),
        }
    }
}

impl ProjectionProjectResolver for BoundedProjectResolver {
    fn is_allowed(&self, project_id: &str) -> bool {
        self.projects.iter().any(|p| p == project_id)
    }
    fn is_session_allowed(&self, project_id: &str, session_id: &str) -> bool {
        if !self.is_allowed(project_id) {
            return false;
        }
        // Session scope inherits from project scope; finer-grained
        // session lists are not part of M3.
        !session_id.is_empty()
    }
}

/// Canonical projection access context.
///
/// Constructed by the daemon transport. The projection subsystem
/// MUST NOT derive any field of this struct from request payload
/// data; if a caller needs a new context it asks the transport for
/// a fresh one.
#[derive(Clone)]
pub struct ProjectionAccessContext {
    pub principal_id: ProjectionPrincipalId,
    pub client_id: ProjectionClientId,
    pub capabilities: ProjectionCapabilitySet,
    pub project_resolver: Arc<dyn ProjectionProjectResolver>,
    pub transport_class: ProjectionTransportClass,
    pub request_correlation_id: String,
}

impl std::fmt::Debug for ProjectionAccessContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectionAccessContext")
            .field("principal_id", &self.principal_id)
            .field("client_id", &self.client_id)
            .field("capabilities", &self.capabilities)
            .field("transport_class", &self.transport_class)
            .field("request_correlation_id", &self.request_correlation_id)
            .finish_non_exhaustive()
    }
}

impl ProjectionAccessContext {
    /// Convenience constructor for the local single-user context.
    pub fn local(client_id: impl Into<String>, correlation_id: impl Into<String>) -> Self {
        Self {
            principal_id: ProjectionPrincipalId::local(),
            client_id: ProjectionClientId::new(client_id),
            capabilities: ProjectionCapabilitySet::local_user(),
            project_resolver: Arc::new(AllowAllProjectResolver),
            transport_class: ProjectionTransportClass::Local,
            request_correlation_id: correlation_id.into(),
        }
    }

    /// Convenience constructor for harness-internal tests.
    pub fn internal_test(client_id: impl Into<String>, correlation_id: impl Into<String>) -> Self {
        Self {
            principal_id: ProjectionPrincipalId::internal_test(),
            client_id: ProjectionClientId::new(client_id),
            capabilities: ProjectionCapabilitySet::internal_test(),
            project_resolver: Arc::new(AllowAllProjectResolver),
            transport_class: ProjectionTransportClass::InternalTest,
            request_correlation_id: correlation_id.into(),
        }
    }

    /// Construct a context with a bounded project resolver.
    pub fn with_projects(
        client_id: impl Into<String>,
        correlation_id: impl Into<String>,
        capabilities: ProjectionCapabilitySet,
        resolver: Arc<dyn ProjectionProjectResolver>,
        transport_class: ProjectionTransportClass,
    ) -> Self {
        let principal = match transport_class {
            ProjectionTransportClass::Local => ProjectionPrincipalId::local(),
            ProjectionTransportClass::InternalTest => ProjectionPrincipalId::internal_test(),
            ProjectionTransportClass::AuthenticatedRemote => {
                ProjectionPrincipalId::new("authenticated-remote")
            }
        };
        Self {
            principal_id: principal,
            client_id: ProjectionClientId::new(client_id),
            capabilities,
            project_resolver: resolver,
            transport_class,
            request_correlation_id: correlation_id.into(),
        }
    }

    pub fn has(&self, cap: ProjectionCapability) -> bool {
        // Admin bypass is only honored on the internal test transport.
        if cap.is_internal_only() && self.transport_class != ProjectionTransportClass::InternalTest
        {
            return false;
        }
        self.capabilities.has(cap)
    }

    /// Authorization decision for a project/session scope. A session
    /// outside the resolver or an absent observe capability fails
    /// closed.
    pub fn authorize_scope(&self, project_id: &str, session_id: Option<&str>) -> bool {
        if !self.has(ProjectionCapability::ObservePublicProjection) {
            return false;
        }
        if !self.project_resolver.is_allowed(project_id) {
            return false;
        }
        if let Some(sid) = session_id {
            return self.project_resolver.is_session_allowed(project_id, sid);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_context_has_public_capabilities() {
        let ctx = ProjectionAccessContext::local("c1", "corr-1");
        assert!(ctx.has(ProjectionCapability::ObservePublicProjection));
        assert!(ctx.has(ProjectionCapability::ObserveSessionProjection));
        assert!(ctx.has(ProjectionCapability::ObserveClientLocal));
        assert!(!ctx.has(ProjectionCapability::AdminBypass));
    }

    #[test]
    fn admin_bypass_refused_for_local_transport() {
        let mut ctx = ProjectionAccessContext::local("c1", "corr-1");
        ctx.capabilities
            .capabilities
            .push(ProjectionCapability::AdminBypass);
        assert!(!ctx.has(ProjectionCapability::AdminBypass));
    }

    #[test]
    fn internal_test_context_has_admin_bypass() {
        let ctx = ProjectionAccessContext::internal_test("c1", "corr-1");
        assert!(ctx.has(ProjectionCapability::AdminBypass));
    }

    #[test]
    fn bounded_project_resolver_rejects_unknown() {
        let resolver: Arc<dyn ProjectionProjectResolver> =
            Arc::new(BoundedProjectResolver::new(["p1", "p2"]));
        let ctx = ProjectionAccessContext::with_projects(
            "c1",
            "corr-1",
            ProjectionCapabilitySet::local_user(),
            resolver,
            ProjectionTransportClass::Local,
        );
        assert!(ctx.authorize_scope("p1", Some("s1")));
        assert!(!ctx.authorize_scope("p3", Some("s1")));
    }

    #[test]
    fn capability_set_dedup_and_sort() {
        let caps = ProjectionCapabilitySet::from_iter([
            ProjectionCapability::ViewOperationalDiagnostics,
            ProjectionCapability::ObservePublicProjection,
            ProjectionCapability::ViewOperationalDiagnostics,
        ]);
        assert_eq!(caps.len(), 2);
        let names: Vec<_> = caps.iter().collect();
        assert_eq!(
            names,
            vec![
                ProjectionCapability::ObservePublicProjection,
                ProjectionCapability::ViewOperationalDiagnostics
            ]
        );
    }
}
