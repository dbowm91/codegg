//! M001: daemon-owned admission policy for explicit remote job targets.
//!
//! A job or schedule that names an Eggwork node crosses a trust
//! boundary: the work leaves the local machine and runs on a host the
//! daemon reaches over TLS. This module owns the *administrator* half of
//! that decision — which nodes a client may name, and what execution
//! posture a targetable node must declare.
//!
//! The *principal* half of the decision (does this caller hold the
//! node-target capability?) lives in the daemon's authorization gate;
//! see `CoreDaemon::authorize_node_target`.
//!
//! Everything here is fail closed. The default policy approves no node
//! at all, so a daemon that never configures `[eggwork.allowed_target_nodes]`
//! refuses every remote target at the host boundary while leaving local
//! execution untouched.
//!
//! This policy holds node *identity* and declared posture only. Endpoints,
//! certificate paths, and key paths stay in the daemon configuration and
//! are never copied into the policy, a job row, a log line, or an error
//! message.

use std::collections::{BTreeMap, BTreeSet};

use codegg_config::schema::{
    EggworkConfig, EggworkIsolationPolicy, EggworkNetworkPolicy, RemoteResourceLimits,
};

/// The declared execution posture of one configured node.
///
/// This is the *declared* posture from the node's profile, not an
/// observed one: the executor re-verifies the advertised posture against
/// the declaration at execution time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodePosture {
    /// Filesystem isolation the node declares.
    pub isolation: EggworkIsolationPolicy,
    /// Network posture the node declares.
    pub network: EggworkNetworkPolicy,
}

/// Why a remote target was refused at the host boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteTargetRejection {
    /// The daemon approves no node with this name.
    ///
    /// Deliberately does not disclose which nodes *are* approved: an
    /// unauthorized caller learns nothing about the daemon's fleet.
    NotApproved {
        /// The node id the caller asked for.
        node_id: String,
    },
    /// The node is approved but no longer configured.
    ///
    /// This is an administrator-facing configuration error, not a
    /// caller error.
    UnknownNode {
        /// The node id the caller asked for.
        node_id: String,
    },
    /// The node's declared posture is below the daemon's ceiling.
    PolicyBelowCeiling {
        /// The node id the caller asked for.
        node_id: String,
        /// Human-readable description of the shortfall.
        detail: String,
    },
}

impl RemoteTargetRejection {
    /// Stable wire code for `CoreResponse::Error`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotApproved { .. } => "remote_target_not_approved",
            Self::UnknownNode { .. } => "remote_target_unknown_node",
            Self::PolicyBelowCeiling { .. } => "remote_target_policy_denied",
        }
    }

    /// Operator-facing explanation. Never contains an endpoint, a
    /// certificate path, or a key path.
    pub fn message(&self) -> String {
        match self {
            Self::NotApproved { node_id } => format!(
                "eggwork node '{node_id}' is not approved for client-driven targeting; \
                 ask an administrator to add it to [eggwork.allowed_target_nodes]"
            ),
            Self::UnknownNode { node_id } => format!(
                "eggwork node '{node_id}' is approved but has no [eggwork.nodes] entry"
            ),
            Self::PolicyBelowCeiling { node_id, detail } => format!(
                "eggwork node '{node_id}' does not meet the daemon execution policy ceiling: {detail}"
            ),
        }
    }
}

/// Daemon-owned admission policy for explicit remote job targets.
///
/// Built once from the daemon configuration and then immutable for the
/// daemon's lifetime. It answers one question: may a client name this
/// node?
#[derive(Debug, Clone, Default)]
pub struct RemoteTargetPolicy {
    /// Nodes an administrator approved for client-driven targeting.
    approved: BTreeSet<String>,
    /// Declared posture per configured node, keyed by node table key.
    postures: BTreeMap<String, NodePosture>,
    /// The operator's resource-limit ceiling per configured node.
    ///
    /// A node with no ceiling accepts no client-supplied limits at all:
    /// the executor treats the ceiling as authoritative, so a request
    /// against a ceiling-less node is refused rather than honored.
    resource_ceilings: BTreeMap<String, RemoteResourceLimits>,
    /// Minimum declared filesystem isolation for a targetable node.
    isolation_ceiling: Option<EggworkIsolationPolicy>,
    /// Minimum declared network posture for a targetable node.
    network_ceiling: Option<EggworkNetworkPolicy>,
}

impl RemoteTargetPolicy {
    /// Build the policy from the daemon's `eggwork` configuration.
    ///
    /// A configuration that approves nothing yields a policy that
    /// refuses everything; that is the fail-closed default, not an
    /// error.
    pub fn from_config(config: &EggworkConfig) -> Self {
        let postures = config
            .nodes
            .as_ref()
            .map(|nodes| {
                nodes
                    .iter()
                    .map(|(table_key, profile)| {
                        (
                            table_key.clone(),
                            NodePosture {
                                isolation: profile.isolation_policy.unwrap_or_default(),
                                network: profile.network_policy.unwrap_or_default(),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let approved = config
            .allowed_target_nodes
            .as_ref()
            .map(|nodes| nodes.iter().cloned().collect())
            .unwrap_or_default();
        let resource_ceilings = config
            .nodes
            .as_ref()
            .map(|nodes| {
                nodes
                    .iter()
                    .filter_map(|(table_key, profile)| {
                        profile
                            .resource_limits
                            .clone()
                            .map(|limits| (table_key.clone(), limits))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            approved,
            postures,
            resource_ceilings,
            isolation_ceiling: config.required_isolation_policy,
            network_ceiling: config.required_network_policy,
        }
    }

    /// The operator's resource-limit ceiling for one node.
    pub fn resource_ceiling(&self, node_id: &str) -> Option<&RemoteResourceLimits> {
        self.resource_ceilings.get(node_id)
    }

    /// Check a job's remote resource request against the node's ceiling.
    ///
    /// This is the host-boundary half of the limit policy. It uses only
    /// the operator's configuration, so a request that widens the
    /// ceiling, weakens a mandatory limit, or names a dimension the
    /// operator never configured is refused *before* the job becomes
    /// durable.
    ///
    /// The node's live capabilities are checked separately, in the
    /// executor's preflight, before any workspace upload.
    pub fn check_resource_request(
        &self,
        node_id: &str,
        request: Option<&RemoteResourceLimits>,
    ) -> Result<(), crate::scheduler::eggwork_limits::RemoteLimitRejection> {
        let ceiling = self.resource_ceilings.get(node_id);
        crate::scheduler::eggwork_limits::combine_limits(ceiling, request).map(|_| ())
    }

    /// `true` when `node_id` is approved for client-driven targeting.
    pub fn approves(&self, node_id: &str) -> bool {
        self.approved.contains(node_id)
    }

    /// The approved node ids, in deterministic order.
    pub fn approved_nodes(&self) -> impl Iterator<Item = &str> {
        self.approved.iter().map(String::as_str)
    }

    /// `true` when no node may be targeted at all.
    pub fn is_empty(&self) -> bool {
        self.approved.is_empty()
    }

    /// Decide whether a remote target is admissible.
    ///
    /// The checks run in a fixed order — approval, then configuration,
    /// then posture ceiling — so an unauthorized caller learns nothing
    /// about the daemon's configured fleet beyond the fact that its own
    /// request was refused.
    pub fn check(&self, node_id: &str) -> Result<(), RemoteTargetRejection> {
        if !self.approved.contains(node_id) {
            return Err(RemoteTargetRejection::NotApproved {
                node_id: node_id.to_owned(),
            });
        }
        let Some(posture) = self.postures.get(node_id) else {
            return Err(RemoteTargetRejection::UnknownNode {
                node_id: node_id.to_owned(),
            });
        };
        if let Some(ceiling) = self.isolation_ceiling {
            if !isolation_at_least(posture.isolation, ceiling) {
                return Err(RemoteTargetRejection::PolicyBelowCeiling {
                    node_id: node_id.to_owned(),
                    detail: format!(
                        "declared filesystem isolation is {:?}, ceiling requires {:?}",
                        posture.isolation, ceiling
                    ),
                });
            }
        }
        if let Some(ceiling) = self.network_ceiling {
            if !network_at_least(posture.network, ceiling) {
                return Err(RemoteTargetRejection::PolicyBelowCeiling {
                    node_id: node_id.to_owned(),
                    detail: format!(
                        "declared network posture is {:?}, ceiling requires {:?}",
                        posture.network, ceiling
                    ),
                });
            }
        }
        Ok(())
    }
}

/// `true` when `declared` is at least as strict as `ceiling`.
///
/// Filesystem isolation: `Required` is stricter than `None`.
fn isolation_at_least(declared: EggworkIsolationPolicy, ceiling: EggworkIsolationPolicy) -> bool {
    match (declared, ceiling) {
        (EggworkIsolationPolicy::None, EggworkIsolationPolicy::None) => true,
        (EggworkIsolationPolicy::Required, _) => true,
        (EggworkIsolationPolicy::None, EggworkIsolationPolicy::Required) => false,
    }
}

/// `true` when `declared` is at least as strict as `ceiling`.
///
/// Network posture: `Disabled` is stricter than `Unrestricted`.
fn network_at_least(declared: EggworkNetworkPolicy, ceiling: EggworkNetworkPolicy) -> bool {
    match (declared, ceiling) {
        (EggworkNetworkPolicy::Unrestricted, EggworkNetworkPolicy::Unrestricted) => true,
        (EggworkNetworkPolicy::Disabled, _) => true,
        (EggworkNetworkPolicy::Unrestricted, EggworkNetworkPolicy::Disabled) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_config::schema::EggworkNodeProfile;
    use std::collections::HashMap;

    fn config_with_node() -> EggworkConfig {
        let mut nodes = HashMap::new();
        nodes.insert(
            "builder".to_owned(),
            EggworkNodeProfile {
                node_id: Some("builder-1".to_owned()),
                endpoint: Some("https://node.invalid".to_owned()),
                isolation_policy: Some(EggworkIsolationPolicy::Required),
                network_policy: Some(EggworkNetworkPolicy::Disabled),
                ..Default::default()
            },
        );
        EggworkConfig {
            nodes: Some(nodes),
            ..Default::default()
        }
    }

    #[test]
    fn default_policy_approves_nothing() {
        let policy = RemoteTargetPolicy::from_config(&EggworkConfig::default());
        assert!(policy.is_empty());
        assert!(!policy.approves("builder"));
        let rejection = policy.check("builder").unwrap_err();
        assert_eq!(rejection.code(), "remote_target_not_approved");
        assert!(!rejection.message().contains("node.invalid"));
    }

    #[test]
    fn approved_node_without_ceiling_is_admitted() {
        let mut config = config_with_node();
        config.allowed_target_nodes = Some(vec!["builder".to_owned()]);
        let policy = RemoteTargetPolicy::from_config(&config);
        assert!(policy.check("builder").is_ok());
    }

    #[test]
    fn approved_node_below_ceiling_is_refused() {
        let mut config = config_with_node();
        config.allowed_target_nodes = Some(vec!["builder".to_owned()]);
        config.required_isolation_policy = Some(EggworkIsolationPolicy::Required);
        config.required_network_policy = Some(EggworkNetworkPolicy::Disabled);
        let policy = RemoteTargetPolicy::from_config(&config);
        assert!(policy.check("builder").is_ok());

        // A node that declares no isolation cannot satisfy a `Required`
        // ceiling.
        let mut weak = config_with_node();
        weak.nodes
            .as_mut()
            .unwrap()
            .get_mut("builder")
            .unwrap()
            .isolation_policy = Some(EggworkIsolationPolicy::None);
        weak.allowed_target_nodes = Some(vec!["builder".to_owned()]);
        weak.required_isolation_policy = Some(EggworkIsolationPolicy::Required);
        let policy = RemoteTargetPolicy::from_config(&weak);
        let rejection = policy.check("builder").unwrap_err();
        assert_eq!(rejection.code(), "remote_target_policy_denied");
    }

    #[test]
    fn approved_but_unconfigured_node_is_refused() {
        let mut config = config_with_node();
        config.allowed_target_nodes = Some(vec!["ghost".to_owned()]);
        let policy = RemoteTargetPolicy::from_config(&config);
        let rejection = policy.check("ghost").unwrap_err();
        assert_eq!(rejection.code(), "remote_target_unknown_node");
    }
}
