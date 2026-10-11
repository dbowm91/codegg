//! Title-agent session naming.
//!
//! The `title` built-in agent (`assets/agents/title.toml`,
//! `runtime_kind = "title"`) exists so turns can produce a concise
//! human-readable session title, but nothing ever invoked it: new
//! sessions default to `"Untitled"` (`SessionStore::create`) and only a
//! manual `SessionRename` changed the title afterwards.
//!
//! This module wires the agent's contract to a lightweight one-shot
//! provider call (no tools, no sub-agent dispatch, no scheduler work).
//! After a turn completes, the turn runtime fires a best-effort
//! background task that generates a title from the turn's in-memory
//! user request + assistant outcome and persists it when the session
//! is still `"Untitled"`. Failures are logged and never fail the turn.
//!
//! Live turns carry their conversation in memory (the TUI builds the
//! provider context from `messages_state`; `MessageStore` only sees
//! imports), so the excerpt comes from the completed turn rather than
//! re-reading the message table.

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
const TITLE_MAX_TOKENS: usize = 32;
const TITLE_TEMPERATURE: f64 = 0.7;

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
/// Short timeout, tiny `max_tokens`, no tools: this is the `title`
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
        thinking_budget: None,
        reasoning_effort: None,
        context,
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
    while let Some(event) = stream.next().await {
        match event {
            Ok(crate::provider::ChatEvent::TextDelta(delta)) => text.push_str(&delta),
            Ok(crate::provider::ChatEvent::Finish { .. }) => break,
            Ok(crate::provider::ChatEvent::Error(message)) => {
                return Err(AppError::Other(anyhow::anyhow!(
                    "title generation failed: {message}"
                )));
            }
            Ok(_) => {}
            Err(error) => return Err(AppError::Other(anyhow::anyhow!("{error}"))),
        }
    }
    sanitize_title(&text).ok_or_else(|| {
        AppError::Other(anyhow::anyhow!("title generation produced no usable title"))
    })
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
}
