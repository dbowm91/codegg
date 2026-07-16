//! Scheduler configuration: validated, resolved, and default.
//!
//! The schema mirrors the proposed `[scheduler]` section from
//! the Phase 5 plan. The config struct is deserialized from the
//! `Config` root, validated, and frozen into `ResolvedSchedulerConfig`
//! that the scheduler, queue, and admission controller consume.
//!
//! Defaults are conservative: the scheduler is enabled in observe
//! mode for migrated families (Test, Build, Lint, Format, Subagent)
//! but the rollout mode controls actual dispatch authority.
//!
//! The on-disk [`SchedulerConfig`] type is owned by `codegg-config` so
//! there is a single source of truth for the user's settings file;
//! this module re-exports it for ergonomic access and adds the
//! resolved/validated layers above it.

pub use codegg_config::{
    SchedulerConfig, SchedulerFairnessConfig, SchedulerQueueConfig, SchedulerResourceConfig,
    SchedulerRolloutConfig as SchedulerRolloutMode,
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchedulerConfigError {
    #[error("max_process_slots must be > 0 when scheduler enabled")]
    ZeroProcessSlots,
    #[error("max_cpu_weight must be > 0 when scheduler enabled")]
    ZeroCpuWeight,
    #[error("max_memory_mb_hint must be > 0 when scheduler enabled")]
    ZeroMemory,
    #[error("max_io_weight must be > 0 when scheduler enabled")]
    ZeroIoWeight,
    #[error("max_network_slots must be > 0 when scheduler enabled")]
    ZeroNetworkSlots,
    #[error("max_total must be > 0 when scheduler enabled")]
    ZeroQueueTotal,
    #[error("max_per_workspace must be > 0 when scheduler enabled")]
    ZeroPerWorkspace,
    #[error("max_high_priority_burst must be > 0 when scheduler enabled")]
    ZeroHighPriorityBurst,
    #[error("weight must be > 0: {0}")]
    ZeroWeight(&'static str),
    #[error("per_workspace queue cap {per_workspace} exceeds global cap {total}")]
    PerWorkspaceExceedsTotal { per_workspace: usize, total: usize },
    #[error("claim_batch must be > 0 and <= max_total")]
    InvalidClaimBatch,
}

/// Resolved configuration. All `Option`s are filled and validated; the
/// admission controller, fair queue, and scheduler main loop consume
/// only this struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSchedulerConfig {
    pub enabled: bool,
    pub rollout: SchedulerRolloutMode,
    pub reconcile_interval_ms: u64,
    pub resources: ResourceBudget,
    pub queue: QueueCaps,
    pub fairness: FairnessWeights,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceBudget {
    pub max_process_slots: u32,
    pub max_cpu_weight: u32,
    pub max_memory_mb_hint: u64,
    pub max_io_weight: u32,
    pub max_network_slots: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueCaps {
    pub max_total: usize,
    pub max_per_workspace: usize,
    pub max_interactive_per_session: usize,
    pub claim_batch: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FairnessWeights {
    pub interactive_weight: u32,
    pub normal_weight: u32,
    pub background_weight: u32,
    pub maintenance_weight: u32,
    pub max_high_priority_burst: u32,
    pub aging_secs: u64,
}

impl Default for ResolvedSchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rollout: SchedulerRolloutMode::default(),
            reconcile_interval_ms: 1000,
            resources: ResourceBudget {
                max_process_slots: 4,
                max_cpu_weight: 8,
                max_memory_mb_hint: 8192,
                max_io_weight: 8,
                max_network_slots: 4,
            },
            queue: QueueCaps {
                max_total: 256,
                max_per_workspace: 64,
                max_interactive_per_session: 8,
                claim_batch: 32,
            },
            fairness: FairnessWeights {
                interactive_weight: 8,
                normal_weight: 4,
                background_weight: 2,
                maintenance_weight: 1,
                max_high_priority_burst: 8,
                aging_secs: 300,
            },
        }
    }
}

impl ResolvedSchedulerConfig {
    pub fn from_input(input: Option<&SchedulerConfig>) -> Result<Self, SchedulerConfigError> {
        let mut out = ResolvedSchedulerConfig::default();
        let Some(cfg) = input else {
            return Ok(out);
        };
        if let Some(e) = cfg.enabled {
            out.enabled = e;
        }
        if let Some(m) = cfg.rollout {
            out.rollout = m;
        }
        if let Some(r) = cfg.reconcile_interval_ms {
            out.reconcile_interval_ms = r;
        }
        if let Some(r) = &cfg.resources {
            if let Some(v) = r.max_process_slots {
                out.resources.max_process_slots = v;
            }
            if let Some(v) = r.max_cpu_weight {
                out.resources.max_cpu_weight = v;
            }
            if let Some(v) = r.max_memory_mb_hint {
                out.resources.max_memory_mb_hint = v;
            }
            if let Some(v) = r.max_io_weight {
                out.resources.max_io_weight = v;
            }
            if let Some(v) = r.max_network_slots {
                out.resources.max_network_slots = v;
            }
        }
        if let Some(q) = &cfg.queue {
            if let Some(v) = q.max_total {
                out.queue.max_total = v;
            }
            if let Some(v) = q.max_per_workspace {
                out.queue.max_per_workspace = v;
            }
            if let Some(v) = q.max_interactive_per_session {
                out.queue.max_interactive_per_session = v;
            }
            if let Some(v) = q.claim_batch {
                out.queue.claim_batch = v;
            }
        }
        if let Some(f) = &cfg.fairness {
            if let Some(v) = f.interactive_weight {
                out.fairness.interactive_weight = v;
            }
            if let Some(v) = f.normal_weight {
                out.fairness.normal_weight = v;
            }
            if let Some(v) = f.background_weight {
                out.fairness.background_weight = v;
            }
            if let Some(v) = f.maintenance_weight {
                out.fairness.maintenance_weight = v;
            }
            if let Some(v) = f.max_high_priority_burst {
                out.fairness.max_high_priority_burst = v;
            }
            if let Some(v) = f.aging_secs {
                out.fairness.aging_secs = v;
            }
        }
        if out.enabled {
            Self::validate(&out)?;
        }
        Ok(out)
    }

    pub fn validate(&self) -> Result<(), SchedulerConfigError> {
        // When the scheduler is disabled, callers may set any value
        // (e.g. zero) — the scheduler is not in use. The preflight
        // public API still validates the structural budget so
        // `enabled = true` configs are not silently misconfigured.
        if !self.enabled {
            return Ok(());
        }
        if self.resources.max_process_slots == 0 {
            return Err(SchedulerConfigError::ZeroProcessSlots);
        }
        if self.resources.max_cpu_weight == 0 {
            return Err(SchedulerConfigError::ZeroCpuWeight);
        }
        if self.resources.max_memory_mb_hint == 0 {
            return Err(SchedulerConfigError::ZeroMemory);
        }
        if self.resources.max_io_weight == 0 {
            return Err(SchedulerConfigError::ZeroIoWeight);
        }
        if self.resources.max_network_slots == 0 {
            return Err(SchedulerConfigError::ZeroNetworkSlots);
        }
        if self.queue.max_total == 0 {
            return Err(SchedulerConfigError::ZeroQueueTotal);
        }
        if self.queue.max_per_workspace == 0 {
            return Err(SchedulerConfigError::ZeroPerWorkspace);
        }
        if self.queue.max_per_workspace > self.queue.max_total {
            return Err(SchedulerConfigError::PerWorkspaceExceedsTotal {
                per_workspace: self.queue.max_per_workspace,
                total: self.queue.max_total,
            });
        }
        if self.queue.claim_batch == 0 || self.queue.claim_batch > self.queue.max_total {
            return Err(SchedulerConfigError::InvalidClaimBatch);
        }
        if self.fairness.max_high_priority_burst == 0 {
            return Err(SchedulerConfigError::ZeroHighPriorityBurst);
        }
        for (name, w) in [
            ("interactive_weight", self.fairness.interactive_weight),
            ("normal_weight", self.fairness.normal_weight),
            ("background_weight", self.fairness.background_weight),
            ("maintenance_weight", self.fairness.maintenance_weight),
        ] {
            if w == 0 {
                return Err(SchedulerConfigError::ZeroWeight(name));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let cfg = ResolvedSchedulerConfig::default();
        cfg.validate().unwrap();
    }

    #[test]
    fn zero_process_slots_rejected() {
        let mut cfg = ResolvedSchedulerConfig::default();
        cfg.resources.max_process_slots = 0;
        assert!(matches!(
            cfg.validate(),
            Err(SchedulerConfigError::ZeroProcessSlots)
        ));
    }

    #[test]
    fn per_workspace_cannot_exceed_total() {
        let mut cfg = ResolvedSchedulerConfig::default();
        cfg.queue.max_total = 10;
        cfg.queue.max_per_workspace = 20;
        assert!(matches!(
            cfg.validate(),
            Err(SchedulerConfigError::PerWorkspaceExceedsTotal { .. })
        ));
    }

    #[test]
    fn disabled_skips_validation() {
        let mut cfg = ResolvedSchedulerConfig::default();
        cfg.enabled = false;
        cfg.resources.max_process_slots = 0;
        cfg.validate().unwrap();
    }

    #[test]
    fn from_input_none_returns_default() {
        let cfg = ResolvedSchedulerConfig::from_input(None).unwrap();
        assert_eq!(cfg, ResolvedSchedulerConfig::default());
    }
}
