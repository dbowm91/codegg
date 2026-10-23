//! The discovery overlay's narrowing property, proven end to end.
//!
//! Milestone 005 of `provider-reasoning-control` retains a provider's
//! published thinking-capability contract from a catalog response and hands
//! it to M003's single resolution point. ADR-0050's load-bearing rule is
//! that the overlay may only **narrow** the compiled adapter baseline: it
//! may remove an option the baseline offers, and it may never add one,
//! introduce a label or a bound the baseline does not declare, or change the
//! always-reasons property.
//!
//! The overlay is parsed in `codegg-providers`; this test drives a **real
//! overlay payload** through the **real resolution function** and reads the
//! effective capability back, so the proof covers the whole path rather
//! than one side of it.

use codegg_config::schema::{
    ReasoningCapabilityRecord, ReasoningCapabilityStatus, MAX_REASONING_CAPABILITY_LABELS,
};
use codegg_core::model_profile::adapter::{AdapterDefinition, BUILTIN_ADAPTER_SOURCES};
use codegg_core::model_profile::{resolve_reasoning_capability, CapabilitySource};
use codegg_providers::eggpool::{
    parse_compatible_models_response_with_overlays, EggpoolCapabilityOverlay, EggpoolProbeOptions,
};

fn labels(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

/// The overlay retained from a catalog response.
fn overlay_of(body: &str) -> EggpoolCapabilityOverlay {
    parse_compatible_models_response_with_overlays(body.as_bytes(), &EggpoolProbeOptions::default())
        .expect("the response parses")
        .1
}

/// The compiled baseline a shipped adapter asset declares.
///
/// The baseline is the compiled upper bound the overlay may only narrow, so
/// the proof uses the assets the binary actually ships.
fn shipped_baseline(adapter_id: &str) -> ReasoningCapabilityRecord {
    let source = BUILTIN_ADAPTER_SOURCES
        .iter()
        .find(|(id, _)| *id == adapter_id)
        .map(|(_, source)| *source)
        .unwrap_or_else(|| panic!("the {adapter_id} adapter asset is shipped"));
    let definition: AdapterDefinition = toml::from_str(source).expect("the asset parses");
    definition
        .profile
        .reasoning_capability
        .unwrap_or_else(|| panic!("the {adapter_id} adapter asset declares a capability record"))
}

#[test]
fn a_published_contract_narrows_the_compiled_baseline() {
    let baseline = shipped_baseline("openai-frontier");
    let overlay = overlay_of(
        r#"{"data":[{"id":"gpt-5","name":"gpt-5","thinking":{"effort":"supported","supported_efforts":["low","medium"]}}]}"#,
    );
    let overlay = overlay
        .for_model("gpt-5")
        .expect("the published contract is retained");

    let resolution = resolve_reasoning_capability(Some(&baseline), None, Some(overlay.record()));

    assert!(
        resolution.rejected.is_empty(),
        "a narrowing overlay must not be refused: {:?}",
        resolution.rejected
    );
    let expected = labels(&["low", "medium"]);
    assert_eq!(
        resolution.effective.accepted_effort_labels.as_deref(),
        Some(expected.as_slice()),
        "the overlay must remove the labels the baseline offers beyond its own"
    );
    // Every dimension the overlay did not declare keeps the baseline's value.
    assert_eq!(resolution.effective.effort_status, baseline.effort_status);
    assert_eq!(resolution.effective.disable_status, baseline.disable_status);
    assert_eq!(resolution.effective.budget_status, baseline.budget_status);
    assert_eq!(resolution.effective.always_reasons, baseline.always_reasons);
}

#[test]
fn a_hostile_published_contract_is_rejected_in_full() {
    let baseline = shipped_baseline("openai-frontier");
    let overlay = overlay_of(
        r#"{"data":[{"id":"gpt-5","name":"gpt-5","thinking":{"effort":"supported","supported_efforts":["low","medium","ultra"]}}]}"#,
    );
    let overlay = overlay
        .for_model("gpt-5")
        .expect("the published contract is retained");

    let resolution = resolve_reasoning_capability(Some(&baseline), None, Some(overlay.record()));

    assert_eq!(
        resolution.rejected.len(),
        1,
        "a contract naming a label the baseline does not declare must be refused"
    );
    assert_eq!(
        resolution.rejected[0].source,
        CapabilitySource::DiscoveryOverlay
    );
    assert!(
        resolution.rejected[0].reason.contains("ultra"),
        "the refusal must name the offending label: {}",
        resolution.rejected[0].reason
    );
    // Rejected **in full**: not one dimension of the overlay was applied,
    // not even the ones that would have narrowed.
    assert_eq!(
        resolution.effective, baseline,
        "a refused overlay must leave the compiled baseline standing"
    );
}

#[test]
fn an_overlay_cannot_change_the_always_reasons_property() {
    // The compiled baseline owns the always-reasons property. A provider's
    // advertisement of the opposite is refused in both directions.
    let always_reasons = shipped_baseline("minimax-fast-executor");
    assert_eq!(always_reasons.always_reasons, Some(true));
    let claim_false = ReasoningCapabilityRecord {
        always_reasons_status: ReasoningCapabilityStatus::Unsupported,
        always_reasons: Some(false),
        ..ReasoningCapabilityRecord::default()
    };
    let resolution = resolve_reasoning_capability(Some(&always_reasons), None, Some(&claim_false));
    assert_eq!(resolution.rejected.len(), 1);
    assert_eq!(
        resolution.rejected[0].source,
        CapabilitySource::DiscoveryOverlay
    );
    assert_eq!(resolution.effective, always_reasons);

    let not_always_reasons = shipped_baseline("openai-frontier");
    assert_eq!(not_always_reasons.always_reasons, Some(false));
    let claim_true = ReasoningCapabilityRecord {
        always_reasons_status: ReasoningCapabilityStatus::Supported,
        always_reasons: Some(true),
        ..ReasoningCapabilityRecord::default()
    };
    let resolution =
        resolve_reasoning_capability(Some(&not_always_reasons), None, Some(&claim_true));
    assert_eq!(resolution.rejected.len(), 1);
    assert_eq!(
        resolution.rejected[0].source,
        CapabilitySource::DiscoveryOverlay
    );
    assert_eq!(resolution.effective, not_always_reasons);
}

#[test]
fn the_effective_capability_is_unchanged_when_the_overlay_is_absent() {
    let baseline = shipped_baseline("openai-frontier");
    let resolution = resolve_reasoning_capability(Some(&baseline), None, None);
    assert_eq!(resolution.effective, baseline);
    assert!(resolution.rejected.is_empty());
}

#[test]
fn a_direct_provider_connection_has_no_catalog_and_no_overlay() {
    // A direct provider connection has no catalog, so it publishes no
    // contract: the overlay is absent and the compiled baseline stands.
    let baseline = shipped_baseline("openai-frontier");
    let (models, overlay) = parse_compatible_models_response_with_overlays(
        br#"{"data":[{"id":"gpt-5","name":"gpt-5","object":"model","owned_by":"openai"}]}"#,
        &EggpoolProbeOptions::default(),
    )
    .expect("the response parses");
    assert_eq!(models.len(), 1);
    assert!(overlay.is_empty());

    let resolution = resolve_reasoning_capability(Some(&baseline), None, None);
    assert_eq!(resolution.effective, baseline);
    assert!(resolution.rejected.is_empty());
}

#[test]
fn an_oversized_published_ladder_never_reaches_the_resolution() {
    // The ladder is bounded before it is used, so an oversized one never
    // becomes a narrowing claim: the dimension degrades, the overlay
    // declares nothing, and the compiled baseline stands.
    let baseline = shipped_baseline("openai-frontier");
    let ladder = (0..(MAX_REASONING_CAPABILITY_LABELS + 1))
        .map(|index| format!("\"level-{index}\""))
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        "{{\"data\":[{{\"id\":\"gpt-5\",\"name\":\"gpt-5\",\"thinking\":{{\"effort\":\"supported\",\"supported_efforts\":[{ladder}]}}}}]}}"
    );
    let overlay = overlay_of(&body);
    assert!(
        overlay.is_empty(),
        "an oversized ladder must not become a retained overlay"
    );

    let resolution = resolve_reasoning_capability(Some(&baseline), None, None);
    assert_eq!(resolution.effective, baseline);
}
