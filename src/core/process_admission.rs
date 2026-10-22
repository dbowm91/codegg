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
//!
//! The contract itself lives in the tool layer
//! ([`crate::tool::process_admission`]), which owns the trait, the typed
//! refusal and the concrete adapter, so a lower layer — the scheduler, for
//! instance — can build a policy without reaching into this module. The
//! daemon's own job is to own the *authority* and to decide what a refusal
//! means for the caller.
//!
//! Ownership: [`codegg_core::process_admission`] owns the durable state
//! machine; this module is the root crate's daemon wiring.

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
