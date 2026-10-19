//! Remote resource limits for Eggwork-targeted jobs.
//!
//! This module owns the only conversion between CodeGG's remote limit
//! vocabulary and the node's physical cgroup limits. It exists because the
//! two vocabularies are *not* interchangeable:
//!
//! - CodeGG's [`codegg_core::jobs::ResourceRequest`] carries **abstract
//!   scheduler tokens**: a CPU *weight*, a memory *hint*, a slot count.
//!   They describe how the local admission controller should weigh a job.
//!   They are not physical quantities.
//! - The node's [`eggwork_core::ResourceRequirements`] carries **physical
//!   limits** in the units the kernel enforces: bytes, a CPU quota, and a
//!   task count.
//!
//! Translating one into the other is invalid, and this module never does
//! it. A job that asks for a remote limit asks in the node's own units,
//! through a separate, explicitly named field.
//!
//! ## The `cpu_millis` trap
//!
//! The node's CPU field is named `cpu_millis`, which reads like a
//! millisecond CPU-time budget. It is not. It is a CPU **quota** expressed
//! in *milli-percent of one CPU*, and the node renders it as
//! `CPUQuota = value/10 . value%10 %`. So `1000` is one full CPU
//! (`100.0%`), `500` is half a CPU (`50.0%`), and `2000` is two CPUs
//! (`200.0%`). Treating it as a time budget would ask for a thousandth of
//! the intended CPU, or — in the other direction — for a thousand times
//! too much. [`cpu_quota_percent`] is the single place that mapping is
//! written down.

use std::collections::BTreeSet;

use codegg_config::schema::{
    RemoteLimitEnforcement, RemoteResourceLimit, RemoteResourceLimits,
    MAX_REMOTE_CPU_QUOTA_MILLI_PERCENT, MAX_REMOTE_MEMORY_BYTES, MAX_REMOTE_PIDS,
};
use eggwork_core::{Requirement, ResourceRequirements};

/// Node capability required to enforce a resident-memory limit.
pub const CAPABILITY_MEMORY: &str = "resources.cgroups-v2.memory";
/// Node capability required to enforce a CPU quota limit.
pub const CAPABILITY_CPU: &str = "resources.cgroups-v2.cpu";
/// Node capability required to enforce a process/thread limit.
pub const CAPABILITY_PIDS: &str = "resources.cgroups-v2.pids";

/// The three dimensions a remote limit can constrain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteLimitDimension {
    /// Resident memory, in bytes.
    Memory,
    /// CPU quota, in milli-percent of one CPU.
    Cpu,
    /// Process/thread count.
    Pids,
}

impl RemoteLimitDimension {
    /// Stable name used in refusals and records.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory_bytes",
            Self::Cpu => "cpu_quota_milli_percent",
            Self::Pids => "pids",
        }
    }

    /// The node capability that indicates the dimension can be enforced.
    pub fn capability(self) -> &'static str {
        match self {
            Self::Memory => CAPABILITY_MEMORY,
            Self::Cpu => CAPABILITY_CPU,
            Self::Pids => CAPABILITY_PIDS,
        }
    }

    /// The largest value the node accepts for this dimension.
    pub fn max_value(self) -> u64 {
        match self {
            Self::Memory => MAX_REMOTE_MEMORY_BYTES,
            Self::Cpu => MAX_REMOTE_CPU_QUOTA_MILLI_PERCENT,
            Self::Pids => MAX_REMOTE_PIDS,
        }
    }
}

/// The dimensions in a fixed order, so diagnostics are deterministic.
pub const REMOTE_LIMIT_DIMENSIONS: [RemoteLimitDimension; 3] = [
    RemoteLimitDimension::Memory,
    RemoteLimitDimension::Cpu,
    RemoteLimitDimension::Pids,
];

/// Why a remote limit request was refused.
///
/// Every variant is a refusal. There is no "ignore it and carry on" path:
/// a limit that cannot be honored exactly is a limit that was not applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteLimitRejection {
    /// The job asked for a limit the operator never authorized.
    ///
    /// An absent ceiling means the operator wrote no limit for that
    /// dimension. A job that supplies one anyway is asking for something
    /// outside the operator's policy, so it is refused rather than
    /// silently accepted.
    NotConfigured {
        /// The dimension the job asked for.
        dimension: &'static str,
    },
    /// The job asked for more than the operator's ceiling allows.
    ExceedsCeiling {
        /// The dimension the job asked for.
        dimension: &'static str,
        /// The value the job asked for.
        requested: u64,
        /// The operator's ceiling for that dimension.
        ceiling: u64,
    },
    /// The job tried to weaken a limit the operator made mandatory.
    DowngradeRefused {
        /// The dimension the job tried to weaken.
        dimension: &'static str,
    },
    /// The value is not usable by the node.
    InvalidValue {
        /// The dimension with the unusable value.
        dimension: &'static str,
        /// Why the value was refused.
        reason: String,
    },
    /// The node cannot enforce a limit the job requires.
    CapabilityUnavailable {
        /// The dimension that could not be enforced.
        dimension: &'static str,
        /// The capability the node did not advertise.
        capability: &'static str,
    },
}

impl RemoteLimitRejection {
    /// Stable machine-readable code for the refusal.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConfigured { .. } => "remote_limit_not_configured",
            Self::ExceedsCeiling { .. } => "remote_limit_exceeds_ceiling",
            Self::DowngradeRefused { .. } => "remote_limit_downgrade_refused",
            Self::InvalidValue { .. } => "remote_limit_invalid_value",
            Self::CapabilityUnavailable { .. } => "remote_limit_capability_unavailable",
        }
    }
}

impl std::fmt::Display for RemoteLimitRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured { dimension } => write!(
                f,
                "remote limit '{dimension}' is not configured for this node"
            ),
            Self::ExceedsCeiling {
                dimension,
                requested,
                ceiling,
            } => write!(
                f,
                "remote limit '{dimension}' of {requested} exceeds the configured ceiling of {ceiling}"
            ),
            Self::DowngradeRefused { dimension } => write!(
                f,
                "remote limit '{dimension}' may not be weakened to best-effort"
            ),
            Self::InvalidValue { dimension, reason } => {
                write!(f, "remote limit '{dimension}' is invalid: {reason}")
            }
            Self::CapabilityUnavailable {
                dimension,
                capability,
            } => write!(
                f,
                "node does not advertise '{capability}', so the required remote limit '{dimension}' cannot be enforced"
            ),
        }
    }
}

/// The limits one remote attempt will actually run under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRemoteLimits {
    /// What the node was asked to enforce.
    pub requirements: ResourceRequirements,
    /// The limits as they were requested, for the durable record.
    pub requested: RemoteResourceLimits,
}

impl EffectiveRemoteLimits {
    /// `true` when no dimension is limited.
    pub fn is_empty(&self) -> bool {
        self.requested.is_empty()
    }
}

/// Render a CPU quota limit in the cgroup units the node applies.
///
/// The node's field is named `cpu_millis`, but it is a CPU **quota** in
/// *milli-percent of one CPU*, not a CPU-time budget. The node turns it
/// into `CPUQuota = value/10 . value%10 %`, so `1000` is `100.0%` (one
/// full CPU), `500` is `50.0%`, and `2000` is `200.0%` (two CPUs).
pub fn cpu_quota_percent(milli_percent: u64) -> String {
    format!("{}.{}%", milli_percent / 10, milli_percent % 10)
}

/// The limit configured for one dimension, if any.
fn limit_for(
    limits: &RemoteResourceLimits,
    dimension: RemoteLimitDimension,
) -> Option<RemoteResourceLimit> {
    match dimension {
        RemoteLimitDimension::Memory => limits.memory_bytes,
        RemoteLimitDimension::Cpu => limits.cpu_quota_milli_percent,
        RemoteLimitDimension::Pids => limits.pids,
    }
}

/// Combine an operator ceiling with a job's request into the limits the
/// node will be asked to enforce.
///
/// The rules, in order of authority:
///
/// 1. The **operator ceiling** is authoritative. A job may narrow a
///    dimension; it may never widen one and it may never introduce a
///    dimension the operator did not configure.
/// 2. A job may not weaken a `required` ceiling to `best_effort`. That
///    attempt is refused, not silently applied.
/// 3. A job may *strengthen* a `best_effort` ceiling to `required`.
///
/// This phase uses only the operator's configuration, so it can run at
/// the host boundary before the job becomes durable.
pub fn combine_limits(
    ceiling: Option<&RemoteResourceLimits>,
    request: Option<&RemoteResourceLimits>,
) -> Result<Option<RemoteResourceLimits>, RemoteLimitRejection> {
    let Some(ceiling) = ceiling else {
        if request.is_some_and(|request| !request.is_empty()) {
            // The operator configured no limits for this node. A job that
            // supplies some is asking for something outside that policy.
            // Refuse rather than invent a ceiling on the job's behalf.
            for dimension in REMOTE_LIMIT_DIMENSIONS {
                if request
                    .and_then(|request| limit_for(request, dimension))
                    .is_some()
                {
                    return Err(RemoteLimitRejection::NotConfigured {
                        dimension: dimension.as_str(),
                    });
                }
            }
        }
        return Ok(None);
    };

    let mut effective = RemoteResourceLimits::default();

    for dimension in REMOTE_LIMIT_DIMENSIONS {
        let operator_limit = limit_for(ceiling, dimension);
        let job_limit = request.and_then(|request| limit_for(request, dimension));

        let (value, enforcement) = match (operator_limit, job_limit) {
            (None, None) => continue,
            (Some(operator), None) => (operator.value, operator.enforcement),
            (None, Some(_)) => {
                return Err(RemoteLimitRejection::NotConfigured {
                    dimension: dimension.as_str(),
                });
            }
            (Some(operator), Some(job)) => {
                if job.value > operator.value {
                    return Err(RemoteLimitRejection::ExceedsCeiling {
                        dimension: dimension.as_str(),
                        requested: job.value,
                        ceiling: operator.value,
                    });
                }
                let enforcement = match (operator.enforcement, job.enforcement) {
                    (RemoteLimitEnforcement::Required, RemoteLimitEnforcement::BestEffort) => {
                        return Err(RemoteLimitRejection::DowngradeRefused {
                            dimension: dimension.as_str(),
                        });
                    }
                    (operator, job) => {
                        // `required` wins over `best_effort` in every
                        // remaining combination: a job may strengthen a
                        // limit, never weaken one.
                        if operator == RemoteLimitEnforcement::Required
                            || job == RemoteLimitEnforcement::Required
                        {
                            RemoteLimitEnforcement::Required
                        } else {
                            RemoteLimitEnforcement::BestEffort
                        }
                    }
                };
                (job.value, enforcement)
            }
        };

        if value == 0 {
            return Err(RemoteLimitRejection::InvalidValue {
                dimension: dimension.as_str(),
                reason: "limit value must be greater than zero; zero is not 'unlimited'".to_owned(),
            });
        }
        if value > dimension.max_value() {
            return Err(RemoteLimitRejection::InvalidValue {
                dimension: dimension.as_str(),
                reason: format!(
                    "limit value exceeds the node's maximum of {}",
                    dimension.max_value()
                ),
            });
        }

        let limit = RemoteResourceLimit { value, enforcement };
        match dimension {
            RemoteLimitDimension::Memory => effective.memory_bytes = Some(limit),
            RemoteLimitDimension::Cpu => effective.cpu_quota_milli_percent = Some(limit),
            RemoteLimitDimension::Pids => effective.pids = Some(limit),
        }
    }

    if effective.is_empty() {
        return Ok(None);
    }
    Ok(Some(effective))
}

/// Resolve the effective limits and gate them on the node's capabilities.
///
/// `capabilities` is the node's *fresh* advertised feature set. Passing a
/// stale or empty set is safe: it refuses required limits instead of
/// assuming the node can enforce them.
pub fn resolve_remote_limits(
    ceiling: Option<&RemoteResourceLimits>,
    request: Option<&RemoteResourceLimits>,
    capabilities: &BTreeSet<String>,
) -> Result<Option<EffectiveRemoteLimits>, RemoteLimitRejection> {
    let Some(effective) = combine_limits(ceiling, request)? else {
        return Ok(None);
    };

    let mut requirements = ResourceRequirements {
        memory_bytes: Requirement::NotRequested,
        cpu_millis: Requirement::NotRequested,
        pids: Requirement::NotRequested,
    };

    for dimension in REMOTE_LIMIT_DIMENSIONS {
        let Some(limit) = limit_for(&effective, dimension) else {
            continue;
        };
        if limit.enforcement == RemoteLimitEnforcement::Required
            && !capabilities.contains(dimension.capability())
        {
            return Err(RemoteLimitRejection::CapabilityUnavailable {
                dimension: dimension.as_str(),
                capability: dimension.capability(),
            });
        }
        match dimension {
            RemoteLimitDimension::Memory => {
                requirements.memory_bytes = requirement(&limit);
            }
            RemoteLimitDimension::Cpu => {
                requirements.cpu_millis = requirement(&limit);
            }
            RemoteLimitDimension::Pids => {
                requirements.pids = match limit.enforcement {
                    RemoteLimitEnforcement::Required => Requirement::Required(limit.value as u32),
                    RemoteLimitEnforcement::BestEffort => {
                        Requirement::BestEffort(limit.value as u32)
                    }
                };
            }
        }
    }

    Ok(Some(EffectiveRemoteLimits {
        requirements,
        requested: effective,
    }))
}

/// Map one limit onto the node's per-dimension requirement.
fn requirement(limit: &RemoteResourceLimit) -> Requirement<u64> {
    match limit.enforcement {
        RemoteLimitEnforcement::Required => Requirement::Required(limit.value),
        RemoteLimitEnforcement::BestEffort => Requirement::BestEffort(limit.value),
    }
}

/// The capabilities a set of effective limits needs from the node.
///
/// Used by the operator surface to explain *why* a node was refused.
pub fn required_capabilities(limits: &RemoteResourceLimits) -> Vec<&'static str> {
    let mut required = Vec::new();
    for dimension in REMOTE_LIMIT_DIMENSIONS {
        let Some(limit) = limit_for(limits, dimension) else {
            continue;
        };
        if limit.enforcement == RemoteLimitEnforcement::Required {
            required.push(dimension.capability());
        }
    }
    required
}

/// A short, bounded, human-readable description of the effective limits.
///
/// Used in durable records and refusals. It never contains endpoints or
/// key paths — only dimensions, values, and enforcement kinds.
pub fn describe_limits(limits: &RemoteResourceLimits) -> String {
    let mut parts = Vec::new();
    for dimension in REMOTE_LIMIT_DIMENSIONS {
        let Some(limit) = limit_for(limits, dimension) else {
            continue;
        };
        let enforcement = match limit.enforcement {
            RemoteLimitEnforcement::Required => "required",
            RemoteLimitEnforcement::BestEffort => "best_effort",
        };
        parts.push(format!(
            "{}={} ({enforcement})",
            dimension.as_str(),
            limit.value
        ));
    }
    if parts.is_empty() {
        "none".to_owned()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_config::schema::RemoteResourceLimit;

    fn limit(value: u64, enforcement: RemoteLimitEnforcement) -> RemoteResourceLimit {
        RemoteResourceLimit { value, enforcement }
    }

    fn capabilities(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn all_capabilities() -> BTreeSet<String> {
        capabilities(&[CAPABILITY_MEMORY, CAPABILITY_CPU, CAPABILITY_PIDS])
    }

    #[test]
    fn cpu_quota_is_milli_percent_not_a_time_budget() {
        // The node renders `cpu_millis` as `CPUQuota = value/10 . value%10 %`.
        assert_eq!(cpu_quota_percent(1000), "100.0%");
        assert_eq!(cpu_quota_percent(500), "50.0%");
        assert_eq!(cpu_quota_percent(2000), "200.0%");
        assert_eq!(cpu_quota_percent(1), "0.1%");
    }

    #[test]
    fn absent_ceiling_and_absent_request_mean_no_limits() {
        let resolved = resolve_remote_limits(None, None, &all_capabilities()).expect("no limits");
        assert!(resolved.is_none());
    }

    #[test]
    fn operator_ceiling_applies_without_a_job_request() {
        let ceiling = RemoteResourceLimits {
            memory_bytes: Some(limit(512 * 1024 * 1024, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let resolved = resolve_remote_limits(Some(&ceiling), None, &all_capabilities())
            .expect("ceiling applies")
            .expect("limits present");
        assert_eq!(
            resolved.requirements.memory_bytes,
            Requirement::Required(512 * 1024 * 1024)
        );
        assert_eq!(
            resolved.requirements.cpu_millis,
            Requirement::<u64>::NotRequested
        );
    }

    #[test]
    fn a_job_may_narrow_a_ceiling() {
        let ceiling = RemoteResourceLimits {
            memory_bytes: Some(limit(1024, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let request = RemoteResourceLimits {
            memory_bytes: Some(limit(512, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let resolved = resolve_remote_limits(Some(&ceiling), Some(&request), &all_capabilities())
            .expect("narrowing is allowed")
            .expect("limits present");
        assert_eq!(
            resolved.requirements.memory_bytes,
            Requirement::Required(512)
        );
    }

    #[test]
    fn a_job_may_not_widen_a_ceiling() {
        let ceiling = RemoteResourceLimits {
            memory_bytes: Some(limit(1024, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let request = RemoteResourceLimits {
            memory_bytes: Some(limit(2048, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let refusal = resolve_remote_limits(Some(&ceiling), Some(&request), &all_capabilities())
            .expect_err("widening is refused");
        assert_eq!(refusal.code(), "remote_limit_exceeds_ceiling");
        assert_eq!(
            refusal,
            RemoteLimitRejection::ExceedsCeiling {
                dimension: "memory_bytes",
                requested: 2048,
                ceiling: 1024,
            }
        );
    }

    #[test]
    fn a_job_may_not_weaken_a_required_limit() {
        let ceiling = RemoteResourceLimits {
            cpu_quota_milli_percent: Some(limit(1000, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let request = RemoteResourceLimits {
            cpu_quota_milli_percent: Some(limit(1000, RemoteLimitEnforcement::BestEffort)),
            ..RemoteResourceLimits::default()
        };
        let refusal = resolve_remote_limits(Some(&ceiling), Some(&request), &all_capabilities())
            .expect_err("downgrade is refused");
        assert_eq!(refusal.code(), "remote_limit_downgrade_refused");
    }

    #[test]
    fn a_job_may_strengthen_a_best_effort_limit() {
        let ceiling = RemoteResourceLimits {
            pids: Some(limit(64, RemoteLimitEnforcement::BestEffort)),
            ..RemoteResourceLimits::default()
        };
        let request = RemoteResourceLimits {
            pids: Some(limit(32, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let resolved = resolve_remote_limits(Some(&ceiling), Some(&request), &all_capabilities())
            .expect("strengthening is allowed")
            .expect("limits present");
        assert_eq!(resolved.requirements.pids, Requirement::Required(32));
    }

    #[test]
    fn a_job_may_not_invent_a_dimension_the_operator_never_configured() {
        let ceiling = RemoteResourceLimits {
            memory_bytes: Some(limit(1024, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let request = RemoteResourceLimits {
            pids: Some(limit(8, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let refusal = resolve_remote_limits(Some(&ceiling), Some(&request), &all_capabilities())
            .expect_err("an unconfigured dimension is refused");
        assert_eq!(refusal.code(), "remote_limit_not_configured");
    }

    #[test]
    fn a_request_without_any_ceiling_is_refused() {
        let request = RemoteResourceLimits {
            memory_bytes: Some(limit(1024, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let refusal =
            resolve_remote_limits(None, Some(&request), &all_capabilities()).expect_err("refused");
        assert_eq!(refusal.code(), "remote_limit_not_configured");
    }

    #[test]
    fn a_required_limit_without_the_node_capability_is_refused() {
        let ceiling = RemoteResourceLimits {
            memory_bytes: Some(limit(1024, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let refusal = resolve_remote_limits(
            Some(&ceiling),
            None,
            &capabilities(&[CAPABILITY_CPU, CAPABILITY_PIDS]),
        )
        .expect_err("a required limit needs its capability");
        assert_eq!(refusal.code(), "remote_limit_capability_unavailable");
    }

    #[test]
    fn a_best_effort_limit_survives_a_node_without_the_capability() {
        let ceiling = RemoteResourceLimits {
            memory_bytes: Some(limit(1024, RemoteLimitEnforcement::BestEffort)),
            ..RemoteResourceLimits::default()
        };
        let resolved = resolve_remote_limits(Some(&ceiling), None, &BTreeSet::new())
            .expect("best effort needs no capability")
            .expect("limits present");
        assert_eq!(
            resolved.requirements.memory_bytes,
            Requirement::BestEffort(1024)
        );
    }

    #[test]
    fn a_zero_limit_is_refused_rather_than_read_as_unlimited() {
        let ceiling = RemoteResourceLimits {
            memory_bytes: Some(limit(0, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let refusal =
            resolve_remote_limits(Some(&ceiling), None, &all_capabilities()).expect_err("refused");
        assert_eq!(refusal.code(), "remote_limit_invalid_value");
    }

    #[test]
    fn a_limit_beyond_the_node_bound_is_refused_rather_than_clamped() {
        let ceiling = RemoteResourceLimits {
            cpu_quota_milli_percent: Some(limit(
                MAX_REMOTE_CPU_QUOTA_MILLI_PERCENT + 1,
                RemoteLimitEnforcement::Required,
            )),
            ..RemoteResourceLimits::default()
        };
        let refusal =
            resolve_remote_limits(Some(&ceiling), None, &all_capabilities()).expect_err("refused");
        assert_eq!(refusal.code(), "remote_limit_invalid_value");
    }

    #[test]
    fn required_capabilities_lists_only_mandatory_dimensions() {
        let limits = RemoteResourceLimits {
            memory_bytes: Some(limit(1024, RemoteLimitEnforcement::Required)),
            cpu_quota_milli_percent: Some(limit(500, RemoteLimitEnforcement::BestEffort)),
            pids: Some(limit(8, RemoteLimitEnforcement::Required)),
        };
        assert_eq!(
            required_capabilities(&limits),
            vec![CAPABILITY_MEMORY, CAPABILITY_PIDS]
        );
    }

    #[test]
    fn describe_limits_is_bounded_and_carries_no_secrets() {
        let limits = RemoteResourceLimits {
            memory_bytes: Some(limit(1024, RemoteLimitEnforcement::Required)),
            ..RemoteResourceLimits::default()
        };
        let described = describe_limits(&limits);
        assert_eq!(described, "memory_bytes=1024 (required)");
        assert!(!described.contains("https://"));
        assert!(!described.contains('/'));
    }
}
