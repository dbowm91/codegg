//! LLM caller helper for model-backed research phases.
//!
//! Provides a simple interface to call an LLM provider and collect the
//! complete text response, used by evidence extraction and claim construction.

use std::sync::Arc;

use futures::StreamExt;

use crate::provider::{ChatEvent, ChatRequest, ContentPart, Message, Provider};

use crate::research::error::{ResearchError, Result};

/// Call an LLM and collect the full text response.
///
/// Sends a single user message with an optional system prompt and returns
/// the concatenated text deltas. Returns an error on timeout (120s) or
/// empty response.
pub async fn call_llm(
    provider: &dyn Provider,
    model: &str,
    system: Option<&str>,
    user_message: &str,
    max_tokens: Option<usize>,
) -> Result<String> {
    let mut messages = Vec::new();

    if let Some(sys) = system {
        messages.push(Message::System {
            content: Arc::new(sys.to_string()),
        });
    }

    messages.push(Message::User {
        content: vec![ContentPart::Text {
            text: Arc::new(user_message.to_string()),
        }],
    });

    let request = ChatRequest {
        messages,
        model: model.to_string(),
        tools: None,
        system: None,
        temperature: Some(0.3),
        top_p: None,
        max_tokens,
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
    };

    let events = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        provider.stream(&request),
    )
    .await
    .map_err(|_| ResearchError::Provider("LLM call timed out after 120s".to_string()))?
    .map_err(|e| ResearchError::Provider(format!("LLM stream error: {e}")))?;

    let mut stream = events;
    let mut text = String::new();

    while let Some(event) = stream.next().await {
        match event {
            Ok(ChatEvent::TextDelta(delta)) => text.push_str(&delta),
            Ok(ChatEvent::Finish { .. }) => break,
            Ok(ChatEvent::Error(e)) => {
                return Err(ResearchError::Provider(format!("LLM error: {e}")));
            }
            _ => {}
        }
    }

    if text.is_empty() {
        return Err(ResearchError::Provider("Empty LLM response".to_string()));
    }

    Ok(text)
}

/// Call an LLM expecting a JSON response. Strips markdown code fences if present.
pub async fn call_llm_json(
    provider: &dyn Provider,
    model: &str,
    system: Option<&str>,
    user_message: &str,
    max_tokens: Option<usize>,
) -> Result<serde_json::Value> {
    let text = call_llm(provider, model, system, user_message, max_tokens).await?;
    let trimmed = text.trim();

    // Strip markdown code fences if present
    let json_str = if trimmed.starts_with("```json") {
        trimmed
            .strip_prefix("```json")
            .unwrap_or(trimmed)
            .strip_suffix("```")
            .unwrap_or(trimmed)
            .trim()
    } else if trimmed.starts_with("```") {
        trimmed
            .strip_prefix("```")
            .unwrap_or(trimmed)
            .strip_suffix("```")
            .unwrap_or(trimmed)
            .trim()
    } else {
        trimmed
    };

    serde_json::from_str(json_str).map_err(|e| {
        ResearchError::Provider(format!(
            "Failed to parse LLM JSON response: {e}\nResponse start: {}",
            &json_str[..json_str.len().min(200)]
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_json_fences_basic() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        let trimmed = input.trim();
        let json_str = if trimmed.starts_with("```json") {
            trimmed
                .strip_prefix("```json")
                .unwrap_or(trimmed)
                .strip_suffix("```")
                .unwrap_or(trimmed)
                .trim()
        } else {
            trimmed
        };
        let v: serde_json::Value = serde_json::from_str(json_str).unwrap();
        assert_eq!(v["key"], "value");
    }

    #[test]
    fn strip_json_fences_plain() {
        let input = "{\"key\": \"value\"}";
        let trimmed = input.trim();
        let v: serde_json::Value = serde_json::from_str(trimmed).unwrap();
        assert_eq!(v["key"], "value");
    }
}
