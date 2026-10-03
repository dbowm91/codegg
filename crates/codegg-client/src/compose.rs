//! Reusable prompt composition for non-TUI frontends (M004 WP D).
//!
//! This module answers the M004 §9 audit with a shared helper instead of
//! a copy: TUI `App` state and provider/agent-runtime resolution are not
//! imported here. Callers resolve `model`/`agents`/`messages` from
//! daemon-owned state (session DTO, `SessionSelection*`, projection
//! snapshot) and pass them explicitly; this module owns validation,
//! canonical request construction, and the at-most-once intent machine.
//!
//! Agent-list resolution itself stays out: it requires root-crate
//! `Agent`/`Config` semantics (`resolve_agents_with_context`) that no
//! frontend-neutral crate may duplicate. A caller that cannot resolve
//! agents from daemon-owned state must stop and split that dependency
//! (plan §9/§24) rather than fabricate hollow `Agent` DTOs.

use codegg_protocol::core::CoreRequest;
use codegg_protocol::dto::{Agent, ProviderMessage};

/// Client-side bound on captured prompt text. The daemon applies its own
/// authoritative limits; this keeps renderer drafts bounded before any
/// request is constructed.
pub const MAX_PROMPT_TEXT_CHARS: usize = 200_000;

/// Fully resolved inputs for one turn submission. Every field is
/// caller-resolved from daemon-owned state plus the captured user text;
/// nothing here reads TUI, provider, or config state.
#[derive(Debug, Clone)]
pub struct TurnSubmitInput {
    pub session_id: String,
    pub text: String,
    pub plan_mode: bool,
    pub model: String,
    pub agents: Vec<Agent>,
    pub current_agent_idx: usize,
    pub messages: Vec<ProviderMessage>,
}

/// Prompt composition failures. All variants fail closed before any
/// request is constructed or sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ComposerError {
    #[error("prompt has no session")]
    EmptySession,
    #[error("prompt text is empty")]
    EmptyText,
    #[error("prompt text is too long ({chars} chars, max {max})")]
    TextTooLong { chars: usize, max: usize },
    #[error("prompt has no model selection")]
    EmptyModel,
    #[error("prompt has no resolved agents")]
    EmptyAgents,
    #[error("agent index {idx} is out of range for {len} agents")]
    AgentIndexOutOfRange { idx: usize, len: usize },
    #[error("prompt has no provider messages")]
    EmptyMessages,
    #[error("invalid prompt intent transition: {action} from {from}")]
    InvalidTransition {
        from: &'static str,
        action: &'static str,
    },
}

/// Build a canonical `TurnSubmit` request from fully resolved inputs.
/// The text is sent verbatim (no normalization beyond validation): one
/// renderer intent produces exactly one request value.
pub fn compose_turn_submit(input: TurnSubmitInput) -> Result<CoreRequest, ComposerError> {
    if input.session_id.is_empty() {
        return Err(ComposerError::EmptySession);
    }
    if input.text.trim().is_empty() {
        return Err(ComposerError::EmptyText);
    }
    let chars = input.text.chars().count();
    if chars > MAX_PROMPT_TEXT_CHARS {
        return Err(ComposerError::TextTooLong {
            chars,
            max: MAX_PROMPT_TEXT_CHARS,
        });
    }
    if input.model.is_empty() {
        return Err(ComposerError::EmptyModel);
    }
    if input.agents.is_empty() {
        return Err(ComposerError::EmptyAgents);
    }
    if input.current_agent_idx >= input.agents.len() {
        return Err(ComposerError::AgentIndexOutOfRange {
            idx: input.current_agent_idx,
            len: input.agents.len(),
        });
    }
    if input.messages.is_empty() {
        return Err(ComposerError::EmptyMessages);
    }
    Ok(CoreRequest::TurnSubmit {
        session_id: input.session_id,
        text: input.text,
        plan_mode: input.plan_mode,
        model: input.model,
        agents: input.agents,
        current_agent_idx: input.current_agent_idx,
        messages: input.messages,
    })
}

/// Lifecycle of one at-most-once prompt intent (plan §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptIntentState {
    Captured,
    CreatingSession,
    Submitting,
    Accepted,
    Failed,
}

/// One renderer click/Enter captured as a single intent. Repeated UI
/// action while the same intent is in flight is coalesced by the caller
/// holding this value: only `Failed` permits a retry with the preserved
/// draft, and only the captured route generation may complete.
#[derive(Debug, Clone)]
pub struct PromptIntent {
    intent_id: String,
    route_generation: u64,
    session_id: Option<String>,
    text: String,
    text_digest: String,
    state: PromptIntentState,
    failure: Option<String>,
}

impl PromptIntent {
    /// Capture one intent. Fails closed on empty/oversize text; the
    /// caller keeps the editable draft on error.
    pub fn capture(route_generation: u64, text: String) -> Result<Self, ComposerError> {
        if text.trim().is_empty() {
            return Err(ComposerError::EmptyText);
        }
        let chars = text.chars().count();
        if chars > MAX_PROMPT_TEXT_CHARS {
            return Err(ComposerError::TextTooLong {
                chars,
                max: MAX_PROMPT_TEXT_CHARS,
            });
        }
        Ok(Self {
            intent_id: format!("prompt-{}", uuid::Uuid::new_v4()),
            route_generation,
            session_id: None,
            text_digest: text_digest(&text),
            text,
            state: PromptIntentState::Captured,
            failure: None,
        })
    }

    pub fn intent_id(&self) -> &str {
        &self.intent_id
    }

    pub fn route_generation(&self) -> u64 {
        self.route_generation
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn text_digest(&self) -> &str {
        &self.text_digest
    }

    pub fn state(&self) -> PromptIntentState {
        self.state
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// `true` once the route has moved past the captured generation:
    /// any in-flight continuation must be dropped without submitting.
    pub fn is_stale(&self, current_route_generation: u64) -> bool {
        current_route_generation != self.route_generation
    }

    fn transition(
        &mut self,
        from: PromptIntentState,
        to: PromptIntentState,
        action: &'static str,
    ) -> Result<(), ComposerError> {
        if self.state != from {
            return Err(ComposerError::InvalidTransition {
                from: self.state.name(),
                action,
            });
        }
        self.state = to;
        Ok(())
    }

    /// Begin the create-then-submit continuation: exactly one session
    /// creation for an intent captured without a session.
    pub fn begin_create_session(&mut self) -> Result<(), ComposerError> {
        self.transition(
            PromptIntentState::Captured,
            PromptIntentState::CreatingSession,
            "begin_create_session",
        )
    }

    /// Bind the created session and move to submitting. The caller must
    /// have verified the returned binding against the captured route.
    pub fn session_created(&mut self, session_id: String) -> Result<(), ComposerError> {
        self.transition(
            PromptIntentState::CreatingSession,
            PromptIntentState::Submitting,
            "session_created",
        )?;
        self.session_id = Some(session_id);
        Ok(())
    }

    /// Submit on an existing session without creating one.
    pub fn begin_submit_existing(&mut self, session_id: String) -> Result<(), ComposerError> {
        self.transition(
            PromptIntentState::Captured,
            PromptIntentState::Submitting,
            "begin_submit_existing",
        )?;
        self.session_id = Some(session_id);
        Ok(())
    }

    /// The daemon accepted the turn. Terminal: no further transition.
    pub fn accepted(&mut self) -> Result<(), ComposerError> {
        self.transition(
            PromptIntentState::Submitting,
            PromptIntentState::Accepted,
            "accepted",
        )
    }

    /// Record failure and restore the terminal-failed state. The
    /// captured text is preserved as the editable draft; no durable user
    /// message is fabricated. Only `Failed` permits `retry`.
    pub fn failed(&mut self, error: String) -> Result<(), ComposerError> {
        if !matches!(
            self.state,
            PromptIntentState::CreatingSession | PromptIntentState::Submitting
        ) {
            return Err(ComposerError::InvalidTransition {
                from: self.state.name(),
                action: "failed",
            });
        }
        self.state = PromptIntentState::Failed;
        self.failure = Some(error);
        Ok(())
    }

    /// Retry a failed intent on the same route generation. Committed
    /// daemon work (if any) is untouched; the caller re-enters at
    /// submit or create and the daemon's idempotency decides.
    pub fn retry(&mut self) -> Result<(), ComposerError> {
        self.transition(
            PromptIntentState::Failed,
            PromptIntentState::Captured,
            "retry",
        )?;
        self.failure = None;
        Ok(())
    }
}

impl PromptIntentState {
    fn name(self) -> &'static str {
        match self {
            PromptIntentState::Captured => "captured",
            PromptIntentState::CreatingSession => "creating_session",
            PromptIntentState::Submitting => "submitting",
            PromptIntentState::Accepted => "accepted",
            PromptIntentState::Failed => "failed",
        }
    }
}

/// Non-cryptographic digest for intent logging/dedup display. Not a
/// security boundary; collision resistance is explicitly not required.
fn text_digest(text: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::dto::{Agent, ContentPart, ProviderMessage};

    fn input() -> TurnSubmitInput {
        TurnSubmitInput {
            session_id: "session-1".into(),
            text: "hello".into(),
            plan_mode: false,
            model: "test/model".into(),
            agents: vec![Agent {
                name: "fixture-agent".into(),
                ..Default::default()
            }],
            current_agent_idx: 0,
            messages: vec![ProviderMessage::User {
                content: vec![ContentPart::Text {
                    text: "hello".into(),
                }],
            }],
        }
    }

    #[test]
    fn valid_inputs_compose_exact_request() {
        let request = compose_turn_submit(input()).expect("compose");
        let CoreRequest::TurnSubmit {
            session_id,
            text,
            plan_mode,
            model,
            agents,
            current_agent_idx,
            messages,
        } = request
        else {
            panic!("expected TurnSubmit");
        };
        assert_eq!(session_id, "session-1");
        assert_eq!(text, "hello");
        assert!(!plan_mode);
        assert_eq!(model, "test/model");
        assert_eq!(agents.len(), 1);
        assert_eq!(current_agent_idx, 0);
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn composition_validates_every_field() {
        let mut bad = input();
        bad.session_id.clear();
        assert_eq!(
            compose_turn_submit(bad).unwrap_err(),
            ComposerError::EmptySession
        );
        let mut bad = input();
        bad.text = "   ".into();
        assert_eq!(
            compose_turn_submit(bad).unwrap_err(),
            ComposerError::EmptyText
        );
        let mut bad = input();
        bad.model.clear();
        assert_eq!(
            compose_turn_submit(bad).unwrap_err(),
            ComposerError::EmptyModel
        );
        let mut bad = input();
        bad.agents.clear();
        assert_eq!(
            compose_turn_submit(bad).unwrap_err(),
            ComposerError::EmptyAgents
        );
        let mut bad = input();
        bad.current_agent_idx = 7;
        assert_eq!(
            compose_turn_submit(bad).unwrap_err(),
            ComposerError::AgentIndexOutOfRange { idx: 7, len: 1 }
        );
        let mut bad = input();
        bad.messages.clear();
        assert_eq!(
            compose_turn_submit(bad).unwrap_err(),
            ComposerError::EmptyMessages
        );
        let mut bad = input();
        bad.text = "x".repeat(MAX_PROMPT_TEXT_CHARS + 1);
        assert!(matches!(
            compose_turn_submit(bad).unwrap_err(),
            ComposerError::TextTooLong { .. }
        ));
    }

    #[test]
    fn intent_create_then_submit_is_exactly_once() {
        let mut intent = PromptIntent::capture(3, "do it".into()).expect("capture");
        assert_eq!(intent.state(), PromptIntentState::Captured);
        assert_eq!(intent.route_generation(), 3);
        assert!(!intent.is_stale(3));
        assert!(intent.is_stale(4));
        // Double create is rejected: one session per intent.
        intent.begin_create_session().expect("create");
        assert!(intent.begin_create_session().is_err());
        intent.session_created("session-9".into()).expect("created");
        assert_eq!(intent.session_id(), Some("session-9"));
        // Double accept path is rejected after terminal accept.
        intent.accepted().expect("accepted");
        assert_eq!(intent.state(), PromptIntentState::Accepted);
        assert!(intent.accepted().is_err());
        assert!(intent.retry().is_err());
    }

    #[test]
    fn intent_failure_preserves_draft_and_allows_retry() {
        let mut intent = PromptIntent::capture(1, "draft text".into()).expect("capture");
        intent
            .begin_submit_existing("session-1".into())
            .expect("submit");
        intent.failed("provider_not_found".into()).expect("failed");
        assert_eq!(intent.state(), PromptIntentState::Failed);
        assert_eq!(intent.failure(), Some("provider_not_found"));
        assert_eq!(intent.text(), "draft text");
        intent.retry().expect("retry");
        assert_eq!(intent.state(), PromptIntentState::Captured);
        assert_eq!(intent.failure(), None);
        // Retry re-enters submit on the same generation.
        intent
            .begin_submit_existing("session-1".into())
            .expect("resubmit");
        intent.accepted().expect("accepted");
    }

    #[test]
    fn intent_rejects_empty_and_oversize_text() {
        assert_eq!(
            PromptIntent::capture(0, "  ".into()).unwrap_err(),
            ComposerError::EmptyText
        );
        assert!(matches!(
            PromptIntent::capture(0, "y".repeat(MAX_PROMPT_TEXT_CHARS + 1)).unwrap_err(),
            ComposerError::TextTooLong { .. }
        ));
    }

    #[test]
    fn intent_digest_is_stable_for_text() {
        let first = PromptIntent::capture(0, "same".into()).expect("capture");
        let second = PromptIntent::capture(0, "same".into()).expect("capture");
        let third = PromptIntent::capture(0, "different".into()).expect("capture");
        assert_eq!(first.text_digest(), second.text_digest());
        assert_ne!(first.text_digest(), third.text_digest());
        // Intent ids stay unique per capture (at-most-once identity).
        assert_ne!(first.intent_id(), second.intent_id());
    }
}
