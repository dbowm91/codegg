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
    InsufficientProcessSlots { requested: u16, available: u16 },
    InsufficientCpuWeight { requested: u32, available: u32 },
    InsufficientMemory { requested: u64, available: u64 },
    InsufficientIoWeight { requested: u32, available: u32 },
    InsufficientNetworkSlots { requested: u16, available: u16 },
    KeyContended { key: String },
    QueueFull,
}

/// Why a request is structurally impossible to admit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum UnschedulableReason {
    ProcessSlotsExceedBudget { requested: u16, budget: u32 },
    CpuWeightExceedsBudget { requested: u32, budget: u32 },
    MemoryExceedsBudget { requested: u64, budget: u64 },
    IoWeightExceedsBudget { requested: u32, budget: u32 },
    NetworkSlotsExceedBudget { requested: u16, budget: u32 },
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
