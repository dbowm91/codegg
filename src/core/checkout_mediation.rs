//! M005 C004 — mediation for CodeGG's non-broker checkout writers.
//!
//! The C003 writer fence mediates the model's native writers. Everything
//! else that CodeGG can point at a checkout — the document service, the
//! change-review LSP applier, project initialization, checkpoint restore,
//! the HTTP file routes — was an unmediated writer until C004.
//!
//! This module owns the daemon-side half of the C004 contract:
//!
//! * [`CheckoutWriterSink`] names each mediated entry point, so the
//!   disposition manifest and the writer-sink guard can talk about the
//!   same set.
//! * [`CheckoutMediator`] asks the authoritative reservation service
//!   whether the target checkout is reserved by a live agent-run group.
//! * [`CheckoutConflict`] is the typed refusal a human caller receives. It
//!   is a conflict, not an authorization failure: the caller may retry once
//!   the group finishes, and nothing about another run's authority is
//!   revealed.
//!
//! The mediation is a **pre-effect** check. It runs before the writer
//! touches the checkout, it takes no lock, and it holds no permit, so it
//! cannot invert the lock order between the document revision lease, the
//! repository lock and the writer permit.
//!
//! Ownership: [`codegg_core::checkout_reservation`] owns the durable state
//! machine; this module is the root crate's adapter and decides what a
//! refusal means for the caller.

use std::path::Path;
use std::sync::Arc;

use codegg_core::audit::AuditDecisionProvenance;
use codegg_core::audit_instrumentation::{AuditChainContext, ExecutionAuditEmitter};
use codegg_core::checkout_reservation::{
    CheckoutReservation, CheckoutReservationError, CheckoutReservationService,
};
use codegg_core::identity::WorktreeId;
use codegg_core::transport_auth::AuthenticatedPrincipal;

/// One CodeGG entry point that writes into a checkout outside the broker.
///
/// Each variant names a *final effect owner*: the code that performs the
/// write, not the button or command that reaches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutWriterSink {
    /// Create a workspace file (`CoreRequest::DocumentCreate`).
    DocumentCreate,
    /// Delete a workspace file (`CoreRequest::DocumentDelete`).
    DocumentDelete,
    /// Save an edited workspace file (`CoreRequest::DocumentSave`).
    DocumentSave,
    /// Publish the project `AGENTS.md` (`CoreRequest::ProjectInitPublish`).
    ProjectInitPublish,
    /// Apply a reviewed LSP workspace edit (`CoreRequest::LspPreviewApply`).
    LspPreviewApply,
    /// Restore a checkpoint (`CoreRequest::EditCheckpointUndo`).
    EditCheckpointUndo,
    /// Restore the latest checkpoint (`CoreRequest::EditCheckpointUndoLatest`).
    EditCheckpointUndoLatest,
    /// Reapply a checkpoint (`CoreRequest::EditCheckpointReapply`).
    EditCheckpointReapply,
    /// Reapply the latest checkpoint (`CoreRequest::EditCheckpointReapplyLatest`).
    EditCheckpointReapplyLatest,
    /// Remove a managed checkout (`CoreRequest::ManagedWorktreeCleanup`).
    ManagedWorktreeCleanup,
    /// Archive a managed checkout (`CoreRequest::ManagedWorktreeArchive`).
    ManagedWorktreeArchive,
    /// Write a file over the HTTP API (`POST /file`).
    HttpFileWrite,
    /// Delete a file over the HTTP API (`DELETE /file`).
    HttpFileDelete,
    /// Create a workspace directory over the HTTP API (`POST /workspace`).
    HttpWorkspaceCreate,
}

impl CheckoutWriterSink {
    /// The canonical sink name recorded in the disposition manifest and in
    /// the durable refusal event.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DocumentCreate => "document_create",
            Self::DocumentDelete => "document_delete",
            Self::DocumentSave => "document_save",
            Self::ProjectInitPublish => "project_init_publish",
            Self::LspPreviewApply => "lsp_preview_apply",
            Self::EditCheckpointUndo => "edit_checkpoint_undo",
            Self::EditCheckpointUndoLatest => "edit_checkpoint_undo_latest",
            Self::EditCheckpointReapply => "edit_checkpoint_reapply",
            Self::EditCheckpointReapplyLatest => "edit_checkpoint_reapply_latest",
            Self::ManagedWorktreeCleanup => "managed_worktree_cleanup",
            Self::ManagedWorktreeArchive => "managed_worktree_archive",
            Self::HttpFileWrite => "http_file_write",
            Self::HttpFileDelete => "http_file_delete",
            Self::HttpWorkspaceCreate => "http_workspace_create",
        }
    }
}

impl std::fmt::Display for CheckoutWriterSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The typed conflict a refused checkout writer receives.
///
/// The conflict names the group that holds the checkout and the checkout's
/// own identity. It never names a path, a file body, or another run's
/// prompt: a refusal is a concurrency answer, not an authorization one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutConflict {
    /// The sink that was refused.
    pub sink: CheckoutWriterSink,
    /// The durable worktree identity of the reserved checkout.
    pub worktree_id: String,
    /// The live agent-run group that holds the checkout.
    pub group_id: String,
    /// The run that owns the checkout lease.
    pub owner_run_id: String,
    /// Why the reservation authority could not answer, when the refusal is
    /// a lookup failure rather than an observed reservation.
    pub lookup_failure: Option<String>,
}

impl CheckoutConflict {
    /// The stable error code a client sees.
    pub fn code(&self) -> &'static str {
        "checkout_reserved"
    }

    /// The recovery path a client is offered.
    ///
    /// The caller may retry once the group finishes; it may not force the
    /// write through, and the group's permit is never released on the
    /// caller's behalf.
    pub fn recovery(&self) -> &'static str {
        "the checkout is reserved by a live agent-run group; retry after that group finishes, or write to a separate checkout"
    }
}

impl std::fmt::Display for CheckoutConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.lookup_failure {
            Some(reason) => write!(
                f,
                "checkout reservation could not be established ({}): {}",
                reason,
                self.recovery()
            ),
            None => write!(
                f,
                "checkout {} is reserved by live group {} (sink {}): {}",
                self.worktree_id,
                self.group_id,
                self.sink,
                self.recovery()
            ),
        }
    }
}

impl std::error::Error for CheckoutConflict {}

/// The checkout a non-broker writer targets.
///
/// The target is a durable identity — a workspace root resolved from the
/// daemon's workspace registry, or a managed worktree id — never a path
/// carried in a request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutWriteTarget {
    /// The canonical root of the workspace the write lands in.
    Root(std::path::PathBuf),
    /// A managed checkout addressed by its durable identity.
    Worktree(WorktreeId),
}

/// The daemon's mediation for non-broker checkout writers.
///
/// The mediator is read-only. It never takes the writer permit and never
/// waits on one: it answers whether the checkout is reserved, and the
/// caller refuses before its first effect.
pub struct CheckoutMediator {
    reservations: Option<Arc<CheckoutReservationService>>,
}

impl CheckoutMediator {
    /// Build the mediator.
    ///
    /// `None` disables mediation. That is the correct state for a
    /// local-only daemon that has no durable worktree or group state: it
    /// can never share a checkout, so it has nothing to refuse.
    pub fn new(reservations: Option<Arc<CheckoutReservationService>>) -> Arc<Self> {
        Arc::new(Self { reservations })
    }

    /// Whether this daemon has a durable reservation authority.
    pub fn is_enabled(&self) -> bool {
        self.reservations.is_some()
    }

    /// The reservation over the checkout that contains `root`.
    pub async fn reservation_for_root(
        &self,
        root: &Path,
    ) -> Result<CheckoutReservation, CheckoutReservationError> {
        let Some(service) = self.reservations.as_ref() else {
            return Ok(CheckoutReservation::Unreserved);
        };
        service.reservation_for_path(root).await
    }

    /// The reservation over a known managed checkout.
    pub async fn reservation_for_worktree(
        &self,
        worktree_id: &WorktreeId,
    ) -> Result<CheckoutReservation, CheckoutReservationError> {
        let Some(service) = self.reservations.as_ref() else {
            return Ok(CheckoutReservation::Unreserved);
        };
        service.reservation_for_worktree(worktree_id).await
    }
}

/// Emit the durable refusal event for a refused checkout writer.
///
/// This is the live owner of the `checkout_writer_refusal` audit action.
/// The event is attributed to the transport-bound principal, carries the
/// M003 decision provenance, and names the sink and the group. It carries
/// no path and no file content.
pub async fn emit_checkout_writer_refusal(
    emitter: &ExecutionAuditEmitter,
    principal: &AuthenticatedPrincipal,
    provenance: &AuditDecisionProvenance,
    chain: &AuditChainContext,
    sink: CheckoutWriterSink,
    group_id: &str,
    outcome: &str,
) {
    let builder = codegg_core::audit_instrumentation::checkout_writer_refusal_event(
        principal,
        provenance,
        chain,
        sink.as_str(),
        sink.as_str(),
        group_id,
        outcome,
    );
    emitter.emit(builder).await;
}

/// Declarative owner pins for the daemon-gate live audit hooks (M005 C004).
///
/// The audit coverage guard parses this table and fails closed if an owner
/// file moves or an emit symbol disappears. Removing a row — or the hook it
/// pins — must fail the guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaemonHookPin {
    /// Canonical audit action wire name.
    pub action: &'static str,
    /// Canonical owner file, relative to the repository root.
    pub owner_file: &'static str,
    /// Canonical emit symbol that must appear in the owner file.
    pub emit_symbol: &'static str,
}

/// The daemon-gate live hook owner table.
///
/// One row per daemon gate that owns a live audit action outside the
/// daemon operation mapping.
pub const DAEMON_HOOK_OWNER_PINS: &[DaemonHookPin] = &[
    DaemonHookPin {
        action: "checkout_writer_refusal",
        owner_file: "src/core/checkout_mediation.rs",
        emit_symbol: "emit_checkout_writer_refusal",
    },
    // M005 C005: the process admission gate refuses a process launch
    // before the process exists, so the refusal has no daemon operation
    // mapping either. The gate owns the live evidence.
    DaemonHookPin {
        action: "process_denial",
        owner_file: "src/tool/process_admission.rs",
        emit_symbol: "emit_process_denial",
    },
];

/// Actions that must have at least one owner pin in this table.
pub const PINNED_ACTIONS: &[&str] = &["checkout_writer_refusal", "process_denial"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_pins_cover_exactly_the_daemon_table() {
        use codegg_core::audit_instrumentation as instr;
        for action in PINNED_ACTIONS {
            assert!(
                instr::is_daemon_live_action(action),
                "pinned action {action} must be daemon-live in core"
            );
        }
        assert_eq!(instr::DAEMON_LIVE_AUDIT_HOOKS.len(), PINNED_ACTIONS.len());
        for hook in instr::DAEMON_LIVE_AUDIT_HOOKS {
            assert!(
                PINNED_ACTIONS.contains(&hook.action),
                "daemon hook {} needs an owner pin",
                hook.action
            );
            assert!(
                DAEMON_HOOK_OWNER_PINS
                    .iter()
                    .any(|pin| pin.action == hook.action),
                "no owner pin for {}",
                hook.action
            );
        }
    }

    #[test]
    fn a_conflict_names_the_group_and_never_a_path() {
        let conflict = CheckoutConflict {
            sink: CheckoutWriterSink::DocumentSave,
            worktree_id: "worktree-1".to_owned(),
            group_id: "group-1".to_owned(),
            owner_run_id: "run-1".to_owned(),
            lookup_failure: None,
        };
        let rendered = conflict.to_string();
        assert!(rendered.contains("group-1"));
        assert!(rendered.contains("document_save"));
        assert!(!rendered.contains('/'));
    }
}
