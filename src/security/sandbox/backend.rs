//! Pluggable OS filesystem-containment backends.
//!
//! CodeGG's containment is expressed **once**, as a [`BackendPolicy`] of
//! paths (see [`super::policy`]), and then rendered into each backend's
//! native enforcement mechanism. This split is what makes the sandbox
//! multi-backend:
//!
//! * **Policy** is platform-neutral — a list of readable roots, writable
//!   roots, and explicitly denied roots. It does not mention Landlock
//!   rulesets or SBPL profiles.
//! * **Backend** is platform-specific — it knows how to compile one policy
//!   into its mechanism and apply it to the current (child) process.
//!
//! Adding a backend (Windows AppContainer, FreeBSD Capsicum, a future Linux
//! seccomp/bubblewrap path, …) means writing one [`SandboxBackend`] const
//! entry below. Nothing else in the crate changes: capability probing,
//! enforcement descriptors, execution-path selection, the helper, the audit
//! token, and the operator-facing text all read the backend identity out of
//! the registry rather than hard-coding one.
//!
//! ## Adding a backend
//!
//! 1. Write `probe_*` (cheap availability check) and `apply_*` (enforce) in
//!    the backend's own module, each behind its `cfg(target_os = …)`.
//! 2. Add one entry to [`BACKENDS`].
//!
//! Registration order *is* preference order: the first backend that probes
//! available is the one used. A backend that never compiles on this host
//! simply reports its platform reason and is skipped.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::SandboxLaunchSpec;

/// Stable identity of one containment backend.
///
/// Deliberately a newtype over `&'static str` rather than a closed enum: a
/// backend id travels into audit records, enforcement descriptors, and the
/// TUI, and a new backend must be representable without editing every
/// `match` that names one. Use the associated constants for the backends
/// CodeGG ships; compare with [`BackendId::as_str`] for anything else.
///
/// Deserialization resolves the wire string against the [`BACKENDS`]
/// registry rather than trusting it, so a fabricated or stale backend name
/// arriving over the private status channel cannot be laundered into a
/// plausible-looking enforcement record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BackendId(&'static str);

impl BackendId {
    /// Linux Landlock LSM (deny-first allowlist ruleset).
    pub const LANDLOCK: BackendId = BackendId("landlock");
    /// macOS Seatbelt (SBPL profile applied with `sandbox_init`).
    pub const SEATBELT: BackendId = BackendId("seatbelt");

    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// `true` when the id carries no identity (the uncontained token).
    pub fn is_empty(self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for BackendId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Serialize for BackendId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for BackendId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        BACKENDS
            .iter()
            .find(|candidate| candidate.id.as_str() == raw)
            .map(|candidate| candidate.id)
            .ok_or_else(|| {
                let known: Vec<&str> = BACKENDS
                    .iter()
                    .map(|candidate| candidate.id.as_str())
                    .collect();
                serde::de::Error::custom(format!(
                    "unknown sandbox backend '{raw}'; registered backends: {}",
                    known.join(", ")
                ))
            })
    }
}

/// Backend token used when no OS containment applies at all.
///
/// Not a real backend: it names the *absence* of one so audit records and
/// the degraded path never have to invent a backend identity.
pub const UNCONTAINED: BackendId = BackendId("uncontained");

/// What a backend actually guarantees once it has applied a policy.
///
/// Enforcement is reported as obtained facts, not as a claim derived from
/// the requested profile. `guarantees` and `limits` are the honest boundary
/// of the mechanism: Seatbelt, for example, genuinely confines writes and
/// denies secret reads, but has no `no_new_privs` equivalent, so it cannot
/// claim the privilege-drop guarantee Landlock can.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackendEnforcement {
    /// Backend-reported policy/ABI version when the mechanism has one.
    /// Seatbelt has no versioned ABI, so it reports `None`.
    pub abi: Option<u32>,
    /// Stable, human-readable guarantees this mechanism provides.
    pub guarantees: Vec<String>,
    /// Known ways containment can be escaped on this mechanism.
    pub limits: Vec<String>,
}

impl BackendEnforcement {
    pub fn new(abi: Option<u32>, guarantees: &[&str], limits: &[&str]) -> Self {
        Self {
            abi,
            guarantees: guarantees
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            limits: limits.iter().map(|value| (*value).to_string()).collect(),
        }
    }
}

/// One pluggable containment mechanism.
pub struct SandboxBackend {
    /// Stable identity recorded everywhere a backend is named.
    pub id: BackendId,
    /// Short operator-facing name.
    pub name: &'static str,
    /// Cheap availability check. `Err(reason)` must explain, in operator
    /// terms, why this mechanism cannot be used on this host — an
    /// unavailable backend that cannot name its reason is a bug.
    pub probe: fn() -> Result<(), String>,
    /// Apply `spec`'s policy in the **current** process.
    ///
    /// Runs only in the one-shot child helper, after which the helper
    /// `exec()`s the target; the restriction must therefore survive `exec`
    /// (both Landlock and Seatbelt do).
    pub apply: fn(&SandboxLaunchSpec) -> Result<BackendEnforcement, String>,
}

/// Every containment backend CodeGG knows, in **preference order**.
///
/// The first backend whose `probe` succeeds is the host's backend. Order is
/// the whole selection policy, so it is data rather than a `match` buried in
/// capability detection.
pub const BACKENDS: &[SandboxBackend] = &[
    SandboxBackend {
        id: BackendId::LANDLOCK,
        name: "Landlock",
        probe: super::landlock::probe,
        apply: super::landlock::apply,
    },
    SandboxBackend {
        id: BackendId::SEATBELT,
        name: "Seatbelt",
        probe: super::seatbelt::probe,
        apply: super::seatbelt::apply_profile,
    },
];

/// Look a backend up by identity.
pub fn backend(id: BackendId) -> Option<&'static SandboxBackend> {
    BACKENDS.iter().find(|backend| backend.id == id)
}

/// Probe every registered backend in preference order and return the first
/// that is usable, together with the reasons the others gave.
///
/// Probing is cheap (a ruleset creation or one compile of a minimal SBPL
/// profile) and side-effect free, so this stays a plain function rather than
/// a cached global: a cached capability would go stale the moment the helper
/// binary or the kernel ABI changes.
pub fn select() -> Result<&'static SandboxBackend, Vec<(BackendId, String)>> {
    let mut reasons = Vec::new();
    for backend in BACKENDS {
        match (backend.probe)() {
            Ok(()) => return Ok(backend),
            Err(reason) => reasons.push((backend.id, reason)),
        }
    }
    Err(reasons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_are_unique_and_ordered() {
        let mut ids: Vec<BackendId> = BACKENDS.iter().map(|backend| backend.id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "backend ids must be unique");
    }

    #[test]
    fn backend_lookup_round_trips_every_registered_backend() {
        for registered in BACKENDS {
            let found = backend(registered.id).expect("registered backend must resolve");
            assert_eq!(found.id, registered.id);
            assert!(!registered.name.is_empty(), "a backend must name itself");
        }
        assert!(
            backend(BackendId("not-a-registered-backend")).is_none(),
            "an unknown id must not resolve to a backend"
        );
    }

    #[test]
    fn select_returns_a_usable_backend_or_names_every_reason() {
        match select() {
            Ok(backend) => {
                assert!(
                    (backend.probe)().is_ok(),
                    "a selected backend must still probe available"
                );
                assert!(!backend.id.is_empty());
            }
            Err(reasons) => {
                // Every registered backend must have spoken, and every
                // reason must be operator-readable rather than empty.
                assert_eq!(reasons.len(), BACKENDS.len());
                for (id, reason) in reasons {
                    assert!(!reason.trim().is_empty(), "{id} gave an empty reason");
                }
            }
        }
    }

    #[test]
    fn uncontained_token_is_not_a_registered_backend() {
        // The degraded path must never be able to name itself as a backend.
        assert!(backend(UNCONTAINED).is_none());
        assert_eq!(UNCONTAINED.as_str(), "uncontained");
    }
}
