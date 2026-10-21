//! Where a delegated agent run is placed, and whether the CodeGG-mediated
//! shared-checkout profile is available on this platform.
//!
//! M005 C006 adds an **opt-in** placement. The default placement is
//! unchanged from the M004 era: every delegated run receives its own isolated
//! managed checkout. Sharing is never inferred from a group, a session or a
//! model request — it is asked for, and it is then proved against durable
//! state before the child executes.

use serde::{Deserialize, Serialize};

use crate::identity::AgentRunGroupId;

/// Where a delegated agent run is placed.
///
/// The default is [`RunPlacement::Auto`], which allocates a fresh isolated
/// managed checkout. Older API clients and older durable job payloads decode
/// as `Auto`, so a client that has never heard of the shared profile keeps
/// exactly the behaviour it had.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RunPlacement {
    /// Allocate a fresh isolated managed checkout for the run.
    ///
    /// This is the pre-M005-C006 behaviour and the default for every payload
    /// that does not name a placement.
    #[default]
    Auto,
    /// Explicitly allocate a fresh isolated managed checkout.
    ///
    /// The effect is identical to [`RunPlacement::Auto`]. The distinction is
    /// in intent: an operator can pin the isolated profile for a run even
    /// when the run's group offers a shared checkout, and the durable
    /// payload records that the isolation was chosen rather than defaulted.
    IsolatedNew,
    /// Join the managed checkout held by a live agent-run group.
    ///
    /// The run writes under CodeGG mediation: native file and typed Git
    /// operations take the checkout's writer permit, so exactly one writer
    /// holds the checkout at a time while readers overlap. Every
    /// process-capable surface is refused for the whole group (M005 C005).
    ///
    /// The group named here must exist, must still be live, and must list
    /// the run as a member. The group cannot grant itself membership: the
    /// check is made against the durable group record, not against the
    /// request.
    SerializedShared { group_id: AgentRunGroupId },
}

impl RunPlacement {
    /// Whether this placement asks for the shared profile.
    pub fn is_shared(&self) -> bool {
        matches!(self, RunPlacement::SerializedShared { .. })
    }

    /// The group this placement names, when it names one.
    pub fn shared_group_id(&self) -> Option<&AgentRunGroupId> {
        match self {
            RunPlacement::SerializedShared { group_id } => Some(group_id),
            _ => None,
        }
    }

    /// A short, stable label for logs and projections.
    pub fn as_str(&self) -> &'static str {
        match self {
            RunPlacement::Auto => "auto",
            RunPlacement::IsolatedNew => "isolated_new",
            RunPlacement::SerializedShared { .. } => "serialized_shared",
        }
    }
}

/// The platforms on which the shared-checkout profile has been qualified with
/// native containment evidence.
///
/// The profile is **disabled** on every other platform. A missing native
/// containment record is not a missing feature; it is the reason the feature
/// is off, and it is reported as such rather than silently degrading to
/// "probably fine".
pub const QUALIFIED_SHARED_CHECKOUT_PLATFORMS: &[&str] = &["linux"];

/// Whether the CodeGG-mediated shared checkout profile is available here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SharedCheckoutAvailability {
    /// The profile is qualified and available on this platform.
    Available,
    /// The profile is disabled on this platform, with the reason why.
    Unavailable { reason: String },
}

impl SharedCheckoutAvailability {
    /// Whether the shared profile may be requested here.
    pub fn is_available(&self) -> bool {
        matches!(self, SharedCheckoutAvailability::Available)
    }

    /// The reason the profile is unavailable, when it is.
    pub fn reason(&self) -> Option<&str> {
        match self {
            SharedCheckoutAvailability::Available => None,
            SharedCheckoutAvailability::Unavailable { reason } => Some(reason.as_str()),
        }
    }
}

/// Whether the shared-checkout profile is available on this platform.
///
/// The gate is deliberately a compile-time platform table rather than a
/// runtime flag: an operator cannot turn the profile on for a platform that
/// has no containment evidence, and the refusal always names the platform.
pub fn shared_checkout_availability() -> SharedCheckoutAvailability {
    let platform = std::env::consts::OS;
    if QUALIFIED_SHARED_CHECKOUT_PLATFORMS.contains(&platform) {
        SharedCheckoutAvailability::Available
    } else {
        SharedCheckoutAvailability::Unavailable {
            reason: format!(
                "the CodeGG-mediated shared checkout profile is qualified on {} only; \
                 this platform ({platform}) has no native containment evidence, so the \
                 profile stays disabled and the isolated-worktree default applies",
                QUALIFIED_SHARED_CHECKOUT_PLATFORMS.join(", ")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_placement_isolates() {
        let placement = RunPlacement::default();
        assert_eq!(placement, RunPlacement::Auto);
        assert!(!placement.is_shared());
        assert!(placement.shared_group_id().is_none());
    }

    #[test]
    fn a_legacy_payload_decodes_as_auto() {
        // The shape a durable job payload had before the placement existed.
        let decoded: RunPlacement = serde_json::from_str(r#"{"kind":"auto"}"#).expect("decodes");
        assert_eq!(decoded, RunPlacement::Auto);
    }

    #[test]
    fn a_shared_placement_names_its_group() {
        let group_id = AgentRunGroupId::new();
        let placement = RunPlacement::SerializedShared {
            group_id: group_id.clone(),
        };
        assert!(placement.is_shared());
        assert_eq!(placement.shared_group_id(), Some(&group_id));
        assert_eq!(placement.as_str(), "serialized_shared");
    }

    #[test]
    fn the_platform_gate_refuses_an_unqualified_platform() {
        let availability = shared_checkout_availability();
        if QUALIFIED_SHARED_CHECKOUT_PLATFORMS.contains(&std::env::consts::OS) {
            assert!(availability.is_available());
            assert!(availability.reason().is_none());
        } else {
            assert!(!availability.is_available());
            let reason = availability
                .reason()
                .expect("an unavailable platform says why");
            assert!(reason.contains(std::env::consts::OS));
        }
    }
}
