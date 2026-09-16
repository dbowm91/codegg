//! Approval routing and durable mode state (M003).
//!
//! One production [`ApprovalRouter`] resolves deterministic
//! permission/security escalations according to an explicit
//! [`ApprovalMode`]. Deterministic `Allow`/`Deny` never reach the router as
//! an escalation: hard deny and caller/session/agent/project authority
//! ceilings are evaluated before routing, and `Yolo` auto-allows only
//! `Escalate`, never `Deny`.
//!
//! Until M006 the `Automatic` mode is a typed placeholder that safely
//! defers to the human (or is feature-gated unavailable) under an explicit
//! rollout flag — never fail-open.
//!
//! M006 lands the dedicated bounded reviewer (`super::reviewer`): the sync
//! [`ApprovalRouter::route_escalation`] still never auto-allows (it defers),
//! while the async reviewer path in `super::reviewer` may resolve an
//! `Automatic` escalation to Allow/Deny/DeferUser with strict schema,
//! read-only investigation bounds, stale-policy invalidation, and
//! fail-closed (Defer/deny, never Allow) semantics.
//!
//! `ApprovalMode`, `SandboxProfile`, and `ExecutionPolicySnapshot` are the
//! canonical domain types from `codegg-core::approval`; this module adds the
//! normalized request/decision shapes and the single human-wait owner.

use std::time::Duration;

pub use codegg_core::approval::{
    child_mode_allowed, child_sandbox_allowed, resolve_effective_mode, ApprovalMode,
    ExecutionPolicySnapshot, PreferenceError, RuntimePreference, RuntimePreferenceStore,
    SandboxProfile,
};

/// Bounded diagnostic source labels for receipts and audit.
pub mod source {
    pub const PERMISSION_EVALUATION: &str = "permission_evaluation";
    pub const WORKSPACE_FILE_MUTATION: &str = "workspace_file_mutation";
    pub const USER_CHOICE: &str = "user_choice";
    pub const USER_CHOICE_UNPERSISTED: &str = "user_choice_unpersisted";
    pub const YOLO: &str = "yolo";
    pub const AUTOMATIC_DEFER: &str = "automatic_defer";
    pub const SECURITY_DENY: &str = "security_deny";
    pub const PERMISSION_DENY: &str = "permission_deny";
    pub const SENSITIVE_DENY: &str = "sensitive_deny";
    pub const TIMEOUT_DENY: &str = "timeout_deny";
    /// M006 reviewer verdict sources. The reviewer is an authorization
    /// helper: it only resolves `Escalate` in `Automatic` mode and can never
    /// override a deterministic `Deny` or widen the sandbox ceiling.
    pub const REVIEWER_ALLOW: &str = "reviewer_allow";
    pub const REVIEWER_DENY: &str = "reviewer_deny";
    pub const REVIEWER_DEFER: &str = "reviewer_defer";
}

/// Normalized escalation request. Raw sensitive args never enter durable
/// preference/audit payloads unless existing redaction policy permits them;
/// callers pass a bounded redacted summary.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub tool: String,
    pub path: Option<String>,
    /// Bounded redacted args/command summary (never raw secrets).
    pub args_summary: Option<String>,
    /// Bounded escalation reasons (policy, security, sensitive-path).
    pub escalation_reasons: Vec<String>,
    /// Bounded effect metadata (risk class, destructive pattern, ...).
    pub effect_metadata: Option<String>,
    pub policy_revision: Option<String>,
    pub session_id: String,
    pub turn_id: Option<String>,
}

impl ApprovalRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tool: impl Into<String>,
        path: Option<String>,
        args_summary: Option<String>,
        escalation_reasons: Vec<String>,
        effect_metadata: Option<String>,
        policy_revision: Option<String>,
        session_id: impl Into<String>,
        turn_id: Option<String>,
    ) -> Self {
        Self {
            tool: truncate_bounded(tool.into(), 128),
            path: path.and_then(|p| {
                let t = p.trim().to_owned();
                if t.is_empty() {
                    None
                } else {
                    Some(truncate_bounded(t, 1024))
                }
            }),
            args_summary: args_summary.map(|s| truncate_bounded(s, 512)),
            escalation_reasons: escalation_reasons
                .into_iter()
                .take(8)
                .map(|r| truncate_bounded(r, 512))
                .collect(),
            effect_metadata: effect_metadata.map(|s| truncate_bounded(s, 512)),
            policy_revision: policy_revision.map(|s| truncate_bounded(s, 256)),
            session_id: truncate_bounded(session_id.into(), 256),
            turn_id: turn_id.map(|s| truncate_bounded(s, 256)),
        }
    }
}

/// Normalized router outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    Allow {
        source: String,
        reason: String,
    },
    Deny {
        source: String,
        reason: String,
    },
    /// Safe deferral to the human (Automatic placeholder, timeout, ...).
    DeferUser {
        source: String,
        reason: String,
    },
}

impl ApprovalDecision {
    pub fn allow(source: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Allow {
            source: truncate_bounded(source.into(), 64),
            reason: truncate_bounded(reason.into(), 512),
        }
    }

    pub fn deny(source: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Deny {
            source: truncate_bounded(source.into(), 64),
            reason: truncate_bounded(reason.into(), 512),
        }
    }

    pub fn defer(source: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::DeferUser {
            source: truncate_bounded(source.into(), 64),
            reason: truncate_bounded(reason.into(), 512),
        }
    }

    pub const fn allowed(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }

    pub fn source(&self) -> &str {
        match self {
            Self::Allow { source, .. }
            | Self::Deny { source, .. }
            | Self::DeferUser { source, .. } => source,
        }
    }

    pub fn reason(&self) -> &str {
        match self {
            Self::Allow { reason, .. }
            | Self::Deny { reason, .. }
            | Self::DeferUser { reason, .. } => reason,
        }
    }
}

/// Normalized deterministic outcome before routing.
#[derive(Debug, Clone)]
pub enum DeterministicVerdict {
    Allow,
    Deny { reason: String, source: String },
    Escalate { request: ApprovalRequest },
}

/// One production approval router.
///
/// Constructed per turn/tool batch from the immutable
/// [`ExecutionPolicySnapshot`] captured at the batch boundary. Concurrent
/// mode changes apply on the next boundary and cannot retroactively bless
/// a pending action because the router never re-reads live mutable mode.
#[derive(Debug, Clone)]
pub struct ApprovalRouter {
    snapshot: ExecutionPolicySnapshot,
    /// Explicit rollout flag for the M006 reviewer. `false` (default)
    /// means `Automatic` safely defers to the human; `true` is reserved
    /// for the reviewer implementation and must never auto-allow in M003.
    automatic_rollout: bool,
}

impl ApprovalRouter {
    pub fn new(snapshot: ExecutionPolicySnapshot) -> Self {
        Self {
            snapshot,
            automatic_rollout: false,
        }
    }

    /// Explicit M006 rollout gate. M003 callers must leave this `false`.
    pub fn with_automatic_rollout(mut self, enabled: bool) -> Self {
        self.automatic_rollout = enabled;
        self
    }

    pub fn snapshot(&self) -> &ExecutionPolicySnapshot {
        &self.snapshot
    }

    pub fn approval_mode(&self) -> ApprovalMode {
        self.snapshot.approval_mode()
    }

    /// Pure routing for deterministic `Allow`/`Deny`. Escalations require
    /// [`Self::resolve_escalation`] (Yolo/Automatic) or
    /// [`Self::request_human_approval`] (Interactive).
    pub fn decide_deterministic(&self, verdict: &DeterministicVerdict) -> ApprovalDecision {
        match verdict {
            DeterministicVerdict::Allow => {
                ApprovalDecision::allow(source::PERMISSION_EVALUATION, "deterministic allow")
            }
            DeterministicVerdict::Deny { reason, source } => {
                ApprovalDecision::deny(source.clone(), reason.clone())
            }
            DeterministicVerdict::Escalate { request } => self.route_escalation(request),
        }
    }

    /// Route an escalation without human I/O. Interactive escalations
    /// return `DeferUser` here; use [`Self::request_human_approval`] to
    /// perform the single human wait.
    pub fn route_escalation(&self, request: &ApprovalRequest) -> ApprovalDecision {
        match self.snapshot.approval_mode() {
            ApprovalMode::Interactive => ApprovalDecision::defer(
                source::USER_CHOICE,
                format!(
                    "interactive escalation for '{}': {}",
                    request.tool,
                    request
                        .escalation_reasons
                        .first()
                        .cloned()
                        .unwrap_or_default()
                ),
            ),
            ApprovalMode::Yolo => ApprovalDecision::allow(
                source::YOLO,
                format!(
                    "yolo auto-allow escalation for '{}' within authority ceiling",
                    request.tool
                ),
            ),
            ApprovalMode::Automatic => {
                // M006: the sync route still never auto-allows. The async
                // reviewer (`super::reviewer::resolve_automatic_escalation`)
                // is the only path that may return Allow/Deny for
                // Automatic, with strict schema and fail-closed semantics.
                // Direct sync callers without a reviewer backend defer
                // safely here, regardless of the rollout flag.
                let _ = self.automatic_rollout;
                ApprovalDecision::defer(
                    source::AUTOMATIC_DEFER,
                    "automatic reviewer unavailable (M006); deferred to human",
                )
            }
        }
    }

    /// The single production human-wait owner. Registers before publishing,
    /// waits with a bounded 300s timeout, and unregisters. Timeout is
    /// recorded distinctly from explicit user deny (`timeout_deny`).
    /// Returns the normalized decision plus whether the human chose an
    /// `Always` persistence style and whether the outcome allows execution.
    pub async fn request_human_approval(
        &self,
        perm_id: &str,
        request: &ApprovalRequest,
        args: Option<serde_json::Value>,
    ) -> HumanApprovalOutcome {
        use crate::bus::events::AppEvent;
        use crate::bus::{global::GlobalEventBus, PermissionDecision, PermissionRegistry};

        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
        PermissionRegistry::register_with_session(
            request.session_id.clone(),
            request.turn_id.clone(),
            perm_id.to_owned(),
            resp_tx,
        );
        GlobalEventBus::publish(AppEvent::PermissionPending {
            session_id: request.session_id.clone(),
            perm_id: perm_id.to_owned(),
            turn_id: request.turn_id.clone(),
            tool: request.tool.clone(),
            path: request.path.clone(),
            args,
        });
        let choice = match tokio::time::timeout(Duration::from_secs(300), resp_rx).await {
            Ok(Ok(choice)) => Some(choice),
            _ => None,
        };
        PermissionRegistry::unregister_scoped(&request.session_id, perm_id);
        match choice {
            Some(PermissionDecision::AllowOnce) => HumanApprovalOutcome {
                decision: ApprovalDecision::allow(source::USER_CHOICE, "human approved"),
                persist: false,
                allow: true,
            },
            Some(PermissionDecision::AlwaysAllow) => HumanApprovalOutcome {
                decision: ApprovalDecision::allow(source::USER_CHOICE, "human approved (always)"),
                persist: true,
                allow: true,
            },
            Some(PermissionDecision::DenyOnce) => HumanApprovalOutcome {
                decision: ApprovalDecision::deny(source::USER_CHOICE, "human denied"),
                persist: false,
                allow: false,
            },
            Some(PermissionDecision::AlwaysDeny) => HumanApprovalOutcome {
                decision: ApprovalDecision::deny(source::USER_CHOICE, "human denied (always)"),
                persist: true,
                allow: false,
            },
            None => HumanApprovalOutcome {
                decision: ApprovalDecision::deny(source::TIMEOUT_DENY, "approval timeout"),
                persist: false,
                allow: false,
            },
        }
    }
}

/// Human-wait outcome with persistence intent preserved.
#[derive(Debug, Clone)]
pub struct HumanApprovalOutcome {
    pub decision: ApprovalDecision,
    /// `true` when the human chose `AlwaysAllow`/`AlwaysDeny` and the
    /// caller must attempt the durable store write.
    pub persist: bool,
    pub allow: bool,
}

fn truncate_bounded(mut value: String, max_len: usize) -> String {
    if value.len() > max_len {
        value.truncate(max_len);
    }
    value = value.replace('\0', "");
    value
}

/// Canonical per-user permission decision path.
///
/// Production `PermissionChecker` constructors must use this so `Always`
/// decisions survive restart. Subagents must not receive an independent
/// global store (pass `None` for ephemeral scope); the parent ceiling
/// remains authoritative.
pub fn canonical_permission_store_path() -> Option<std::path::PathBuf> {
    if let Ok(raw) = std::env::var("CODEGG_PERMISSIONS_PATH") {
        let trimmed = raw.trim().to_owned();
        if !trimmed.is_empty() {
            return Some(std::path::PathBuf::from(trimmed));
        }
    }
    crate::permission::default_store_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ApprovalRequest {
        ApprovalRequest::new(
            "edit",
            Some("/tmp/file".into()),
            Some("edit summary".into()),
            vec!["policy ask".into()],
            None,
            Some("config:1".into()),
            "session-1",
            None,
        )
    }

    fn snapshot_for(mode: ApprovalMode) -> ExecutionPolicySnapshot {
        ExecutionPolicySnapshot::capture(
            mode,
            SandboxProfile::WorkspaceWrite,
            None,
            Some("session-1".into()),
            None,
            Some("config:1".into()),
            None,
        )
    }

    #[test]
    fn router_matrix_allow_deny_escalate() {
        let req = request();
        // Allow/Deny are mode-independent.
        for mode in [
            ApprovalMode::Interactive,
            ApprovalMode::Automatic,
            ApprovalMode::Yolo,
        ] {
            let router = ApprovalRouter::new(snapshot_for(mode));
            assert!(router
                .decide_deterministic(&DeterministicVerdict::Allow)
                .allowed());
            let deny = router.decide_deterministic(&DeterministicVerdict::Deny {
                reason: "hard deny".into(),
                source: source::PERMISSION_DENY.into(),
            });
            assert!(!deny.allowed());
            assert_eq!(deny.source(), source::PERMISSION_DENY);
        }
        // Escalate routing per mode.
        let interactive =
            ApprovalRouter::new(snapshot_for(ApprovalMode::Interactive)).route_escalation(&req);
        assert!(matches!(interactive, ApprovalDecision::DeferUser { .. }));

        let yolo = ApprovalRouter::new(snapshot_for(ApprovalMode::Yolo)).route_escalation(&req);
        assert!(yolo.allowed());
        assert_eq!(yolo.source(), source::YOLO);

        let automatic =
            ApprovalRouter::new(snapshot_for(ApprovalMode::Automatic)).route_escalation(&req);
        assert!(!automatic.allowed());
        assert!(matches!(automatic, ApprovalDecision::DeferUser { .. }));
        assert_eq!(automatic.source(), source::AUTOMATIC_DEFER);

        // Automatic never auto-allows even with rollout enabled in M003.
        let rollout = ApprovalRouter::new(snapshot_for(ApprovalMode::Automatic))
            .with_automatic_rollout(true)
            .route_escalation(&req);
        assert!(!rollout.allowed());
    }

    #[test]
    fn hard_deny_always_wins_over_yolo() {
        let router = ApprovalRouter::new(snapshot_for(ApprovalMode::Yolo));
        let deny = router.decide_deterministic(&DeterministicVerdict::Deny {
            reason: "explicit deny".into(),
            source: source::PERMISSION_DENY.into(),
        });
        assert!(!deny.allowed());
        assert_ne!(deny.source(), source::YOLO);
    }

    #[test]
    fn snapshot_is_captured_and_child_ceiling_enforced() {
        let parent = snapshot_for(ApprovalMode::Interactive);
        assert!(parent
            .narrow_for_child(ApprovalMode::Yolo, SandboxProfile::WorkspaceWrite)
            .is_err());
        let yolo_parent = snapshot_for(ApprovalMode::Yolo);
        assert!(yolo_parent
            .narrow_for_child(ApprovalMode::Interactive, SandboxProfile::ReadOnly)
            .is_ok());
    }

    #[test]
    fn request_bounds_sensitive_shapes() {
        let req = ApprovalRequest::new(
            "bash",
            Some("/tmp/x".into()),
            Some("x".repeat(5000)),
            vec!["r".repeat(5000); 20],
            None,
            None,
            "s",
            None,
        );
        assert!(req.args_summary.unwrap().len() <= 512);
        assert_eq!(req.escalation_reasons.len(), 8);
        assert!(req.tool.len() <= 128);
    }

    #[test]
    fn canonical_store_path_prefers_env_override() {
        std::env::remove_var("CODEGG_PERMISSIONS_PATH");
        let _ = crate::permission::default_store_path();
        std::env::set_var("CODEGG_PERMISSIONS_PATH", "/tmp/m003-perms.json");
        assert_eq!(
            canonical_permission_store_path(),
            Some(std::path::PathBuf::from("/tmp/m003-perms.json"))
        );
        std::env::remove_var("CODEGG_PERMISSIONS_PATH");
    }
}
