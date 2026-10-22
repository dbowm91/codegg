//! C002 (WP4): the machine-readable host-admission manifest.
//!
//! The manifest separates four facts that are easy to conflate:
//!
//!   discovered             what this host offers, with evidence
//!   can_launch             what could be requested here
//!   native_negative_proven what a maintainer actually recorded proving a refusal
//!   maintainer_admitted    a maintainer's decision to support this host
//!
//! The load-bearing property is that the manifest is **capability, not
//! enforcement**. A requested boundary becomes `Enforced` only when the
//! runtime observes the receipt that a particular launch produced. A
//! manifest entry — however complete, however favourable — can never stand
//! in for that receipt, and discovery must never grant an admission.

use codegg::security::sandbox::host_admission::{
    discover, Capability, HostAdmissionManifest, MaintainerAdmission,
};

/// Discovery must not grant an admission.
#[test]
fn discovery_never_grants_an_admission() {
    let manifest = discover();
    assert!(
        !manifest.is_maintainer_admitted(),
        "a discovery result must never carry a maintainer admission"
    );
    assert!(
        manifest.maintainer_admitted.admitted_by.is_none(),
        "a discovery result must never name an admitting maintainer"
    );
    assert!(
        manifest.maintainer_admitted.admitted_at.is_none(),
        "a discovery result must never carry an admission timestamp"
    );
}

/// Discovery must not invent native refusal evidence.
#[test]
fn discovery_never_records_native_evidence() {
    let manifest = discover();
    assert!(
        manifest.native_negative_proven.is_empty(),
        "a discovery result must not claim native refusal evidence; only a maintainer record may"
    );
}

/// The manifest reports capability, never enforcement.
#[test]
fn the_manifest_makes_no_enforcement_claim() {
    let manifest = discover();
    let serialized = serde_json::to_string(&manifest).expect("the manifest must serialize");
    assert!(
        !HostAdmissionManifest::contains_enforcement_claim(&serialized),
        "the manifest must not claim that a boundary was enforced: {serialized}"
    );
    assert!(
        !serialized.to_ascii_lowercase().contains("enforced"),
        "the manifest must not contain an enforcement verdict: {serialized}"
    );
}

/// The four sections stay distinct.
#[test]
fn the_sections_stay_distinct() {
    let manifest = discover();
    let serialized = serde_json::to_string(&manifest).expect("the manifest must serialize");
    let value: serde_json::Value =
        serde_json::from_str(&serialized).expect("the manifest must round-trip");
    for section in [
        "discovered",
        "can_launch",
        "native_negative_proven",
        "maintainer_admitted",
    ] {
        assert!(
            value.get(section).is_some(),
            "the manifest must carry the `{section}` section"
        );
    }
    // `discovered` and `can_launch` are not the same section: one reports
    // facts, the other reports a derived request capability.
    assert_ne!(
        value.get("discovered"),
        value.get("can_launch"),
        "discovered facts and derived request capability must remain distinct sections"
    );
}

/// Every discovered dimension carries its own evidence, and an unknown
/// capability is not silently reported as available.
#[test]
fn every_dimension_carries_evidence() {
    let manifest = discover();
    assert!(
        !manifest.discovered.capabilities.is_empty(),
        "the manifest must report the host dimensions"
    );
    for capability in &manifest.discovered.capabilities {
        assert!(
            !capability.dimension.is_empty(),
            "a capability must name its dimension"
        );
        assert!(
            !capability.evidence.is_empty(),
            "the `{}` dimension must carry evidence for its state",
            capability.dimension
        );
        assert!(
            !matches!(capability.state, Capability::Available) || !capability.evidence.is_empty(),
            "an available capability must state why"
        );
    }
}

/// `can_launch` is derived from the discovered facts and says out loud that
/// it is not an enforcement statement.
#[test]
fn can_launch_states_that_it_is_not_enforcement() {
    let manifest = discover();
    assert!(
        manifest
            .can_launch
            .notes
            .iter()
            .any(|note| note.contains("never a statement that any launch obtained it")),
        "can_launch must say that it is not an enforcement statement: {:?}",
        manifest.can_launch.notes
    );
    assert!(
        manifest
            .can_launch
            .notes
            .iter()
            .any(|note| note.contains("receipt")),
        "can_launch must point at the per-launch receipt as the only source of enforcement: {:?}",
        manifest.can_launch.notes
    );
}

/// A maintainer record is the only thing that can set an admission, and it
/// is a plain data value rather than something discovery computes.
#[test]
fn only_a_maintainer_record_can_set_an_admission() {
    let manifest = discover();
    assert_eq!(
        manifest.maintainer_admitted,
        MaintainerAdmission::default(),
        "the default admission must be a refusal"
    );
    assert!(!MaintainerAdmission::default().admitted);

    // A maintainer record is expressible, and it is a separate value from
    // anything discovery produced.
    let admitted = MaintainerAdmission {
        admitted: true,
        admitted_by: Some("maintainer".into()),
        admitted_at: Some("2026-10-21T00:00:00Z".into()),
        scope: Some("linux-native-offline".into()),
    };
    assert!(admitted.admitted);
}
