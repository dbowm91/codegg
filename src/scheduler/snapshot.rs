//! Scheduler snapshot types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use codegg_core::jobs::JobPriority;
use codegg_core::workspace::WorkspaceId;

use crate::scheduler::admission::AdmissionState;
use crate::scheduler::executor::ExecutorHealth;

/// Stable redacted label for an exclusivity key. The snapshot must
/// never echo raw paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExclusivityKeyLabel(pub String);

impl ExclusivityKeyLabel {
    pub fn from_key(key: &str) -> Self {
        // Strip path prefixes by hashing the canonical absolute
        // path component. We only stable-hash; the original key is
        // never reconstructed.
        let mut h: u64 = 1469598103934665603;
        for b in key.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        Self(format!("key:{:016x}", h))
    }
}

/// Per-workspace queue + running summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerWorkspaceSummary {
    pub workspace_id: WorkspaceId,
    pub queued: usize,
    pub running: usize,
    pub ready_window: usize,
}

impl Default for PerWorkspaceSummary {
    fn default() -> Self {
        Self {
            workspace_id: WorkspaceId::new_unchecked(""),
            queued: 0,
            running: 0,
            ready_window: 0,
        }
    }
}

/// Per-priority counts of queued jobs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotCounts {
    pub by_priority: BTreeMap<String, usize>,
    pub by_kind: BTreeMap<String, usize>,
}

/// Summary of resource usage.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceSummary {
    pub used_process: u32,
    pub budget_process: u32,
    pub used_cpu: u32,
    pub budget_cpu: u32,
    pub used_memory_mb: u64,
    pub budget_memory_mb: u64,
    pub used_io: u32,
    pub budget_io: u32,
    pub used_network: u32,
    pub budget_network: u32,
    pub held_exclusivity_keys: Vec<ExclusivityKeyLabel>,
}

impl ResourceSummary {
    pub fn from_admission(state: &AdmissionState, max: &ResourceBudgetView) -> Self {
        Self {
            used_process: state.used_process,
            budget_process: max.max_process_slots,
            used_cpu: state.used_cpu,
            budget_cpu: max.max_cpu_weight,
            used_memory_mb: state.used_memory,
            budget_memory_mb: max.max_memory_mb_hint,
            used_io: state.used_io,
            budget_io: max.max_io_weight,
            used_network: state.used_network,
            budget_network: max.max_network_slots,
            held_exclusivity_keys: state
                .held_keys
                .keys()
                .map(|k| ExclusivityKeyLabel::from_key(k))
                .collect(),
        }
    }
}

/// Plain read-only view of the resource budget for snapshot
/// composition.
#[derive(Debug, Clone, Copy)]
pub struct ResourceBudgetView {
    pub max_process_slots: u32,
    pub max_cpu_weight: u32,
    pub max_memory_mb_hint: u64,
    pub max_io_weight: u32,
    pub max_network_slots: u32,
}

/// Per-executor health snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorHealthSnapshot {
    pub executor: String,
    pub health: ExecutorHealth,
    pub total_invocations: u64,
    pub total_failures: u64,
}

/// Aggregate overload summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OverloadSummary {
    pub rejected_admissions: u64,
    pub impossible_admissions: u64,
    pub queue_overflows: u64,
}

/// Admission block summary by reason. Bounded: keeps the most
/// recent N reasons with counters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdmissionBlockSummary {
    pub total: u64,
    pub by_reason: BTreeMap<String, u64>,
}

/// Snapshot of the scheduler's externally visible state. Returned by
/// `JobScheduler::snapshot()` and serialized over the protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerSnapshot {
    pub ready_window_count: usize,
    pub durable_queued_count: usize,
    pub running_attempts: usize,
    pub per_priority: SnapshotCounts,
    pub per_workspace: Vec<PerWorkspaceSummary>,
    pub resources: ResourceSummary,
    pub executors: Vec<ExecutorHealthSnapshot>,
    pub overload: OverloadSummary,
    pub admission_blocks: AdmissionBlockSummary,
    pub oldest_queued_age_secs: Option<u64>,
    pub rollout_mode: String,
    pub enabled: bool,
}

impl SchedulerSnapshot {
    pub fn priority_label(p: JobPriority) -> &'static str {
        p.as_str()
    }
}
