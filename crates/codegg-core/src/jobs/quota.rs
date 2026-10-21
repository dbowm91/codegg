//! M007 — typed, keyed quota requests.
//!
//! A keyed quota is a *counted* admission bound. It differs from the
//! scheduler's exclusivity keys in the two ways that matter:
//!
//! * **Scope.** A quota key is `<namespace>:<scope>`, where the scope is
//!   host-computed (today: a canonical `WorkspaceId`). Two workspaces
//!   never share a key, so an unrelated workspace's build load cannot
//!   consume another workspace's slots.
//! * **Count.** A quota grants *N* weighted slots to a scope instead of
//!   a binary mutex. `N` is owned by configuration, never by the
//!   request.
//!
//! The request may only *consume* slots; it can never widen the limit.
//! The limit is resolved by the admission controller from its own
//! configuration, keyed by the request's namespace. A request naming a
//! namespace the controller does not know is refused rather than
//! silently ignored — an unenforced bound is a lie, and a silently
//! ignored one is worse.
//!
//! Classification is derived from the **typed job kind** only. The kind
//! itself is produced by a canonical inspection of the command route
//! (see the command-intent plan), never from a model-authored
//! description, so a model cannot mint a build class to obtain capacity
//! or to escape one.

use serde::{Deserialize, Serialize};

use crate::workspace::WorkspaceId;

/// Maximum bytes in a persisted quota key.
///
/// Bounded so a durable request cannot grow an unbounded index entry or
/// log line.
pub const MAX_QUOTA_KEY_BYTES: usize = 256;

/// Maximum weighted slots a single request may consume.
///
/// A request larger than this is refused rather than clamped: a silently
/// clamped request would report a reservation the job never held.
pub const MAX_QUOTA_SLOTS: u32 = 64;

/// The fixed resource classes that can carry a keyed quota.
///
/// The taxonomy is closed on purpose. A quota namespace is a *host*
/// concept: the set of classes is fixed in code, so no caller — and no
/// model — can introduce a new one to obtain capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceClass {
    /// Workspace-mutating build-class work.
    ///
    /// `Build` and `Format` jobs compile or rewrite the workspace. They
    /// previously shared the *global* `exclusive:workspace-mutation`
    /// exclusivity key, which serialized every build in every
    /// workspace — including unrelated ones. The roadmap already
    /// recorded that key as unscoped and therefore wrong. The build
    /// class replaces it with a counted, workspace-scoped quota whose
    /// limit is configuration-owned.
    Build,
}

impl ResourceClass {
    /// The stable namespace prefix used in quota keys.
    pub fn namespace(self) -> &'static str {
        match self {
            ResourceClass::Build => "build",
        }
    }

    /// The class a typed job kind belongs to, if any.
    pub fn for_job_kind(kind: crate::jobs::JobKind) -> Option<Self> {
        match kind {
            crate::jobs::JobKind::Build | crate::jobs::JobKind::Format => {
                Some(ResourceClass::Build)
            }
            _ => None,
        }
    }

    /// The canonical quota key for a scope.
    ///
    /// An empty scope yields the *unbound* key for the namespace (for
    /// example `build:`). The unbound key is a single global bucket: a
    /// request that never had its scope bound is still counted, and is
    /// counted against one shared bucket rather than escaping the bound.
    pub fn quota_key(self, scope: &str) -> String {
        format!("{}:{}", self.namespace(), scope)
    }
}

/// A weighted, keyed quota slot request.
///
/// Attached to a job's [`crate::jobs::ResourceRequest`]; the admission
/// controller acquires and releases the slots atomically with the other
/// permit dimensions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyedQuotaRequest {
    /// Canonical `<namespace>:<scope>` key.
    pub key: String,
    /// Weighted slots consumed while the job runs.
    pub slots: u32,
}

impl KeyedQuotaRequest {
    /// Build a request for an explicit key.
    pub fn new(key: impl Into<String>, slots: u32) -> Self {
        Self {
            key: key.into(),
            slots,
        }
    }

    /// The request for a class with an unbound scope.
    ///
    /// Used by the kind-derived policy, which does not know the
    /// workspace. The submission boundary binds the scope.
    pub fn unbound(class: ResourceClass) -> Self {
        Self {
            key: class.quota_key(""),
            slots: 1,
        }
    }

    /// The request for a class scoped to a workspace.
    pub fn for_workspace(class: ResourceClass, workspace_id: &WorkspaceId) -> Self {
        Self {
            key: class.quota_key(workspace_id.as_str()),
            slots: 1,
        }
    }

    /// The namespace part of the key, or `None` when the key is
    /// malformed (no `:` separator).
    pub fn namespace(&self) -> Option<&str> {
        self.key.split_once(':').map(|(namespace, _)| namespace)
    }

    /// The scope part of the key. An unbound key has an empty scope.
    pub fn scope(&self) -> &str {
        self.key
            .split_once(':')
            .map(|(_, scope)| scope)
            .unwrap_or("")
    }

    /// Whether the request is structurally usable.
    ///
    /// A malformed key, an empty namespace, an oversized key or a
    /// zero/out-of-range slot count is refused: a request that cannot be
    /// checked is a request that cannot be enforced.
    pub fn is_valid(&self) -> bool {
        !self.key.is_empty()
            && self.key.len() <= MAX_QUOTA_KEY_BYTES
            && self
                .namespace()
                .is_some_and(|namespace| !namespace.is_empty())
            && (1..=MAX_QUOTA_SLOTS).contains(&self.slots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::JobKind;

    #[test]
    fn build_class_covers_build_and_format_only() {
        assert_eq!(
            ResourceClass::for_job_kind(JobKind::Build),
            Some(ResourceClass::Build)
        );
        assert_eq!(
            ResourceClass::for_job_kind(JobKind::Format),
            Some(ResourceClass::Build)
        );
        for kind in [
            JobKind::AgentTurn,
            JobKind::Subagent,
            JobKind::Test,
            JobKind::Lint,
            JobKind::Shell,
            JobKind::ManagedProcess,
            JobKind::Python,
            JobKind::GitRead,
            JobKind::GitMutation,
            JobKind::Research,
            JobKind::Maintenance,
            JobKind::ToolProgram,
        ] {
            assert_eq!(
                ResourceClass::for_job_kind(kind),
                None,
                "{kind:?} must not be classified into a build class"
            );
        }
    }

    #[test]
    fn quota_key_splits_namespace_and_scope() {
        let key = ResourceClass::Build.quota_key("ws-1");
        assert_eq!(key, "build:ws-1");
        let request = KeyedQuotaRequest::new(key, 1);
        assert_eq!(request.namespace(), Some("build"));
        assert_eq!(request.scope(), "ws-1");
        assert!(request.is_valid());
    }

    #[test]
    fn unbound_class_is_a_global_bucket_not_an_escape() {
        let request = KeyedQuotaRequest::unbound(ResourceClass::Build);
        assert_eq!(request.key, "build:");
        assert_eq!(request.namespace(), Some("build"));
        assert_eq!(request.scope(), "");
        assert!(request.is_valid());
    }

    #[test]
    fn malformed_requests_are_refused() {
        assert!(!KeyedQuotaRequest::new("", 1).is_valid());
        assert!(!KeyedQuotaRequest::new("build", 1).is_valid());
        assert!(!KeyedQuotaRequest::new(":ws", 1).is_valid());
        assert!(!KeyedQuotaRequest::new("build:ws", 0).is_valid());
        assert!(!KeyedQuotaRequest::new("build:ws", MAX_QUOTA_SLOTS + 1).is_valid());
        assert!(
            !KeyedQuotaRequest::new(format!("build:{}", "x".repeat(MAX_QUOTA_KEY_BYTES)), 1)
                .is_valid()
        );
    }

    #[test]
    fn workspace_scope_is_the_canonical_workspace_id() {
        let workspace = WorkspaceId::new_unchecked("ws-42");
        let request = KeyedQuotaRequest::for_workspace(ResourceClass::Build, &workspace);
        assert_eq!(request.key, "build:ws-42");
        assert_eq!(request.scope(), "ws-42");
    }
}
