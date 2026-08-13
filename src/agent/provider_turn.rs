//! Provider-turn normalization boundary.

use super::r#loop::AgentLoop;
use crate::bus::events::AppEvent;
use crate::error::{AppError, ProviderError};
use crate::provider::{ChatEvent, ChatRequest};
use std::sync::Arc;
use std::time::Duration;

/// Adapter-owned entry point for provider streaming, retries, and normalized
/// chat events. The turn driver does not need to know wire compatibility
/// details; it consumes the canonical event stream.
pub(super) struct ProviderTurnAdapter;

impl ProviderTurnAdapter {
    pub(super) async fn receive(
        loop_: &mut AgentLoop,
        request: &ChatRequest,
    ) -> Result<Vec<ChatEvent>, AppError> {
        stream_with_retry(loop_, request).await
    }
}

async fn stream_with_retry(
    loop_: &mut AgentLoop,
    request: &ChatRequest,
) -> Result<Vec<ChatEvent>, AppError> {
    const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
    let max_retries = 3;
    let mut delay = Duration::from_secs(1);
    let mut last_retryable_err: Option<AppError> = None;

    for attempt in 0..max_retries {
        if attempt > 0 {
            tracing::info!("Retry attempt {} after {:?}", attempt, delay);
            tokio::time::sleep(delay).await;
            delay = delay.saturating_mul(2).min(MAX_RETRY_DELAY);
        }

        match stream_once(loop_, request).await {
            Ok(events) => return Ok(events),
            Err(e) => {
                let is_retryable = matches!(&e, AppError::Provider(p) if p.is_retryable());
                if is_retryable {
                    last_retryable_err = Some(e);
                    continue;
                } else {
                    return Err(e);
                }
            }
        }
    }

    Err(last_retryable_err.unwrap_or_else(|| AppError::Provider(ProviderError::RateLimit)))
}

async fn stream_once(
    loop_: &mut AgentLoop,
    request: &ChatRequest,
) -> Result<Vec<ChatEvent>, AppError> {
    let stream = tokio::time::timeout(Duration::from_secs(120), loop_.provider.stream(request))
        .await
        .map_err(|_| {
            AppError::Provider(ProviderError::Timeout(
                "provider stream timeout".to_string(),
            ))
        })??;
    let mut events = Vec::with_capacity(64);
    let session_id_arc: Arc<str> = Arc::from(loop_.session_id.as_str());
    let model_name = request.model.clone();
    let provider_name = loop_.provider.name().to_string();
    let usage_store = loop_.usage_store.clone();
    let pricing_service = crate::util::pricing::PricingService::new();

    use futures_util::StreamExt;
    let mut stream = stream;
    const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
    loop {
        let next_event = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next())
            .await
            .map_err(|_| {
                AppError::Provider(ProviderError::Timeout(
                    "provider stream stalled waiting for next event".to_string(),
                ))
            })?;
        let Some(event) = next_event else {
            break;
        };
        match event {
            Ok(evt) => {
                match &evt {
                    ChatEvent::TextDelta(text) => {
                        crate::bus::global::GlobalEventBus::publish(AppEvent::TextDelta {
                            session_id: Arc::clone(&session_id_arc),
                            delta: Arc::from(text.as_str()),
                        });
                    }
                    ChatEvent::ReasoningDelta(text) => {
                        crate::bus::global::GlobalEventBus::publish(AppEvent::ReasoningDelta {
                            session_id: Arc::clone(&session_id_arc),
                            delta: text.to_string(),
                        });
                    }
                    ChatEvent::ToolCall(tc) => {
                        crate::bus::global::GlobalEventBus::publish(AppEvent::ToolCallStarted {
                            session_id: loop_.session_id.clone(),
                            tool_name: tc.name.to_string(),
                            tool_id: tc.id.to_string(),
                            arguments: tc.arguments.to_string(),
                        });
                    }
                    ChatEvent::Finish { usage, .. } => {
                        if let Some(ref store) = usage_store {
                            let session_id = loop_.session_id.clone();
                            let model = model_name.clone();
                            let provider = provider_name.clone();
                            let input_tokens = usage.input_tokens as i64;
                            let output_tokens = usage.output_tokens as i64;
                            let cached_tokens = usage.cached_tokens.unwrap_or(0) as i64;
                            let cost_usd = pricing_service.calculate_cost(
                                &provider,
                                &model,
                                input_tokens,
                                output_tokens,
                                cached_tokens,
                            );
                            let timestamp = chrono::Utc::now().timestamp_millis();
                            let record = crate::session::UsageRecord {
                                id: uuid::Uuid::new_v4().to_string(),
                                session_id,
                                provider,
                                model,
                                input_tokens,
                                output_tokens,
                                cached_tokens,
                                cost_usd,
                                timestamp,
                            };
                            let store = store.clone();
                            tokio::spawn(async move {
                                if let Err(e) = store.insert(record).await {
                                    tracing::error!("failed to insert usage record: {}", e);
                                }
                            });
                        }
                        // Provider usage is a response delta. Accumulate it
                        // for goal accounting while keeping the hard limits'
                        // counters cumulative independently.
                        loop_.state.unaccounted_input_tokens = loop_
                            .state
                            .unaccounted_input_tokens
                            .saturating_add(usage.input_tokens as i64);
                        loop_.state.unaccounted_output_tokens = loop_
                            .state
                            .unaccounted_output_tokens
                            .saturating_add(usage.output_tokens as i64);
                        // Context cache stats are now recorded once per
                        // provider response via the main loop's call to
                        // record_context_cache_stats_from_processor().
                    }
                    _ => {}
                }
                events.push(evt);
            }
            Err(e) => return Err(AppError::Provider(e)),
        }
    }

    Ok(events)
}
