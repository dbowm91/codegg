//! Provider-turn normalization boundary with attempt safety.
//!
//! M001 (execution-reliability) owns this retry loop. The policy is:
//!
//! - Every logical turn gets one UUID-scoped attempt chain; each replay is
//!   a new `ProviderAttemptId` published on the bus for attribution.
//! - Failures are classified by the canonical
//!   [`ProviderError::retry_disposition`](crate::provider::ProviderError::retry_disposition)
//!   taxonomy. Only `Transient` failures retry; `Permanent` (auth, invalid
//!   request, missing model) and `Conditional` (circuit-open, credential
//!   refresh) never retry inside this loop.
//! - Once an attempt has emitted externally visible output (`TextDelta`,
//!   `ReasoningDelta`, `ToolCallStarted`), the turn does NOT transparently
//!   replay. It publishes an explicit supersession marker and returns a
//!   typed interrupted-attempt error carrying the attempt ID, so two
//!   generations can never masquerade as one authoritative stream and an
//!   abandoned tool-call start is never executed.
//! - Backoff is bounded exponential with full jitter plus a capped
//!   server `Retry-After` hint, and sleeps are cancellation-aware.
//! - The session-selected provider object is never replaced here; retry
//!   reuses the same request and provider handle (fallback internals are
//!   owned by `FallbackProvider`, not by session selection).

use super::r#loop::AgentLoop;
use crate::bus::events::AppEvent;
use crate::error::{AppError, ProviderError};
use crate::provider::{ChatEvent, ChatRequest, RetryContext, RetryDisposition};
use std::sync::Arc;
use std::time::Duration;

/// Adapter-owned entry point for provider streaming, retries, and normalized
/// chat events. The turn driver does not need to know wire compatibility
/// details; it consumes the canonical event stream.
///
/// M002: the retry loop consumes the caller's [`RetryContext`] rather than
/// an independent budget. Provider caps remain lower ceilings; a caller chain
/// with fewer remaining attempts narrows this loop, and nested layers can
/// never replenish it.
pub(super) struct ProviderTurnAdapter;

impl ProviderTurnAdapter {
    /// Legacy single-chain entry point. Preserved for callers that have
    /// not yet threaded a parent [`RetryContext`](crate::provider::RetryContext).
    #[allow(dead_code)]
    pub(super) async fn receive(
        loop_: &mut AgentLoop,
        request: &ChatRequest,
    ) -> Result<Vec<ChatEvent>, AppError> {
        stream_with_retry(loop_, request, None).await
    }

    pub(super) async fn receive_with_retry_context(
        loop_: &mut AgentLoop,
        request: &ChatRequest,
        retry: Option<RetryContext>,
    ) -> Result<Vec<ChatEvent>, AppError> {
        stream_with_retry(loop_, request, retry).await
    }
}

const MAX_ATTEMPTS: usize = 3;
const BASE_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
const STREAM_SETUP_TIMEOUT: Duration = Duration::from_secs(120);
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

async fn stream_with_retry(
    loop_: &mut AgentLoop,
    request: &ChatRequest,
    retry: Option<RetryContext>,
) -> Result<Vec<ChatEvent>, AppError> {
    let session_id = loop_.session_id.clone();
    // Logical provider/model identity for diagnostics only. Credentials and
    // request URLs are never logged here.
    let provider_name = loop_.services.provider.name().to_string();
    let model_name = request.model.clone();
    let mut last_err: Option<AppError> = None;

    // M002: bind to the caller chain when present; otherwise fall back to
    // the provider-local ceiling. The effective bound is always the
    // minimum, so a parent budget can only narrow this loop.
    let mut chain = retry.unwrap_or_else(RetryContext::for_provider_turn);
    let chain_id = chain.chain_id().as_str().to_string();
    let effective_max = (MAX_ATTEMPTS as u8).min(chain.attempts_remaining().max(1)) as usize;
    if chain.is_expired() {
        tracing::warn!(
            session_id = %session_id,
            chain_id = %chain_id,
            "provider retry chain already expired; no attempt made"
        );
        return Err(AppError::Provider(ProviderError::Timeout(
            "provider retry chain expired".to_string(),
        )));
    }

    for attempt_index in 0..effective_max {
        if is_cancelled(loop_) {
            tracing::info!(
                session_id = %session_id,
                attempt_index,
                "provider turn cancelled before attempt"
            );
            return Err(cancelled_error());
        }
        let attempt_id = new_attempt_id();
        crate::bus::global::GlobalEventBus::publish(AppEvent::ProviderAttemptStarted {
            session_id: session_id.clone(),
            attempt_id: attempt_id.clone(),
            attempt_index,
        });

        match stream_once(loop_, request, &attempt_id).await {
            Ok((events, _visible)) => {
                tracing::debug!(
                    session_id = %session_id,
                    chain_id = %chain_id,
                    provider = %provider_name,
                    model = %model_name,
                    attempt_id = %attempt_id,
                    attempt_index,
                    attempts_consumed = chain.attempts_consumed() + 1,
                    "provider attempt succeeded"
                );
                return Ok(events);
            }
            Err((error, visible_output)) => {
                let disposition = provider_disposition(&error);
                let error_class = provider_error_class(&error);
                let retryable = matches!(disposition, RetryDisposition::Transient);

                if is_cancelled(loop_) {
                    publish_attempt_failed(
                        &session_id,
                        &attempt_id,
                        attempt_index,
                        &error_class,
                        visible_output,
                        false,
                    );
                    tracing::info!(
                        session_id = %session_id,
                        provider = %provider_name,
                        model = %model_name,
                        attempt_id = %attempt_id,
                        attempt_index,
                        error_class = %error_class,
                        "provider turn cancelled; no further attempt"
                    );
                    return Err(cancelled_error());
                }

                // Visible-output gate: never silently replay after visible
                // output. Mark the attempt superseded and stop with a typed
                // interruption that carries the attempt identity. The
                // partial `events` buffer was already discarded by
                // `stream_once`, so no abandoned tool call is executed.
                if visible_output {
                    crate::bus::global::GlobalEventBus::publish(
                        AppEvent::ProviderAttemptSuperseded {
                            session_id: session_id.clone(),
                            attempt_id: attempt_id.clone(),
                            attempt_index,
                            reason: format!(
                                "interrupted after visible output ({error_class}); \
                                 not retried to avoid merging generations"
                            ),
                        },
                    );
                    publish_attempt_failed(
                        &session_id,
                        &attempt_id,
                        attempt_index,
                        &error_class,
                        true,
                        false,
                    );
                    tracing::warn!(
                        session_id = %session_id,
                        provider = %provider_name,
                        model = %model_name,
                        attempt_id = %attempt_id,
                        attempt_index,
                        error_class = %error_class,
                        retryable,
                        "provider attempt interrupted after visible output; superseded without replay"
                    );
                    return Err(AppError::Provider(ProviderError::Stream(format!(
                        "provider stream interrupted after visible output \
                         (attempt {attempt_id}, class {error_class}); \
                         not retried to avoid merging generations: {error}"
                    ))));
                }

                // M002: consume the shared chain for every failed attempt,
                // including the terminal one, so nested layers observe the
                // same bound. Cancellation/deadline wins over backoff.
                chain.consume_one();
                if chain.is_expired() {
                    publish_attempt_failed(
                        &session_id,
                        &attempt_id,
                        attempt_index,
                        &error_class,
                        visible_output,
                        false,
                    );
                    tracing::warn!(
                        session_id = %session_id,
                        chain_id = %chain_id,
                        attempt_id = %attempt_id,
                        attempt_index,
                        error_class = %error_class,
                        "provider retry chain deadline reached; stopping"
                    );
                    return Err(error);
                }
                let attempts_left =
                    (effective_max - attempt_index - 1).min(chain.attempts_remaining() as usize);
                if retryable && attempts_left > 0 {
                    let hint = provider_retry_after(&error);
                    let cap = backoff_cap(attempt_index, hint);
                    let delay = apply_full_jitter(cap);
                    publish_attempt_failed(
                        &session_id,
                        &attempt_id,
                        attempt_index,
                        &error_class,
                        false,
                        true,
                    );
                    tracing::info!(
                        session_id = %session_id,
                        provider = %provider_name,
                        model = %model_name,
                        attempt_id = %attempt_id,
                        attempt_index,
                        error_class = %error_class,
                        delay_ms = delay.as_millis() as u64,
                        attempts_left,
                        "provider attempt failed before visible output; retrying"
                    );
                    last_err = Some(error);
                    if sleep_cancellable(loop_, delay).await {
                        tracing::info!(
                            session_id = %session_id,
                            attempt_id = %attempt_id,
                            "provider turn cancelled during backoff"
                        );
                        return Err(cancelled_error());
                    }
                    continue;
                }

                publish_attempt_failed(
                    &session_id,
                    &attempt_id,
                    attempt_index,
                    &error_class,
                    visible_output,
                    false,
                );
                tracing::warn!(
                    session_id = %session_id,
                    chain_id = %chain_id,
                    provider = %provider_name,
                    model = %model_name,
                    attempt_id = %attempt_id,
                    attempt_index,
                    error_class = %error_class,
                    retryable,
                    attempts_consumed = chain.attempts_consumed(),
                    "provider attempt failed terminally"
                );
                return Err(error);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| AppError::Provider(ProviderError::RateLimit)))
}

/// Short random attempt identity, stable for exactly one replay.
/// Nested under the loop's session/turn; concurrent turns generate
/// independent IDs.
fn new_attempt_id() -> String {
    let id = uuid::Uuid::new_v4().to_string();
    id[..8].to_string()
}

fn is_cancelled(loop_: &AgentLoop) -> bool {
    loop_.cancel_rx.as_ref().is_some_and(|rx| *rx.borrow())
}

fn cancelled_error() -> AppError {
    AppError::Other(anyhow::anyhow!("provider turn cancelled"))
}

fn provider_disposition(error: &AppError) -> RetryDisposition {
    match error {
        AppError::Provider(p) => p.retry_disposition(),
        _ => RetryDisposition::Permanent,
    }
}

fn provider_error_class(error: &AppError) -> String {
    match error {
        AppError::Provider(p) => p.error_class().to_string(),
        _ => "non_provider".to_string(),
    }
}

fn provider_retry_after(error: &AppError) -> Option<Duration> {
    match error {
        AppError::Provider(p) => p.retry_after(),
        _ => None,
    }
}

fn publish_attempt_failed(
    session_id: &str,
    attempt_id: &str,
    attempt_index: usize,
    error_class: &str,
    visible_output: bool,
    will_retry: bool,
) {
    crate::bus::global::GlobalEventBus::publish(AppEvent::ProviderAttemptFailed {
        session_id: session_id.to_string(),
        attempt_id: attempt_id.to_string(),
        attempt_index,
        error_class: error_class.to_string(),
        visible_output,
        will_retry,
    });
}

/// Deterministic pre-jitter backoff ceiling for retry ordinal
/// `failed_attempt_index` (0-based index of the attempt that just failed).
/// Bounded exponential (`BASE * 2^n`, capped at [`MAX_RETRY_DELAY`]);
/// a server `Retry-After` hint raises the floor but never the cap.
fn backoff_cap(failed_attempt_index: usize, hint: Option<Duration>) -> Duration {
    let shift = failed_attempt_index.min(5) as u32;
    let exponential = BASE_RETRY_DELAY
        .saturating_mul(1 << shift)
        .min(MAX_RETRY_DELAY);
    match hint {
        None => exponential,
        Some(h) => exponential.max(h.min(MAX_RETRY_DELAY)),
    }
}

/// Full jitter: uniform random in `[0, cap]`.
fn apply_full_jitter(cap: Duration) -> Duration {
    let cap_ms = cap.as_millis().min(u64::MAX as u128) as u64;
    let jitter_ms = rand::random::<u64>() % cap_ms.saturating_add(1);
    Duration::from_millis(jitter_ms)
}

/// Cancellation-aware sleep. Returns `true` when cancellation was observed.
async fn sleep_cancellable(loop_: &mut AgentLoop, duration: Duration) -> bool {
    if duration.is_zero() {
        return is_cancelled(loop_);
    }
    if is_cancelled(loop_) {
        return true;
    }
    let Some(rx) = loop_.cancel_rx.as_mut() else {
        tokio::time::sleep(duration).await;
        return false;
    };
    tokio::select! {
        _ = tokio::time::sleep(duration) => is_cancelled(loop_),
        _ = async { let _ = rx.changed().await; } => true,
    }
}

/// Run one streaming attempt.
///
/// Publishes `TextDelta`/`ReasoningDelta`/`ToolCallStarted` bus events as
/// they arrive (streaming UX). Returns the visible-output flag alongside
/// the outcome so the retry loop can enforce the no-replay-after-visible
/// policy. On failure the partial event buffer is discarded: the caller
/// never executes a tool from an abandoned incomplete attempt.
async fn stream_once(
    loop_: &mut AgentLoop,
    request: &ChatRequest,
    attempt_id: &str,
) -> Result<(Vec<ChatEvent>, bool), (AppError, bool)> {
    let stream = tokio::time::timeout(
        STREAM_SETUP_TIMEOUT,
        loop_.services.provider.stream(request),
    )
    .await
    .map_err(|_| {
        (
            AppError::Provider(ProviderError::Timeout(
                "provider stream timeout".to_string(),
            )),
            false,
        )
    })?
    .map_err(|e| (AppError::Provider(e), false))?;
    let mut events = Vec::with_capacity(64);
    let mut visible_output = false;
    let session_id_arc: Arc<str> = Arc::from(loop_.session_id.as_str());
    let model_name = request.model.clone();
    let provider_name = loop_.services.provider.name().to_string();
    let usage_store = loop_.services.usage_store.clone();
    let pricing_service = crate::util::pricing::PricingService::new();
    let session_id_string = loop_.session_id.clone();

    use futures_util::StreamExt;
    let mut stream = stream;
    loop {
        if is_cancelled(loop_) {
            return Err((cancelled_error(), visible_output));
        }
        let next_event = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next())
            .await
            .map_err(|_| {
                (
                    AppError::Provider(ProviderError::Timeout(
                        "provider stream stalled waiting for next event".to_string(),
                    )),
                    visible_output,
                )
            })?;
        let Some(event) = next_event else {
            break;
        };
        match event {
            Ok(evt) => {
                match &evt {
                    ChatEvent::TextDelta(text) => {
                        visible_output = true;
                        tracing::trace!(
                            session_id = %session_id_string,
                            attempt_id = %attempt_id,
                            delta_len = text.len(),
                            "provider attempt text delta"
                        );
                        crate::bus::global::GlobalEventBus::publish(AppEvent::TextDelta {
                            session_id: Arc::clone(&session_id_arc),
                            delta: Arc::from(text.as_str()),
                        });
                    }
                    ChatEvent::ReasoningDelta(text) => {
                        visible_output = true;
                        crate::bus::global::GlobalEventBus::publish(AppEvent::ReasoningDelta {
                            session_id: Arc::clone(&session_id_arc),
                            delta: text.to_string(),
                        });
                    }
                    ChatEvent::ToolCall(tc) => {
                        visible_output = true;
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
            Err(e) => return Err((AppError::Provider(e), visible_output)),
        }
    }

    Ok((events, visible_output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_cap_grows_and_respects_hint_cap() {
        assert_eq!(backoff_cap(0, None), Duration::from_secs(1));
        assert_eq!(backoff_cap(1, None), Duration::from_secs(2));
        assert_eq!(backoff_cap(2, None), Duration::from_secs(4));
        assert_eq!(backoff_cap(10, None), MAX_RETRY_DELAY);
        // Hint raises the floor but never exceeds the cap.
        assert_eq!(
            backoff_cap(0, Some(Duration::from_secs(5))),
            Duration::from_secs(5)
        );
        assert_eq!(
            backoff_cap(0, Some(Duration::from_secs(3600))),
            MAX_RETRY_DELAY
        );
        assert_eq!(
            backoff_cap(2, Some(Duration::from_secs(1))),
            Duration::from_secs(4)
        );
    }

    #[test]
    fn full_jitter_stays_within_bounds() {
        let cap = Duration::from_secs(4);
        for _ in 0..500 {
            let d = apply_full_jitter(cap);
            assert!(d <= cap, "jitter {d:?} exceeded cap {cap:?}");
        }
        assert!(apply_full_jitter(Duration::ZERO).is_zero());
    }

    #[test]
    fn attempt_ids_are_unique_per_attempt() {
        let a = new_attempt_id();
        let b = new_attempt_id();
        assert_eq!(a.len(), 8);
        assert_ne!(a, b);
    }

    #[test]
    fn non_provider_errors_are_permanent() {
        let err = AppError::Other(anyhow::anyhow!("boom"));
        assert_eq!(provider_disposition(&err), RetryDisposition::Permanent);
        assert_eq!(provider_error_class(&err), "non_provider");
        assert!(provider_retry_after(&err).is_none());
    }
}
