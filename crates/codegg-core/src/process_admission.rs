//! M005 C005 — the authoritative process admission authority.
//!
//! # What this module is
//!
//! The C003 fence mediates the **model's native writers**, and the C004
//! checkout gate mediates every **non-broker** writer CodeGG can point at a
//! checkout. Neither of them covers the surface that neither of them can
//! contain: an **OS process**.
//!
//! A native file write names the file it is about to touch, so it can be
//! contained to a checkout and serialized against other writers. A shell
//! command does not. `make test`, `cargo build`, a Python script, a formatter,
//! a test runner and an interactive PTY all name a *program*, and the program's
//! write set is decided at runtime by whatever the program decides to do. The
//! only honest containment for a process is the process group or job object
//! that owns it — and CodeGG's managed process group is explicitly escapable:
//! a descendant that calls `setsid` leaves the group and survives its
//! parent's cleanup (see `src/managed_process.rs`).
//!
//! So under the restricted shared-checkout profile the answer is not "contain
//! the process" but **"refuse the process"**. This module is the authority
//! that answers that question from durable state:
//!
//! 1. The target checkout is resolved to a **canonical checkout identity** —
//!    the same identity the C004 reservation check uses, so an alias path
//!    resolves to the same checkout.
//! 2. The checkout's worktree record names the **lease owner run**.
//! 3. The owner run belongs to a **live** (non-terminal) agent-run group.
//!
//! A checkout with a live group behind it is *reserved*: it is a shared
//! checkout, and the restricted profile forbids every unqualified process
//! surface there. A checkout with no live group behind it is ordinary: shells
//! keep working exactly as they did before this milestone.
//!
//! # What this module is not
//!
//! It is **not** a process supervisor, a sandbox, or a promise that a
//! process's descendants are quiet. It refuses a launch; it does not contain
//! one. It is also not a filesystem-exclusion mechanism: a process that
//! CodeGG launched *before* a checkout became reserved keeps whatever
//! authority it had, which is why
//! [`ProcessAdmissionService::preflight_reservation`] refuses a reservation
//! over a checkout that still has a live persistent process.
//!
//! Ownership: this crate owns the typed state machine. The root crate's
//! daemon adapter supplies the scope and decides what a refusal means for the
//! caller.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::checkout_reservation::{
    canonical_identity, CheckoutReservation, CheckoutReservationService,
};
use crate::identity::WorktreeId;

// ---------------------------------------------------------------------------
// Process kind
// ---------------------------------------------------------------------------

/// The kind of process a launch would create.
///
/// The kind is a **closed enum** on purpose. The denial contract names the
/// capability that was refused, and a free-text label would let a future
/// surface slip through untyped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProcessKind {
    /// A shell command run for its output (`bash`).
    Command,
    /// An interactive terminal session.
    Pty,
    /// A language server.
    LanguageServer,
    /// A local MCP server.
    McpServer,
    /// A process-backed plugin.
    PluginProcess,
    /// A lifecycle hook command.
    HookCommand,
    /// A Tool Program.
    ToolProgram,
    /// A managed background process.
    ManagedProcess,
    /// A scheduled job.
    ScheduledJob,
    /// A persistent process CodeGG could not classify.
    ///
    /// Unknown is not "probably fine". It is refused under a reserved
    /// checkout, exactly like an explicitly unknown effect class in the
    /// C003 fence.
    Unknown,
}

impl ProcessKind {
    /// The canonical name recorded in the denial event and the disposition
    /// manifest.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Pty => "pty",
            Self::LanguageServer => "language_server",
            Self::McpServer => "mcp_server",
            Self::PluginProcess => "plugin_process",
            Self::HookCommand => "hook_command",
            Self::ToolProgram => "tool_program",
            Self::ManagedProcess => "managed_process",
            Self::ScheduledJob => "scheduled_job",
            Self::Unknown => "unknown",
        }
    }

    /// Parse the canonical name.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "command" => Some(Self::Command),
            "pty" => Some(Self::Pty),
            "language_server" => Some(Self::LanguageServer),
            "mcp_server" => Some(Self::McpServer),
            "plugin_process" => Some(Self::PluginProcess),
            "hook_command" => Some(Self::HookCommand),
            "tool_program" => Some(Self::ToolProgram),
            "managed_process" => Some(Self::ManagedProcess),
            "scheduled_job" => Some(Self::ScheduledJob),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

impl std::fmt::Display for ProcessKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Denial
// ---------------------------------------------------------------------------

/// Why a process launch was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProcessDenialReason {
    /// The target checkout is reserved by a live agent-run group.
    ///
    /// The restricted shared profile excludes every unqualified process
    /// surface: a process's write set is not statically enumerable, so it
    /// cannot be contained to the checkout and cannot be serialized against
    /// another writer.
    SharedCheckout,
    /// A reservation was refused because a persistent process is still live
    /// in the affected checkout.
    ///
    /// A process that started before the checkout became reserved keeps the
    /// authority it had. Rather than pretend it is gone, the reservation is
    /// refused until the process is demonstrably drained.
    PersistentProcessPresent,
    /// The authority could not answer, so the launch was refused.
    ///
    /// Absence of an answer is not an admission.
    AuthorityUnavailable,
}

impl ProcessDenialReason {
    /// The canonical wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SharedCheckout => "shared_checkout",
            Self::PersistentProcessPresent => "persistent_process_present",
            Self::AuthorityUnavailable => "authority_unavailable",
        }
    }
    /// Parse the canonical wire name.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "shared_checkout" => Some(Self::SharedCheckout),
            "persistent_process_present" => Some(Self::PersistentProcessPresent),
            "authority_unavailable" => Some(Self::AuthorityUnavailable),
            _ => None,
        }
    }
}

impl std::fmt::Display for ProcessDenialReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The typed refusal a process launch receives.
///
/// The denial names the **capability** that was refused, the **profile** that
/// refused it and the **remediation**. It never names a path, a command, a
/// file body, a permit id or a sibling run's identity: a denial is a
/// concurrency answer, not an authorization one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDenial {
    /// Why the launch was refused.
    pub reason: ProcessDenialReason,
    /// The kind of process that was refused.
    pub kind: ProcessKind,
    /// The durable worktree identity of the reserved checkout, when the
    /// refusal is about a reservation.
    pub worktree_id: Option<String>,
    /// The live agent-run group that holds the checkout, when the refusal is
    /// about a reservation.
    pub group_id: Option<String>,
}

impl ProcessDenial {
    /// The stable error code a client sees.
    pub fn code(&self) -> &'static str {
        "process_denied"
    }

    /// The recovery path a caller is offered.
    ///
    /// The supported fallback is the existing isolated worktree. There is no
    /// "retry without the profile" path: the profile is not a best-effort
    /// filter, and a caller cannot opt a process back in.
    pub fn recovery(&self) -> &'static str {
        match self.reason {
            ProcessDenialReason::SharedCheckout => {
                "this checkout is reserved by a live agent-run group, which admits no unqualified process; use an isolated checkout instead"
            }
            ProcessDenialReason::PersistentProcessPresent => {
                "a persistent process is still live in this checkout; the reservation is refused until it is drained"
            }
            ProcessDenialReason::AuthorityUnavailable => {
                "the process admission authority could not be consulted; the launch is refused until it can be"
            }
        }
    }
}

impl std::fmt::Display for ProcessDenial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.worktree_id, &self.group_id) {
            (Some(worktree), Some(group)) => write!(
                f,
                "refusing a {} process: checkout {} is reserved by live group {}: {}",
                self.kind.as_str(),
                worktree,
                group,
                self.recovery()
            ),
            _ => write!(
                f,
                "refusing a {} process: {}: {}",
                self.kind.as_str(),
                self.reason.as_str(),
                self.recovery()
            ),
        }
    }
}

impl std::error::Error for ProcessDenial {}

/// The outcome of a process admission check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessAdmission {
    /// The launch may proceed.
    Admitted,
    /// The launch is refused, before any process exists.
    Denied(ProcessDenial),
}

impl ProcessAdmission {
    /// Whether the launch may proceed.
    pub fn is_admitted(&self) -> bool {
        matches!(self, Self::Admitted)
    }

    /// The denial, when the launch was refused.
    pub fn denial(&self) -> Option<&ProcessDenial> {
        match self {
            Self::Admitted => None,
            Self::Denied(denial) => Some(denial),
        }
    }
}

// ---------------------------------------------------------------------------
// Persistent process registry
// ---------------------------------------------------------------------------

/// A live persistent process, as the admission authority sees it.
///
/// The entry records **where** the process can write, not **what** it can
/// write. The registry exists to answer one question: is there a process
/// that was started under an older authority and is still alive?
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistentProcessEntry {
    /// The daemon-assigned identity of the process.
    pub process_id: String,
    /// The kind of persistent process.
    pub kind: ProcessKind,
    /// The checkout the process was started in.
    pub checkout_root: PathBuf,
}

impl PersistentProcessEntry {
    /// A redacted, human-readable label.
    ///
    /// It names the kind and the durable identity only. It never carries a
    /// command line, a path or any other free text that could carry user
    /// content into a log or an audit record.
    pub fn label(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.process_id)
    }
}

/// The registry of live persistent processes.
///
/// The registry is **in-memory** on purpose. A persistent process is a
/// property of a running daemon, not a durable fact: when the daemon stops,
/// the processes it started are gone with it. Making the registry durable
/// would create rows that outlive their meaning.
#[derive(Debug, Default)]
pub struct PersistentProcessRegistry {
    inner: Mutex<HashMap<String, PersistentProcessEntry>>,
}

impl PersistentProcessRegistry {
    /// Build an empty registry.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Record a live persistent process.
    ///
    /// Registering the same process id twice replaces the entry, so a
    /// restarted service does not leave a stale root behind.
    pub fn register(&self, entry: PersistentProcessEntry) {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            // A poisoned lock means another thread panicked while holding
            // it. The registry is advisory state; losing it must not wedge
            // the daemon, and the admission check fails closed anyway when
            // the authority itself cannot answer.
            Err(poisoned) => poisoned.into_inner(),
        };
        inner.insert(entry.process_id.clone(), entry);
    }

    /// Forget a process. Returns whether the process was known.
    pub fn release(&self, process_id: &str) -> bool {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        };
        inner.remove(process_id).is_some()
    }

    /// The live persistent processes rooted in `root` or below it.
    ///
    /// The comparison is on canonical identities, so an alias path — a
    /// symlinked parent, a trailing `..` — resolves to the same checkout.
    pub fn live_under(&self, root: &Path) -> Vec<PersistentProcessEntry> {
        let canonical_root = canonical_identity(root);
        let inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut live: Vec<PersistentProcessEntry> = inner
            .values()
            .filter(|entry| {
                let entry_root = canonical_identity(&entry.checkout_root);
                entry_root == canonical_root || entry_root.starts_with(&canonical_root)
            })
            .cloned()
            .collect();
        // A stable order keeps the refusal reason deterministic.
        live.sort_by(|left, right| left.process_id.cmp(&right.process_id));
        live
    }

    /// The number of live persistent processes.
    pub fn len(&self) -> usize {
        match self.inner.lock() {
            Ok(inner) => inner.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }

    /// Whether no persistent process is live.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------------------
// The service
// ---------------------------------------------------------------------------

/// Why the admission authority could not answer.
#[derive(Debug, thiserror::Error)]
pub enum ProcessAdmissionError {
    /// The durable state backing the lookup could not be read.
    #[error("process admission lookup failed: {0}")]
    Lookup(String),
}

/// The authoritative process admission service.
///
/// The service is **read-only with respect to durable state**: it consults
/// the C004 reservation authority and the live process registry, and it
/// answers a question. It never takes a permit, never waits on one, and
/// never holds a lock across an effect. That keeps it free of the
/// lock-order inversion the C003 closure record warned about.
pub struct ProcessAdmissionService {
    reservations: Option<Arc<CheckoutReservationService>>,
    processes: Arc<PersistentProcessRegistry>,
}

impl ProcessAdmissionService {
    /// Build the service.
    ///
    /// `reservations` is optional because a local-only daemon has no durable
    /// group store. Without it there are no live groups, so no checkout is
    /// ever reserved — which is the correct answer for a single-user daemon
    /// that never shares a checkout.
    pub fn new(
        reservations: Option<Arc<CheckoutReservationService>>,
        processes: Arc<PersistentProcessRegistry>,
    ) -> Arc<Self> {
        Arc::new(Self {
            reservations,
            processes,
        })
    }

    /// The live persistent process registry.
    pub fn processes(&self) -> &Arc<PersistentProcessRegistry> {
        &self.processes
    }

    /// Whether this daemon has a durable reservation authority.
    pub fn is_enabled(&self) -> bool {
        self.reservations.is_some()
    }

    /// The admission answer for a checkout scope.
    ///
    /// `scope_root` is the **daemon-resolved** checkout root of the
    /// execution — never a model-supplied working directory. A model that
    /// points a process at `/tmp` does not thereby leave the reserved
    /// checkout: the process's write set is unconstrained either way, so the
    /// decision is made about the execution's scope.
    pub async fn admit_scope(
        &self,
        scope_root: &Path,
        kind: ProcessKind,
    ) -> Result<ProcessAdmission, ProcessAdmissionError> {
        let Some(reservations) = self.reservations.as_ref() else {
            return Ok(ProcessAdmission::Admitted);
        };
        let reservation = match reservations.reservation_for_path(scope_root).await {
            Ok(reservation) => reservation,
            Err(_error) => {
                // Fail closed: an authority that cannot answer is not an
                // admission.
                return Ok(ProcessAdmission::Denied(ProcessDenial {
                    reason: ProcessDenialReason::AuthorityUnavailable,
                    kind,
                    worktree_id: None,
                    group_id: None,
                }));
            }
        };
        Ok(Self::deny_if_reserved(reservation, kind))
    }

    /// The admission answer for a known managed checkout.
    pub async fn admit_worktree(
        &self,
        worktree_id: &WorktreeId,
        kind: ProcessKind,
    ) -> Result<ProcessAdmission, ProcessAdmissionError> {
        let Some(reservations) = self.reservations.as_ref() else {
            return Ok(ProcessAdmission::Admitted);
        };
        let reservation = match reservations.reservation_for_worktree(worktree_id).await {
            Ok(reservation) => reservation,
            Err(_error) => {
                return Ok(ProcessAdmission::Denied(ProcessDenial {
                    reason: ProcessDenialReason::AuthorityUnavailable,
                    kind,
                    worktree_id: Some(worktree_id.to_string()),
                    group_id: None,
                }));
            }
        };
        Ok(Self::deny_if_reserved(reservation, kind))
    }

    /// The refusal a reservation produces.
    fn deny_if_reserved(reservation: CheckoutReservation, kind: ProcessKind) -> ProcessAdmission {
        match reservation {
            CheckoutReservation::Unreserved => ProcessAdmission::Admitted,
            CheckoutReservation::Reserved(claim) => ProcessAdmission::Denied(ProcessDenial {
                reason: ProcessDenialReason::SharedCheckout,
                kind,
                worktree_id: Some(claim.worktree_id.to_string()),
                group_id: Some(claim.group_id.to_string()),
            }),
        }
    }

    /// The preflight a **reservation** must pass.
    ///
    /// A persistent process that was started before the checkout became
    /// reserved keeps the authority it had: it was launched under a
    /// checkout that was not yet shared, and nothing has stopped it from
    /// writing. Rather than pretend such a process is gone, the reservation
    /// is refused while one is live in the affected checkout.
    ///
    /// The caller may retry once the process is drained. The refusal never
    /// kills the process: the process's lifetime belongs to its own owner.
    pub async fn preflight_reservation(
        &self,
        worktree_id: &WorktreeId,
    ) -> Result<ProcessAdmission, ProcessAdmissionError> {
        let Some(reservations) = self.reservations.as_ref() else {
            return Ok(ProcessAdmission::Admitted);
        };
        let record = reservations
            .worktree_record(worktree_id)
            .await
            .map_err(|error| ProcessAdmissionError::Lookup(error.to_string()))?;
        let Some(record) = record else {
            return Ok(ProcessAdmission::Admitted);
        };
        let live = self.processes.live_under(&record.path);
        if live.is_empty() {
            return Ok(ProcessAdmission::Admitted);
        }
        Ok(ProcessAdmission::Denied(ProcessDenial {
            reason: ProcessDenialReason::PersistentProcessPresent,
            kind: ProcessKind::Unknown,
            worktree_id: Some(worktree_id.to_string()),
            group_id: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_denial_names_the_capability_and_the_recovery() {
        let denial = ProcessDenial {
            reason: ProcessDenialReason::SharedCheckout,
            kind: ProcessKind::Command,
            worktree_id: Some("wt-1".to_owned()),
            group_id: Some("grp-1".to_owned()),
        };
        let rendered = denial.to_string();
        assert!(rendered.contains("command"));
        assert!(rendered.contains("grp-1"));
        assert!(rendered.contains("isolated checkout"));
    }

    #[test]
    fn the_registry_matches_a_process_to_its_checkout() {
        let registry = PersistentProcessRegistry::new();
        let root = std::env::temp_dir().join("codegg-c005-registry");
        registry.register(PersistentProcessEntry {
            process_id: "pty-1".to_owned(),
            kind: ProcessKind::Pty,
            checkout_root: root.clone(),
        });
        assert_eq!(registry.live_under(&root).len(), 1);
        assert!(registry
            .live_under(&std::env::temp_dir().join("codegg-c005-other"))
            .is_empty());
        assert!(registry.release("pty-1"));
        assert!(registry.live_under(&root).is_empty());
    }

    #[test]
    fn an_unreserved_checkout_admits_every_process() {
        let admission = ProcessAdmission::Admitted;
        assert!(admission.is_admitted());
        assert!(admission.denial().is_none());
    }
}
