//! Title-agent session naming.
//!
//! The `title` built-in agent (`assets/agents/title.toml`,
//! `runtime_kind = "title"`) exists so turns can produce a concise
//! human-readable session title. At turn start, the turn runtime fires a
//! best-effort background task that generates a title from the turn's
//! in-memory user request and persists it when the session is still
//! `"Untitled"` (see `maybe_auto_title_session`), so naming does not wait
//! for a minutes-long turn to finish. A post-completion trigger retries
//! with the full assistant outcome when the early attempt loses.
//! Failures are logged and never fail the turn; the next completed turn
//! in the same session retries naturally, since the title is still the
//! creation default.
//!
//! This module wires the agent's contract to a lightweight one-shot
//! provider call (no tools, no sub-agent dispatch, no scheduler work).
//!
//! The excerpt comes from the completed turn rather than the message table:
//! the title task fires while the run is finishing, before
//! [`crate::agent::transcript`] persists the turn.
//!
//! A truncated stream still names the session: this call is capped, so a
//! length-cut terminator arrives as a stream error *after* the title text,
//! and the provider wire layer escalates that terminator to a hard failure.
//! `generate_title_via_provider` salvages the received prefix through the
//! same sanitizer instead of discarding it.
//!
//! A reasoning model must also not be able to starve the answer: the output
//! budget covers reasoning tokens as well as visible text, so the call asks
//! for thinking to be disabled ([`title_request_context`]) and tolerates a
//! reasoning-only preamble (`ReasoningDelta` is counted, never treated as an
//! error or as title text). When no usable title emerges the error carries the
//! bounded stop reason and the reasoning volume, so the daemon log — the only
//! surface this has — explains itself.

use std::sync::Arc;

use futures_util::StreamExt;

use crate::error::AppError;
use crate::provider::{ChatRequest, ContentPart, Message, Provider, ProviderRequestContext};

/// Built-in agent name that owns title generation.
pub(crate) const TITLE_AGENT_NAME: &str = "title";

/// System default title assigned at session creation. Auto-title only
/// replaces this exact value so an explicit user rename is never
/// overwritten.
pub(crate) const UNTITLED_MARKER: &str = "Untitled";

/// Upper bound for a generated title in characters. The TUI renders
/// titles truncated at 48 columns; 60 keeps stored titles concise
/// while leaving display truncation room.
pub(crate) const MAX_TITLE_CHARS: usize = 60;

/// Upper bound for the combined user + assistant excerpt in characters.
pub(crate) const MAX_EXCERPT_CHARS: usize = 2000;

/// Per-side cap so one side cannot starve the other.
const MAX_EXCERPT_SIDE_CHARS: usize = 1000;

/// Bounded one-shot title call.
const TITLE_TIMEOUT_SECS: u64 = 30;
/// Output budget for the title call.
///
/// A 60-char title costs ~15-20 tokens, so the old 64-token cap left almost
/// no headroom — and on a reasoning model the reasoning tokens are billed
/// against the *same* output budget as the visible text. A model that thinks
/// for 64 tokens never emits a text delta at all, the sanitizer rejects the
/// empty string, and the session stays `"Untitled"` forever. 256 leaves ~4x
/// the title's own cost as headroom for preamble the sanitizer strips while
/// keeping the call bounded, one-shot, and far below a normal turn budget
/// (the sanitizer truncates to 60 chars whatever arrives).
const TITLE_MAX_TOKENS: usize = 256;
const TITLE_TEMPERATURE: f64 = 0.7;

/// Bound applied to a provider stop reason before it reaches an error
/// message. Stop reasons are short protocol tokens (`stop`, `length`,
/// `max_tokens`, `max_output_tokens`), but the field is provider-supplied.
const MAX_STOP_REASON_CHARS: usize = 64;

/// Build the wire context for this call.
///
/// Two reasoning-suppression rules are in play, and they are not the same
/// field:
///
/// 1. `thinking_budget` / `reasoning_effort` stay **unset**. The wire bridge
///    treats "neither set" as *thinking not requested* (`wire.rs`), and that
///    is the only encoding the bridge supports for a disable (see
///    `request_preparation.rs` "Off clears both fields"). Setting a budget
///    here would *request* reasoning — and `Some(0)` is worse than that: the
///    kernel encodes it as `thinking: {"type":"enabled","budget_tokens":0}`
///    on the Anthropic surface, which the API rejects outright.
/// 2. `wire_policy.enable_thinking = Some(false)` is the adapter-side toggle.
///    Model adapters that declare a thinking parameter otherwise default it
///    on (`chat_template_kwargs.enable_thinking` in the OpenAI-compatible
///    encoder), and a server-side default cannot be turned off by omitting a
///    field. Any adapter projection already present is preserved; only the
///    thinking toggle is forced off.
fn title_request_context(context: ProviderRequestContext) -> ProviderRequestContext {
    let mut policy = context.wire_policy.as_deref().cloned().unwrap_or_default();
    policy.enable_thinking = Some(false);
    ProviderRequestContext {
        wire_policy: Some(Arc::new(policy)),
        ..context
    }
}

/// System prompt implementing the `title` agent contract
/// (`src/agent/prompt.rs` role contract + output contract).
pub(crate) fn title_system_prompt() -> &'static str {
    "Role contract: You are a title generation agent. Produce a concise session title. \
     Output contract: Return ONLY the title as plain text: 3-8 words, max 60 characters, \
     no quotes, no markdown, no trailing period, no prefix like \"Title:\". \
     Describe the user's task, not the assistant's behavior."
}

/// Strip model chatter down to a storable title.
///
/// Returns `None` when the output is unusable (empty, still untitled,
/// or structured data like JSON) so the caller keeps `"Untitled"`
/// rather than persisting noise.
pub(crate) fn sanitize_title(raw: &str) -> Option<String> {
    let mut text = raw.trim().to_string();
    if text.is_empty() {
        return None;
    }
    // Keep the first non-empty line; multi-line output is chatter.
    text = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string();
    if text.is_empty() {
        return None;
    }
    // Reject structured output rather than guessing inside it.
    if text.starts_with('{') || text.starts_with('[') || text.starts_with('<') {
        return None;
    }
    // Strip a "Title:"-style prefix (case-insensitive).
    if let Some((prefix, rest)) = text.split_once(':') {
        if prefix.trim().eq_ignore_ascii_case("title")
            || prefix.trim().to_lowercase().ends_with("title")
        {
            text = rest.trim().to_string();
        }
    }
    // Strip surrounding quotes/backticks repeatedly ("'foo'" -> foo).
    loop {
        let stripped = text
            .trim()
            .strip_prefix(['"', '\'', '`', '“', '‘'])
            .and_then(|s| s.strip_suffix(['"', '\'', '`', '”', '’']).map(str::trim));
        match stripped {
            Some(inner) if !inner.is_empty() => text = inner.to_string(),
            _ => break,
        }
    }
    // Collapse internal whitespace, drop trailing periods.
    text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    text = text.trim_end_matches('.').trim().to_string();
    if text.is_empty() || text.eq_ignore_ascii_case(UNTITLED_MARKER) {
        return None;
    }
    // Char-boundary truncation, then re-trim trailing punctuation.
    if text.chars().count() > MAX_TITLE_CHARS {
        text = text.chars().take(MAX_TITLE_CHARS).collect();
        text = text
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ' '])
            .to_string();
    }
    let text = text.trim().to_string();
    if text.is_empty() || text.eq_ignore_ascii_case(UNTITLED_MARKER) {
        return None;
    }
    Some(text)
}

/// Truncate to a char boundary.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max).collect()
    }
}

/// Build the bounded user prompt from a turn's in-memory excerpts.
pub(crate) fn build_title_user_prompt(user_excerpt: &str, assistant_excerpt: &str) -> String {
    let user = truncate_chars(user_excerpt.trim(), MAX_EXCERPT_SIDE_CHARS);
    let assistant = truncate_chars(assistant_excerpt.trim(), MAX_EXCERPT_SIDE_CHARS);
    let mut prompt =
        String::from("Generate a concise title for this coding session.\n\nUser request:\n");
    prompt.push_str(&user);
    prompt.push_str("\n\nAssistant outcome:\n");
    prompt.push_str(&assistant);
    prompt.push_str("\n\nTitle:");
    truncate_chars(&prompt, MAX_EXCERPT_CHARS + 256)
}

/// One-shot title generation through the turn's own provider/model.
///
/// Bounded output budget, short timeout, no tools: this is the `title`
/// agent's contract executed inline rather than as a dispatched
/// sub-agent loop (which would need scheduler admission and a result
/// round-trip for a single short string).
pub(crate) async fn generate_title_via_provider(
    provider: &dyn Provider,
    model: &str,
    user_excerpt: &str,
    assistant_excerpt: &str,
    context: ProviderRequestContext,
) -> Result<String, AppError> {
    if model.trim().is_empty() {
        return Err(AppError::Other(anyhow::anyhow!("title model missing")));
    }
    if user_excerpt.trim().is_empty() && assistant_excerpt.trim().is_empty() {
        return Err(AppError::Other(anyhow::anyhow!("title excerpt empty")));
    }
    let request = ChatRequest {
        messages: vec![Message::User {
            content: vec![ContentPart::Text {
                text: build_title_user_prompt(user_excerpt, assistant_excerpt).into(),
            }],
        }],
        model: model.to_string(),
        tools: None,
        system: Some(title_system_prompt().to_string()),
        temperature: Some(TITLE_TEMPERATURE),
        top_p: None,
        max_tokens: Some(TITLE_MAX_TOKENS),
        response_format: None,
        // Reasoning suppression: see `title_request_context`. Both fields
        // stay unset because the wire bridge reads that as "not requested".
        thinking_budget: None,
        reasoning_effort: None,
        context: title_request_context(context),
    };
    let events = tokio::time::timeout(
        std::time::Duration::from_secs(TITLE_TIMEOUT_SECS),
        provider.stream(&request),
    )
    .await
    .map_err(|_| {
        AppError::Other(anyhow::anyhow!(
            "title generation timed out after {TITLE_TIMEOUT_SECS}s"
        ))
    })??;
    let mut text = String::new();
    let mut stream = events;
    // A truncated stream still carries a usable prefix (see module docs):
    // remember the failure, drain no further, and let the sanitizer decide.
    // A stream that produced nothing usable still reports the original error.
    let mut stream_error: Option<AppError> = None;
    // Diagnostics for the "no usable title" path. Only counts and the
    // bounded stop reason are reported — never reasoning content, which is
    // private model output.
    let mut stop_reason: Option<String> = None;
    let mut reasoning_bytes: usize = 0;
    while let Some(event) = stream.next().await {
        match event {
            Ok(crate::provider::ChatEvent::TextDelta(delta)) => text.push_str(&delta),
            // A reasoning model may think before it answers. That is not an
            // error and never becomes title text: count the bytes and keep
            // waiting for a text delta.
            Ok(crate::provider::ChatEvent::ReasoningDelta(delta)) => {
                reasoning_bytes = reasoning_bytes.saturating_add(delta.len());
            }
            Ok(crate::provider::ChatEvent::Finish {
                stop_reason: reason,
                ..
            }) => {
                stop_reason = Some(truncate_chars(reason.trim(), MAX_STOP_REASON_CHARS));
                break;
            }
            Ok(crate::provider::ChatEvent::Error(message)) => {
                stream_error = Some(AppError::Other(anyhow::anyhow!(
                    "title generation failed: {message}"
                )));
                break;
            }
            Ok(_) => {}
            Err(error) => {
                stream_error = Some(AppError::Other(anyhow::anyhow!("{error}")));
                break;
            }
        }
    }
    // The stop reason is the piece the old message lacked: "no usable title
    // from \"\"" is indistinguishable between a length-cut reasoning model
    // and a refusal, and the daemon log is the only place this surfaces.
    let finish = match stop_reason.as_deref() {
        Some(reason) => format!("stop_reason={reason}"),
        None => "stop_reason=<none>".to_string(),
    };
    tracing::debug!(
        model = %model,
        output_chars = text.chars().count(),
        reasoning_bytes,
        %finish,
        "title generation stream finished"
    );
    match sanitize_title(&text) {
        Some(title) => Ok(title),
        None => Err(stream_error.unwrap_or_else(|| {
            // Carry the raw output (bounded) plus the stop reason and the
            // reasoning volume so the next failure names its own cause.
            AppError::Other(anyhow::anyhow!(
                "title generation produced no usable title from {:?} ({finish}, reasoning_bytes={reasoning_bytes})",
                truncate_chars(text.trim(), 200)
            ))
        })),
    }
}

/// `SessionSummaryProvider` backed by a one-shot provider call.
///
/// This makes the existing `SessionStore::generate_title` /
/// `generate_summary` stubs callable; the background auto-title path
/// below uses the in-memory turn excerpt directly instead of
/// re-reading the message table.
pub(crate) struct TitleLlmProvider {
    provider: Box<dyn Provider>,
    model: String,
    context: ProviderRequestContext,
}

impl TitleLlmProvider {
    pub(crate) fn new(
        provider: Box<dyn Provider>,
        model: String,
        context: ProviderRequestContext,
    ) -> Self {
        Self {
            provider,
            model,
            context,
        }
    }
}

#[async_trait::async_trait]
impl crate::session::SessionSummaryProvider for TitleLlmProvider {
    async fn generate_summary(&self, conversation: &str) -> Result<String, crate::error::AppError> {
        let excerpt = truncate_chars(conversation.trim(), 8000);
        if excerpt.is_empty() {
            return Err(AppError::Other(anyhow::anyhow!("summary excerpt empty")));
        }
        let request = ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: format!(
                        "Summarize the following conversation concisely, preserving key facts, decisions, and context. \
                         Format as a single paragraph suitable for insertion into a conversation as context.\n\nConversation:\n{excerpt}\n\nSummary:"
                    )
                    .into(),
                }],
            }],
            model: self.model.clone(),
            tools: None,
            system: Some(
                "You are a concise summarizer. Return only the summary text, no formatting."
                    .to_string(),
            ),
            temperature: Some(0.3),
            top_p: None,
            max_tokens: Some(500),
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: self.context.clone(),
        };
        let events = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            self.provider.stream(&request),
        )
        .await
        .map_err(|_| AppError::Other(anyhow::anyhow!("summary generation timed out")))??;
        let mut summary = String::new();
        let mut stream = events;
        while let Some(event) = stream.next().await {
            match event {
                Ok(crate::provider::ChatEvent::TextDelta(delta)) => summary.push_str(&delta),
                Ok(crate::provider::ChatEvent::Finish { .. }) => break,
                Ok(crate::provider::ChatEvent::Error(message)) => {
                    return Err(AppError::Other(anyhow::anyhow!(
                        "summary generation failed: {message}"
                    )));
                }
                Ok(_) => {}
                Err(error) => return Err(AppError::Other(anyhow::anyhow!("{error}"))),
            }
        }
        let summary = summary.trim().to_string();
        if summary.is_empty() {
            return Err(AppError::Other(anyhow::anyhow!("empty summary response")));
        }
        Ok(summary)
    }

    async fn generate_title(&self, conversation: &str) -> Result<String, crate::error::AppError> {
        let text = generate_title_via_provider(
            self.provider.as_ref(),
            &self.model,
            conversation,
            "",
            self.context.clone(),
        )
        .await?;
        Ok(text)
    }
}

/// Best-effort background auto-title for a just-completed turn.
///
/// Only acts when the session still carries the `"Untitled"` creation
/// default; any explicit title (including a manual rename that raced
/// us) is left untouched. Returns the new title on success.
pub(crate) async fn maybe_auto_title_session(
    pool: sqlx::SqlitePool,
    session_id: String,
    provider: Box<dyn Provider>,
    model: String,
    user_excerpt: String,
    assistant_excerpt: String,
    event_log: Option<Arc<crate::core::event_log::EventLog>>,
) -> Option<String> {
    if session_id.trim().is_empty() || model.trim().is_empty() {
        return None;
    }
    if user_excerpt.trim().is_empty() && assistant_excerpt.trim().is_empty() {
        return None;
    }
    let store = crate::session::SessionStore::new(pool);
    let current = match store.get(&session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return None,
        Err(error) => {
            tracing::warn!(session_id = %session_id, error = %error, "auto-title session load failed");
            return None;
        }
    };
    if current.title != UNTITLED_MARKER {
        return None;
    }
    let context = ProviderRequestContext {
        session_id: Some(Arc::from(session_id.as_str())),
        ..Default::default()
    };
    // Route through the `SessionSummaryProvider` impl so the store's
    // `generate_title` stub and this background path share one prompt,
    // one timeout, and one sanitizer.
    let provider_impl = TitleLlmProvider::new(provider, model, context);
    let excerpt = format!("{}\n{}", user_excerpt.trim(), assistant_excerpt.trim());
    let title = match crate::session::SessionSummaryProvider::generate_title(
        &provider_impl,
        &excerpt,
    )
    .await
    {
        Ok(title) => title,
        Err(error) => {
            tracing::warn!(session_id = %session_id, error = %error, "auto-title generation failed");
            return None;
        }
    };
    if title.eq_ignore_ascii_case(&current.title) {
        return None;
    }
    let updated = store
        .update(
            &session_id,
            crate::session::UpdateSession {
                title: Some(title.clone()),
                share_url: None,
                summary_additions: None,
                summary_deletions: None,
                summary_files: None,
                summary_diffs: None,
                revert: None,
                permission: None,
                tags: None,
                time_compacting: None,
                time_archived: None,
                provider_connection_id: None,
                provider_connection_revision: None,
                model_catalog_revision: None,
                selected_model_id: None,
            },
        )
        .await;
    match updated {
        Ok(_) => {
            tracing::info!(session_id = %session_id, title = %title, "session auto-titled");
            crate::bus::global::GlobalEventBus::publish(
                crate::bus::events::AppEvent::SessionUpdated {
                    id: session_id.clone(),
                },
            );
            if let Some(log) = event_log {
                log.publish(
                    Some(session_id.clone()),
                    None,
                    crate::protocol::core::CoreEvent::SessionUpdated {
                        session_id: session_id.clone(),
                        title: Some(title.clone()),
                    },
                )
                .await;
            }
            Some(title)
        }
        Err(error) => {
            tracing::warn!(session_id = %session_id, error = %error, "auto-title session update failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_title_passes_through() {
        assert_eq!(
            sanitize_title("Fix login redirect loop"),
            Some("Fix login redirect loop".to_string())
        );
    }

    #[test]
    fn strips_quotes_prefix_and_trailing_period() {
        assert_eq!(
            sanitize_title("Title: \"Add retry to flaky sync.\"\nSome chatter"),
            Some("Add retry to flaky sync".to_string())
        );
    }

    #[test]
    fn rejects_empty_untitled_and_structured_output() {
        assert_eq!(sanitize_title(""), None);
        assert_eq!(sanitize_title("   \n  "), None);
        assert_eq!(sanitize_title("Untitled"), None);
        assert_eq!(sanitize_title("  \"untitled\"  "), None);
        assert_eq!(sanitize_title("{\"title\": \"x\"}"), None);
        assert_eq!(sanitize_title("[title]"), None);
    }

    #[test]
    fn truncates_to_char_boundary() {
        let long = "w".repeat(MAX_TITLE_CHARS + 20);
        let title = sanitize_title(&long).expect("long word repeats are usable");
        assert_eq!(title.chars().count(), MAX_TITLE_CHARS);
    }

    #[test]
    fn collapses_whitespace_and_takes_first_line() {
        assert_eq!(
            sanitize_title("  Fix   login\nredirect   loop  "),
            Some("Fix login".to_string())
        );
    }

    #[test]
    fn title_prompt_is_bounded() {
        let prompt = build_title_user_prompt(&"u".repeat(5000), &"a".repeat(5000));
        assert!(prompt.chars().count() <= MAX_EXCERPT_CHARS + 256);
        assert!(prompt.contains("Title:"));
    }

    #[derive(Clone)]
    struct FakeTitleProvider {
        text: String,
    }

    #[async_trait::async_trait]
    impl crate::provider::Provider for FakeTitleProvider {
        fn id(&self) -> &str {
            "fake-title"
        }

        fn name(&self) -> &str {
            "fake-title"
        }

        fn clone_box(&self) -> Box<dyn crate::provider::Provider> {
            Box::new(self.clone())
        }

        async fn stream(
            &self,
            _request: &crate::provider::ChatRequest,
        ) -> Result<crate::provider::EventStream, crate::provider::ProviderError> {
            let events: Vec<Result<crate::provider::ChatEvent, crate::provider::ProviderError>> = vec![
                Ok(crate::provider::ChatEvent::TextDelta(Arc::new(
                    self.text.clone(),
                ))),
                Ok(crate::provider::ChatEvent::Finish {
                    stop_reason: Arc::new("stop".to_string()),
                    usage: Default::default(),
                }),
            ];
            Ok(Box::pin(futures_util::stream::iter(events)))
        }

        async fn models(
            &self,
        ) -> Result<Vec<crate::provider::ModelInfo>, crate::provider::ProviderError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn provider_output_is_sanitized_before_store() {
        let provider = FakeTitleProvider {
            text: "Title: \"Migrate auth to sessions.\"\nExtra chatter".to_string(),
        };
        let title = generate_title_via_provider(
            &provider,
            "fake/model",
            "migrate auth to sessions",
            "done",
            Default::default(),
        )
        .await
        .expect("fake provider yields a title");
        assert_eq!(title, "Migrate auth to sessions");
    }

    #[tokio::test]
    async fn empty_excerpt_is_rejected_without_calling_provider() {
        let provider = FakeTitleProvider {
            text: "Anything".to_string(),
        };
        assert!(
            generate_title_via_provider(&provider, "fake/model", "  ", "", Default::default())
                .await
                .is_err()
        );
    }

    /// A provider stub replaying text deltas followed by an optional
    /// terminal event, including stream failures like the length-cut
    /// `ResponseIncomplete` terminator the wire layer surfaces as an error.
    #[derive(Clone)]
    struct ScriptedTitleProvider {
        texts: Vec<String>,
        finish: bool,
        error: Option<(String, String)>,
    }

    #[async_trait::async_trait]
    impl crate::provider::Provider for ScriptedTitleProvider {
        fn id(&self) -> &str {
            "scripted-title"
        }

        fn name(&self) -> &str {
            "scripted-title"
        }

        fn clone_box(&self) -> Box<dyn crate::provider::Provider> {
            Box::new(self.clone())
        }

        async fn stream(
            &self,
            _request: &crate::provider::ChatRequest,
        ) -> Result<crate::provider::EventStream, crate::provider::ProviderError> {
            let mut events: Vec<
                Result<crate::provider::ChatEvent, crate::provider::ProviderError>,
            > = self
                .texts
                .iter()
                .map(|text| {
                    Ok(crate::provider::ChatEvent::TextDelta(Arc::new(
                        text.clone(),
                    )))
                })
                .collect();
            if self.finish {
                events.push(Ok(crate::provider::ChatEvent::Finish {
                    stop_reason: Arc::new("stop".to_string()),
                    usage: Default::default(),
                }));
            }
            if let Some((code, message)) = self.error.clone() {
                events.push(Err(crate::provider::ProviderError::api(code, message)));
            }
            Ok(Box::pin(futures_util::stream::iter(events)))
        }

        async fn models(
            &self,
        ) -> Result<Vec<crate::provider::ModelInfo>, crate::provider::ProviderError> {
            Ok(Vec::new())
        }
    }

    /// The 32-token cap era ended streams with an incomplete-response
    /// terminator *after* the title text: the session must still be named.
    #[tokio::test]
    async fn truncated_stream_salvages_usable_prefix() {
        let provider = ScriptedTitleProvider {
            texts: vec!["Fix login redirect loop".to_string()],
            finish: false,
            error: Some((
                "provider_stream".to_string(),
                "provider stream tool-call state was invalid: provider stream reported incomplete response".to_string(),
            )),
        };
        let title = generate_title_via_provider(
            &provider,
            "fake/model",
            "fix login",
            "done",
            Default::default(),
        )
        .await
        .expect("usable prefix survives truncation");
        assert_eq!(title, "Fix login redirect loop");
    }

    /// A stream that fails before producing anything usable still fails —
    /// and the original error survives so the daemon log stays diagnosable.
    #[tokio::test]
    async fn stream_error_without_text_still_fails() {
        let provider = ScriptedTitleProvider {
            texts: Vec::new(),
            finish: false,
            error: Some((
                "provider_stream".to_string(),
                "provider stream reported incomplete response".to_string(),
            )),
        };
        let error = generate_title_via_provider(
            &provider,
            "fake/model",
            "fix login",
            "done",
            Default::default(),
        )
        .await
        .expect_err("no text means no salvage");
        assert!(
            error.to_string().contains("incomplete response"),
            "original error preserved, got: {error}"
        );
    }

    /// An empty stream is a sanitizer rejection, and the error now carries
    /// the (empty) raw output for the log.
    #[tokio::test]
    async fn unusable_output_reports_what_the_model_returned() {
        let provider = ScriptedTitleProvider {
            texts: Vec::new(),
            finish: true,
            error: None,
        };
        let error = generate_title_via_provider(
            &provider,
            "fake/model",
            "fix login",
            "done",
            Default::default(),
        )
        .await
        .expect_err("empty output is unusable");
        assert!(
            error.to_string().contains("no usable title"),
            "got: {error}"
        );
        assert!(
            error.to_string().contains("stop_reason=stop"),
            "the finish stop reason must survive into the log line, got: {error}"
        );
    }

    /// A reasoning model opens with `ReasoningDelta` before any text. The
    /// production failure was the opposite stream — reasoning consumed the
    /// output budget and no text delta ever arrived — so this pins the two
    /// halves of the fix: the reasoning prefix is neither an error nor title
    /// text, and the text that follows it still becomes the session title.
    #[derive(Clone)]
    struct ReasoningThenTextProvider {
        reasoning: Vec<String>,
        texts: Vec<String>,
    }

    #[async_trait::async_trait]
    impl crate::provider::Provider for ReasoningThenTextProvider {
        fn id(&self) -> &str {
            "reasoning-title"
        }

        fn name(&self) -> &str {
            "reasoning-title"
        }

        fn clone_box(&self) -> Box<dyn crate::provider::Provider> {
            Box::new(self.clone())
        }

        async fn stream(
            &self,
            _request: &crate::provider::ChatRequest,
        ) -> Result<crate::provider::EventStream, crate::provider::ProviderError> {
            let mut events: Vec<
                Result<crate::provider::ChatEvent, crate::provider::ProviderError>,
            > = self
                .reasoning
                .iter()
                .map(|text| {
                    Ok(crate::provider::ChatEvent::ReasoningDelta(Arc::new(
                        text.clone(),
                    )))
                })
                .chain(self.texts.iter().map(|text| {
                    Ok(crate::provider::ChatEvent::TextDelta(Arc::new(
                        text.clone(),
                    )))
                }))
                .collect();
            events.push(Ok(crate::provider::ChatEvent::Finish {
                stop_reason: Arc::new("stop".to_string()),
                usage: Default::default(),
            }));
            Ok(Box::pin(futures_util::stream::iter(events)))
        }

        async fn models(
            &self,
        ) -> Result<Vec<crate::provider::ModelInfo>, crate::provider::ProviderError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn reasoning_preamble_then_text_still_names_the_session() {
        let provider = ReasoningThenTextProvider {
            reasoning: vec!["First I should decide what this session is about.".to_string()],
            texts: vec!["Fix login redirect loop".to_string()],
        };
        let title = generate_title_via_provider(
            &provider,
            "reasoning/model",
            "fix login",
            "done",
            Default::default(),
        )
        .await
        .expect("a reasoning preamble is not a failure");
        assert_eq!(
            title, "Fix login redirect loop",
            "reasoning text must never be mistaken for the title"
        );
    }

    /// A reasoning-only stream still fails, and the failure now names both
    /// the stop reason and how much reasoning the model spent — the two
    /// facts that make "the model returned an empty string" actionable.
    #[tokio::test]
    async fn reasoning_only_stream_reports_stop_reason_and_volume() {
        let reasoning = "thinking about it".to_string();
        let provider = ReasoningThenTextProvider {
            reasoning: vec![reasoning.clone()],
            texts: Vec::new(),
        };
        let error = generate_title_via_provider(
            &provider,
            "reasoning/model",
            "fix login",
            "done",
            Default::default(),
        )
        .await
        .expect_err("no text delta means no title");
        let message = error.to_string();
        assert!(message.contains("no usable title"), "got: {message}");
        assert!(message.contains("stop_reason=stop"), "got: {message}");
        assert!(
            message.contains(&format!("reasoning_bytes={}", reasoning.len())),
            "got: {message}"
        );
    }

    /// The request must not invite a reasoning model to spend the title's
    /// output budget on thinking: no budget/effort is *requested* (the wire
    /// bridge reads "neither set" as thinking not requested), the adapter
    /// toggle is explicitly off, and the budget is the documented ceiling.
    #[tokio::test]
    async fn title_request_suppresses_reasoning_and_bounds_output() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
        #[derive(Clone)]
        struct RecordingTitleProvider {
            seen: std::sync::Arc<std::sync::Mutex<Option<ChatRequest>>>,
        }
        #[async_trait::async_trait]
        impl crate::provider::Provider for RecordingTitleProvider {
            fn id(&self) -> &str {
                "recording-title"
            }

            fn name(&self) -> &str {
                "recording-title"
            }

            fn clone_box(&self) -> Box<dyn crate::provider::Provider> {
                Box::new(self.clone())
            }

            async fn stream(
                &self,
                request: &crate::provider::ChatRequest,
            ) -> Result<crate::provider::EventStream, crate::provider::ProviderError> {
                *self.seen.lock().expect("recording lock") = Some(request.clone());
                Ok(Box::pin(futures_util::stream::iter(Vec::new())))
            }

            async fn models(
                &self,
            ) -> Result<Vec<crate::provider::ModelInfo>, crate::provider::ProviderError>
            {
                Ok(Vec::new())
            }
        }
        let provider = RecordingTitleProvider {
            seen: std::sync::Arc::clone(&seen),
        };
        // An existing adapter projection must survive: only the thinking
        // toggle is forced off.
        let context = ProviderRequestContext {
            session_id: Some(Arc::from("session-1")),
            wire_policy: Some(Arc::new(crate::provider::ProviderWirePolicy {
                tool_aliases: [("bash".to_string(), "shell".to_string())]
                    .into_iter()
                    .collect(),
                enable_thinking: Some(true),
                ..Default::default()
            })),
        };
        let _ =
            generate_title_via_provider(&provider, "reasoning/model", "fix login", "done", context)
                .await;
        let request = seen
            .lock()
            .expect("recording lock")
            .clone()
            .expect("provider saw the request");
        assert_eq!(request.max_tokens, Some(TITLE_MAX_TOKENS));
        // The old 64-token budget starved the title text entirely on a
        // reasoning model: thinking is billed against it, so no text delta
        // ever arrived. Asserted against the request, not the constant, so
        // this fails if the budget is ever lowered again.
        assert_eq!(
            request.max_tokens,
            Some(TITLE_MAX_TOKENS),
            "the title request must actually carry the budget"
        );
        assert_eq!(
            (request.thinking_budget, request.reasoning_effort),
            (None, None),
            "setting either field *requests* reasoning on the wire"
        );
        let policy = request
            .context
            .wire_policy
            .expect("title call carries the thinking toggle");
        assert_eq!(policy.enable_thinking, Some(false));
        assert_eq!(
            policy.tool_aliases.get("bash").map(String::as_str),
            Some("shell"),
            "the rest of the adapter projection must be preserved"
        );
        assert_eq!(
            request.context.session_id.as_deref(),
            Some("session-1"),
            "the session identity must survive the request projection"
        );
    }

    /// A successful auto-title publishes `SessionUpdated` carrying the new
    /// title, so the TUI can apply it directly with no refetch round trip.
    #[tokio::test]
    async fn auto_title_publishes_session_updated_with_title() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;

        let url = format!(
            "file:title_event_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::from_str(&url)
                    .expect("valid sqlite options")
                    .create_if_missing(true),
            )
            .await
            .expect("connect in-memory sqlite");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");

        let store = crate::session::SessionStore::new(pool.clone());
        let created = store
            .create(crate::session::CreateSession {
                project_id: "legacy-projection".to_string(),
                directory: "/tmp".to_string(),
                title: None,
                ..Default::default()
            })
            .await
            .expect("create untitled session");
        assert_eq!(created.title, "Untitled");

        let log = Arc::new(crate::core::event_log::EventLog::new(16));
        let mut rx = log.subscribe();
        let title = maybe_auto_title_session(
            pool.clone(),
            created.id.clone(),
            Box::new(FakeTitleProvider {
                text: "Fix login redirect loop".to_string(),
            }),
            "fake/model".to_string(),
            "the login redirect is broken".to_string(),
            "fixed the redirect target".to_string(),
            Some(Arc::clone(&log)),
        )
        .await
        .expect("auto-title succeeds");
        assert_eq!(title, "Fix login redirect loop");

        let envelope = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("event arrives")
            .expect("broadcast stays open");
        match envelope.payload {
            crate::protocol::core::CoreEvent::SessionUpdated { session_id, title } => {
                assert_eq!(session_id, created.id);
                assert_eq!(
                    title.as_deref(),
                    Some("Fix login redirect loop"),
                    "the event must carry the title for direct TUI apply"
                );
            }
            other => panic!("expected SessionUpdated, got {other:?}"),
        }

        let stored = store
            .get(&created.id)
            .await
            .expect("reload")
            .expect("session row");
        assert_eq!(stored.title, "Fix login redirect loop");
        assert_eq!(
            stored.slug, "fix-login-redirect-loop",
            "the slug must move with the title"
        );
    }
}
