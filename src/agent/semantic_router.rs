//! Codegg-owned execution for the shared semantic model-routing policy.
//!
//! The shared crate supplies deterministic compilation and exact route-ID
//! validation. This module supplies the bounded selector request, cancellation
//! and fallback behavior, and returns a concrete model reference. It never
//! selects or mutates a durable provider connection.

use std::sync::Arc;
use std::time::Duration;

use codegg_core::model_routing::{
    compile_model_routers, is_virtual_model, session_identity_from_header, CompiledModelRouter,
    ModelRouterRegistry, ModelRoutingError, SessionIdentity,
};
use codegg_providers::{ChatEvent, ChatRequest, ContentPart, Message, Provider, ProviderRegistry};
use futures_util::StreamExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticDecisionSource {
    Selector,
    Repair,
    Default,
}

impl SemanticDecisionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selector => "selector",
            Self::Repair => "repair",
            Self::Default => "default",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SemanticRouteDecision {
    pub requested_model: String,
    pub resolved_model: String,
    pub route_id: String,
    pub route_label: String,
    pub source: SemanticDecisionSource,
    pub attempts: u8,
    pub fallback_reason: Option<&'static str>,
    pub session_identity: Option<SessionIdentity>,
}

#[derive(Debug, Clone)]
pub struct SemanticRouter {
    registry: ModelRouterRegistry,
}

impl SemanticRouter {
    pub fn from_config(config: &codegg_config::schema::Config) -> Result<Self, ModelRoutingError> {
        Ok(Self {
            registry: compile_model_routers(config)?,
        })
    }

    pub fn empty() -> Self {
        Self {
            registry: ModelRouterRegistry::empty(),
        }
    }

    pub fn is_virtual_model(&self, model: &str) -> bool {
        is_virtual_model(model) && self.registry.is_virtual(model)
    }

    pub fn has_virtual_model(&self, model: &str) -> bool {
        self.registry.is_virtual(model)
    }

    pub fn registry(&self) -> &ModelRouterRegistry {
        &self.registry
    }

    /// Resolve a virtual model to one exact configured concrete route.
    ///
    /// Selector failures, invalid output, and missing selector providers use
    /// the compiled default route. Cancellation is the one exception: it is
    /// propagated so a cancelled caller never starts default work.
    pub async fn resolve(
        &self,
        request: &ChatRequest,
        providers: &ProviderRegistry,
        mut cancel_rx: Option<&mut tokio::sync::watch::Receiver<bool>>,
    ) -> Result<Option<SemanticRouteDecision>, crate::error::AppError> {
        let Some(router) = self.registry.get(&request.model) else {
            return Ok(None);
        };

        let session_identity = session_identity_from_header(request.context.session_id.as_deref());
        let input = bounded_semantic_input(request, router.max_input_bytes as usize);
        let default_route = default_route(&router);
        let mut attempts = 0u8;

        let Some(selector_provider_id) = router.selector_model.split('/').next() else {
            return Ok(Some(default_decision(
                request,
                default_route,
                session_identity,
                attempts,
                "selector_model_invalid",
            )));
        };
        let selector_provider = providers.get(selector_provider_id);

        if let Some(provider) = selector_provider {
            attempts = attempts.saturating_add(1);
            match selector_call(
                provider,
                selector_request(&router, &input, false),
                Duration::from_secs_f64(router.selector_timeout_s),
                &mut cancel_rx,
            )
            .await
            {
                Ok(output) => {
                    if let Some(route) = router.route_for_id(output.trim()) {
                        return Ok(Some(decision(
                            request,
                            route,
                            SemanticDecisionSource::Selector,
                            attempts,
                            None,
                            session_identity,
                        )));
                    }

                    if router.repair_attempts > 0 {
                        attempts = attempts.saturating_add(1);
                        if let Ok(repaired) = selector_call(
                            provider,
                            selector_request(&router, &input, true),
                            Duration::from_secs_f64(router.selector_timeout_s),
                            &mut cancel_rx,
                        )
                        .await
                        {
                            if let Some(route) = router.route_for_id(repaired.trim()) {
                                return Ok(Some(decision(
                                    request,
                                    route,
                                    SemanticDecisionSource::Repair,
                                    attempts,
                                    None,
                                    session_identity,
                                )));
                            }
                        } else if cancelled(&cancel_rx) {
                            return Err(cancel_error());
                        }
                    }
                    return Ok(Some(default_decision(
                        request,
                        default_route,
                        session_identity,
                        attempts,
                        "invalid_selector_output",
                    )));
                }
                Err(SelectorCallError::Cancelled) => return Err(cancel_error()),
                Err(SelectorCallError::Failed) => {}
            }
        }

        if cancelled(&cancel_rx) {
            return Err(cancel_error());
        }
        Ok(Some(default_decision(
            request,
            default_route,
            session_identity,
            attempts,
            if selector_provider.is_none() {
                "selector_provider_unavailable"
            } else {
                "selector_failed"
            },
        )))
    }
}

fn default_route(router: &CompiledModelRouter) -> &codegg_core::model_routing::CompiledModelRoute {
    router
        .routes
        .iter()
        .find(|route| route.model == router.default_model)
        .expect("shared policy validation guarantees a default route")
}

fn decision(
    request: &ChatRequest,
    route: &codegg_core::model_routing::CompiledModelRoute,
    source: SemanticDecisionSource,
    attempts: u8,
    fallback_reason: Option<&'static str>,
    session_identity: Option<SessionIdentity>,
) -> SemanticRouteDecision {
    SemanticRouteDecision {
        requested_model: request.model.clone(),
        resolved_model: route.model.clone(),
        route_id: route.route_id.clone(),
        route_label: route.label.clone(),
        source,
        attempts,
        fallback_reason,
        session_identity,
    }
}

fn default_decision(
    request: &ChatRequest,
    route: &codegg_core::model_routing::CompiledModelRoute,
    session_identity: Option<SessionIdentity>,
    attempts: u8,
    reason: &'static str,
) -> SemanticRouteDecision {
    decision(
        request,
        route,
        SemanticDecisionSource::Default,
        attempts,
        Some(reason),
        session_identity,
    )
}

fn bounded_semantic_input(request: &ChatRequest, max_bytes: usize) -> String {
    let prompt = request
        .messages
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::User { content } => Some(
                content
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        })
        .unwrap_or_default();
    let indicators = format!(
        "coding_context: tools={}, images={}, reasoning={}\nlatest_user_instruction: ",
        request.tools.as_ref().map_or(0, Vec::len),
        request.messages.iter().any(|message| {
            matches!(
                message,
                Message::User { content }
                    if content.iter().any(|part| matches!(part, ContentPart::Image { .. }))
            )
        }),
        request.reasoning_effort.is_some() || request.thinking_budget.is_some(),
    );
    truncate_utf8(&format!("{indicators}{prompt}"), max_bytes)
}

fn selector_request(router: &CompiledModelRouter, input: &str, repair: bool) -> ChatRequest {
    let repair_hint = if repair {
        "\nThe previous response was invalid. Reply with one route id only."
    } else {
        ""
    };
    ChatRequest {
        messages: vec![Message::User {
            content: vec![ContentPart::Text {
                text: Arc::from(format!("{input}{repair_hint}")),
            }],
        }],
        model: router
            .selector_model
            .split('/')
            .next_back()
            .unwrap_or(&router.selector_model)
            .to_string(),
        tools: None,
        system: Some(String::from_utf8_lossy(&router.static_policy).into_owned()),
        temperature: Some(0.0),
        top_p: None,
        max_tokens: Some(16),
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
        context: Default::default(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectorCallError {
    Failed,
    Cancelled,
}

async fn selector_call(
    provider: &dyn Provider,
    request: ChatRequest,
    timeout: Duration,
    cancel_rx: &mut Option<&mut tokio::sync::watch::Receiver<bool>>,
) -> Result<String, SelectorCallError> {
    if cancelled(cancel_rx) {
        return Err(SelectorCallError::Cancelled);
    }
    let collect = async {
        let mut stream = provider
            .stream(&request)
            .await
            .map_err(|_| SelectorCallError::Failed)?;
        let mut text = String::new();
        while let Some(event) = stream.next().await {
            match event.map_err(|_| SelectorCallError::Failed)? {
                ChatEvent::TextDelta(delta) => text.push_str(&delta),
                ChatEvent::Finish { .. } => break,
                _ => {}
            }
            if text.len() > 256 {
                return Err(SelectorCallError::Failed);
            }
        }
        Ok(text)
    };
    let timed = tokio::time::timeout(timeout, collect);
    if let Some(rx) = cancel_rx.as_deref_mut() {
        tokio::select! {
            result = timed => result.unwrap_or(Err(SelectorCallError::Failed)),
            changed = rx.changed() => {
                let _ = changed;
                Err(SelectorCallError::Cancelled)
            }
        }
    } else {
        timed.await.unwrap_or(Err(SelectorCallError::Failed))
    }
}

fn cancelled(cancel_rx: &Option<&mut tokio::sync::watch::Receiver<bool>>) -> bool {
    cancel_rx.as_ref().map(|rx| *rx.borrow()).unwrap_or(false)
}

fn cancel_error() -> crate::error::AppError {
    crate::error::AgentError::Invalid("semantic route selection cancelled".to_string()).into()
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_config::schema::{Config, ModelRouteConfig, ModelRouterConfig};
    use futures_util::stream;
    use std::collections::HashMap;

    #[derive(Clone)]
    struct FixtureProvider {
        output: Option<&'static str>,
    }

    #[async_trait::async_trait]
    impl Provider for FixtureProvider {
        fn id(&self) -> &str {
            "fixture"
        }

        fn name(&self) -> &str {
            "fixture"
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(self.clone())
        }

        async fn stream(
            &self,
            _request: &ChatRequest,
        ) -> Result<codegg_providers::EventStream, codegg_providers::ProviderError> {
            let output = self.output.unwrap_or("nope").to_string();
            Ok(Box::pin(stream::iter(vec![Ok(ChatEvent::TextDelta(
                Arc::from(output),
            ))])))
        }

        async fn models(
            &self,
        ) -> Result<Vec<codegg_providers::ModelInfo>, codegg_providers::ProviderError> {
            Ok(Vec::new())
        }
    }

    fn router_config(output: &'static str) -> (SemanticRouter, ProviderRegistry) {
        let config = Config {
            model_routers: Some(HashMap::from([(
                "virtual:code".to_string(),
                ModelRouterConfig {
                    selector_model: "fixture/selector".to_string(),
                    default_model: "fixture/default".to_string(),
                    routes: HashMap::from([
                        (
                            "fast".to_string(),
                            ModelRouteConfig {
                                model: "fixture/fast".to_string(),
                                description: "fast".to_string(),
                            },
                        ),
                        (
                            "default".to_string(),
                            ModelRouteConfig {
                                model: "fixture/default".to_string(),
                                description: "default".to_string(),
                            },
                        ),
                    ]),
                    max_input_bytes: 128,
                    ..Default::default()
                },
            )])),
            ..Default::default()
        };
        let router = SemanticRouter::from_config(&config).unwrap();
        let mut providers = ProviderRegistry::new();
        providers.register(FixtureProvider {
            output: Some(output),
        });
        (router, providers)
    }

    fn request(model: &str) -> ChatRequest {
        ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::from("implement the requested change".to_string()),
                }],
            }],
            model: model.to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: codegg_providers::ProviderRequestContext {
                session_id: Some(Arc::from("session-1")),
            },
        }
    }

    #[tokio::test]
    async fn valid_selector_output_resolves_exact_route() {
        let (router, providers) = router_config("1");
        let decision = router
            .resolve(&request("virtual:code"), &providers, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(decision.resolved_model, "fixture/fast");
        assert_eq!(decision.source, SemanticDecisionSource::Selector);
        assert!(decision.session_identity.is_some());
    }

    #[tokio::test]
    async fn invalid_selector_output_uses_default_without_injection() {
        let (router, providers) = router_config("fixture/attacker");
        let decision = router
            .resolve(&request("virtual:code"), &providers, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(decision.resolved_model, "fixture/default");
        assert_eq!(decision.source, SemanticDecisionSource::Default);
        assert_eq!(decision.fallback_reason, Some("invalid_selector_output"));
    }

    #[tokio::test]
    async fn concrete_models_bypass_semantic_routing() {
        let (router, providers) = router_config("0");
        assert!(router
            .resolve(&request("fixture/concrete"), &providers, None)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn cancellation_is_propagated_before_default_work() {
        let (router, providers) = router_config("0");
        let (tx, mut rx) = tokio::sync::watch::channel(true);
        let _ = tx.send(true);
        let error = router
            .resolve(&request("virtual:code"), &providers, Some(&mut rx))
            .await
            .expect_err("cancelled selector should propagate");
        assert!(error.to_string().contains("cancelled"));
    }
}
