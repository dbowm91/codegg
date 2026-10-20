//! Atomic admission controller.
//!
//! Every admission decision is atomic: all requested dimensions and
//! exclusivity keys are reserved together, or none of them. A blocked
//! job never holds a partial reservation that could starve smaller
//! eligible work.
//!
//! The controller exposes a nonblocking `try_admit` so the scheduler
//! main loop can try candidates and advance past temporarily blocked
//! entries. Oversized jobs (request > configured budget) are reported
//! as `Unschedulable` and never silently clamped.
//!
//! The state lives behind a `parking_lot::Mutex` because the
//! admission decision is short, deterministic, and contended
//! between reconciliation ticks. The scheduler does not hold the
//! lock during executor awaits.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::scheduler::config::{QueueCaps, ResolvedSchedulerConfig, ResourceBudget};
use crate::scheduler::permit::{PermitDimensions, ResourcePermitGuard};

/// Controller-side cost of one remote reservation.
///
/// A remote attempt always moves the workspace to the node and streams
/// events back, so it occupies a small slice of the controller's own
/// network/transfer budget. This is deliberately *not* the remote job's
/// abstract `network_slots` token: that token describes network demand
/// on the *node*, which the node enforces.
pub const REMOTE_TRANSFER_SLOTS: u32 = 1;

/// Decision returned by [`AdmissionController::try_admit`].
#[derive(Debug)]
pub enum AdmissionDecision {
    /// All requested resources and exclusivity keys are reserved.
    Admitted(ResourcePermitGuard),
    /// Resources temporarily unavailable. The caller should advance
    /// to the next candidate.
    TemporarilyBlocked(BlockReason),
    /// Request exceeds the configured budget. The job cannot run
    /// without an admin override / config change.
    Impossible(UnschedulableReason),
}

/// Why a request was temporarily blocked. Each variant carries enough
/// context for the snapshot/diagnostic surface to report the cause.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum BlockReason {
    InsufficientProcessSlots {
        requested: u16,
        available: u16,
    },
    InsufficientCpuWeight {
        requested: u32,
        available: u32,
    },
    InsufficientMemory {
        requested: u64,
        available: u64,
    },
    InsufficientIoWeight {
        requested: u32,
        available: u32,
    },
    InsufficientNetworkSlots {
        requested: u16,
        available: u16,
    },
    KeyContended {
        key: String,
    },
    QueueFull,
    /// The named remote node is at its own concurrency ceiling.
    RemoteNodeCapacity {
        /// The node the reservation was for.
        node_id: String,
        /// Executions the reservation asked for.
        requested: u32,
        /// Executions the node still has room for.
        available: u32,
    },
    /// The controller is at its remote-transfer ceiling.
    RemoteTransferCapacity {
        /// Transfers the reservation asked for.
        requested: u32,
        /// Transfers the controller still has room for.
        available: u32,
    },
}

/// Why a request is structurally impossible to admit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum UnschedulableReason {
    ProcessSlotsExceedBudget {
        requested: u16,
        budget: u32,
    },
    CpuWeightExceedsBudget {
        requested: u32,
        budget: u32,
    },
    MemoryExceedsBudget {
        requested: u64,
        budget: u64,
    },
    IoWeightExceedsBudget {
        requested: u32,
        budget: u32,
    },
    NetworkSlotsExceedBudget {
        requested: u16,
        budget: u32,
    },
    /// The named remote node accepts no executions at all.
    RemoteNodeCapacityExceedsBudget {
        /// The node the reservation was for.
        node_id: String,
        /// Executions the reservation asked for.
        requested: u32,
        /// The node's ceiling.
        budget: u32,
    },
    /// The controller accepts no remote transfer at all.
    RemoteTransferCapacityExceedsBudget {
        /// Transfers the reservation asked for.
        requested: u32,
        /// The controller's ceiling.
        budget: u32,
    },
}

/// Internal admission state. Held under a `Mutex`; the lock is held
/// only for the brief decision/release critical section.
#[derive(Debug, Default)]
struct InnerState {
    used_process: u32,
    used_cpu: u32,
    used_memory: u64,
    used_io: u32,
    used_network: u32,
    /// Map of exclusivity key -> holder count. Multiple holders may
    /// share a non-conflicting key (e.g. `read-only`), but a
    /// conflicting key like `workspace-mutation` must be exclusive
    /// (count > 0).
    held_keys: HashMap<String, u32>,
    /// Remote transfers the controller has in flight. This is the
    /// controller's own cost of moving a workspace to a node, and it is
    /// budgeted separately from the local network demand.
    used_remote_transfers: u32,
    /// Map of remote node id -> admitted execution count.
    ///
    /// A remote job does not consume the controller's local CPU, memory,
    /// or process budget: those tokens describe the *remote node's*
    /// capacity, which the node enforces with its own cgroup limits. The
    /// controller's job is to bound how much remote work it has in flight
    /// against each node, so that a saturated node is not flooded with
    /// submissions it will only refuse.
    used_remote: HashMap<String, u32>,
    /// Total rejected (TemporarilyBlocked) requests since boot.
    rejected: u64,
    /// Total impossible (Unschedulable) requests since boot.
    impossible: u64,
}

/// Public summary counters for [`AdmissionState`].
#[derive(Debug, Default, Clone)]
pub struct AdmissionState {
    pub rejected: u64,
    pub impossible: u64,
    pub used_process: u32,
    pub used_cpu: u32,
    pub used_memory: u64,
    pub used_io: u32,
    pub used_network: u32,
    pub held_keys: HashMap<String, u32>,
    /// Admitted remote executions per node.
    pub used_remote: HashMap<String, u32>,
    /// Remote transfers the controller has in flight.
    pub used_remote_transfers: u32,
}

#[derive(Debug)]
pub struct AdmissionController {
    cfg: ResolvedSchedulerConfig,
    inner: Mutex<InnerState>,
    /// Total admitted count (for snapshots / overload events).
    admitted_total: AtomicU64,
}

impl AdmissionController {
    pub fn new(cfg: ResolvedSchedulerConfig) -> Self {
        Self {
            cfg,
            inner: Mutex::new(InnerState::default()),
            admitted_total: AtomicU64::new(0),
        }
    }

    pub fn config(&self) -> &ResolvedSchedulerConfig {
        &self.cfg
    }

    pub fn resource_budget(&self) -> &ResourceBudget {
        &self.cfg.resources
    }

    pub fn queue_caps(&self) -> &QueueCaps {
        &self.cfg.queue
    }

    pub fn used_process_slots(&self) -> u32 {
        self.inner.lock().used_process
    }

    pub fn admitted_total(&self) -> u64 {
        self.admitted_total.load(Ordering::SeqCst)
    }

    /// Nonblocking admission via a shared `Arc`. Returns
    /// [`AdmissionDecision`] with the reserved guard on success.
    /// The returned guard keeps the `Arc` alive so the controller
    /// can be released on drop without any further plumbing.
    pub fn try_admit_arc(self: &Arc<Self>, dims: &PermitDimensions) -> AdmissionDecision {
        let decision = self.try_admit(dims);
        match decision {
            AdmissionDecision::Admitted(_) => {
                let controller = Arc::clone(self);
                AdmissionDecision::Admitted(ResourcePermitGuard::new(controller, dims.clone()))
            }
            other => other,
        }
    }

    /// Nonblocking admission. Returns [`AdmissionDecision`] with the
    /// reserved guard on success.
    pub fn try_admit(&self, dims: &PermitDimensions) -> AdmissionDecision {
        // First, structural impossibility checks (request exceeds
        // budget). These never change under config reload (the budget
        // is captured at construction).
        if let Some(why) = self.check_impossible(dims) {
            let mut g = self.inner.lock();
            g.impossible += 1;
            return AdmissionDecision::Impossible(why);
        }

        let mut g = self.inner.lock();
        let budget = &self.cfg.resources;

        // A remote reservation is scoped to a node, not to the controller's
        // local resources. It consumes the node's own capacity key plus a
        // small controller transfer budget, and it keeps every exclusivity
        // key (workspace exclusivity is not a node-local property).
        if let Some(node_id) = dims.remote_node.as_deref() {
            let node_limit = self.cfg.remote_node_limit(node_id);
            let used = g.used_remote.get(node_id).copied().unwrap_or(0);
            if used + 1 > node_limit {
                g.rejected += 1;
                return AdmissionDecision::TemporarilyBlocked(BlockReason::RemoteNodeCapacity {
                    node_id: node_id.to_owned(),
                    requested: 1,
                    available: node_limit.saturating_sub(used),
                });
            }
            if g.used_remote_transfers + REMOTE_TRANSFER_SLOTS > budget.max_remote_transfers {
                g.rejected += 1;
                return AdmissionDecision::TemporarilyBlocked(
                    BlockReason::RemoteTransferCapacity {
                        requested: REMOTE_TRANSFER_SLOTS,
                        available: budget
                            .max_remote_transfers
                            .saturating_sub(g.used_remote_transfers),
                    },
                );
            }
            for key in &dims.exclusivity_keys {
                if key.starts_with("exclusive:") {
                    let bare = key.trim_start_matches("exclusive:").to_string();
                    if g.held_keys.get(&bare).copied().unwrap_or(0) > 0 {
                        g.rejected += 1;
                        return AdmissionDecision::TemporarilyBlocked(BlockReason::KeyContended {
                            key: bare,
                        });
                    }
                }
            }
            *g.used_remote.entry(node_id.to_owned()).or_insert(0) += 1;
            g.used_remote_transfers += REMOTE_TRANSFER_SLOTS;
            for key in &dims.exclusivity_keys {
                if key.starts_with("exclusive:") {
                    let bare = key.trim_start_matches("exclusive:").to_string();
                    *g.held_keys.entry(bare).or_insert(0) += 1;
                }
            }
            drop(g);
            self.admitted_total.fetch_add(1, Ordering::SeqCst);
            return AdmissionDecision::Admitted(ResourcePermitGuard::new_orphan(dims.clone()));
        }

        if g.used_process + dims.process_slots as u32 > budget.max_process_slots {
            g.rejected += 1;
            return AdmissionDecision::TemporarilyBlocked(BlockReason::InsufficientProcessSlots {
                requested: dims.process_slots,
                available: budget.max_process_slots.saturating_sub(g.used_process) as u16,
            });
        }
        if g.used_cpu + dims.cpu_weight > budget.max_cpu_weight {
            g.rejected += 1;
            return AdmissionDecision::TemporarilyBlocked(BlockReason::InsufficientCpuWeight {
                requested: dims.cpu_weight,
                available: budget.max_cpu_weight.saturating_sub(g.used_cpu),
            });
        }
        if g.used_memory + dims.memory_mb_hint > budget.max_memory_mb_hint {
            g.rejected += 1;
            return AdmissionDecision::TemporarilyBlocked(BlockReason::InsufficientMemory {
                requested: dims.memory_mb_hint,
                available: budget.max_memory_mb_hint.saturating_sub(g.used_memory),
            });
        }
        if g.used_io + dims.io_weight > budget.max_io_weight {
            g.rejected += 1;
            return AdmissionDecision::TemporarilyBlocked(BlockReason::InsufficientIoWeight {
                requested: dims.io_weight,
                available: budget.max_io_weight.saturating_sub(g.used_io),
            });
        }
        if g.used_network + dims.network_slots as u32 > budget.max_network_slots {
            g.rejected += 1;
            return AdmissionDecision::TemporarilyBlocked(BlockReason::InsufficientNetworkSlots {
                requested: dims.network_slots,
                available: budget.max_network_slots.saturating_sub(g.used_network) as u16,
            });
        }
        // Exclusivity keys: a key is "conflicting" if its name starts
        // with `exclusive:`. All other keys are read-only/no-conflict.
        for key in &dims.exclusivity_keys {
            if key.starts_with("exclusive:") {
                let bare = key.trim_start_matches("exclusive:").to_string();
                if g.held_keys.get(&bare).copied().unwrap_or(0) > 0 {
                    g.rejected += 1;
                    return AdmissionDecision::TemporarilyBlocked(BlockReason::KeyContended {
                        key: bare,
                    });
                }
            }
        }

        // Reserve atomically: all dimensions and keys update together
        // before releasing the lock.
        g.used_process += dims.process_slots as u32;
        g.used_cpu += dims.cpu_weight;
        g.used_memory += dims.memory_mb_hint;
        g.used_io += dims.io_weight;
        g.used_network += dims.network_slots as u32;
        for key in &dims.exclusivity_keys {
            if key.starts_with("exclusive:") {
                let bare = key.trim_start_matches("exclusive:").to_string();
                *g.held_keys.entry(bare).or_insert(0) += 1;
            }
        }
        drop(g);
        self.admitted_total.fetch_add(1, Ordering::SeqCst);

        // Caller wraps the guard in an Arc via `try_admit_arc` once
        // the controller is itself wrapped. Returning a raw guard
        // here keeps the borrow rules simple; `try_admit_arc` is the
        // scheduler-friendly entry point.
        AdmissionDecision::Admitted(ResourcePermitGuard::new_orphan(dims.clone()))
    }

    fn check_impossible(&self, dims: &PermitDimensions) -> Option<UnschedulableReason> {
        let budget = &self.cfg.resources;
        // A remote reservation is bounded by its node's own ceiling, not by
        // the controller's local process/CPU/memory budget: those tokens
        // describe the node's capacity, which the node enforces.
        if let Some(node_id) = dims.remote_node.as_deref() {
            // A node with a *configured* ceiling of zero can never take a
            // remote execution. An absent ceiling is not zero: it falls
            // back to the controller's process-slot budget, exactly as
            // `try_admit` does.
            if let Some(configured) = self.cfg.remote_node_capacity.get(node_id).copied() {
                if configured == 0 {
                    return Some(UnschedulableReason::RemoteNodeCapacityExceedsBudget {
                        node_id: node_id.to_owned(),
                        requested: 1,
                        budget: 0,
                    });
                }
            }
            if REMOTE_TRANSFER_SLOTS > budget.max_remote_transfers {
                return Some(UnschedulableReason::RemoteTransferCapacityExceedsBudget {
                    requested: REMOTE_TRANSFER_SLOTS,
                    budget: budget.max_remote_transfers,
                });
            }
            return None;
        }
        if dims.process_slots as u32 > budget.max_process_slots {
            return Some(UnschedulableReason::ProcessSlotsExceedBudget {
                requested: dims.process_slots,
                budget: budget.max_process_slots,
            });
        }
        if dims.cpu_weight > budget.max_cpu_weight {
            return Some(UnschedulableReason::CpuWeightExceedsBudget {
                requested: dims.cpu_weight,
                budget: budget.max_cpu_weight,
            });
        }
        if dims.memory_mb_hint > budget.max_memory_mb_hint {
            return Some(UnschedulableReason::MemoryExceedsBudget {
                requested: dims.memory_mb_hint,
                budget: budget.max_memory_mb_hint,
            });
        }
        if dims.io_weight > budget.max_io_weight {
            return Some(UnschedulableReason::IoWeightExceedsBudget {
                requested: dims.io_weight,
                budget: budget.max_io_weight,
            });
        }
        if dims.network_slots as u32 > budget.max_network_slots {
            return Some(UnschedulableReason::NetworkSlotsExceedBudget {
                requested: dims.network_slots,
                budget: budget.max_network_slots,
            });
        }
        None
    }

    /// Internal release. Called from `ResourcePermitGuard::Drop`.
    pub fn release(&self, dims: PermitDimensions) {
        let mut g = self.inner.lock();
        if let Some(node_id) = dims.remote_node.as_deref() {
            // The remote reservation charged the node's capacity key and
            // the controller's transfer budget — nothing else.
            if let Some(used) = g.used_remote.get_mut(node_id) {
                *used = used.saturating_sub(1);
                if *used == 0 {
                    g.used_remote.remove(node_id);
                }
            }
            g.used_remote_transfers = g
                .used_remote_transfers
                .saturating_sub(REMOTE_TRANSFER_SLOTS);
            for key in &dims.exclusivity_keys {
                if key.starts_with("exclusive:") {
                    let bare = key.trim_start_matches("exclusive:").to_string();
                    if let Some(c) = g.held_keys.get_mut(&bare) {
                        *c = c.saturating_sub(1);
                        if *c == 0 {
                            g.held_keys.remove(&bare);
                        }
                    }
                }
            }
            return;
        }
        g.used_process = g.used_process.saturating_sub(dims.process_slots as u32);
        g.used_cpu = g.used_cpu.saturating_sub(dims.cpu_weight);
        g.used_memory = g.used_memory.saturating_sub(dims.memory_mb_hint);
        g.used_io = g.used_io.saturating_sub(dims.io_weight);
        g.used_network = g.used_network.saturating_sub(dims.network_slots as u32);
        for key in &dims.exclusivity_keys {
            if key.starts_with("exclusive:") {
                let bare = key.trim_start_matches("exclusive:").to_string();
                if let Some(c) = g.held_keys.get_mut(&bare) {
                    *c = c.saturating_sub(1);
                    if *c == 0 {
                        g.held_keys.remove(&bare);
                    }
                }
            }
        }
    }

    pub fn snapshot(&self) -> AdmissionState {
        let g = self.inner.lock();
        AdmissionState {
            rejected: g.rejected,
            impossible: g.impossible,
            used_process: g.used_process,
            used_cpu: g.used_cpu,
            used_memory: g.used_memory,
            used_io: g.used_io,
            used_network: g.used_network,
            held_keys: g.held_keys.clone(),
            used_remote: g.used_remote.clone(),
            used_remote_transfers: g.used_remote_transfers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::permit::permit_from_request;

    fn make() -> AdmissionController {
        AdmissionController::new(ResolvedSchedulerConfig::default())
    }

    /// A remote reservation for one node, with the abstract tokens a
    /// remote job of that kind would carry.
    fn remote_dims(node_id: &str) -> PermitDimensions {
        let mut dims = permit_from_request(3, 2048, 1, 3, 0, vec![]);
        dims.remote_node = Some(node_id.to_owned());
        dims
    }

    #[test]
    fn a_remote_reservation_does_not_consume_local_cpu_or_process_capacity() {
        let c = Arc::new(make());
        // Four remote build-shaped reservations, each with a CPU weight of
        // 3 and a 2 GiB memory hint — more than the whole local budget.
        let mut guards = Vec::new();
        for _ in 0..4 {
            let dims = remote_dims("node-1");
            match c.try_admit_arc(&dims) {
                AdmissionDecision::Admitted(g) => guards.push(g),
                other => panic!("expected Admitted, got {:?}", other),
            }
        }
        let state = c.snapshot();
        assert_eq!(state.used_cpu, 0, "remote work is not local CPU");
        assert_eq!(state.used_memory, 0, "remote work is not local memory");
        assert_eq!(state.used_process, 0, "remote work is not a local slot");
        assert_eq!(
            state.used_remote_transfers, 4,
            "each remote attempt costs one controller transfer slot"
        );
        assert_eq!(
            state.used_network, 0,
            "a remote transfer does not consume the local network budget"
        );
        assert_eq!(state.used_remote.get("node-1").copied(), Some(4));
        drop(guards);
    }

    #[test]
    fn remote_reservations_on_two_nodes_use_separate_budgets() {
        let c = Arc::new(make());
        let mut guards = Vec::new();
        for node in ["node-1", "node-2"] {
            for _ in 0..4 {
                let dims = remote_dims(node);
                match c.try_admit_arc(&dims) {
                    AdmissionDecision::Admitted(g) => guards.push(g),
                    other => panic!("expected Admitted, got {:?}", other),
                }
            }
        }
        let state = c.snapshot();
        assert_eq!(state.used_remote.get("node-1").copied(), Some(4));
        assert_eq!(state.used_remote.get("node-2").copied(), Some(4));
        assert_eq!(state.used_process, 0);
        assert_eq!(
            state.used_remote_transfers, 8,
            "each remote attempt costs one controller transfer slot"
        );
        assert_eq!(
            state.used_network, 0,
            "a remote transfer does not consume the local network budget"
        );
        drop(guards);
    }

    #[test]
    fn a_node_without_a_configured_ceiling_is_still_admissible() {
        // The absence of a per-node ceiling is not a ceiling of zero: it
        // falls back to the controller's process-slot budget, so remote
        // work is not silently unschedulable.
        let c = Arc::new(make());
        match c.try_admit_arc(&remote_dims("node-without-a-ceiling")) {
            AdmissionDecision::Admitted(_) => {}
            other => panic!("expected Admitted, got {:?}", other),
        }
    }

    #[test]
    fn a_node_ceiling_blocks_the_next_remote_reservation() {
        let cfg = ResolvedSchedulerConfig::default()
            .with_remote_node_capacity([("node-1".to_owned(), 2u32)].into_iter().collect());
        let c = Arc::new(AdmissionController::new(cfg));
        let mut guards = Vec::new();
        for _ in 0..2 {
            match c.try_admit_arc(&remote_dims("node-1")) {
                AdmissionDecision::Admitted(g) => guards.push(g),
                other => panic!("expected Admitted, got {:?}", other),
            }
        }
        match c.try_admit_arc(&remote_dims("node-1")) {
            AdmissionDecision::TemporarilyBlocked(BlockReason::RemoteNodeCapacity {
                node_id,
                requested,
                available,
            }) => {
                assert_eq!(node_id, "node-1");
                assert_eq!(requested, 1);
                assert_eq!(available, 0);
            }
            other => panic!("expected RemoteNodeCapacity block, got {:?}", other),
        }
        // A different node is unaffected.
        match c.try_admit_arc(&remote_dims("node-2")) {
            AdmissionDecision::Admitted(g) => guards.push(g),
            other => panic!("expected Admitted, got {:?}", other),
        }
        drop(guards);
    }

    #[test]
    fn the_controller_transfer_ceiling_is_its_own_budget() {
        let cfg = ResolvedSchedulerConfig::default();
        let c = Arc::new(AdmissionController::new(cfg));
        // The default transfer budget is above the per-node ceiling, so
        // the node key binds first.
        let mut guards = Vec::new();
        for _ in 0..4 {
            match c.try_admit_arc(&remote_dims("node-1")) {
                AdmissionDecision::Admitted(g) => guards.push(g),
                other => panic!("expected Admitted, got {:?}", other),
            }
        }
        assert!(matches!(
            c.try_admit_arc(&remote_dims("node-1")),
            AdmissionDecision::TemporarilyBlocked(BlockReason::RemoteNodeCapacity { .. })
        ));
        drop(guards);
    }

    #[test]
    fn a_released_remote_reservation_frees_the_node_capacity_key() {
        let cfg = ResolvedSchedulerConfig::default()
            .with_remote_node_capacity([("node-1".to_owned(), 1u32)].into_iter().collect());
        let c = Arc::new(AdmissionController::new(cfg));
        let guard = match c.try_admit_arc(&remote_dims("node-1")) {
            AdmissionDecision::Admitted(g) => g,
            other => panic!("expected Admitted, got {:?}", other),
        };
        assert!(matches!(
            c.try_admit_arc(&remote_dims("node-1")),
            AdmissionDecision::TemporarilyBlocked(BlockReason::RemoteNodeCapacity { .. })
        ));
        drop(guard);
        match c.try_admit_arc(&remote_dims("node-1")) {
            AdmissionDecision::Admitted(_) => {}
            other => panic!("expected Admitted after release, got {:?}", other),
        }
        let state = c.snapshot();
        assert_eq!(
            state.used_remote_transfers, 0,
            "the transfer slot is released too"
        );
    }

    #[test]
    fn a_remote_reservation_keeps_workspace_exclusivity() {
        let c = Arc::new(make());
        let mut dims = remote_dims("node-1");
        dims.exclusivity_keys = vec!["exclusive:workspace-mutation".to_owned()];
        let guard = match c.try_admit_arc(&dims) {
            AdmissionDecision::Admitted(g) => g,
            other => panic!("expected Admitted, got {:?}", other),
        };
        let mut other = remote_dims("node-1");
        other.exclusivity_keys = vec!["exclusive:workspace-mutation".to_owned()];
        match c.try_admit_arc(&other) {
            AdmissionDecision::TemporarilyBlocked(BlockReason::KeyContended { key }) => {
                assert_eq!(key, "workspace-mutation");
            }
            decision => panic!("expected KeyContended, got {:?}", decision),
        }
        drop(guard);
    }

    #[test]
    fn local_work_still_consumes_the_local_budget() {
        let c = Arc::new(make());
        let mut guards = Vec::new();
        for _ in 0..4 {
            match c.try_admit_arc(&permit_from_request(2, 1024, 1, 1, 0, vec![])) {
                AdmissionDecision::Admitted(g) => guards.push(g),
                other => panic!("expected Admitted, got {:?}", other),
            }
        }
        let state = c.snapshot();
        assert_eq!(state.used_process, 4);
        assert_eq!(state.used_cpu, 8);
        // The local process budget is full, so a fifth local job is
        // blocked — remote work does not change that.
        assert!(matches!(
            c.try_admit_arc(&permit_from_request(1, 256, 1, 1, 0, vec![])),
            AdmissionDecision::TemporarilyBlocked(BlockReason::InsufficientProcessSlots { .. })
        ));
        drop(guards);
    }

    #[test]
    fn remote_work_is_admitted_while_the_local_budget_is_full() {
        let c = Arc::new(make());
        let mut guards = Vec::new();
        for _ in 0..4 {
            match c.try_admit_arc(&permit_from_request(2, 1024, 1, 1, 0, vec![])) {
                AdmissionDecision::Admitted(g) => guards.push(g),
                other => panic!("expected Admitted, got {:?}", other),
            }
        }
        // The local CPU budget is exhausted, but a remote job does not
        // draw on it.
        match c.try_admit_arc(&remote_dims("node-1")) {
            AdmissionDecision::Admitted(g) => guards.push(g),
            other => panic!("expected Admitted, got {:?}", other),
        }
        drop(guards);
    }

    #[test]
    fn admit_consumes_capacity() {
        let c = Arc::new(make());
        let d = permit_from_request(2, 1024, 1, 1, 0, vec![]);
        let _g = match c.try_admit_arc(&d) {
            AdmissionDecision::Admitted(g) => g,
            other => panic!("expected Admitted, got {:?}", other),
        };
        assert_eq!(c.used_process_slots(), 1);
    }

    #[test]
    fn block_when_process_slots_full() {
        let c = Arc::new(make());
        let d = permit_from_request(0, 0, 4, 0, 0, vec![]);
        let _g = match c.try_admit_arc(&d) {
            AdmissionDecision::Admitted(g) => g,
            _ => panic!(),
        };
        let d2 = permit_from_request(0, 0, 1, 0, 0, vec![]);
        match c.try_admit_arc(&d2) {
            AdmissionDecision::TemporarilyBlocked(BlockReason::InsufficientProcessSlots {
                ..
            }) => {}
            other => panic!("expected TemporarilyBlocked, got {:?}", other),
        }
    }

    #[test]
    fn impossible_when_request_exceeds_budget() {
        let c = Arc::new(make());
        let d = permit_from_request(0, 0, 100, 0, 0, vec![]);
        match c.try_admit_arc(&d) {
            AdmissionDecision::Impossible(UnschedulableReason::ProcessSlotsExceedBudget {
                ..
            }) => {}
            other => panic!("expected Impossible, got {:?}", other),
        }
    }

    #[test]
    fn exclusive_key_blocks() {
        let c = Arc::new(make());
        let d1 = permit_from_request(1, 0, 1, 0, 0, vec!["exclusive:workspace-mutation".into()]);
        let _g = match c.try_admit_arc(&d1) {
            AdmissionDecision::Admitted(g) => g,
            _ => panic!(),
        };
        let d2 = permit_from_request(0, 0, 1, 0, 0, vec!["exclusive:workspace-mutation".into()]);
        match c.try_admit_arc(&d2) {
            AdmissionDecision::TemporarilyBlocked(BlockReason::KeyContended { .. }) => {}
            other => panic!("expected KeyContended, got {:?}", other),
        }
    }

    #[test]
    fn release_restores_capacity() {
        let c = Arc::new(make());
        let d = permit_from_request(1, 0, 1, 0, 0, vec![]);
        let g = match c.try_admit_arc(&d) {
            AdmissionDecision::Admitted(g) => g,
            _ => panic!(),
        };
        drop(g);
        assert_eq!(c.used_process_slots(), 0);
        let d2 = permit_from_request(0, 0, 4, 0, 0, vec![]);
        let _g2 = match c.try_admit_arc(&d2) {
            AdmissionDecision::Admitted(g) => g,
            other => panic!("expected admit after release, got {:?}", other),
        };
    }

    #[test]
    fn partial_release_does_not_happen() {
        // If one dimension would block, the entire admission must be
        // rejected without partial reservation.
        let c = Arc::new(make());
        // budget: 4 process, 8 cpu.
        // Take 2 process + 4 cpu.
        let d1 = permit_from_request(4, 0, 2, 0, 0, vec![]);
        let _g = match c.try_admit_arc(&d1) {
            AdmissionDecision::Admitted(g) => g,
            _ => panic!(),
        };
        // Take 1 more process + 4 cpu. Both fit (3/4 process, 8/8 cpu).
        let d2 = permit_from_request(4, 0, 1, 0, 0, vec![]);
        let _g2 = match c.try_admit_arc(&d2) {
            AdmissionDecision::Admitted(g) => g,
            other => panic!("expected Admitted, got {:?}", other),
        };
        // Now try 1 process + 1 cpu (would exceed 8 cpu total).
        let d3 = permit_from_request(1, 0, 1, 0, 0, vec![]);
        match c.try_admit_arc(&d3) {
            AdmissionDecision::TemporarilyBlocked(BlockReason::InsufficientCpuWeight {
                ..
            }) => {}
            other => panic!("expected blocked, got {:?}", other),
        }
        // And the cpu used should still be 8 (not 9).
        let snap = c.snapshot();
        assert_eq!(snap.used_cpu, 8);
        assert_eq!(snap.used_process, 3);
    }
}
