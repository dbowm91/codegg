//! The kernel's reasoning-capability machinery, reached without a second
//! `eggpool-wire` declaration.
//!
//! `eggpool-wire` is declared exactly once in this workspace, in
//! `crates/codegg-providers/Cargo.toml`. Cargo resolves a git
//! dependency's path dependencies against the **first** declaration it
//! encounters, so declaring the same git dependency in a second member
//! manifest gives the workspace two copies of the crate — two distinct
//! `WireSurface` types, and two of everything else in it.
//!
//! `codegg-core` must reach the kernel's capability machinery, so it
//! reaches it *through this module* rather than through a direct
//! dependency on `eggpool-wire`. Everything re-exported here is the
//! kernel's own type, not a copy of it, so a `NeutralThinkingCapability`
//! built here is the same type the kernel's decision function reads.
//!
//! The kernel's own documentation states the division this module
//! implements: *"Capability outcomes are caller-owned facts, never
//! inferred by codecs."* The kernel supplies the decision function; the
//! facts are CodeGG's to own.

pub use eggpool_wire::adaptation::{
    CapabilityDisposition, NeutralCapabilityStatus, NeutralThinkingCapability,
    ReasoningCapabilityPolicy,
};
pub use eggpool_wire::codec::{AdaptationNotice, CodecError};
pub use eggpool_wire::profile::WireSurface;

use eggpool_wire::adaptation::reasoning_capability_notices_neutral;
use eggpool_wire::ir::{CanonicalRequest, ReasoningIntent, ReasoningMode};

/// The notice code the kernel emits when it drops an unsupported
/// reasoning dimension.
pub const NOTICE_REASONING_DROPPED: &str = "reasoning_control_dropped_by_capability";

/// The notice code the kernel emits when it proceeds with an uncertain
/// reasoning dimension.
pub const NOTICE_REASONING_UNCERTAIN: &str = "reasoning_capability_uncertain";

/// The reasoning control one turn selected, in CodeGG's own vocabulary.
///
/// The kernel's decision function reads a canonical request; a local
/// admission check must not build one. This is the narrow shape the
/// decision function is asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningCapabilitySelection<'a> {
    /// No explicit reasoning control: the model's own default applies.
    Unspecified,
    /// Reasoning is explicitly turned off for this turn.
    Off,
    /// A provider-scoped reasoning-effort label.
    Effort(&'a str),
    /// A fixed extended-thinking token budget.
    Budget(u64),
}

/// The kernel's verdict for one selection against one capability record.
///
/// `Err` is the kernel's refusal; `Ok` carries the dispositions that
/// are not refusals.
pub type ReasoningCapabilityNotices = Result<Vec<AdaptationNotice>, CodecError>;

/// Apply the kernel's reasoning-capability policy to one selection.
///
/// A thin wrapper over
/// `eggpool_wire::adaptation::reasoning_capability_notices_neutral`: it
/// builds the minimal canonical request that function reads and returns
/// the kernel's own result unchanged. The kernel decides the
/// disposition; this module only supplies the facts.
///
/// The kernel's neutral capability type is **status-only** — it carries
/// no accepted-label list — so it never validates a label. The accepted
/// set is CodeGG's own record's business.
///
/// `target` is the wire surface the request will be encoded on. The
/// kernel reads it only to **label** the notice and the error it
/// returns; the disposition does not depend on it. A caller that does
/// not know the surface passes `None`, and the label is then omitted
/// rather than filled with a surface the caller did not know.
pub fn apply_reasoning_capability_policy(
    model: &str,
    selection: ReasoningCapabilitySelection<'_>,
    capability: &NeutralThinkingCapability,
    policy: &ReasoningCapabilityPolicy,
    target: Option<WireSurface>,
) -> ReasoningCapabilityNotices {
    let mut request = CanonicalRequest::from_canonical(model, Vec::new());
    request.reasoning = decision_intent(selection);
    let outcome = reasoning_capability_notices_neutral(
        &request,
        capability,
        policy,
        target.unwrap_or(WireSurface::OpenaiChatCompletions),
    );
    match (target, outcome) {
        // The kernel's signature requires a surface, so an unknown one is
        // passed as a placeholder and the placeholder label is removed
        // again before the value leaves this module. The disposition is
        // identical either way — the kernel reads the surface only to
        // label its output.
        (None, Ok(mut notices)) => {
            for notice in &mut notices {
                notice.target_surface = None;
            }
            Ok(notices)
        }
        (None, Err(mut error)) => {
            error.target_surface = None;
            Err(error)
        }
        (_, outcome) => outcome,
    }
}

/// The kernel intent a selection maps to on the wire.
///
/// This is the shape that reaches a provider, and it is built from the
/// kernel's **own constructors** so it cannot drift from the provider
/// bridge:
///
/// | selection | kernel intent |
/// |---|---|
/// | no preference | `ReasoningIntent::default()` |
/// | explicit disable | `ReasoningIntent::disabled()` |
/// | fixed budget | `ReasoningIntent::fixed(..)` |
/// | effort label | mode `Effort`, label carried verbatim |
///
/// The explicit disable is the kernel's constructor, never a hand-rolled
/// literal: `requested: Some(false)` with `explicit_disable: true` is
/// what makes "off" a *disable* on the wire instead of "no preference".
/// The Anthropic encoder keys on `requested`, so a disable that lost its
/// `Some(false)` would emit no `thinking` field at all — the request
/// would look honoured while the provider received something else.
/// `scripts/check_reasoning_intent_fidelity.py` enforces the same rule
/// for the provider crate.
pub(crate) fn wire_intent(selection: ReasoningCapabilitySelection<'_>) -> ReasoningIntent {
    match selection {
        ReasoningCapabilitySelection::Unspecified => ReasoningIntent::default(),
        ReasoningCapabilitySelection::Off => ReasoningIntent::disabled(),
        ReasoningCapabilitySelection::Budget(tokens) => ReasoningIntent::fixed(tokens),
        ReasoningCapabilitySelection::Effort(label) => ReasoningIntent {
            requested: Some(true),
            mode: ReasoningMode::Effort,
            effort: Some(label.to_string()),
            ..ReasoningIntent::default()
        },
    }
}

/// The intent the kernel's capability **decision** is asked about.
///
/// The kernel's decision function only decides a request that asks for
/// reasoning — it returns early for anything else — so an explicit
/// disable would never be decided at all. The disable is therefore asked
/// as a **toggle request**: the question the kernel's `toggle` dimension
/// answers, "may this model carry a boolean reasoning control?", is
/// exactly the question the disable dimension asks, and an
/// always-reasons model answers it in the negative.
///
/// The probe is derived from [`wire_intent`] so the two cannot drift, and
/// it is never encoded: only [`wire_intent`] reaches a provider.
fn decision_intent(selection: ReasoningCapabilitySelection<'_>) -> ReasoningIntent {
    let mut intent = wire_intent(selection);
    if selection == ReasoningCapabilitySelection::Off {
        intent.requested = Some(true);
    }
    intent
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_core::ReasoningControl;
    use crate::wire::{encode_anthropic_messages, encode_openai_chat, reasoning_intent};
    use crate::{ChatRequest, ContentPart, Message, ProviderRequestContext};
    use std::sync::Arc;

    fn supported() -> NeutralThinkingCapability {
        NeutralThinkingCapability {
            status: NeutralCapabilityStatus::Supported,
            toggle: NeutralCapabilityStatus::Supported,
            effort: NeutralCapabilityStatus::Supported,
            budget: NeutralCapabilityStatus::Supported,
        }
    }

    /// A request carrying one reasoning state, with the concrete selection
    /// spelled out separately so each intent can be built in isolation.
    fn reasoning_request(
        control: ReasoningControl,
        reasoning_effort: Option<&str>,
        thinking_budget: Option<usize>,
    ) -> ChatRequest {
        ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".into()),
                }],
            }],
            model: "test-model".into(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget,
            reasoning_effort: reasoning_effort.map(|label| label.to_string()),
            context: ProviderRequestContext {
                reasoning: control,
                ..Default::default()
            },
        }
    }

    #[test]
    fn unspecified_selection_is_allowed() {
        let policy = ReasoningCapabilityPolicy::default();
        let notices = apply_reasoning_capability_policy(
            "model",
            ReasoningCapabilitySelection::Unspecified,
            &supported(),
            &policy,
            Some(WireSurface::AnthropicMessages),
        )
        .expect("an unspecified selection is never refused");
        assert!(notices.is_empty());
    }

    #[test]
    fn an_unknown_target_surface_leaves_no_surface_claim() {
        let policy = ReasoningCapabilityPolicy::default();
        let notices = apply_reasoning_capability_policy(
            "model",
            ReasoningCapabilitySelection::Unspecified,
            &supported(),
            &policy,
            None,
        )
        .expect("an unspecified selection is never refused");
        assert!(
            notices.iter().all(|notice| notice.target_surface.is_none()),
            "an unknown surface must not be claimed: {notices:?}"
        );
    }

    #[test]
    fn an_unsupported_toggle_is_refused() {
        let policy = ReasoningCapabilityPolicy::default();
        let capability = NeutralThinkingCapability {
            status: NeutralCapabilityStatus::Supported,
            toggle: NeutralCapabilityStatus::Unsupported,
            effort: NeutralCapabilityStatus::Supported,
            budget: NeutralCapabilityStatus::Supported,
        };
        let error = apply_reasoning_capability_policy(
            "model",
            ReasoningCapabilitySelection::Off,
            &capability,
            &policy,
            Some(WireSurface::AnthropicMessages),
        )
        .expect_err("an unsupported disable is refused");
        assert_eq!(error.field.as_deref(), Some("reasoning"));
    }

    #[test]
    fn the_default_policy_is_fail_closed() {
        let policy = ReasoningCapabilityPolicy::default();
        assert_eq!(policy.unsupported, CapabilityDisposition::Reject);
        assert_eq!(policy.unknown, CapabilityDisposition::Reject);
        assert_eq!(policy.mixed, CapabilityDisposition::WarnDrop);
    }

    #[test]
    fn the_capability_path_and_the_bridge_agree_on_every_selection() {
        assert_eq!(
            wire_intent(ReasoningCapabilitySelection::Off),
            ReasoningIntent::disabled(),
            "the explicit disable is the kernel's own constructor"
        );
        assert_eq!(
            wire_intent(ReasoningCapabilitySelection::Off),
            reasoning_intent(&reasoning_request(ReasoningControl::Disabled, None, None)),
            "the capability path and the provider bridge must not disagree about a disable"
        );
        assert_eq!(
            wire_intent(ReasoningCapabilitySelection::Unspecified),
            reasoning_intent(&reasoning_request(
                ReasoningControl::Unspecified,
                None,
                None
            )),
        );
        assert_eq!(
            wire_intent(ReasoningCapabilitySelection::Effort("xhigh")),
            reasoning_intent(&reasoning_request(
                ReasoningControl::Unspecified,
                Some("xhigh"),
                None
            )),
        );
        assert_eq!(
            wire_intent(ReasoningCapabilitySelection::Budget(2048)),
            reasoning_intent(&reasoning_request(
                ReasoningControl::Unspecified,
                None,
                Some(2048)
            )),
        );
    }

    #[test]
    fn an_explicit_disable_reaches_the_anthropic_surface_as_a_disable() {
        let body =
            encode_anthropic_messages(&reasoning_request(ReasoningControl::Disabled, None, None))
                .expect("the anthropic encoder");
        assert_eq!(
            body.get("thinking"),
            Some(&serde_json::json!({"type": "disabled"})),
            "an explicit disable must reach the provider as a disable: {body}"
        );
        assert!(
            body.get("thinking").is_some(),
            "an explicit disable must never travel as no preference: {body}"
        );
    }

    #[test]
    fn an_explicit_disable_reaches_the_openai_surface_as_a_disable() {
        let body = encode_openai_chat(
            &reasoning_request(ReasoningControl::Disabled, None, None),
            None,
            false,
        )
        .expect("the openai chat encoder");
        assert_eq!(
            body.get("reasoning_effort"),
            Some(&serde_json::json!("none")),
            "an explicit disable must reach the provider as a disable: {body}"
        );
    }
}
