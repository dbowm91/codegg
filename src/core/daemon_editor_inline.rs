use futures_util::StreamExt;

use codegg_protocol::core::{
    editor_inline_refusal as refusal, CoreResponse, EditorInlineOutcome, ReasoningOverride,
};

use crate::core::daemon::CoreDaemon;
use crate::provider::wire::ReasoningControl;
use crate::provider::{ChatRequest, ContentPart, Message, Provider, ProviderRequestContext};

const MAX_OUTPUT_TOKENS: usize = 1024;
const MAX_OUTPUT_BYTES: usize = 8 * 1024;
const TIMEOUT_SECS: u64 = 60;
const SYSTEM_PROMPT: &str = "You are editing a source file in a code editor. \
The user asks for a short piece of text to insert at the cursor. \
Reply with that text and nothing else: no explanation, no Markdown fences, \
no repetition of the surrounding context.";

pub(crate) struct InlineGenerateRequest {
    pub request_id: String,
    pub project_id: String,
    pub document_id: String,
    pub document_revision: u64,
    pub anchor: u64,
    pub context_before: u32,
    pub context_after: u32,
    pub instruction: String,
    pub model: Option<String>,
    pub reasoning: Option<ReasoningOverride>,
}

impl CoreDaemon {
    pub(crate) async fn editor_inline_generate(
        &self,
        request: InlineGenerateRequest,
        client_id: &str,
    ) -> CoreResponse {
        let request_id = request.request_id.clone();
        let document_id = request.document_id.clone();
        let document_revision = request.document_revision;
        let refuse = |code: &str| CoreResponse::EditorInlineGenerated {
            request_id: request_id.clone(),
            document_id: document_id.clone(),
            document_revision,
            outcome: EditorInlineOutcome::Refused {
                code: code.to_owned(),
            },
        };

        let Some(instruction) = accepted_instruction(&request.instruction) else {
            return refuse(refusal::INSTRUCTION_TOO_LARGE);
        };
        if context_request_is_too_large(request.context_before, request.context_after) {
            return refuse(refusal::CONTEXT_TOO_LARGE);
        }
        let budget = codegg_protocol::core::MAX_EDITOR_INLINE_CONTEXT_BYTES as usize;

        let snapshot = match self
            .documents
            .snapshot(&request.document_id, &request.project_id, client_id)
            .await
        {
            Ok(snapshot) => snapshot,
            Err(_) => return refuse(refusal::DOCUMENT_UNAVAILABLE),
        };
        if snapshot.revision != request.document_revision {
            return refuse(refusal::DOCUMENT_MISMATCH);
        }
        let text = snapshot.text.as_str();
        let Some(anchor) = usize::try_from(request.anchor)
            .ok()
            .filter(|a| *a <= text.len())
        else {
            return refuse(refusal::DOCUMENT_MISMATCH);
        };
        if !text.is_char_boundary(anchor) {
            return refuse(refusal::DOCUMENT_MISMATCH);
        }
        let start =
            floor_char_boundary(text, anchor.saturating_sub(request.context_before as usize));
        let end = ceil_char_boundary(
            text,
            anchor
                .saturating_add(request.context_after as usize)
                .min(text.len()),
        );
        let context = &text[start..end];
        if context.len() > budget {
            return refuse(refusal::CONTEXT_TOO_LARGE);
        }
        let cursor = anchor - start;
        let (before, after) = context.split_at(cursor);

        let config = super::load_config_or_default();
        let requested_model = request
            .model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty());
        let preferred_model = self.preferred_inline_model(client_id).await;
        let configured_model = config
            .model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty());
        let Some(model) = resolve_inline_model(requested_model, preferred_model, configured_model)
        else {
            return refuse(refusal::MODEL_UNAVAILABLE);
        };
        let provider_name =
            crate::core::session_selection::runtime_model_provider_segment(&model).to_owned();
        if provider_name.is_empty() {
            return refuse(refusal::MODEL_UNAVAILABLE);
        }

        let mut registry = crate::provider::ProviderRegistry::new();
        let locators = match self.pool.clone() {
            Some(pool) => {
                crate::core::session_selection::pool_connection_credential_locators(&pool).await
            }
            None => Vec::new(),
        };
        crate::provider::register_builtin_with_config_connections(
            &mut registry,
            &config,
            &locators,
            None,
        );
        let provider = match registry.get(&provider_name) {
            Some(base) => base.clone_box(),
            None => return refuse(refusal::MODEL_UNAVAILABLE),
        };

        let reasoning =
            match self.admit_reasoning_capability(&config, &model, request.reasoning.as_ref()) {
                Ok(admission) => admission.selection,
                Err(_) => return refuse(refusal::REASONING_NOT_ADMITTED),
            };

        let model_name = match model.split_once('/') {
            Some((_, id)) => id.to_owned(),
            None => model.clone(),
        };
        let provider_context = inline_provider_context(
            self.clients.attached_session_for(client_id),
            &request.document_id,
        );
        let mut chat_request = ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: build_user_prompt(instruction, before, after).into(),
                }],
            }],
            model: model_name,
            tools: None,
            system: Some(SYSTEM_PROMPT.to_owned()),
            temperature: Some(0.2),
            top_p: None,
            max_tokens: Some(MAX_OUTPUT_TOKENS),
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: provider_context,
        };
        let effort = apply_reasoning_choice(&mut chat_request, reasoning);

        let collected = tokio::time::timeout(std::time::Duration::from_secs(TIMEOUT_SECS), async {
            let mut stream = match provider.stream(&chat_request).await {
                Ok(stream) => stream,
                Err(error) => {
                    tracing::warn!(
                        provider_id = %provider.id(),
                        model_id = %chat_request.model,
                        error_class = error.error_class(),
                        error = %error,
                        "inline editor provider stream failed"
                    );
                    return Err(refusal::PROVIDER_ERROR);
                }
            };
            let mut generated = String::new();
            while let Some(event) = stream.next().await {
                match event {
                    Ok(crate::provider::ChatEvent::TextDelta(delta)) => {
                        if generated.len().saturating_add(delta.len()) > MAX_OUTPUT_BYTES {
                            let room = MAX_OUTPUT_BYTES.saturating_sub(generated.len());
                            let cut = floor_char_boundary(&delta, room);
                            generated.push_str(&delta[..cut]);
                            break;
                        }
                        generated.push_str(&delta);
                    }
                    Ok(crate::provider::ChatEvent::Finish { .. }) => break,
                    Ok(crate::provider::ChatEvent::Error(error)) => {
                        tracing::warn!(
                            provider_id = %provider.id(),
                            model_id = %chat_request.model,
                            error_class = "provider_stream_event",
                            error = %error,
                            "inline editor provider stream emitted an error"
                        );
                        return Err(refusal::PROVIDER_ERROR);
                    }
                    Err(error) => {
                        tracing::warn!(
                            provider_id = %provider.id(),
                            model_id = %chat_request.model,
                            error_class = error.error_class(),
                            error = %error,
                            "inline editor provider stream failed"
                        );
                        return Err(refusal::PROVIDER_ERROR);
                    }
                    _ => {}
                }
            }
            Ok(generated)
        })
        .await;
        let generated = match collected {
            Ok(Ok(generated)) => generated,
            Ok(Err(code)) => return refuse(code),
            Err(_) => return refuse(refusal::TIMEOUT),
        };

        let generated = generated.trim().to_owned();
        if generated.is_empty() {
            return refuse(refusal::EMPTY_GENERATION);
        }
        CoreResponse::EditorInlineGenerated {
            request_id,
            document_id,
            document_revision,
            outcome: EditorInlineOutcome::Generated {
                model,
                effort,
                text: generated,
            },
        }
    }

    async fn preferred_inline_model(&self, client_id: &str) -> Option<String> {
        let principal = self.clients.principal_for(client_id)?;
        let pool = self.pool.clone()?;
        let preference_store = codegg_core::approval::RuntimePreferenceStore::new(pool.clone());
        let preference = preference_store
            .get(principal.principal_id().as_str())
            .await
            .ok()??;
        if !preference.has_model_preference() {
            return None;
        }
        let model_id = preference.last_model_id.clone()?;
        let connection_id = preference.last_provider_connection_id.clone()?;
        let connection_id =
            codegg_core::identity::ProviderConnectionId::parse(&connection_id).ok()?;
        let connection_store =
            codegg_core::provider_connections::ProviderConnectionStore::new(pool);
        let connection = connection_store.get(&connection_id).await.ok()??;
        if connection.state != codegg_core::provider_connections::ProviderConnectionState::Active {
            return None;
        }
        Some(format!(
            "{}/{}",
            connection.provider_kind.as_str(),
            model_id
        ))
    }
}

/// The editor's provider context: the conversation the connection is attached
/// to, or a stable bounded id derived from the document when there is none.
fn inline_provider_context(
    attached_session: Option<String>,
    document_id: &str,
) -> ProviderRequestContext {
    let session_id = attached_session.unwrap_or_else(|| {
        use sha2::Digest;
        format!("editor-{:x}", sha2::Sha256::digest(document_id.as_bytes()))
    });

    ProviderRequestContext {
        session_id: Some(session_id.into()),
        ..Default::default()
    }
}

fn resolve_inline_model(
    requested: Option<&str>,
    preferred: Option<String>,
    configured: Option<&str>,
) -> Option<String> {
    requested
        .filter(|model| !model.trim().is_empty())
        .map(str::to_owned)
        .or(preferred)
        .or_else(|| {
            configured
                .filter(|model| !model.trim().is_empty())
                .map(str::to_owned)
        })
}

fn accepted_instruction(instruction: &str) -> Option<&str> {
    let trimmed = instruction.trim();
    if trimmed.is_empty()
        || instruction.len() > codegg_protocol::core::MAX_EDITOR_INLINE_INSTRUCTION_BYTES
    {
        return None;
    }
    Some(trimmed)
}

fn context_request_is_too_large(context_before: u32, context_after: u32) -> bool {
    context_before as usize + context_after as usize
        > codegg_protocol::core::MAX_EDITOR_INLINE_CONTEXT_BYTES as usize
}

fn build_user_prompt(instruction: &str, before: &str, after: &str) -> String {
    let mut prompt = String::with_capacity(instruction.len() + before.len() + after.len() + 128);
    prompt.push_str("Instruction:\n");
    prompt.push_str(instruction);
    prompt.push_str("\n\nText before the cursor:\n");
    prompt.push_str(before);
    prompt.push_str("\n\nText after the cursor:\n");
    prompt.push_str(after);
    prompt.push_str("\n\nReply with only the text to insert at the cursor.");
    prompt
}

fn apply_reasoning_choice(
    request: &mut ChatRequest,
    chosen: Option<ReasoningOverride>,
) -> Option<String> {
    match chosen? {
        ReasoningOverride::Effort { level } => {
            request.thinking_budget = None;
            request.reasoning_effort = Some(level.clone());
            request.context.reasoning = ReasoningControl::Unspecified;
            Some(level)
        }
        ReasoningOverride::Budget { tokens } => {
            request.reasoning_effort = None;
            request.thinking_budget = Some(tokens);
            request.context.reasoning = ReasoningControl::Unspecified;
            None
        }
        ReasoningOverride::Off => {
            request.reasoning_effort = None;
            request.thinking_budget = None;
            request.context.reasoning = ReasoningControl::Disabled;
            None
        }
    }
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_or_oversized_instruction_is_refused_before_any_provider_call() {
        assert_eq!(
            accepted_instruction("   "),
            None,
            "a blank instruction must never reach the provider"
        );
        let oversized = "x".repeat(codegg_protocol::core::MAX_EDITOR_INLINE_INSTRUCTION_BYTES + 1);
        assert_eq!(
            accepted_instruction(&oversized),
            None,
            "an instruction over the byte budget must be refused"
        );
    }

    #[test]
    fn an_instruction_inside_the_budget_is_accepted_trimmed() {
        assert_eq!(
            accepted_instruction("  add a doc comment  "),
            Some("add a doc comment"),
            "the accepted instruction is the trimmed text"
        );
        let exact = "x".repeat(codegg_protocol::core::MAX_EDITOR_INLINE_INSTRUCTION_BYTES);
        assert_eq!(
            accepted_instruction(&exact).map(str::len),
            Some(codegg_protocol::core::MAX_EDITOR_INLINE_INSTRUCTION_BYTES),
            "the budget is inclusive"
        );
    }

    #[test]
    fn the_model_precedence_is_request_then_preference_then_configured() {
        assert_eq!(
            resolve_inline_model(
                Some("openai/gpt-4o"),
                Some("anthropic/claude".to_owned()),
                Some("google/gemini"),
            ),
            Some("openai/gpt-4o".to_owned()),
            "an explicit request override outranks every other tier"
        );
        assert_eq!(
            resolve_inline_model(
                None,
                Some("anthropic/claude".to_owned()),
                Some("google/gemini"),
            ),
            Some("anthropic/claude".to_owned()),
            "the persisted principal preference outranks the configured default"
        );
        assert_eq!(
            resolve_inline_model(None, None, Some("google/gemini")),
            Some("google/gemini".to_owned()),
            "the configured default is the last tier that names a model"
        );
        assert_eq!(
            resolve_inline_model(None, None, None),
            None,
            "no tier names a model, so the request is refused rather than guessed"
        );
        assert_eq!(
            resolve_inline_model(Some("   "), None, Some("google/gemini")),
            Some("google/gemini".to_owned()),
            "a blank request model is not an override"
        );
    }

    #[test]
    fn a_context_window_over_the_budget_is_refused() {
        assert!(
            context_request_is_too_large(8_193, 8_192),
            "the two sides share one budget"
        );
        assert!(
            !context_request_is_too_large(8_192, 8_192),
            "a request exactly at the budget is not refused"
        );
        assert!(
            !context_request_is_too_large(2_048, 2_048),
            "a request inside the budget is not refused"
        );
    }

    #[test]
    fn the_prompt_carries_the_instruction_and_both_sides_of_the_cursor() {
        let prompt = build_user_prompt("add a doc comment", "let a = 1;", "let b = 2;");
        let instruction_at = prompt
            .find("add a doc comment")
            .expect("instruction present");
        let before_at = prompt.find("let a = 1;").expect("before present");
        let after_at = prompt.find("let b = 2;").expect("after present");
        assert!(
            instruction_at < before_at && before_at < after_at,
            "the prompt orders the instruction, the before text, then the after text: {prompt}"
        );
    }

    #[test]
    fn an_effort_choice_is_carried_verbatim_and_disables_a_budget() {
        let mut request = ChatRequest {
            messages: Vec::new(),
            model: "fake/model".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: Some(512),
            reasoning_effort: None,
            context: ProviderRequestContext::default(),
        };
        let effort = apply_reasoning_choice(
            &mut request,
            Some(ReasoningOverride::Effort {
                level: "xhigh".to_string(),
            }),
        );
        assert_eq!(effort, Some("xhigh".to_string()));
        assert_eq!(request.reasoning_effort, Some("xhigh".to_string()));
        assert_eq!(request.thinking_budget, None);
        assert_eq!(request.context.reasoning, ReasoningControl::Unspecified);
    }

    #[test]
    fn a_budget_choice_clears_the_effort_and_an_off_choice_marks_an_explicit_disable() {
        let mut request = ChatRequest {
            messages: Vec::new(),
            model: "fake/model".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: Some("high".to_string()),
            context: ProviderRequestContext::default(),
        };
        assert_eq!(
            apply_reasoning_choice(
                &mut request,
                Some(ReasoningOverride::Budget { tokens: 256 })
            ),
            None
        );
        assert_eq!(request.thinking_budget, Some(256));
        assert_eq!(request.reasoning_effort, None);
        assert_eq!(
            apply_reasoning_choice(&mut request, Some(ReasoningOverride::Off)),
            None
        );
        assert_eq!(request.thinking_budget, None);
        assert_eq!(request.reasoning_effort, None);
        assert_eq!(
            request.context.reasoning,
            ReasoningControl::Disabled,
            "off must reach the wire as an explicit disable"
        );
    }

    #[test]
    fn editor_provider_context_prefers_the_attached_session() {
        let context = inline_provider_context(Some("attached-session".to_owned()), "doc-a");

        assert_eq!(
            context.session_id.as_deref(),
            Some("attached-session"),
            "the connection's conversation takes precedence over the document fallback"
        );
    }

    #[test]
    fn editor_provider_context_fallback_is_stable_bounded_and_document_specific() {
        let first = inline_provider_context(None, "doc-a")
            .session_id
            .expect("fallback session id");
        let same_document = inline_provider_context(None, "doc-a")
            .session_id
            .expect("repeated fallback session id");
        let other_document = inline_provider_context(None, "doc-b")
            .session_id
            .expect("other document fallback session id");

        assert_eq!(
            first, same_document,
            "one document maps to one stable session id"
        );
        assert_ne!(
            first, other_document,
            "different documents map to different session ids"
        );
        assert!(
            first.len() <= codegg_core::context::MAX_SESSION_ID_LENGTH,
            "the projected session id stays within the session limit"
        );
        assert!(
            first
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-'),
            "the projected session id uses header-safe ASCII characters"
        );
    }

    #[test]
    fn a_boundary_snapped_to_a_multibyte_character_keeps_the_text_valid() {
        let text = "aé";
        assert_eq!(floor_char_boundary(text, 2), 1, "2 is inside the é");
        assert_eq!(
            ceil_char_boundary(text, 2),
            3,
            "the window ends after the é"
        );
        assert_eq!(
            &text[floor_char_boundary(text, 2)..ceil_char_boundary(text, 2)],
            "é"
        );
    }
}
