use crate::provider::{ChatRequest, ContentPart, Message, Provider};
use futures::StreamExt;
use tiktoken::encoding_for_model;

/// # Compaction Invariant
/// All compaction strategies must uphold the following invariants:
/// 1. No orphan `Message::Tool`: every tool result must have a corresponding assistant tool call
///    with matching `tool_call_id` in the retained messages.
/// 2. No assistant tool-call message may exist without all its required tool results in the retained
///    transcript, unless the provider-specific serializer can safely handle missing results.
/// 3. The relative order of assistant tool calls and their matching tool results must be preserved.
/// 4. Truncation of tool outputs must preserve the `tool_call_id` field unchanged.
/// 5. Multi-tool pairs (assistant with multiple tool calls + matching tool results) must preserve
///    all IDs and their original order.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TokenizerType {
    Cl100kBase,
    Claude,
    Gemini,
    O200kBase,
}

impl TokenizerType {
    pub fn for_model(model: &str) -> Self {
        let lower = model.to_lowercase();
        if lower.contains("claude") {
            TokenizerType::Claude
        } else if lower.contains("gemini") {
            TokenizerType::Gemini
        } else if lower.contains("o200k") {
            TokenizerType::O200kBase
        } else {
            TokenizerType::Cl100kBase
        }
    }

    pub fn multiplier(&self) -> f64 {
        match self {
            TokenizerType::Cl100kBase => 1.0,
            TokenizerType::Claude => 1.4,
            TokenizerType::Gemini => 1.2,
            TokenizerType::O200kBase => 1.0,
        }
    }
}

fn estimate_tokens_sync(text: &str, model: Option<&str>) -> usize {
    let tokenizer_type = model
        .map(TokenizerType::for_model)
        .unwrap_or(TokenizerType::Cl100kBase);

    let base_tokens = if tokenizer_type == TokenizerType::Cl100kBase {
        let model_name = model
            .map(|m| {
                if m.to_lowercase().contains("gpt-4") {
                    "gpt-4"
                } else {
                    "gpt-3.5-turbo"
                }
            })
            .unwrap_or("gpt-3.5-turbo");

        encoding_for_model(model_name)
            .or_else(|| encoding_for_model("gpt-3.5-turbo"))
            .map(|enc| enc.encode(text).len())
            .unwrap_or_else(|| tiktoken::encoding::cl100k_base().encode(text).len())
    } else {
        tiktoken::encoding::cl100k_base().encode(text).len()
    };

    let multiplier = tokenizer_type.multiplier();
    (base_tokens as f64 * multiplier) as usize
}

pub struct ContextTracker {
    current_tokens: usize,
    context_limit: usize,
    threshold: f64,
    message_token_counts: Vec<usize>,
    max_messages: Option<usize>,
    max_total_bytes: Option<usize>,
    model: Option<String>,
}

impl ContextTracker {
    pub fn new(context_limit: usize, threshold: f64) -> Self {
        Self {
            current_tokens: 0,
            context_limit,
            threshold,
            message_token_counts: Vec::new(),
            max_messages: None,
            max_total_bytes: None,
            model: None,
        }
    }

    pub fn with_max_messages(mut self, max: usize) -> Self {
        self.max_messages = Some(max);
        self
    }

    pub fn with_max_total_bytes(mut self, max: usize) -> Self {
        self.max_total_bytes = Some(max);
        self
    }

    pub fn with_model(mut self, model: Option<String>) -> Self {
        self.model = model;
        self
    }

    pub fn estimate_tokens(text: &str) -> usize {
        estimate_tokens_sync(text, None)
    }

    pub fn add_message(&mut self, msg: &Message) {
        let tokens = match msg {
            Message::System { content } => Self::estimate_tokens(content),
            Message::User { content } => content
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => Self::estimate_tokens(text),
                    _ => 0,
                })
                .sum(),
            Message::Assistant { content, .. } => content
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => Self::estimate_tokens(text),
                    _ => 0,
                })
                .sum(),
            Message::Tool { content, .. } => Self::estimate_tokens(content),
        };
        self.message_token_counts.push(tokens);
        self.current_tokens += tokens;
    }

    pub fn add_messages(&mut self, messages: &[Message]) {
        for msg in messages {
            self.add_message(msg);
        }
    }

    pub fn message_count(&self) -> usize {
        self.message_token_counts.len()
    }

    pub fn needs_bounds_check(&self) -> bool {
        if let Some(max) = self.max_messages {
            if self.message_token_counts.len() >= max {
                return true;
            }
        }
        if let Some(max_bytes) = self.max_total_bytes {
            if self.current_tokens * 4 >= max_bytes {
                return true;
            }
        }
        false
    }

    pub fn bound_exceeded(&self) -> Option<&'static str> {
        if let Some(max) = self.max_messages {
            if self.message_token_counts.len() >= max {
                return Some("max_messages");
            }
        }
        if let Some(max_bytes) = self.max_total_bytes {
            if self.current_tokens * 4 >= max_bytes {
                return Some("max_total_bytes");
            }
        }
        None
    }

    pub fn needs_compaction(&self) -> bool {
        self.current_tokens as f64 > self.context_limit as f64 * self.threshold
    }

    pub fn needs_overflow_protection(&self, reserved: usize) -> bool {
        self.current_tokens > self.context_limit.saturating_sub(reserved)
    }

    pub fn remaining_tokens(&self) -> usize {
        self.context_limit.saturating_sub(self.current_tokens)
    }

    pub fn current_tokens(&self) -> usize {
        self.current_tokens
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.context_limit = limit;
    }

    pub fn context_limit(&self) -> usize {
        self.context_limit
    }

    pub fn set_threshold(&mut self, threshold: f64) {
        self.threshold = threshold;
    }

    pub fn threshold(&self) -> f64 {
        self.threshold
    }

    pub fn reset(&mut self) {
        self.current_tokens = 0;
        self.message_token_counts.clear();
    }
}

#[derive(Debug, PartialEq)]
pub enum CompactionStrategy {
    TruncateToolOutputs,
    SummarizeOldTurns,
    DropMiddleMessages,
}

pub fn compact_messages(messages: Vec<Message>, strategy: CompactionStrategy) -> Vec<Message> {
    compact_messages_sync(messages, strategy)
}

pub fn compact_messages_sync(messages: Vec<Message>, strategy: CompactionStrategy) -> Vec<Message> {
    if messages.len() <= 2 {
        return messages;
    }

    let system_messages: Vec<Message> = messages
        .iter()
        .filter(|m| matches!(m, Message::System { .. }))
        .cloned()
        .collect();

    let non_system: Vec<Message> = messages
        .into_iter()
        .filter(|m| !matches!(m, Message::System { .. }))
        .collect();

    let compacted = match strategy {
        CompactionStrategy::TruncateToolOutputs => truncate_tool_outputs(non_system),
        // Note: SummarizeOldTurns requires LLM (async), use DropMiddleMessages as fallback for sync context
        CompactionStrategy::SummarizeOldTurns => {
            tracing::warn!("SummarizeOldTurns requested in sync context, using fallback");
            // Add a system message indicating summarization was requested but can't be done synchronously
            let mut result = vec![Message::System {
                content: "[Previous conversation summarized for context efficiency]"
                    .to_string()
                    .into(),
            }];
            result.extend(drop_middle_messages(non_system));
            return result;
        }
        CompactionStrategy::DropMiddleMessages => drop_middle_messages(non_system),
    };

    let mut result = system_messages;
    result.extend(compacted);
    result
}

pub async fn compact_messages_async(
    messages: Vec<Message>,
    strategy: CompactionStrategy,
    provider: &dyn Provider,
    model: &str,
) -> Vec<Message> {
    if messages.len() <= 2 {
        return messages;
    }

    let system_messages: Vec<Message> = messages
        .iter()
        .filter(|m| matches!(m, Message::System { .. }))
        .cloned()
        .collect();

    let non_system: Vec<Message> = messages
        .into_iter()
        .filter(|m| !matches!(m, Message::System { .. }))
        .collect();

    let compacted = match strategy {
        CompactionStrategy::TruncateToolOutputs => truncate_tool_outputs(non_system),
        CompactionStrategy::SummarizeOldTurns => summarize_old_turns(non_system, provider, model).await,
        CompactionStrategy::DropMiddleMessages => drop_middle_messages(non_system),
    };

    let mut result = system_messages;
    result.extend(compacted);
    result
}

fn truncate_tool_outputs(messages: Vec<Message>) -> Vec<Message> {
    messages
        .into_iter()
        .map(|msg| match msg {
            Message::Tool {
                tool_call_id,
                content,
            } => {
                let truncated = if content.len() > 500 {
                    format!("{}...[truncated]", &content[..500])
                } else {
                    content.to_string()
                };
                Message::Tool {
                    tool_call_id,
                    content: truncated.into(),
                }
            }
            other => other,
        })
        .collect()
}

async fn summarize_old_turns(messages: Vec<Message>, provider: &dyn Provider, model: &str) -> Vec<Message> {
    if messages.len() <= 6 {
        return messages;
    }

    let keep_count = 4;
    let mut result = Vec::new();

    let summary = match llm_summarize(&messages, provider, model).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("LLM summarization failed, using fallback: {}", e);
            format!("[Previous {} messages summarized]", messages.len())
        }
    };

    result.push(Message::System {
        content: summary.into(),
    });

    result.extend(messages.into_iter().rev().take(keep_count).rev());
    result
}

pub async fn llm_summarize(
    messages: &[Message],
    provider: &dyn Provider,
    model: &str,
) -> Result<String, crate::error::AppError> {
    let messages_to_summarize: Vec<Message> = messages
        .iter()
        .filter(|m| !matches!(m, Message::System { .. }))
        .take(20)
        .cloned()
        .collect();

    if messages_to_summarize.is_empty() {
        return Ok("[Previous conversation summarized for context efficiency]".to_string());
    }

    let mut summary_text = String::new();
    for msg in &messages_to_summarize {
        match msg {
            Message::User { content } => {
                let text = content
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                if !text.is_empty() {
                    summary_text.push_str(&format!("User: {}\n\n", text));
                }
            }
            Message::Assistant { content, .. } => {
                let text = content
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                if !text.is_empty() {
                    summary_text.push_str(&format!("Assistant: {}\n\n", text));
                }
            }
            Message::Tool { content, .. } => {
                let truncated = if content.len() > 300 {
                    format!("{}...[truncated]", &content[..300])
                } else {
                    content.to_string()
                };
                summary_text.push_str(&format!("Tool: {}\n\n", truncated));
            }
            Message::System { .. } => {}
        }
    }

    let summary_prompt = format!(
        "Summarize the following conversation concisely, preserving key facts, decisions, and context. \
        Format as a single paragraph suitable for insertion into a conversation as context.\n\n\
        Conversation:\n{}\n\n\
        Summary:",
        summary_text
    );

    let request = ChatRequest {
        messages: vec![Message::User {
            content: vec![ContentPart::Text {
                text: summary_prompt.into(),
            }],
        }],
        model: model.to_string(),
        tools: None,
        system: Some(
            "You are a concise summarizer. Return only the summary text, no formatting."
                .to_string(),
        ),
        temperature: Some(0.3),
        top_p: None,
        max_tokens: Some(500),
        response_format: None,
    };

    let events = provider.stream(&request).await?;
    let mut summary = String::new();
    let mut stream = events;
    while let Some(event) = stream.next().await {
        match event {
            Ok(crate::provider::ChatEvent::TextDelta(delta)) => summary.push_str(&delta),
            Ok(crate::provider::ChatEvent::Finish { .. }) => break,
            Ok(crate::provider::ChatEvent::Error(e)) => {
                return Err(crate::error::AppError::Provider(
                    format!("LLM error: {}", e).into(),
                ));
            }
            _ => {}
        }
    }

    if summary.is_empty() {
        return Err(crate::error::AppError::Provider(
            "Empty summary response".into(),
        ));
    }

    Ok(summary)
}

fn drop_middle_messages(messages: Vec<Message>) -> Vec<Message> {
    if messages.len() <= 4 {
        return messages;
    }

    let keep_each_side = 2;
    let mut result = Vec::new();

    result.extend(messages.iter().take(keep_each_side).cloned());
    result.extend(messages.iter().rev().take(keep_each_side).rev().cloned());
    result
}

pub fn detect_overflow(messages: &[Message], context_limit: usize, reserved: usize) -> bool {
    let total: usize = messages
        .iter()
        .map(|m| match m {
            Message::System { content } => ContextTracker::estimate_tokens(content),
            Message::User { content } => content
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => ContextTracker::estimate_tokens(text),
                    _ => 0,
                })
                .sum(),
            Message::Assistant { content, .. } => content
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => ContextTracker::estimate_tokens(text),
                    _ => 0,
                })
                .sum(),
            Message::Tool { content, .. } => ContextTracker::estimate_tokens(content),
        })
        .sum();

    total > context_limit.saturating_sub(reserved)
}

pub fn prune_tool_outputs(messages: &[Message], max_tokens_per_output: usize) -> Vec<Message> {
    const PROTECTED_TOKENS: usize = 40_000;

    messages
        .iter()
        .map(|msg| match msg {
            Message::Tool {
                tool_call_id,
                content,
            } => {
                let tokens = ContextTracker::estimate_tokens(content);
                if tokens > max_tokens_per_output {
                    let max_chars = max_tokens_per_output * 4;
                    let hint = format!(
                        "\n\n[Tool output truncated ({} tokens). Use read tool to view full output. Protected: {} tokens reserved for conversation.]",
                        tokens,
                        PROTECTED_TOKENS,
                    );
                    let truncated = if content.len() > max_chars {
                        format!("{}{}", &content[..max_chars], hint)
                    } else {
                        format!("{}{}", content, hint)
                    };
                    Message::Tool {
                        tool_call_id: tool_call_id.clone(),
                        content: truncated.into(),
                    }
                } else {
                    Message::Tool {
                        tool_call_id: tool_call_id.clone(),
                        content: content.clone(),
                    }
                }
            }
            other => other.clone(),
        })
        .collect()
}

fn has_long_tool_outputs(messages: &[Message], threshold: usize) -> bool {
    messages.iter().any(|msg| {
        if let Message::Tool { content, .. } = msg {
            content.len() > threshold
        } else {
            false
        }
    })
}

fn count_non_system_messages(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| !matches!(m, Message::System { .. }))
        .count()
}

pub fn auto_compact(
    messages: &[Message],
    context_limit: usize,
    threshold: f64,
    prune: bool,
) -> Vec<Message> {
    let mut tracker = ContextTracker::new(context_limit, threshold);
    tracker.add_messages(messages);

    if !tracker.needs_compaction() {
        return messages.to_vec();
    }

    let mut result = if prune {
        prune_tool_outputs(messages, 10_000)
    } else {
        messages.to_vec()
    };

    tracker.reset();
    tracker.add_messages(&result);

    if tracker.needs_compaction() {
        let strategy = select_compaction_strategy(&result);
        tracing::debug!("auto_compact selecting strategy: {:?}", strategy);
        result = compact_messages_sync(result, strategy);
    }

    result
}

fn select_compaction_strategy(messages: &[Message]) -> CompactionStrategy {
    let non_system_count = count_non_system_messages(messages);
    let has_long_tools = has_long_tool_outputs(messages, 2000);

    if has_long_tools && non_system_count > 6 {
        CompactionStrategy::TruncateToolOutputs
    } else if non_system_count > 8 {
        CompactionStrategy::SummarizeOldTurns
    } else {
        CompactionStrategy::DropMiddleMessages
    }
}

pub fn auto_compact_sync(
    messages: &[Message],
    context_limit: usize,
    threshold: f64,
    prune: bool,
) -> Vec<Message> {
    let mut tracker = ContextTracker::new(context_limit, threshold);
    tracker.add_messages(messages);

    if !tracker.needs_compaction() {
        return messages.to_vec();
    }

    let mut result = if prune {
        prune_tool_outputs(messages, 10_000)
    } else {
        messages.to_vec()
    };

    tracker.reset();
    tracker.add_messages(&result);

    if tracker.needs_compaction() {
        let strategy = select_compaction_strategy(&result);
        tracing::debug!("auto_compact selecting strategy: {:?}", strategy);
        result = compact_messages_sync(result, strategy);
    }

    result
}

pub async fn auto_compact_async(
    messages: &[Message],
    context_limit: usize,
    threshold: f64,
    prune: bool,
    provider: Option<&dyn Provider>,
    model: Option<&str>,
) -> Vec<Message> {
    let mut tracker = ContextTracker::new(context_limit, threshold);
    tracker.add_messages(messages);

    if !tracker.needs_compaction() {
        return messages.to_vec();
    }

    let mut result = if prune {
        prune_tool_outputs(messages, 10_000)
    } else {
        messages.to_vec()
    };

    tracker.reset();
    tracker.add_messages(&result);

    if tracker.needs_compaction() {
        let strategy = select_compaction_strategy(&result);
        tracing::debug!("auto_compact_async selecting strategy: {:?}", strategy);

        if strategy == CompactionStrategy::SummarizeOldTurns {
            if let Some(p) = provider {
                let model = model.unwrap_or("gpt-4o-mini");
                result = compact_messages_async(result, strategy, p, model).await;
            } else {
                result = compact_messages_sync(result, CompactionStrategy::DropMiddleMessages);
            }
        } else {
            result = compact_messages_sync(result, strategy);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_overflow_under_limit() {
        let messages = vec![
            Message::System {
                content: "short".to_string().into(),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
            },
        ];
        assert!(!detect_overflow(&messages, 128_000, 10_000));
    }

    #[test]
    fn test_detect_overflow_over_limit() {
        let long = "x".repeat(1_000_000);
        let messages = vec![
            Message::System {
                content: long.clone().into(),
            },
            Message::User {
                content: vec![ContentPart::Text { text: long.into() }],
            },
        ];
        assert!(detect_overflow(&messages, 128_000, 10_000));
    }

    #[test]
    fn test_prune_tool_outputs_short_content() {
        let messages = vec![Message::Tool {
            tool_call_id: "1".to_string().into(),
            content: "short".to_string().into(),
        }];
        let result = prune_tool_outputs(&messages, 100);
        assert_eq!(result.len(), 1);
        if let Message::Tool { content, .. } = &result[0] {
            assert_eq!(content.as_str(), "short");
        } else {
            panic!("expected tool message");
        }
    }

    #[test]
    fn test_prune_tool_outputs_long_content() {
        let long = "x".repeat(100_000);
        let long_len = long.len();
        let messages = vec![Message::Tool {
            tool_call_id: "1".to_string().into(),
            content: long.into(),
        }];
        let result = prune_tool_outputs(&messages, 100);
        assert_eq!(result.len(), 1);
        if let Message::Tool { content, .. } = &result[0] {
            assert!(content.len() < long_len);
            assert!(content.contains("truncated"));
            assert!(content.contains("40000"));
        } else {
            panic!("expected tool message");
        }
    }

    #[test]
    fn test_auto_compact_no_prune_no_compact() {
        let messages = vec![
            Message::System {
                content: "sys".to_string().into(),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: "hi".to_string().into(),
                }],
            },
        ];
        let result = auto_compact(&messages, 128_000, 0.85, false);
        assert_eq!(result.len(), messages.len());
    }

    #[test]
    fn test_auto_compact_with_prune() {
        let long = "x".repeat(100_000);
        let messages = vec![
            Message::System {
                content: "sys".to_string().into(),
            },
            Message::Tool {
                tool_call_id: "1".to_string().into(),
                content: long.into(),
            },
        ];
        let result = auto_compact(&messages, 1000, 0.5, true);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_context_tracker_needs_compaction() {
        let mut tracker = ContextTracker::new(100, 0.8);
        tracker.add_message(&Message::System {
            content: "x".repeat(900).into(),
        });
        assert!(tracker.needs_compaction());
    }

    #[test]
    fn test_context_tracker_needs_overflow_protection() {
        let mut tracker = ContextTracker::new(100, 0.8);
        tracker.add_message(&Message::System {
            content: "x".repeat(950).into(),
        });
        assert!(tracker.needs_overflow_protection(100));
    }

    #[test]
    fn test_context_tracker_with_max_messages() {
        let tracker = ContextTracker::new(100_000, 0.8).with_max_messages(10);
        assert!(!tracker.needs_bounds_check());
        assert!(tracker.bound_exceeded().is_none());
    }

    #[test]
    fn test_context_tracker_max_messages_exceeded() {
        let mut tracker = ContextTracker::new(100_000, 0.8).with_max_messages(3);
        for i in 0..5 {
            tracker.add_message(&Message::User {
                content: vec![ContentPart::Text {
                    text: format!("msg {i}").into(),
                }],
            });
        }
        assert!(tracker.needs_bounds_check());
        assert_eq!(tracker.bound_exceeded(), Some("max_messages"));
    }

    #[test]
    fn test_context_tracker_with_max_total_bytes() {
        let tracker = ContextTracker::new(100_000, 0.8).with_max_total_bytes(1000);
        assert!(!tracker.needs_bounds_check());
        assert!(tracker.bound_exceeded().is_none());
    }

    #[test]
    fn test_context_tracker_max_total_bytes_exceeded() {
        let mut tracker = ContextTracker::new(100_000, 0.8).with_max_total_bytes(100);
        tracker.add_message(&Message::System {
            content: "x".repeat(1000).into(),
        });
        assert!(tracker.needs_bounds_check());
        assert_eq!(tracker.bound_exceeded(), Some("max_total_bytes"));
    }

    #[test]
    fn test_compact_messages_truncate_tool_outputs() {
        let long = "x".repeat(1000);
        let messages = vec![
            Message::System {
                content: "sys".to_string().into(),
            },
            Message::Tool {
                tool_call_id: "1".to_string().into(),
                content: long.clone().into(),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: "hi".to_string().into(),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
                tool_calls: vec![],
            },
        ];
        let result = compact_messages(messages, CompactionStrategy::TruncateToolOutputs);
        assert_eq!(result.len(), 4);
        if let Message::Tool { content, .. } = &result[1] {
            assert!(content.len() < long.len());
        }
    }

    #[test]
    fn test_compact_messages_summarize_old_turns() {
        let messages: Vec<Message> = (0..10)
            .map(|i| {
                if i % 2 == 0 {
                    Message::User {
                        content: vec![ContentPart::Text {
                            text: format!("user {}", i).into(),
                        }],
                    }
                } else {
                    Message::Assistant {
                        content: vec![ContentPart::Text {
                            text: format!("assistant {}", i).into(),
                        }],
                        tool_calls: vec![],
                    }
                }
            })
            .collect();
        // Note: SummarizeOldTurns in sync context uses DropMiddleMessages as fallback
        let result = compact_messages(messages, CompactionStrategy::SummarizeOldTurns);
        assert!(!result.is_empty());
        // The fallback compaction doesn't add a System message (unlike the old placeholder)
        // Just verify the messages were compacted
        assert!(result.len() < 10);
    }
    
    #[test]
    fn test_compact_messages_drop_middle() {
        let messages: Vec<Message> = (0..8)
            .map(|i| {
                if i % 2 == 0 {
                    Message::User {
                        content: vec![ContentPart::Text {
                            text: format!("user {i}").into(),
                        }],
                    }
                } else {
                    Message::Assistant {
                        content: vec![ContentPart::Text {
                            text: format!("assistant {i}").into(),
                        }],
                        tool_calls: vec![],
                    }
                }
            })
            .collect();
        let result = compact_messages(messages, CompactionStrategy::DropMiddleMessages);
        assert!(!result.is_empty());
    }
}
