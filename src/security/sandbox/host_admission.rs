//! M005/C002: the machine-readable Linux supported-host admission manifest.
//!
//! Four facts, kept deliberately apart because they answer different
//! questions and have different owners:
//!
//! | Section                | Question                                            | Owner     |
//! |------------------------|-----------------------------------------------------|-----------|
//! | `discovered`           | What does this host actually offer, with evidence?   | the host  |
//! | `can_launch`           | What could the runtime ask for on this host?         | derived   |
//! | `native_negative_proven` | What refusal did a maintainer actually record?     | maintainer|
//! | `maintainer_admitted`  | Does a maintainer support this host?                | maintainer|
//!
//! **The manifest is capability, not enforcement.** Nothing in it may turn a
//! requested boundary into `Enforced`: enforcement is a property of a
//! particular launch, and only the receipt that launch actually produced can
//! establish it. A host can offer every capability in this file and still
//! fail to confine a child, and a manifest entry can never substitute for
//! observing that receipt.
//!
//! `discover()` therefore never grants an admission: it reports what it found
//! and leaves `maintainer_admitted.admitted` false. Only a maintainer-owned
//! record can set it, and this module provides no path that sets it from a
//! probe result.

use serde::{Deserialize, Serialize};

/// The state of one host capability dimension.
///
/// `Unknown` is a first-class value: a capability that could not be probed
/// is not the same as one that was probed and found missing, and the two
/// must not be collapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Available,
    Unavailable,
    Unknown,
}

impl Capability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

/// One probed host dimension and the evidence behind it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostCapability {
    pub dimension: String,
    pub state: Capability,
    /// Why the probe reached that state. Never a capability claim on its own.
    pub evidence: String,
}

/// What the host actually offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredHost {
    pub platform: String,
    pub arch: String,
    /// The kernel release, when it could be read without spawning a process.
    pub kernel_release: Option<String>,
    /// The Landlock ABI 6 signal and abstract-socket scope qualification.
    ///
    /// The Landlock crate exposes its effective ABI only from a ruleset that
    /// has already been applied, and applying one is a one-way door for the
    /// calling thread. This probe therefore reports the qualification the
    /// runtime actually depends on rather than a number it cannot read
    /// without a side effect.
    pub landlock_process_scopes: Capability,
    pub capabilities: Vec<HostCapability>,
}

/// What the runtime could ask for on this host, derived from `discovered`.
///
/// This is a *request* capability, not an enforcement claim: each dimension
/// says whether the corresponding boundary could be requested at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanLaunch {
    pub filesystem_containment: Capability,
    pub network_isolation: Capability,
    pub process_isolation: Capability,
    /// The derivation rules and what remains unqualified for each dimension.
    pub notes: Vec<String>,
}

/// Native refusal evidence a maintainer recorded for one exact revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeNegativeEvidence {
    /// The exact revision the evidence was produced against.
    pub revision: String,
    /// The CI run or local invocation that produced it.
    pub run_id: Option<String>,
    /// The refusal cases that were exercised, e.g. `ipv4_tcp_denied`.
    pub cases: Vec<String>,
    pub recorded_at: String,
}

/// A maintainer's decision about whether this host configuration is supported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MaintainerAdmission {
    /// False unless a maintainer recorded a decision.
    pub admitted: bool,
    pub admitted_by: Option<String>,
    pub admitted_at: Option<String>,
    /// What the admission covers, when it is granted.
    pub scope: Option<String>,
}

/// The complete manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostAdmissionManifest {
    pub schema_version: u32,
    pub discovered: DiscoveredHost,
    pub can_launch: CanLaunch,
    /// Empty until a maintainer records native refusal evidence.
    pub native_negative_proven: Vec<NativeNegativeEvidence>,
    /// Never granted by discovery.
    pub maintainer_admitted: MaintainerAdmission,
}

impl HostAdmissionManifest {
    /// Report whether a serialized manifest makes an enforcement claim.
    ///
    /// A manifest that says a boundary was `Enforced` would be claiming
    /// something only a launch receipt can establish. This predicate exists
    /// so that claim can be rejected mechanically rather than by review.
    pub fn contains_enforcement_claim(serialized: &str) -> bool {
        let lowered = serialized.to_ascii_lowercase();
        lowered.contains("enforced") || lowered.contains("\"guaranteed\"")
    }

    /// Whether the manifest carries a maintainer admission.
    ///
    /// This is a report of the maintainer record, never a grant.
    pub fn is_maintainer_admitted(&self) -> bool {
        self.maintainer_admitted.admitted
    }
}

fn capability(dimension: &str, state: Capability, evidence: impl Into<String>) -> HostCapability {
    HostCapability {
        dimension: dimension.to_string(),
        state,
        evidence: evidence.into(),
    }
}

/// Read the kernel release without spawning a process.
///
/// Spawning here would make a read-only capability probe a process-execution
/// surface; `/proc` is a plain file read and keeps the probe side-effect
/// free.
fn kernel_release() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "linux")]
fn linux_capabilities() -> Vec<HostCapability> {
    use crate::security::sandbox::linux_launch::{probe_linux_host_features, ProbeState};

    let features = probe_linux_host_features();
    let mut out = Vec::new();
    let mut push = |dimension: &str, fact: &crate::security::sandbox::linux_launch::ProbeFact| {
        let state = match fact.state {
            ProbeState::Available => Capability::Available,
            ProbeState::Unavailable => Capability::Unavailable,
            ProbeState::Unknown => Capability::Unknown,
        };
        out.push(capability(dimension, state, fact.evidence.clone()));
    };
    push("user_namespace_creation", &features.user_namespace_creation);
    push("mount_namespace", &features.mount_namespace);
    push("pid_namespace", &features.pid_namespace);
    push("network_namespace", &features.network_namespace);
    push("ipc_namespace", &features.ipc_namespace);
    push("seccomp_filter", &features.seccomp_filter);
    push("proc_mount_inputs", &features.proc_mount_inputs);
    push("trusted_bubblewrap", &features.trusted_bubblewrap);
    push("landlock", &features.landlock);
    push("cgroup_v2_delegation", &features.cgroup_v2_delegation);
    push("apparmor", &features.apparmor);
    push("selinux", &features.selinux);
    out
}

#[cfg(not(target_os = "linux"))]
fn linux_capabilities() -> Vec<HostCapability> {
    vec![capability(
        "linux_host",
        Capability::Unavailable,
        "this host is not Linux; the Linux isolation dimensions do not apply",
    )]
}

fn landlock_process_scopes_state() -> Capability {
    match crate::security::sandbox::landlock::probe_process_scopes() {
        Ok(()) => Capability::Available,
        Err(_) => Capability::Unavailable,
    }
}

/// Derive the request capabilities from the discovered facts.
///
/// Every rule is stated explicitly so that a reviewer can see why a
/// dimension is `available`, and so that a missing capability produces a
/// `notes` entry naming what is still unqualified.
fn derive_can_launch(discovered: &DiscoveredHost) -> CanLaunch {
    let state_of = |dimension: &str| {
        discovered
            .capabilities
            .iter()
            .find(|entry| entry.dimension == dimension)
            .map(|entry| entry.state)
    };
    let landlock_state = state_of("landlock").unwrap_or(Capability::Unknown);
    let bubblewrap_state = state_of("trusted_bubblewrap").unwrap_or(Capability::Unknown);
    let userns_state = state_of("user_namespace_creation").unwrap_or(Capability::Unknown);
    let netns_state = state_of("network_namespace").unwrap_or(Capability::Unknown);

    // Filesystem containment is Landlock's own dimension: it needs no
    // namespace and no helper.
    let filesystem_containment = landlock_state;

    // Process isolation additionally needs the M004 Landlock ABI 6 signal
    // and abstract-socket scopes, plus a trusted helper to apply them.
    let process_scopes = crate::security::sandbox::landlock::probe_process_scopes();
    let process_isolation = match (process_scopes.is_ok(), bubblewrap_state, userns_state) {
        (true, Capability::Available, Capability::Available) => Capability::Available,
        (false, _, _) => Capability::Unavailable,
        _ => Capability::Unknown,
    };

    // Network isolation needs a trusted helper, a user namespace, and a
    // network namespace. It is the dimension the M004 ABI 6 scopes extend to
    // the abstract-socket and signal domains.
    let network_isolation = match (bubblewrap_state, userns_state, netns_state) {
        (Capability::Available, Capability::Available, Capability::Available) => {
            Capability::Available
        }
        (Capability::Unavailable, _, _) | (_, Capability::Unavailable, _) => {
            Capability::Unavailable
        }
        _ => Capability::Unknown,
    };

    let mut notes = Vec::new();
    notes.push(
        "can_launch reports what a boundary could be requested on this host; it is never a \
         statement that any launch obtained it."
            .to_string(),
    );
    if let Some(reason) = process_scopes.as_ref().err() {
        notes.push(format!("process isolation is unqualified: {reason}"));
    }
    notes.push(
        "no dimension here may be read as an enforcement verdict; only a per-launch receipt \
         observed by the runtime can establish that a boundary was obtained."
            .to_string(),
    );

    CanLaunch {
        filesystem_containment,
        network_isolation,
        process_isolation,
        notes,
    }
}

/// Discover the host and produce the manifest.
///
/// Discovery never grants an admission and never records native evidence:
/// `native_negative_proven` stays empty and `maintainer_admitted.admitted`
/// stays false. Those two sections are filled from maintainer-owned records,
/// not from a probe.
pub fn discover() -> HostAdmissionManifest {
    let capabilities = linux_capabilities();
    let discovered = DiscoveredHost {
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        kernel_release: kernel_release(),
        landlock_process_scopes: landlock_process_scopes_state(),
        capabilities,
    };
    let can_launch = derive_can_launch(&discovered);
    HostAdmissionManifest {
        schema_version: 1,
        discovered,
        can_launch,
        native_negative_proven: Vec::new(),
        maintainer_admitted: MaintainerAdmission::default(),
    }
}
