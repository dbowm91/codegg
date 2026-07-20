//! Projection capability and version negotiation.
//!
//! [`ProjectionCapabilities`] is declared by both daemons and clients
//! during capability negotiation. The negotiated projection version is
//! the intersection of the client's
//! [`ProjectionCapabilities::min_version`]..=`max_version` range and
//! the daemon's range. Reducers and consumers that receive a
//! projection whose `protocol_version` is outside their declared
//! range must produce an explicit resync / unsupported diagnostic
//! rather than silently degrading.
//!
//! The current projection protocol version is
//! [`PROJECTION_PROTOCOL_VERSION`]. Old clients that ignore unknown
//! fields remain forward-compatible as long as the negotiated version
//! is at least [`PROJECTION_PROTOCOL_VERSION_MIN`].

use serde::{Deserialize, Serialize};

/// Stable identifier used during capability negotiation.
pub const PROJECTION_CAPABILITY: &str = "session_projection.v1";

/// Current projection protocol version.
///
/// Bumped whenever an additive change lands that the reducer MUST be
/// able to interpret. Bumping this value is the only signal a client
/// needs to opt into the new contract.
pub const PROJECTION_PROTOCOL_VERSION: u32 = 1;

/// Minimum projection protocol version this build can interoperate
/// with. Anything below this requires an explicit `ResyncRequired`
/// response and a fresh snapshot.
pub const PROJECTION_PROTOCOL_VERSION_MIN: u32 = 1;

/// Capability declaration for the projection contract.
///
/// Carried inside the existing `ClientCapabilities` / `ServerCapabilities`
/// structures. The negotiated projection version is bounded by the
/// intersection of the two sides' ranges.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionCapabilities {
    /// Lower bound (inclusive) of supported projection versions.
    pub min_version: u32,
    /// Upper bound (inclusive) of supported projection versions.
    pub max_version: u32,
    /// `true` when the side can apply ordered projection events on
    /// top of a snapshot to reach an equivalent logical state.
    #[serde(default = "default_true")]
    pub supports_incremental_events: bool,
    /// `true` when the side tolerates unknown optional fields without
    /// emitting diagnostics. Required field mismatches still produce a
    /// resync.
    #[serde(default = "default_true")]
    pub supports_unknown_fields: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ProjectionCapabilities {
    fn default() -> Self {
        Self {
            min_version: PROJECTION_PROTOCOL_VERSION_MIN,
            max_version: PROJECTION_PROTOCOL_VERSION,
            supports_incremental_events: true,
            supports_unknown_fields: true,
        }
    }
}

impl ProjectionCapabilities {
    /// Capability advertised by this build.
    pub fn current() -> Self {
        Self::default()
    }

    /// Negotiate a version between two capability declarations.
    ///
    /// Returns the highest version `v` such that
    /// `client.min_version <= v <= client.max_version` and the same
    /// condition holds for `daemon`. `None` indicates that no
    /// compatible projection version exists.
    pub fn negotiate(client: &Self, daemon: &Self) -> Option<u32> {
        let lo = client.min_version.max(daemon.min_version);
        let hi = client.max_version.min(daemon.max_version);
        if lo > hi {
            return None;
        }
        Some(hi)
    }

    /// `true` when `version` falls within the declared range.
    pub fn supports(&self, version: u32) -> bool {
        version >= self.min_version && version <= self.max_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiate_picks_intersection_high() {
        let client = ProjectionCapabilities {
            min_version: 1,
            max_version: 2,
            ..Default::default()
        };
        let daemon = ProjectionCapabilities {
            min_version: 1,
            max_version: 1,
            ..Default::default()
        };
        assert_eq!(ProjectionCapabilities::negotiate(&client, &daemon), Some(1));
    }

    #[test]
    fn negotiate_returns_none_for_disjoint() {
        let client = ProjectionCapabilities {
            min_version: 2,
            max_version: 3,
            ..Default::default()
        };
        let daemon = ProjectionCapabilities {
            min_version: 1,
            max_version: 1,
            ..Default::default()
        };
        assert_eq!(ProjectionCapabilities::negotiate(&client, &daemon), None);
    }

    #[test]
    fn current_capability_supports_current_version() {
        let cap = ProjectionCapabilities::current();
        assert!(cap.supports(PROJECTION_PROTOCOL_VERSION));
        assert!(cap.supports(PROJECTION_PROTOCOL_VERSION_MIN));
        assert!(!cap.supports(PROJECTION_PROTOCOL_VERSION + 1));
    }
}
