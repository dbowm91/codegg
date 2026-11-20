//! A scripted model provider shared by the suites that need a deterministic
//! model.
//!
//! The provider answers with a fixed list of scripted turns and records every
//! request it receives, so a suite can assert what the agent actually asked
//! for. It lives in the shared tree because two suites need it — the
//! agent-loop harness and the M011 composed two-agent race — and a private
//! copy in each would let the two drift apart.
//!
//! The optional barrier is the reason this is shared rather than copied. It
//! lets two independent providers hold their **first** request until every
//! participant has arrived, which is how the M011 race makes two agents live
//! at the same time instead of running one after the other. A suite that
//! wants the provider to answer immediately simply does not set one.

#![allow(dead_code)]

use std::sync::Arc;

use async_trait::async_trait;
use codegg::provider::{
    ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError, TokenUsage,
};
use tokio::sync::{Barrier, Mutex};

/// What the provider does when the scripted turns run out.
///
/// The suite that wants an immediate answer — the agent-loop harness and the
/// M011 race — is the reason the default is [`Exhaustion::Finish`]: a script
/// that ran one turn short used to end the turn quietly, and changing that
/// would change what those suites measure.
///
/// The replay tier needs the opposite. A recorded scenario declares exactly
/// the turns it expects, so a loop that asks for one more has left the
/// fixture — and a synthetic empty `Finish` would let that pass as a
/// completed turn. [`Exhaustion::Fail`] names the request index instead.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Exhaustion {
    /// Answer an extra request with an empty completed turn.
    #[default]
    Finish,
    /// Fail the extra request, naming its index.
    Fail,
}

/// A provider that answers with scripted turns and records its requests.
#[derive(Clone)]
pub struct ScriptedProvider {
    responses: Vec<Vec<ChatEvent>>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    /// When each request arrived.
    request_times: Arc<Mutex<Vec<std::time::Instant>>>,
    response_index: Arc<Mutex<usize>>,
    /// Held by the **first** request so a suite can line two providers up
    /// against each other. `None` for a suite that wants an immediate answer.
    barrier: Option<Arc<Barrier>>,
    /// Whether this provider has already claimed its barrier slot. Shared
    /// across clones so a cloned provider does not wait a second time.
    barrier_claimed: Arc<Mutex<bool>>,
    /// What to do when the script runs out.
    exhaustion: Exhaustion,
}

impl ScriptedProvider {
    pub fn new(responses: Vec<Vec<ChatEvent>>) -> Self {
        Self {
            responses,
            requests: Arc::new(Mutex::new(Vec::new())),
            request_times: Arc::new(Mutex::new(Vec::new())),
            response_index: Arc::new(Mutex::new(0)),
            barrier: None,
            barrier_claimed: Arc::new(Mutex::new(false)),
            exhaustion: Exhaustion::Finish,
        }
    }

    /// Choose what happens when the script runs out. See [`Exhaustion`].
    pub fn with_exhaustion(mut self, exhaustion: Exhaustion) -> Self {
        self.exhaustion = exhaustion;
        self
    }

    /// Hold the first request until the barrier has as many participants as
    /// it was built with.
    pub fn with_barrier(mut self, barrier: Arc<Barrier>) -> Self {
        self.barrier = Some(barrier);
        self
    }

    /// Every request the provider has received, in order.
    pub async fn get_requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().await.clone()
    }

    /// The request log itself, for a suite that wants to watch it as the
    /// turn unfolds rather than read it after the fact.
    pub fn request_log(&self) -> Arc<Mutex<Vec<ChatRequest>>> {
        Arc::clone(&self.requests)
    }

    /// When each request arrived.
    ///
    /// The first request opens an agent's turn and the second closes it, so
    /// the gap between the two is the window in which the agent's tool calls
    /// ran. A suite that needs to show two agents were live at the same time
    /// reads the windows from here.
    pub async fn request_instants(&self) -> Vec<std::time::Instant> {
        self.request_times.lock().await.clone()
    }

    /// How many requests the provider has served.
    pub async fn request_count(&self) -> usize {
        *self.response_index.lock().await
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    fn id(&self) -> &str {
        "scripted"
    }

    fn name(&self) -> &str {
        "Scripted Provider"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        // Line the first request up against the other participants before the
        // agent is allowed to act on it.
        if let Some(barrier) = self.barrier.as_ref() {
            let mut claimed = self.barrier_claimed.lock().await;
            if !*claimed {
                *claimed = true;
                drop(claimed);
                barrier.wait().await;
            }
        }

        self.requests.lock().await.push(request.clone());
        self.request_times
            .lock()
            .await
            .push(std::time::Instant::now());

        let mut idx = self.response_index.lock().await;
        let request_index = *idx;
        if request_index >= self.responses.len() {
            match self.exhaustion {
                // The provider has already recorded the request, so the
                // caller can still see exactly which request had no script.
                Exhaustion::Fail => {
                    *idx += 1;
                    return Err(ProviderError::api(
                        "replay_fixture_exhausted",
                        format!(
                            "fixture script exhausted at request index {request_index}: \
                             the loop asked for a turn the fixture does not script"
                        ),
                    ));
                }
                Exhaustion::Finish => {}
            }
        }
        let events = if *idx < self.responses.len() {
            self.responses[*idx].clone()
        } else {
            vec![ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            }]
        };
        *idx += 1;

        let stream = futures_util::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: "test/model".to_string(),
            name: "Test Model".to_string(),
            provider: "test".to_string(),
            context_window: 4096,
            max_output_tokens: Some(2048),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        }])
    }
}
