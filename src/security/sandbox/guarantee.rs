//! Typed requested-versus-obtained guarantee contract (C042).
//!
//! Internal negotiation only: no public profile, config, or wire schema
//! changes. A future Windows backend must satisfy *every* requested
//! dimension before an untrusted target starts; a missing mechanism
//! refuses with a typed reason rather than degrading to `FullHost` or
//! broadening a child beyond its parent ceiling.
//!
//! ADR-0027 authority is bounded test-only preparation. Proposed ADR-0028
//! (broad-read compatibility profile) is NOT authorization: this module
//! never invents a broad-read public profile and never reinterprets
//! strict `ReadOnly`/`WorkspaceWrite` as broad-read.

use codegg_core::approval::{child_sandbox_allowed, SandboxProfile};

/// One observed fact about a single capability dimension.
///
/// Semantics are exact:
/// - `ObservedEnforced` — a real child proved the restriction.
/// - `ObservedDenied` — a real child attempted the operation and the OS
///   refused it (used for negative probes, e.g. host-secret reads).
/// - `Unsupported` — the backend honestly reports it cannot provide this
///   dimension on this host/tier.
/// - `Unavailable` — a transient setup/host condition (missing helper,
///   wrong OS edition, unprovisioned identity).
/// - `SetupFailed` — the backend was invoked and failed; distinct from a
///   proof of denial.
/// - `NotRun` — the probe did not execute; never counts as evidence.
/// - `Unknown` — an attestation arrived without a recognizable fact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapabilityFact {
    ObservedEnforced,
    ObservedDenied,
    Unsupported,
    Unavailable(String),
    SetupFailed(String),
    NotRun,
    Unknown,
}

impl CapabilityFact {
    /// `true` only for a positive proof of restriction.
    pub fn is_proven(&self) -> bool {
        matches!(self, Self::ObservedEnforced | Self::ObservedDenied)
    }
}

/// Immutable requested requirements for one launch attempt.
///
/// The request is frozen before negotiation: a backend may prove it meets
/// the requirements, never renegotiate them into something weaker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestedGuarantees {
    /// The authorized profile. Strict profiles require proven read
    /// confinement; broad host reads never satisfy them.
    pub profile: SandboxProfile,
    /// Parent ceiling. A child requesting broader containment than its
    /// parent is refused even when the backend could provide it.
    pub parent_ceiling: Option<SandboxProfile>,
    /// Filesystem reads outside the workspace must be OS-denied.
    pub require_read_confinement: bool,
    /// Filesystem writes outside the workspace/scratch must be OS-denied.
    pub require_write_confinement: bool,
    /// Egress must be OS-denied (offline) or mediated by a qualified
    /// proxy. `Unrestricted` network never satisfies this.
    pub require_network_offline: bool,
    /// Descendants must remain in the Job and under the same restrictions.
    pub require_job_containment: bool,
    /// The backend must not mutate original host descriptors/owners as a
    /// side effect of providing containment.
    pub forbid_host_mutation: bool,
}

impl RequestedGuarantees {
    /// Requirements implied by a profile with secure defaults.
    ///
    /// Constrained profiles require read/write confinement, Job
    /// containment, and no host mutation. Network offline is never
    /// implied: it must be requested explicitly and proved by WFP or an
    /// equivalent qualified mechanism.
    pub fn for_profile(profile: SandboxProfile) -> Self {
        let constrained = profile.requires_filesystem_containment();
        Self {
            profile,
            parent_ceiling: None,
            require_read_confinement: constrained,
            require_write_confinement: constrained,
            require_network_offline: false,
            require_job_containment: constrained,
            forbid_host_mutation: true,
        }
    }

    pub fn with_parent_ceiling(mut self, parent: SandboxProfile) -> Self {
        self.parent_ceiling = Some(parent);
        self
    }

    pub fn with_network_offline(mut self) -> Self {
        self.require_network_offline = true;
        self
    }
}

/// Attested candidate capabilities for one host/backend/tier.
///
/// Every field is an *observed* fact, never a vendor tier label or a
/// successful-launch inference. `broad_read_allowed` records that the
/// candidate permits host reads beyond the workspace; when `true`, the
/// candidate can never satisfy a strict read-confinement request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttestedCapabilities {
    /// Backend identity, e.g. `mxc-basecontainer`, `two-hop-restricted`.
    pub backend: String,
    /// Effective tier as reported *and verified* on this host.
    /// Empty or `unknown` never satisfies host-mutation or confinement
    /// requirements that depend on tier fidelity.
    pub effective_tier: String,
    /// `true` when the tier was downgraded or a fallback path was taken.
    pub tier_downgraded: bool,
    /// `true` when the candidate allows host reads beyond the workspace.
    pub broad_read_allowed: bool,
    /// `true` when the candidate is test-only scaffolding. Test-only
    /// scaffolding can never report production `Enforced`.
    pub test_only: bool,
    pub filesystem_read: CapabilityFact,
    pub filesystem_write: CapabilityFact,
    pub network_offline: CapabilityFact,
    pub job_containment: CapabilityFact,
    /// `true` when the backend provably performs no original-host
    /// descriptor/owner mutation (no DACL fallback) on this host.
    pub no_host_mutation_proven: bool,
    /// Opaque provenance (host edition/build/UBR/arch, SDK pin, run id).
    /// Recorded, never trusted as proof by itself.
    pub provenance: String,
}

impl AttestedCapabilities {
    /// `true` when the tier label is usable as a negotiation input.
    pub fn tier_is_known(&self) -> bool {
        let tier = self.effective_tier.trim().to_lowercase();
        !(tier.is_empty() || tier == "unknown" || tier == "unverified")
    }
}

/// Satisfied contract: the exact backend/tier/provenance that met every
/// requirement. Returned to the caller for audit; it grants nothing by
/// itself — the launcher must still verify private launch status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SatisfiedContract {
    pub backend: String,
    pub effective_tier: String,
    pub profile: SandboxProfile,
    pub provenance: String,
}

/// Why negotiation refused to authorize a launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub reason: String,
    pub dimension: &'static str,
}

impl Refusal {
    fn new(dimension: &'static str, reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            dimension,
        }
    }
}

/// Pure negotiation: either every requirement is proven or the launch is
/// refused before any untrusted target starts.
///
/// Rules (all fail-closed):
/// - Unknown/not-run/unavailable/setup-failed facts never satisfy a
///   required dimension.
/// - Strict `ReadOnly`/`WorkspaceWrite` read confinement is never
///   satisfied when `broad_read_allowed` is set, even if the backend
///   claims a read fact.
/// - Network offline requires a proven `network_offline` fact; an
///   `Unrestricted` network (the truthful state for all current shell
///   paths) never satisfies it.
/// - A downgraded/unknown tier never satisfies `forbid_host_mutation`
///   unless `no_host_mutation_proven` is set on a known tier.
/// - Test-only scaffolding never satisfies production enforcement: any
///   proven fact from a `test_only` candidate is treated as diagnostic,
///   not as `Enforced`.
/// - A child profile broader than its parent ceiling is refused even
///   when the backend could provide it.
/// - Refusal never converts to `FullHost`: the caller must deny or route
///   through explicit authorized escalation.
pub fn negotiate(
    requested: &RequestedGuarantees,
    observed: &AttestedCapabilities,
) -> Result<SatisfiedContract, Refusal> {
    if observed.backend.trim().is_empty() {
        return Err(Refusal::new(
            "backend",
            "candidate names no backend identity",
        ));
    }
    if let Some(parent) = requested.parent_ceiling {
        if !child_sandbox_allowed(parent, requested.profile) {
            return Err(Refusal::new(
                "parent-ceiling",
                format!(
                    "child profile {} exceeds parent ceiling {}",
                    requested.profile.as_str(),
                    parent.as_str()
                ),
            ));
        }
    }
    // FullHost requests no containment; negotiation is vacuous but must
    // never be reached with confinement requirements set.
    if !requested.profile.requires_filesystem_containment() {
        if requested.require_read_confinement
            || requested.require_write_confinement
            || requested.require_network_offline
        {
            return Err(Refusal::new(
                "profile",
                "FullHost cannot carry confinement requirements",
            ));
        }
        return Ok(SatisfiedContract {
            backend: "uncontained".to_string(),
            effective_tier: "none".to_string(),
            profile: requested.profile,
            provenance: observed.provenance.clone(),
        });
    }
    if observed.test_only {
        return Err(Refusal::new(
            "test-only",
            format!(
                "test-only candidate '{}' cannot report production Enforced",
                observed.backend
            ),
        ));
    }
    if requested.require_read_confinement {
        if observed.broad_read_allowed {
            return Err(Refusal::new(
                "filesystem-read",
                "strict ReadOnly/WorkspaceWrite is never satisfied by broad host reads",
            ));
        }
        if !observed.filesystem_read.is_proven() {
            return Err(Refusal::new(
                "filesystem-read",
                format!(
                    "read confinement unproven (fact: {:?})",
                    observed.filesystem_read
                ),
            ));
        }
    }
    if requested.require_write_confinement && !observed.filesystem_write.is_proven() {
        return Err(Refusal::new(
            "filesystem-write",
            format!(
                "write confinement unproven (fact: {:?})",
                observed.filesystem_write
            ),
        ));
    }
    if requested.require_network_offline && !observed.network_offline.is_proven() {
        return Err(Refusal::new(
            "network",
            "offline requires independently proven WFP (or equivalent); \
             Unrestricted network never satisfies it",
        ));
    }
    if requested.require_job_containment && !observed.job_containment.is_proven() {
        return Err(Refusal::new(
            "job",
            format!(
                "Job/descendant containment unproven (fact: {:?})",
                observed.job_containment
            ),
        ));
    }
    if requested.forbid_host_mutation {
        if !observed.tier_is_known() {
            return Err(Refusal::new(
                "host-mutation",
                "unknown/unverified tier cannot satisfy the no-host-mutation prohibition",
            ));
        }
        if observed.tier_downgraded {
            return Err(Refusal::new(
                "host-mutation",
                "downgraded/fallback tier cannot satisfy the no-host-mutation prohibition",
            ));
        }
        if !observed.no_host_mutation_proven {
            return Err(Refusal::new(
                "host-mutation",
                "no-host-DACL-mutation path is unproven on this host/tier",
            ));
        }
    }
    Ok(SatisfiedContract {
        backend: observed.backend.clone(),
        effective_tier: observed.effective_tier.clone(),
        profile: requested.profile,
        provenance: observed.provenance.clone(),
    })
}

/// Validate a private helper status claim against the negotiated contract.
///
/// The helper must name the negotiated backend; a mismatched, empty, or
/// unregistered backend identity is refused as forged. `Enforced` from a
/// test-only helper is refused: diagnostics are never production proof.
pub fn verify_helper_backend(
    negotiated: &SatisfiedContract,
    reported_backend: &str,
    reported_test_only: bool,
) -> Result<(), Refusal> {
    if reported_test_only {
        return Err(Refusal::new(
            "helper-status",
            "test-only helper cannot report production Enforced",
        ));
    }
    if reported_backend.trim().is_empty() {
        return Err(Refusal::new(
            "helper-status",
            "helper reported an empty backend identity",
        ));
    }
    if reported_backend != negotiated.backend {
        return Err(Refusal::new(
            "helper-status",
            format!(
                "helper backend '{reported_backend}' does not match negotiated '{}'",
                negotiated.backend
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strict_request() -> RequestedGuarantees {
        RequestedGuarantees::for_profile(SandboxProfile::WorkspaceWrite)
    }

    fn proven_candidate() -> AttestedCapabilities {
        AttestedCapabilities {
            backend: "mxc-basecontainer".to_string(),
            effective_tier: "base-container".to_string(),
            tier_downgraded: false,
            broad_read_allowed: false,
            test_only: false,
            filesystem_read: CapabilityFact::ObservedDenied,
            filesystem_write: CapabilityFact::ObservedDenied,
            network_offline: CapabilityFact::Unsupported,
            job_containment: CapabilityFact::ObservedEnforced,
            no_host_mutation_proven: true,
            provenance: "windows-11 26200 arm64 mxc-1.0.0".to_string(),
        }
    }

    #[test]
    fn strict_filesystem_contract_negotiates_when_every_fact_is_proven() {
        let contract = negotiate(&strict_request(), &proven_candidate())
            .expect("proven candidate must satisfy strict filesystem requirements");
        assert_eq!(contract.backend, "mxc-basecontainer");
        assert_eq!(contract.profile, SandboxProfile::WorkspaceWrite);
    }

    #[test]
    fn strict_read_is_never_satisfied_by_broad_read() {
        let mut candidate = proven_candidate();
        candidate.broad_read_allowed = true;
        let refusal =
            negotiate(&strict_request(), &candidate).expect_err("broad read must refuse strict");
        assert_eq!(refusal.dimension, "filesystem-read");
    }

    #[test]
    fn parent_ceiling_blocks_broader_child_even_when_backend_proves_it() {
        let request = RequestedGuarantees::for_profile(SandboxProfile::FullHost)
            .with_parent_ceiling(SandboxProfile::ReadOnly);
        // FullHost with confinement requirements is itself refused; use a
        // constrained child under a stricter parent instead.
        let child = RequestedGuarantees::for_profile(SandboxProfile::WorkspaceWrite)
            .with_parent_ceiling(SandboxProfile::ReadOnly);
        let refusal =
            negotiate(&child, &proven_candidate()).expect_err("child exceeds parent ceiling");
        assert_eq!(refusal.dimension, "parent-ceiling");
        // A FullHost child under FullHost parent negotiates vacuously.
        let full = RequestedGuarantees::for_profile(SandboxProfile::FullHost);
        let contract = negotiate(&full, &proven_candidate()).expect("FullHost is vacuous");
        assert_eq!(contract.backend, "uncontained");
        let _ = request;
    }

    #[test]
    fn tier_downgrade_or_unknown_tier_denies_host_mutation_prohibition() {
        let mut downgraded = proven_candidate();
        downgraded.tier_downgraded = true;
        let refusal = negotiate(&strict_request(), &downgraded).expect_err("downgrade must refuse");
        assert_eq!(refusal.dimension, "host-mutation");

        let mut unknown = proven_candidate();
        unknown.effective_tier = "unknown".to_string();
        let refusal = negotiate(&strict_request(), &unknown).expect_err("unknown tier refuses");
        assert_eq!(refusal.dimension, "host-mutation");

        let mut mutated = proven_candidate();
        mutated.no_host_mutation_proven = false;
        let refusal = negotiate(&strict_request(), &mutated).expect_err("unproven path refuses");
        assert_eq!(refusal.dimension, "host-mutation");
    }

    #[test]
    fn offline_requires_proven_wfp_and_unrestricted_never_satisfies_it() {
        let request = strict_request().with_network_offline();
        // Candidate without a proven offline fact refuses.
        let refusal =
            negotiate(&request, &proven_candidate()).expect_err("unproven offline must refuse");
        assert_eq!(refusal.dimension, "network");

        let mut enforced = proven_candidate();
        enforced.network_offline = CapabilityFact::ObservedEnforced;
        negotiate(&request, &enforced).expect("proven WFP satisfies offline");
    }

    #[test]
    fn job_only_evidence_never_satisfies_filesystem_confinement() {
        let mut job_only = proven_candidate();
        job_only.filesystem_read = CapabilityFact::NotRun;
        job_only.filesystem_write = CapabilityFact::NotRun;
        let refusal =
            negotiate(&strict_request(), &job_only).expect_err("Job-only must not satisfy fs");
        assert_eq!(refusal.dimension, "filesystem-read");

        for fact in [
            CapabilityFact::Unsupported,
            CapabilityFact::Unavailable("no backend".to_string()),
            CapabilityFact::SetupFailed("helper exited".to_string()),
            CapabilityFact::NotRun,
            CapabilityFact::Unknown,
        ] {
            let mut candidate = proven_candidate();
            candidate.filesystem_read = fact.clone();
            assert!(
                negotiate(&strict_request(), &candidate).is_err(),
                "unproven fact {fact:?} must refuse"
            );
        }
    }

    #[test]
    fn test_only_candidate_never_reports_production_enforced() {
        let mut candidate = proven_candidate();
        candidate.test_only = true;
        let refusal = negotiate(&strict_request(), &candidate).expect_err("test-only must refuse");
        assert_eq!(refusal.dimension, "test-only");
    }

    #[test]
    fn malformed_and_forged_helper_status_is_refused() {
        let contract = negotiate(&strict_request(), &proven_candidate()).expect("negotiated");
        verify_helper_backend(&contract, "mxc-basecontainer", false).expect("matching backend");
        let forged = verify_helper_backend(&contract, "landlock", false).expect_err("mismatch");
        assert_eq!(forged.dimension, "helper-status");
        let empty = verify_helper_backend(&contract, "", false).expect_err("empty backend");
        assert_eq!(empty.dimension, "helper-status");
        let test_only =
            verify_helper_backend(&contract, "mxc-basecontainer", true).expect_err("test-only");
        assert_eq!(test_only.dimension, "helper-status");
    }

    #[test]
    fn unavailable_never_converts_to_full_host() {
        let mut candidate = proven_candidate();
        candidate.filesystem_read = CapabilityFact::Unavailable("wrong edition".to_string());
        let refusal =
            negotiate(&strict_request(), &candidate).expect_err("unavailable must refuse");
        assert!(!refusal.reason.contains("FullHost"));
        assert_eq!(refusal.dimension, "filesystem-read");
    }

    #[test]
    fn readonly_and_workspace_write_share_strict_read_semantics() {
        for profile in [SandboxProfile::ReadOnly, SandboxProfile::WorkspaceWrite] {
            let request = RequestedGuarantees::for_profile(profile);
            assert!(request.require_read_confinement);
            let mut broad = proven_candidate();
            broad.broad_read_allowed = true;
            assert!(
                negotiate(&request, &broad).is_err(),
                "{profile:?} must refuse broad reads"
            );
        }
    }
}
