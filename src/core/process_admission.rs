//! M005 C005 — the daemon's process admission wiring.
//!
//! The C003 fence mediates the model's native writers and the C004 checkout
//! gate mediates CodeGG's non-broker writers. Neither can contain a
//! **process**: a program decides at runtime what to write, so the only
//! honest containment is a process group, and CodeGG's managed process
//! group is explicitly escapable by a descendant that calls `setsid`.
//!
//! So the restricted shared-checkout profile refuses processes rather than
//! containing them, and this module owns the daemon's half of that
//! contract: the daemon builds the authority, threads it into every turn,
//! and the durable `process_denial` audit action records the refusals.

pub use crate::tool::process_admission::{
    build_process_admission, ProcessAdmissionPolicy, ProcessScope, ServiceProcessAdmission,
    SharedProcessAdmission,
};

/// The daemon's process admission authority.
pub type DaemonProcessAdmissionService =
    std::sync::Arc<codegg_core::process_admission::ProcessAdmissionService>;

#[cfg(test)]
mod tests {
    use codegg_core::process_admission::{ProcessDenial, ProcessDenialReason, ProcessKind};

    #[test]
    fn a_denial_is_rendered_without_a_path() {
        let denial = ProcessDenial {
            reason: ProcessDenialReason::SharedCheckout,
            kind: ProcessKind::Command,
            worktree_id: Some("worktree-1".to_owned()),
            group_id: Some("group-1".to_owned()),
        };
        let rendered = denial.to_string();
        assert!(rendered.contains("group-1"));
        assert!(rendered.contains("worktree-1"));
        assert!(!rendered.contains('/'));
    }
}
