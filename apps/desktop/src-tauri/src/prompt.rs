//! Desktop prompt submission host (M004 WP D3).
//!
//! One renderer intent (`text` + `plan_mode` + route token) becomes at
//! most one daemon turn through the narrow
//! `CoreRequest::SessionPromptSubmit` request. The host never resolves
//! model/agents/messages itself; the daemon composes those from
//! durable selection and daemon-owned configuration.
//!
//! At-most-once rules enforced here:
//! - one renderer click/Enter captures one [`PromptIntent`];
//! - a second command while the stored intent for the same route
//!   generation is in flight is coalesced, never submitted twice;
//! - an intent captured without a session creates exactly one session
//!   (reusing `route_session_create` fencing) before submitting once;
//! - any completion that arrives after a route/connection generation
//!   change is stale-dropped: the daemon-side turn (if committed) is
//!   untouched, but the intent is never marked accepted for the new
//!   route;
//! - daemon rejection marks the stored intent failed and returns the
//!   failure to the renderer, which keeps the editable draft (no
//!   durable user message is fabricated anywhere in this path).

use codegg_client::{PromptIntent, PromptIntentState};
use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};
use codegg_protocol::human_shell::HumanShellPromotionStageDto;

use super::bridge::PromptSubmitView;
use super::HostState;

impl HostState {
    /// Submit one prompt for the route-selected session, creating the
    /// session first when the route has none. Returns the accepted
    /// intent view; failures are `Err` strings and leave the renderer
    /// draft intact.
    ///
    /// M007-I (plan 022, work package F): `human_shell_promotions` is the
    /// ask. A non-empty list turns the submit into the daemon's atomic ask
    /// request, which stages the evidence and admits the turn together; an
    /// empty list is the ordinary prompt submit.
    pub(crate) async fn route_prompt_submit(
        &self,
        text: String,
        plan_mode: bool,
        human_shell_promotions: Vec<HumanShellPromotionStageDto>,
        expected_generation: u64,
    ) -> Result<PromptSubmitView, String> {
        let captured =
            PromptIntent::capture(expected_generation, text).map_err(|error| error.to_string())?;
        // The ask's evidence is the ask's half of the intent's identity: the
        // text alone cannot tell two asks apart, and the retry of a failed
        // ask must carry the same evidence or it would admit a turn with no
        // evidence attached.
        let promotions = if human_shell_promotions.is_empty() {
            None
        } else {
            Some(human_shell_promotions)
        };
        // Snapshot the route and publish the in-flight intent before
        // any network I/O so a concurrent command coalesces instead
        // of submitting a second turn.
        let (session_id, connection_generation, mut intent) = {
            let mut route = self.route.lock().await;
            if route.connection_generation != self.current_generation()
                || route.project_id.is_none()
            {
                return Err("no current route; select a project first".into());
            }
            if route.route_generation != expected_generation {
                return Err("stale route; refresh before submitting".into());
            }
            let mut intent = captured;
            if let Some(stored) = route.prompt.as_ref() {
                if stored.route_generation() != expected_generation {
                    route.prompt = None;
                    route.prompt_human_shell_promotions = None;
                } else if matches!(
                    stored.state(),
                    PromptIntentState::CreatingSession | PromptIntentState::Submitting
                ) {
                    if stored.text_digest() == intent.text_digest() {
                        return Err("this prompt is already being submitted".into());
                    }
                    return Err("another prompt is already being submitted for this route".into());
                } else if stored.state() == PromptIntentState::Failed
                    && stored.text_digest() == intent.text_digest()
                    && route.prompt_human_shell_promotions.as_deref() == promotions.as_deref()
                {
                    let mut retry = stored.clone();
                    retry.retry().map_err(|error| error.to_string())?;
                    intent = retry;
                }
            }
            let session_id = route.session_id.clone();
            if let Some(session_id) = session_id.as_deref() {
                intent
                    .begin_submit_existing(session_id.to_owned())
                    .map_err(|error| error.to_string())?;
            } else {
                intent
                    .begin_create_session()
                    .map_err(|error| error.to_string())?;
            }
            let connection_generation = route.connection_generation;
            route.prompt = Some(intent.clone());
            route.prompt_human_shell_promotions = promotions.clone();
            (session_id, connection_generation, intent)
        };
        // Create-then-submit continuation: exactly one session for an
        // intent captured without one. Creation reuses the fenced
        // `route_session_create` path (explicit ids, Rust-resolved root,
        // binding + generation checks).
        let session_id = match session_id {
            Some(session_id) => session_id,
            None => match self.route_session_create(None, expected_generation).await {
                Ok(created) => {
                    intent
                        .session_created(created.session.session_id.clone())
                        .map_err(|error| error.to_string())?;
                    created.session.session_id
                }
                Err(error) => {
                    self.fail_stored_intent(&intent, expected_generation, &error)
                        .await;
                    return Err(error);
                }
            },
        };
        let (client, _) = match self.route_request().await {
            Ok(client) => client,
            Err(error) => {
                self.fail_stored_intent(&intent, expected_generation, &error)
                    .await;
                return Err(error);
            }
        };
        // Pre-send fence: a route switch that landed between session
        // creation and this send must prevent the submit. A switch in
        // the residual check-then-act window is still caught by the
        // post-response fence below, which never marks stale accepted.
        {
            let route = self.route.lock().await;
            if self.current_generation() != connection_generation
                || route.connection_generation != connection_generation
                || route.route_generation != expected_generation
            {
                drop(route);
                let failure = "stale route; prompt discarded before submit".to_string();
                self.fail_stored_intent(&intent, expected_generation, &failure)
                    .await;
                return Err(failure);
            }
        }
        // M007-I (plan 022, work package F): the ask is **one** request. The
        // daemon stages the evidence and admits the turn together, so a
        // failed ask leaves no `pending` row behind.
        let payload = match promotions {
            Some(human_shell_promotions) => {
                CoreRequest::SessionPromptSubmitWithHumanShellPromotions {
                    session_id,
                    text: intent.text().to_owned(),
                    plan_mode,
                    human_shell_promotions,
                }
            }
            None => CoreRequest::SessionPromptSubmit {
                session_id,
                text: intent.text().to_owned(),
                plan_mode,
                human_shell_promotion_ids: Vec::new(),
            },
        };
        let response = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: intent.intent_id().to_owned(),
                payload,
            })
            .await
            .map_err(|error| error.to_string());
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let stale = {
                    let route = self.route.lock().await;
                    self.current_generation() != connection_generation
                        || route.connection_generation != connection_generation
                        || route.route_generation != expected_generation
                };
                if stale {
                    return Err("stale route; prompt result discarded".into());
                }
                self.fail_stored_intent(&intent, expected_generation, &error)
                    .await;
                return Err(error);
            }
        };
        let mut route = self.route.lock().await;
        // Generation fence before result application: a route switch,
        // workspace switch, or reconnect/disconnect mid-flight discards
        // the late result. A committed daemon turn keeps running; the
        // new route converges through projection resync, never through
        // this stale completion.
        if self.current_generation() != connection_generation
            || route.connection_generation != connection_generation
            || route.route_generation != expected_generation
        {
            return Err("stale route; prompt result discarded".into());
        }
        let stored_matches = route
            .prompt
            .as_ref()
            .is_some_and(|stored| stored.intent_id() == intent.intent_id());
        if !stored_matches {
            return Err("stale route; prompt result discarded".into());
        }
        match response {
            CoreResponse::Ack => {
                intent.accepted().map_err(|error| error.to_string())?;
                let token = route.current_token();
                route.prompt = None;
                route.prompt_human_shell_promotions = None;
                Ok(PromptSubmitView {
                    intent_id: intent.intent_id().to_owned(),
                    route_token: token,
                })
            }
            CoreResponse::TurnAdmissionReceipt { receipt } => {
                if receipt.submission_id != intent.intent_id() {
                    let failure = "daemon returned a receipt for a different prompt submission";
                    self.fail_locked_intent(&mut route, &mut intent, failure);
                    return Err(failure.to_owned());
                }
                intent.accepted().map_err(|error| error.to_string())?;
                let token = route.current_token();
                route.prompt = None;
                route.prompt_human_shell_promotions = None;
                Ok(PromptSubmitView {
                    intent_id: intent.intent_id().to_owned(),
                    route_token: token,
                })
            }
            CoreResponse::Error { code, message } => {
                let failure = format!("{code}: {message}");
                self.fail_locked_intent(&mut route, &mut intent, &failure);
                Err(failure)
            }
            unexpected => {
                let failure =
                    format!("daemon returned an unexpected prompt response: {unexpected:?}");
                self.fail_locked_intent(&mut route, &mut intent, &failure);
                Err(failure)
            }
        }
    }

    /// Record failure on the stored intent when it is still ours so a
    /// same-text retry coalesces through `PromptIntent::retry`.
    async fn fail_stored_intent(
        &self,
        intent: &PromptIntent,
        expected_generation: u64,
        failure: &str,
    ) {
        let mut route = self.route.lock().await;
        if route.route_generation != expected_generation {
            return;
        }
        if let Some(stored) = route.prompt.as_mut() {
            if stored.intent_id() == intent.intent_id() {
                let mut failed = stored.clone();
                if failed.failed(failure.to_owned()).is_ok() {
                    *stored = failed;
                }
            }
        }
    }

    /// Same as [`Self::fail_stored_intent`] but the route lock is
    /// already held.
    fn fail_locked_intent(
        &self,
        route: &mut super::route::RouteState,
        intent: &mut PromptIntent,
        failure: &str,
    ) {
        if intent.failed(failure.to_owned()).is_ok() {
            route.prompt = Some(intent.clone());
        }
    }
}
