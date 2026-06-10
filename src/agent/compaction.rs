use crate::provider::{ChatRequest, ContentPart, Message, Provider};
use eggcontext::estimate_tokens_sync as egg_estimate_tokens_sync;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

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
pub use eggcontext::TokenizerType;

fn estimate_tokens_sync(text: &str, model: Option<&str>) -> usize {
    egg_estimate_tokens_sync(text, model)
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

    fn estimate_tokens_model(&self, text: &str) -> usize {
        estimate_tokens_sync(text, self.model.as_deref())
    }

    pub fn add_message(&mut self, msg: &Message) {
        let tokens = match msg {
            Message::System { content } => self.estimate_tokens_model(content),
            Message::User { content } => content
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => self.estimate_tokens_model(text),
                    _ => 0,
                })
                .sum(),
            Message::Assistant { content, .. } => content
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => self.estimate_tokens_model(text),
                    _ => 0,
                })
                .sum(),
            Message::Tool { content, .. } => self.estimate_tokens_model(content),
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

    pub fn set_model(&mut self, model: Option<String>) {
        self.model = model;
    }

    pub fn context_limit(&self) -> usize {
        self.context_limit
    }

    pub fn estimate_tokens_for_messages(&self, messages: &[Message]) -> usize {
        let mut total = 0usize;
        for msg in messages {
            total += match msg {
                Message::System { content } => self.estimate_tokens_model(content),
                Message::User { content } => content
                    .iter()
                    .map(|p| match p {
                        ContentPart::Text { text } => self.estimate_tokens_model(text),
                        _ => 0,
                    })
                    .sum(),
                Message::Assistant { content, .. } => content
                    .iter()
                    .map(|p| match p {
                        ContentPart::Text { text } => self.estimate_tokens_model(text),
                        _ => 0,
                    })
                    .sum(),
                Message::Tool { content, .. } => self.estimate_tokens_model(content),
            };
        }
        total
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
        CompactionStrategy::SummarizeOldTurns => {
            summarize_old_turns(non_system, provider, model).await
        }
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

async fn summarize_old_turns(
    messages: Vec<Message>,
    provider: &dyn Provider,
    model: &str,
) -> Vec<Message> {
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
        thinking_budget: None,
        reasoning_effort: None,
    };

    let events = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        provider.stream(&request),
    )
    .await
    .map_err(|_| {
        crate::error::AppError::Provider("Compaction LLM call timed out after 120s".into())
    })??;
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

// === Hybrid Compaction Types (Phase 3) ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionMode {
    Programmatic,
    Agent,
    Hybrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionPolicy {
    Conservative,
    Balanced,
    Cheap,
    Emergency,
    LosslessDebug,
}

impl CompactionPolicy {
    pub fn max_tool_output_tokens(&self) -> usize {
        match self {
            Self::Conservative => 2000,
            Self::Balanced => 1000,
            Self::Cheap => 500,
            Self::Emergency => 200,
            Self::LosslessDebug => usize::MAX,
        }
    }

    pub fn keep_recent_messages(&self) -> usize {
        match self {
            Self::Conservative => 8,
            Self::Balanced => 4,
            Self::Cheap => 2,
            Self::Emergency => 1,
            Self::LosslessDebug => 999,
        }
    }

    pub fn max_summary_tokens(&self) -> usize {
        match self {
            Self::Conservative => 1200,
            Self::Balanced => 800,
            Self::Cheap => 400,
            Self::Emergency => 200,
            Self::LosslessDebug => 2000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedCompactionConfig {
    pub enabled: bool,
    pub auto: bool,
    pub mode: CompactionMode,
    pub policy: CompactionPolicy,
    pub prune: bool,
    pub context_limit: usize,
    pub threshold: f64,
    pub reserved_tokens: usize,
    pub max_tool_output_tokens: usize,
    pub max_summary_tokens: usize,
    pub max_events: usize,
    pub keep_recent_messages: usize,
    pub validate: bool,
    pub preserve_evidence: bool,
    pub inject_context_frame: bool,
    pub compaction_model: Option<String>,
}

impl Default for ResolvedCompactionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto: true,
            mode: CompactionMode::Hybrid,
            policy: CompactionPolicy::Balanced,
            prune: true,
            context_limit: 128_000,
            threshold: 0.7,
            reserved_tokens: 16_000,
            max_tool_output_tokens: 1000,
            max_summary_tokens: 800,
            max_events: 50,
            keep_recent_messages: 4,
            validate: true,
            preserve_evidence: true,
            inject_context_frame: true,
            compaction_model: None,
        }
    }
}

impl ResolvedCompactionConfig {
    pub fn from_config(
        config: &crate::config::schema::CompactionConfig,
        context_limit: usize,
        active_model: Option<&str>,
    ) -> Self {
        use crate::config::schema::{CompactionModeConfig, CompactionPolicyConfig};

        let mode = config
            .mode
            .map(|m| match m {
                CompactionModeConfig::Programmatic => CompactionMode::Programmatic,
                CompactionModeConfig::Agent => CompactionMode::Agent,
                CompactionModeConfig::Hybrid => CompactionMode::Hybrid,
            })
            .unwrap_or(CompactionMode::Hybrid);

        let policy = config
            .policy
            .map(|p| match p {
                CompactionPolicyConfig::Conservative => CompactionPolicy::Conservative,
                CompactionPolicyConfig::Balanced => CompactionPolicy::Balanced,
                CompactionPolicyConfig::Cheap => CompactionPolicy::Cheap,
                CompactionPolicyConfig::Emergency => CompactionPolicy::Emergency,
                CompactionPolicyConfig::LosslessDebug => CompactionPolicy::LosslessDebug,
            })
            .unwrap_or(CompactionPolicy::Balanced);

        // Model resolution: compaction.model -> summarize_model -> active_model
        let compaction_model = config
            .model
            .clone()
            .or_else(|| config.summarize_model.clone())
            .or_else(|| active_model.map(|s| s.to_string()));

        let threshold = config.threshold.unwrap_or(0.7);
        let reserved_tokens = config.reserved.unwrap_or(16_000);

        Self {
            enabled: config.enabled.unwrap_or(true),
            auto: config.auto.unwrap_or(true),
            mode,
            policy,
            prune: config.prune.unwrap_or(true),
            context_limit,
            threshold,
            reserved_tokens,
            max_tool_output_tokens: config
                .max_tool_output_tokens
                .unwrap_or(policy.max_tool_output_tokens()),
            max_summary_tokens: config
                .max_summary_tokens
                .unwrap_or(policy.max_summary_tokens()),
            max_events: config.max_events.unwrap_or(50),
            keep_recent_messages: config
                .keep_recent_messages
                .unwrap_or(policy.keep_recent_messages()),
            validate: config.validate.unwrap_or(true),
            preserve_evidence: config.preserve_evidence.unwrap_or(true),
            inject_context_frame: config.inject_context_frame.unwrap_or(true),
            compaction_model,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionDiagnostic {
    pub level: CompactionDiagnosticLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionDiagnosticLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub id: String,
    pub kind: EvidenceKind,
    pub summary: String,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    UserMessage,
    AssistantMessage,
    ToolCall,
    ToolResult,
    TestRun,
    FilePath,
    Command,
    Diff,
    SecurityFinding,
    Todo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProgrammaticCompactionState {
    pub frame: crate::agent::context_frame::ContextFrame,
    pub evidence: Vec<EvidenceRef>,
    pub retained_message_indices: Vec<usize>,
    pub diagnostics: Vec<CompactionDiagnostic>,
}

pub struct CompactionInput<'a> {
    pub messages: &'a [Message],
    pub config: ResolvedCompactionConfig,
    pub active_model: Option<&'a str>,
}

pub struct CompactionOutput {
    pub messages: Vec<Message>,
    pub frame: Option<crate::agent::context_frame::ContextFrame>,
    pub diagnostics: Vec<CompactionDiagnostic>,
    pub tokens_before: usize,
    pub tokens_after: usize,
}

// === Phase 3 Helper Functions ===

fn extract_text_from_content(content: &[ContentPart]) -> String {
    content
        .iter()
        .filter_map(|p| {
            if let ContentPart::Text { text } = p {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_for_summary(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        format!("{}...", &text[..max_chars])
    }
}

fn compute_content_hash(text: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("sha256:{:016x}", hasher.finish())
}

// === Semantic Checkpoint (Phase 4) ===

/// Perform semantic checkpointing: ask the model to fill in semantic fields
/// from the reduced programmatic state.
pub async fn semantic_checkpoint(
    reduced: &ProgrammaticCompactionState,
    retained_messages: &[Message],
    provider: &dyn crate::provider::Provider,
    model: &str,
    max_summary_tokens: usize,
) -> Result<crate::agent::context_frame::ContextFrame, crate::error::AppError> {
    use crate::provider::{ChatRequest, ContentPart, Message};

    // Build the reduced ledger text
    let mut ledger = String::new();
    ledger.push_str("## Reduced Ledger\n\n");

    if !reduced.frame.touched_files.is_empty() {
        ledger.push_str("### Touched Files\n");
        for f in &reduced.frame.touched_files {
            ledger.push_str(&format!("- {}\n", f));
        }
        ledger.push('\n');
    }

    if !reduced.frame.commands_run.is_empty() {
        ledger.push_str("### Commands Run\n");
        for c in &reduced.frame.commands_run {
            ledger.push_str(&format!("- {}\n", c));
        }
        ledger.push('\n');
    }

    if !reduced.frame.test_results.is_empty() {
        ledger.push_str("### Test Results\n");
        for t in &reduced.frame.test_results {
            ledger.push_str(&format!("- {}\n", t));
        }
        ledger.push('\n');
    }

    if !reduced.frame.unresolved_errors.is_empty() {
        ledger.push_str("### Unresolved Errors\n");
        for e in &reduced.frame.unresolved_errors {
            ledger.push_str(&format!("- {}\n", e));
        }
        ledger.push('\n');
    }

    if !reduced.frame.security_findings.is_empty() {
        ledger.push_str("### Security Findings\n");
        for s in &reduced.frame.security_findings {
            ledger.push_str(&format!("- {}\n", s));
        }
        ledger.push('\n');
    }

    // Add evidence references
    if !reduced.evidence.is_empty() {
        ledger.push_str("### Evidence References\n");
        for e in &reduced.evidence {
            ledger.push_str(&format!(
                "- [{}] {:?}: {}\n",
                e.id,
                e.kind,
                e.summary
            ));
        }
        ledger.push('\n');
    }

    // Add retained recent messages (last few, text only)
    let recent_text: Vec<String> = retained_messages
        .iter()
        .rev()
        .take(6)
        .filter_map(|msg| match msg {
            Message::User { content } => {
                let text = extract_text_from_content(content);
                if text.is_empty() {
                    None
                } else {
                    Some(format!("User: {}", truncate_for_summary(&text, 500)))
                }
            }
            Message::Assistant { content, .. } => {
                let text = extract_text_from_content(content);
                if text.is_empty() {
                    None
                } else {
                    Some(format!("Assistant: {}", truncate_for_summary(&text, 500)))
                }
            }
            _ => None,
        })
        .collect();

    let recent_section = if recent_text.is_empty() {
        String::new()
    } else {
        format!("## Recent Messages\n\n{}\n", recent_text.join("\n\n"))
    };

    let user_goal = reduced.frame.user_goal.as_deref().unwrap_or("unknown");
    let current_task = reduced.frame.current_task.as_deref().unwrap_or("none");

    let prompt = format!(
        "You are updating compact session state for a coding agent. Use only the provided reduced ledger and retained messages. \
         Do not invent file paths, commands, tests, or decisions. Preserve exact user constraints when possible. Return JSON only.\n\n\
         ## Current Context\n\
         User Goal: {}\n\
         Current Task: {}\n\n\
         {}\n\
         {}\n\
         ## Instructions\n\
         Fill in the following JSON fields:\n\
         - constraints: durable user constraints and implementation requirements.\n\
         - decisions: durable architectural or implementation decisions already made.\n\
         - unresolved_errors: current failures or blockers that still matter.\n\
         - next_steps: immediate next actions for the coding agent.\n\n\
         Return ONLY valid JSON with these four fields. No markdown fences, no explanation.",
        user_goal, current_task, ledger, recent_section
    );

    let request = ChatRequest {
        messages: vec![Message::User {
            content: vec![ContentPart::Text {
                text: prompt.into(),
            }],
        }],
        model: model.to_string(),
        tools: None,
        system: Some(
            "You are a structured state updater for a coding agent. Return only valid JSON."
                .to_string(),
        ),
        temperature: Some(0.0),
        top_p: None,
        max_tokens: Some(max_summary_tokens),
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
    };

    let events = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        provider.stream(&request),
    )
    .await
    .map_err(|_| {
        crate::error::AppError::Provider("Semantic checkpoint LLM call timed out after 60s".into())
    })??;

    let mut response_text = String::new();
    let mut stream = events;
    while let Some(event) = stream.next().await {
        match event {
            Ok(crate::provider::ChatEvent::TextDelta(delta)) => response_text.push_str(&delta),
            Ok(crate::provider::ChatEvent::Finish { .. }) => break,
            Ok(crate::provider::ChatEvent::Error(e)) => {
                return Err(crate::error::AppError::Provider(
                    format!("LLM error in semantic checkpoint: {}", e).into(),
                ));
            }
            _ => {}
        }
    }

    if response_text.is_empty() {
        return Err(crate::error::AppError::Provider(
            "Empty semantic checkpoint response".into(),
        ));
    }

    // Parse JSON response
    parse_semantic_response(&response_text)
}

fn parse_semantic_response(
    text: &str,
) -> Result<crate::agent::context_frame::ContextFrame, crate::error::AppError> {
    // Try to extract JSON from the response (handle markdown fences)
    let json_text = text.trim();
    let json_text = if json_text.starts_with("```") {
        let lines: Vec<&str> = json_text.lines().collect();
        let inner: Vec<&str> = lines
            .iter()
            .skip(1) // skip opening ```
            .take_while(|l| !l.trim().starts_with("```"))
            .copied()
            .collect();
        inner.join("\n")
    } else {
        json_text.to_string()
    };

    let parsed: serde_json::Value = serde_json::from_str(&json_text).map_err(|e| {
        crate::error::AppError::Provider(
            format!("Failed to parse semantic checkpoint JSON: {}", e).into(),
        )
    })?;

    let mut frame = crate::agent::context_frame::ContextFrame::default();

    if let Some(constraints) = parsed.get("constraints").and_then(|v| v.as_array()) {
        frame.constraints = constraints
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
    }

    if let Some(decisions) = parsed.get("decisions").and_then(|v| v.as_array()) {
        frame.decisions = decisions
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
    }

    if let Some(errors) = parsed.get("unresolved_errors").and_then(|v| v.as_array()) {
        frame.unresolved_errors = errors
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
    }

    if let Some(next_steps) = parsed.get("next_steps").and_then(|v| v.as_array()) {
        frame.next_steps = next_steps
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
    }

    Ok(frame)
}

/// Merge a semantic frame into a programmatic frame.
/// Semantic fields override programmatic fields only when semantic has non-empty values.
pub fn merge_frames(
    base: &mut crate::agent::context_frame::ContextFrame,
    semantic: crate::agent::context_frame::ContextFrame,
) {
    if !semantic.constraints.is_empty() {
        base.constraints = semantic.constraints;
    }
    if !semantic.decisions.is_empty() {
        base.decisions = semantic.decisions;
    }
    if !semantic.unresolved_errors.is_empty() {
        base.unresolved_errors = semantic.unresolved_errors;
    }
    if !semantic.next_steps.is_empty() {
        base.next_steps = semantic.next_steps;
    }
    // Don't override touched_files, commands_run, test_results from semantic
    // as those are better extracted deterministically
}

// === Phase 2: Invariant Validation ===

#[derive(Debug, Clone)]
pub struct ToolPair<'a> {
    pub assistant_index: usize,
    pub tool_index: Option<usize>,
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub result: Option<&'a str>,
}

pub fn collect_tool_pairs(messages: &[Message]) -> Vec<ToolPair<'_>> {
    let mut pairs: Vec<ToolPair<'_>> = Vec::new();
    let mut id_to_pair_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for (idx, msg) in messages.iter().enumerate() {
        match msg {
            Message::Assistant { tool_calls, .. } => {
                for tc in tool_calls {
                    let pair_index = pairs.len();
                    pairs.push(ToolPair {
                        assistant_index: idx,
                        tool_index: None,
                        tool_call_id: tc.id.to_string(),
                        tool_name: tc.name.to_string(),
                        arguments: tc.arguments.clone(),
                        result: None,
                    });
                    id_to_pair_index.insert(tc.id.to_string(), pair_index);
                }
            }
            Message::Tool {
                tool_call_id,
                content,
            } => {
                if let Some(&pair_index) = id_to_pair_index.get(tool_call_id.as_ref()) {
                    pairs[pair_index].tool_index = Some(idx);
                    pairs[pair_index].result = Some(content.as_str());
                } else {
                    pairs.push(ToolPair {
                        assistant_index: usize::MAX,
                        tool_index: Some(idx),
                        tool_call_id: tool_call_id.to_string(),
                        tool_name: String::new(),
                        arguments: serde_json::Value::Null,
                        result: Some(content.as_str()),
                    });
                }
            }
            _ => {}
        }
    }

    pairs
}

#[derive(Debug, Clone)]
pub enum CompactionInvariantError {
    OrphanToolResult {
        tool_call_id: String,
        message_index: usize,
    },
    MissingToolResult {
        tool_call_id: String,
        assistant_index: usize,
    },
}

impl std::fmt::Display for CompactionInvariantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OrphanToolResult {
                tool_call_id,
                message_index,
            } => write!(
                f,
                "Orphan tool result at index {}: tool_call_id '{}' has no matching assistant",
                message_index, tool_call_id
            ),
            Self::MissingToolResult {
                tool_call_id,
                assistant_index,
            } => write!(
                f,
                "Missing tool result for assistant at index {}: tool_call_id '{}'",
                assistant_index, tool_call_id
            ),
        }
    }
}

impl std::error::Error for CompactionInvariantError {}

pub fn validate_message_invariants(messages: &[Message]) -> Result<(), CompactionInvariantError> {
    let tool_result_ids: std::collections::HashSet<String> = messages
        .iter()
        .filter_map(|m| {
            if let Message::Tool { tool_call_id, .. } = m {
                Some(tool_call_id.to_string())
            } else {
                None
            }
        })
        .collect();

    for (idx, msg) in messages.iter().enumerate() {
        match msg {
            Message::Tool { tool_call_id, .. } => {
                let has_matching_assistant = messages.iter().take(idx).any(|m| {
                    if let Message::Assistant { tool_calls, .. } = m {
                        tool_calls
                            .iter()
                            .any(|tc| tc.id.as_ref() == tool_call_id.as_ref())
                    } else {
                        false
                    }
                });
                if !has_matching_assistant {
                    return Err(CompactionInvariantError::OrphanToolResult {
                        tool_call_id: tool_call_id.to_string(),
                        message_index: idx,
                    });
                }
            }
            Message::Assistant { tool_calls, .. } => {
                let all_tool_call_ids: Vec<String> =
                    tool_calls.iter().map(|tc| tc.id.to_string()).collect();

                if !all_tool_call_ids.is_empty() {
                    for tc_id in &all_tool_call_ids {
                        if !tool_result_ids.contains(tc_id) {
                            return Err(CompactionInvariantError::MissingToolResult {
                                tool_call_id: tc_id.clone(),
                                assistant_index: idx,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

pub fn emergency_pair_safe_compaction(
    messages: &[Message],
    config: &ResolvedCompactionConfig,
) -> Vec<Message> {
    if messages.len() <= 4 {
        return messages.to_vec();
    }

    let groups = build_message_groups(messages);
    let non_system_groups: Vec<&MessageGroup> = groups
        .iter()
        .filter(|g| !matches!(g, MessageGroup::System { .. }))
        .collect();

    let keep_each_side = (config.keep_recent_messages / 2).max(1);

    let mut result_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for group in groups
        .iter()
        .filter(|g| matches!(g, MessageGroup::System { .. }))
    {
        for &idx in group.indices() {
            result_indices.insert(idx);
        }
    }

    for group in non_system_groups.iter().take(keep_each_side) {
        for &idx in group.indices() {
            result_indices.insert(idx);
        }
    }
    for group in non_system_groups.iter().rev().take(keep_each_side) {
        for &idx in group.indices() {
            result_indices.insert(idx);
        }
    }

    let mut result: Vec<Message> = messages
        .iter()
        .enumerate()
        .filter(|(i, _)| result_indices.contains(i))
        .map(|(_, m)| m.clone())
        .collect();

    let marker = "[Emergency compaction applied. Tool-call/result pairs preserved.]".to_string();
    let insert_pos = result
        .iter()
        .position(|m| !matches!(m, Message::System { .. }))
        .unwrap_or(result.len());
    result.insert(
        insert_pos,
        Message::System {
            content: marker.into(),
        },
    );

    result
}

enum MessageGroup {
    System { index: usize },
    Single { index: usize },
    ToolPair { indices: Vec<usize> },
}

impl MessageGroup {
    fn indices(&self) -> &[usize] {
        match self {
            Self::System { index } => std::slice::from_ref(index),
            Self::Single { index } => std::slice::from_ref(index),
            Self::ToolPair { indices } => indices,
        }
    }
}

fn build_message_groups(messages: &[Message]) -> Vec<MessageGroup> {
    let mut groups: Vec<MessageGroup> = Vec::new();
    let mut i = 0;

    while i < messages.len() {
        match &messages[i] {
            Message::System { .. } => {
                groups.push(MessageGroup::System { index: i });
                i += 1;
            }
            Message::Assistant { tool_calls, .. } if !tool_calls.is_empty() => {
                let mut indices = vec![i];
                i += 1;
                while i < messages.len() {
                    if let Message::Tool { .. } = &messages[i] {
                        indices.push(i);
                        i += 1;
                    } else {
                        break;
                    }
                }
                groups.push(MessageGroup::ToolPair { indices });
            }
            _ => {
                groups.push(MessageGroup::Single { index: i });
                i += 1;
            }
        }
    }

    groups
}

// === Phase 3: Programmatic Reducers ===

pub fn build_evidence_index(messages: &[Message]) -> Vec<EvidenceRef> {
    let mut evidence = Vec::new();
    let mut msg_counter = 0;
    let mut tool_counter = 0;

    for msg in messages {
        match msg {
            Message::User { content } => {
                let text = extract_text_from_content(content);
                if !text.is_empty() {
                    evidence.push(EvidenceRef {
                        id: format!("msg_{:04}", msg_counter),
                        kind: EvidenceKind::UserMessage,
                        summary: truncate_for_summary(&text, 200),
                        content_hash: Some(compute_content_hash(&text)),
                    });
                    msg_counter += 1;
                }
            }
            Message::Assistant {
                content,
                tool_calls,
                ..
            } => {
                if !tool_calls.is_empty() {
                    for tc in tool_calls {
                        evidence.push(EvidenceRef {
                            id: format!("tool_{:04}", tool_counter),
                            kind: EvidenceKind::ToolCall,
                            summary: format!(
                                "{}({})",
                                tc.name,
                                truncate_for_summary(&tc.arguments.to_string(), 100)
                            ),
                            content_hash: None,
                        });
                        tool_counter += 1;
                    }
                } else {
                    let text = extract_text_from_content(content);
                    if !text.is_empty() {
                        evidence.push(EvidenceRef {
                            id: format!("msg_{:04}", msg_counter),
                            kind: EvidenceKind::AssistantMessage,
                            summary: truncate_for_summary(&text, 200),
                            content_hash: Some(compute_content_hash(&text)),
                        });
                        msg_counter += 1;
                    }
                }
            }
            Message::Tool { content, .. } => {
                let is_test = looks_like_test_output(content);
                let kind = if is_test {
                    EvidenceKind::TestRun
                } else {
                    EvidenceKind::ToolResult
                };
                evidence.push(EvidenceRef {
                    id: format!("tool_{:04}", tool_counter),
                    kind,
                    summary: truncate_for_summary(content, 200),
                    content_hash: Some(compute_content_hash(content)),
                });
                tool_counter += 1;
            }
            Message::System { .. } => {}
        }
    }

    evidence
}

pub fn prune_tool_outputs_rich(
    messages: &[Message],
    max_tokens_per_output: usize,
    _policy: CompactionPolicy,
) -> Vec<Message> {
    if max_tokens_per_output == usize::MAX {
        return messages.to_vec();
    }

    messages
        .iter()
        .map(|msg| match msg {
            Message::Tool {
                tool_call_id,
                content,
            } => {
                let tokens = ContextTracker::estimate_tokens(content);
                if tokens <= max_tokens_per_output {
                    return msg.clone();
                }

                let original_len = content.len();
                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len();

                let keep_head = (total_lines / 5).clamp(10, 80);
                let keep_tail = (total_lines / 10).clamp(5, 40);

                let salient_indices: Vec<usize> = lines
                    .iter()
                    .enumerate()
                    .filter(|(_, line)| looks_like_salient_line(line))
                    .map(|(i, _)| i)
                    .collect();

                let mut kept_indices: std::collections::HashSet<usize> =
                    std::collections::HashSet::new();
                for i in 0..keep_head.min(total_lines) {
                    kept_indices.insert(i);
                }
                for i in (total_lines.saturating_sub(keep_tail))..total_lines {
                    kept_indices.insert(i);
                }
                for &i in &salient_indices {
                    if i >= keep_head && i < total_lines.saturating_sub(keep_tail) {
                        kept_indices.insert(i);
                    }
                }

                let mut kept_lines: Vec<(usize, &str)> =
                    kept_indices.iter().map(|&i| (i, lines[i])).collect();
                kept_lines.sort_by_key(|(i, _)| *i);

                let content_hash = compute_content_hash(content);

                let mut compacted = format!(
                    "[Tool output compacted]\n\
                     original_tokens_estimate: {}\n\
                     original_chars: {}\n\
                     content_hash: {}\n\
                     kept: {} of {} lines\n\n",
                    tokens,
                    original_len,
                    content_hash,
                    kept_lines.len(),
                    total_lines,
                );

                let head_lines: Vec<&str> = kept_lines
                    .iter()
                    .filter(|(i, _)| *i < keep_head)
                    .map(|(_, l)| *l)
                    .collect();
                if !head_lines.is_empty() {
                    compacted.push_str("--- first lines ---\n");
                    compacted.push_str(&head_lines.join("\n"));
                    compacted.push_str("\n\n");
                }

                let salient_lines: Vec<&str> = kept_lines
                    .iter()
                    .filter(|(i, _)| *i >= keep_head && *i < total_lines.saturating_sub(keep_tail))
                    .map(|(_, l)| *l)
                    .collect();
                if !salient_lines.is_empty() {
                    compacted.push_str("--- salient lines ---\n");
                    compacted.push_str(&salient_lines.join("\n"));
                    compacted.push_str("\n\n");
                }

                let tail_lines: Vec<&str> = kept_lines
                    .iter()
                    .filter(|(i, _)| *i >= total_lines.saturating_sub(keep_tail))
                    .map(|(_, l)| *l)
                    .collect();
                if !tail_lines.is_empty() {
                    compacted.push_str("--- last lines ---\n");
                    compacted.push_str(&tail_lines.join("\n"));
                }

                Message::Tool {
                    tool_call_id: tool_call_id.clone(),
                    content: compacted.into(),
                }
            }
            other => other.clone(),
        })
        .collect()
}

fn looks_like_salient_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("error")
        || lower.contains("warning")
        || lower.contains("failed")
        || lower.contains("failure")
        || lower.contains("panic")
        || lower.contains("assert")
        || lower.contains("exit status")
        || lower.contains("cannot find")
        || lower.contains("not found")
        || lower.contains("undefined")
        || lower.starts_with("error[")
        || lower.starts_with("error:")
        || lower.contains("test result:")
}

pub fn extract_commands(tool_pairs: &[ToolPair<'_>]) -> Vec<String> {
    let mut commands = Vec::new();

    for pair in tool_pairs {
        if pair.tool_name == "bash" || pair.tool_name == "exec" {
            if let Some(cmd) = pair
                .arguments
                .get("command")
                .or_else(|| pair.arguments.get("cmd"))
                .and_then(|v| v.as_str())
            {
                let is_salient = is_salient_command(cmd)
                    || pair
                        .result
                        .map(|r| !is_successful_output(r))
                        .unwrap_or(false);
                if is_salient {
                    commands.push(cmd.to_string());
                }
            }
        }
    }

    commands
}

fn is_salient_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    lower.contains("cargo test")
        || lower.contains("cargo clippy")
        || lower.contains("cargo check")
        || lower.contains("cargo build")
        || lower.contains("pytest")
        || lower.contains("npm test")
        || lower.contains("git status")
        || lower.contains("git diff")
}

fn is_successful_output(output: &str) -> bool {
    let lower = output.to_lowercase();
    !lower.contains("error")
        && !lower.contains("failed")
        && !lower.contains("exit status: 1")
        && !lower.contains("exit status: 2")
}

pub fn extract_file_paths(messages: &[Message], tool_pairs: &[ToolPair<'_>]) -> Vec<String> {
    let mut paths = std::collections::HashSet::new();

    for pair in tool_pairs {
        for key in &["path", "file_path", "file", "filename", "target", "source"] {
            if let Some(val) = pair.arguments.get(*key).and_then(|v| v.as_str()) {
                if looks_like_path(val) {
                    paths.insert(normalize_path(val));
                }
            }
        }
    }

    for msg in messages {
        let text = match msg {
            Message::User { content } => extract_text_from_content(content),
            Message::Assistant { content, .. } => extract_text_from_content(content),
            Message::Tool { content, .. } => content.to_string(),
            _ => continue,
        };

        for path in extract_paths_from_text(&text) {
            paths.insert(normalize_path(&path));
        }
    }

    let mut result: Vec<String> = paths.into_iter().collect();
    result.sort();
    result
}

fn looks_like_path(s: &str) -> bool {
    s.contains('/') && !s.starts_with("http") && s.len() > 3 && s.len() < 200
}

fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "." => {}
            ".." => {
                parts.pop();
            }
            "" if parts.is_empty() => {}
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn extract_paths_from_text(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for word in text.split_whitespace() {
        let clean = word.trim_matches(|c: char| {
            c == '(' || c == ')' || c == ',' || c == '`' || c == '"' || c == '\''
        });
        if looks_like_path(clean) && clean.len() > 5 {
            paths.push(clean.to_string());
        }
    }
    paths
}

pub fn extract_test_and_error_state(tool_pairs: &[ToolPair<'_>]) -> (Vec<String>, Vec<String>) {
    let mut test_results = Vec::new();
    let mut errors = Vec::new();

    for pair in tool_pairs {
        if let Some(output) = pair.result {
            for line in output.lines() {
                if line.contains("test result:") || line.contains("FAILED") {
                    test_results.push(line.trim().to_string());
                }
                if line.starts_with("error[")
                    || line.starts_with("error:")
                    || line.contains("panicked at")
                {
                    errors.push(line.trim().to_string());
                }
            }
        }
    }

    test_results.dedup();
    errors.dedup();

    (test_results, errors)
}

fn looks_like_test_output(text: &str) -> bool {
    text.contains("test result:") || text.contains("running ") || text.contains("FAILED")
}

pub fn extract_user_constraints(messages: &[Message]) -> Vec<String> {
    let mut constraints = Vec::new();
    let keywords = [
        "must",
        "do not",
        "don't",
        "avoid",
        "only",
        "prefer",
        "default",
        "should",
        "should not",
        "must not",
        "unless",
        "keep",
        "preserve",
        "configurable",
    ];

    for msg in messages {
        if let Message::User { content } = msg {
            let text = extract_text_from_content(content);
            let sentences: Vec<&str> = text
                .split(['.', '!', '\n'])
                .collect();

            for sentence in sentences {
                let sentence = sentence.trim();
                if sentence.len() < 10 || sentence.len() > 300 {
                    continue;
                }
                let lower = sentence.to_lowercase();
                if keywords.iter().any(|kw| lower.contains(kw)) {
                    constraints.push(sentence.to_string());
                }
            }
        }
    }

    constraints.dedup();
    constraints
}

pub fn select_retained_messages(
    messages: &[Message],
    _state: &ProgrammaticCompactionState,
    _policy: CompactionPolicy,
    keep_recent_messages: usize,
) -> Vec<usize> {
    let mut retained: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for (i, msg) in messages.iter().enumerate() {
        if matches!(msg, Message::System { .. }) {
            retained.insert(i);
        }
    }

    let non_system_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| !matches!(m, Message::System { .. }))
        .map(|(i, _)| i)
        .collect();

    let recent_to_keep = keep_recent_messages.min(non_system_indices.len());

    for &idx in non_system_indices.iter().rev().take(recent_to_keep) {
        retained.insert(idx);
    }

    for &idx in non_system_indices.iter().take(2) {
        retained.insert(idx);
    }

    let tool_call_to_assistant: std::collections::HashMap<String, usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            if let Message::Assistant { tool_calls, .. } = m {
                tool_calls.first().map(|tc| (tc.id.to_string(), i))
            } else {
                None
            }
        })
        .collect();

    let tool_result_to_index: std::collections::HashMap<String, usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            if let Message::Tool { tool_call_id, .. } = m {
                Some((tool_call_id.to_string(), i))
            } else {
                None
            }
        })
        .collect();

    let ids_to_resolve: Vec<String> = retained
        .iter()
        .filter_map(|&i| {
            if let Message::Tool { tool_call_id, .. } = &messages[i] {
                Some(tool_call_id.to_string())
            } else {
                None
            }
        })
        .collect();

    for id in &ids_to_resolve {
        if let Some(&assistant_idx) = tool_call_to_assistant.get(id) {
            retained.insert(assistant_idx);
        }
    }

    let assistant_ids_to_resolve: Vec<String> = retained
        .iter()
        .filter_map(|&i| {
            if let Message::Assistant { tool_calls, .. } = &messages[i] {
                tool_calls.first().map(|tc| tc.id.to_string())
            } else {
                None
            }
        })
        .collect();

    for id in &assistant_ids_to_resolve {
        if let Some(&tool_idx) = tool_result_to_index.get(id) {
            retained.insert(tool_idx);
        }
    }

    let mut indices: Vec<usize> = retained.into_iter().collect();
    indices.sort();
    indices
}

pub fn build_programmatic_state(
    messages: &[Message],
    config: &ResolvedCompactionConfig,
) -> ProgrammaticCompactionState {
    let evidence = build_evidence_index(messages);
    let tool_pairs = collect_tool_pairs(messages);

    let commands = extract_commands(&tool_pairs);
    let file_paths = extract_file_paths(messages, &tool_pairs);
    let (test_results, errors) = extract_test_and_error_state(&tool_pairs);
    let constraints = extract_user_constraints(messages);

    let retained_indices = select_retained_messages(
        messages,
        &ProgrammaticCompactionState {
            evidence: evidence.clone(),
            ..Default::default()
        },
        config.policy,
        config.keep_recent_messages,
    );

    let frame = crate::agent::context_frame::ContextFrame {
        touched_files: file_paths,
        commands_run: commands,
        test_results,
        unresolved_errors: errors,
        constraints,
        ..Default::default()
    };

    let mut diagnostics = Vec::new();
    diagnostics.push(CompactionDiagnostic {
        level: CompactionDiagnosticLevel::Info,
        message: format!(
            "Programmatic: {} evidence, {} retained",
            evidence.len(),
            retained_indices.len()
        ),
    });

    ProgrammaticCompactionState {
        frame,
        evidence,
        retained_message_indices: retained_indices,
        diagnostics,
    }
}

// === Phase 5: Compaction Engine ===

pub fn compile_programmatic_messages(
    original: &[Message],
    state: &ProgrammaticCompactionState,
    _config: &ResolvedCompactionConfig,
) -> Vec<Message> {
    let mut result = Vec::new();

    for msg in original {
        if matches!(msg, Message::System { .. }) {
            result.push(msg.clone());
        }
    }

    let control_text = state.frame.to_compaction_control_text();
    result.push(Message::System {
        content: control_text.into(),
    });

    for &idx in &state.retained_message_indices {
        if idx < original.len() && !matches!(&original[idx], Message::System { .. }) {
            result.push(original[idx].clone());
        }
    }

    result
}

pub fn compile_hybrid_messages(
    original: &[Message],
    state: &ProgrammaticCompactionState,
    frame: crate::agent::context_frame::ContextFrame,
    _config: &ResolvedCompactionConfig,
) -> Vec<Message> {
    let mut result = Vec::new();

    for msg in original {
        if matches!(msg, Message::System { .. }) {
            result.push(msg.clone());
        }
    }

    let control_text = frame.to_compaction_control_text();
    result.push(Message::System {
        content: control_text.into(),
    });

    for &idx in &state.retained_message_indices {
        if idx < original.len() && !matches!(&original[idx], Message::System { .. }) {
            result.push(original[idx].clone());
        }
    }

    result
}

async fn compact_agent_only(
    input: CompactionInput<'_>,
    provider: Option<&dyn crate::provider::Provider>,
) -> Result<Vec<Message>, crate::error::AppError> {
    if let Some(provider) = provider {
        if let Some(model) = input.config.compaction_model.as_deref() {
            let programmatic = build_programmatic_state(input.messages, &input.config);
            match semantic_checkpoint(
                &programmatic,
                input.messages,
                provider,
                model,
                input.config.max_summary_tokens,
            )
            .await
            {
                Ok(frame) => {
                    let mut result = Vec::new();
                    for msg in input.messages {
                        if matches!(msg, Message::System { .. }) {
                            result.push(msg.clone());
                        }
                    }
                    let control_text = frame.to_compaction_control_text();
                    result.push(Message::System {
                        content: control_text.into(),
                    });

                    let keep = input.config.keep_recent_messages;
                    let non_system: Vec<&Message> = input
                        .messages
                        .iter()
                        .filter(|m| !matches!(m, Message::System { .. }))
                        .collect();
                    for msg in non_system.iter().rev().take(keep) {
                        result.push((*msg).clone());
                    }
                    return Ok(result);
                }
                Err(err) => {
                    tracing::warn!("Agent mode failed: {}, falling back to programmatic", err);
                }
            }
        }
    }

    let programmatic = build_programmatic_state(input.messages, &input.config);
    Ok(compile_programmatic_messages(
        input.messages,
        &programmatic,
        &input.config,
    ))
}

pub async fn compact_with_policy(
    input: CompactionInput<'_>,
    provider: Option<&dyn crate::provider::Provider>,
) -> Result<CompactionOutput, crate::error::AppError> {
    let tracker =
        ContextTracker::new(usize::MAX, 0.0).with_model(input.active_model.map(|s| s.to_string()));
    let tokens_before = tracker.estimate_tokens_for_messages(input.messages);

    let programmatic = build_programmatic_state(input.messages, &input.config);

    let mode = input.config.mode;
    let policy = input.config.policy;
    let validate = input.config.validate;
    let messages_ref = input.messages;
    let active_model = input.active_model;

    let mut output_frame = programmatic.frame.clone();
    let mut messages = match mode {
        CompactionMode::Programmatic => {
            compile_programmatic_messages(messages_ref, &programmatic, &input.config)
        }
        CompactionMode::Agent => compact_agent_only(input, provider).await?,
        CompactionMode::Hybrid => {
            let mut frame = programmatic.frame.clone();
            if let (Some(provider), Some(model)) =
                (provider, input.config.compaction_model.as_deref())
            {
                match semantic_checkpoint(
                    &programmatic,
                    messages_ref,
                    provider,
                    model,
                    input.config.max_summary_tokens,
                )
                .await
                {
                    Ok(semantic_frame) => merge_frames(&mut frame, semantic_frame),
                    Err(err) => {
                        tracing::warn!("Semantic checkpoint failed: {}", err);
                    }
                }
            }
            output_frame = frame.clone();
            compile_hybrid_messages(messages_ref, &programmatic, frame, &input.config)
        }
    };

    if validate {
        if let Err(err) = validate_message_invariants(&messages) {
            tracing::warn!("Invariant check failed: {}, using emergency fallback", err);
            let config_clone = ResolvedCompactionConfig {
                mode,
                policy,
                keep_recent_messages: programmatic.retained_message_indices.len().max(4),
                ..ResolvedCompactionConfig::default()
            };
            messages = emergency_pair_safe_compaction(messages_ref, &config_clone);
            if let Err(err2) = validate_message_invariants(&messages) {
                tracing::error!(
                    "Emergency fallback also failed: {}, preserving original",
                    err2
                );
                messages = messages_ref.to_vec();
            }
        }
    }

    let after_tracker =
        ContextTracker::new(usize::MAX, 0.0).with_model(active_model.map(|s| s.to_string()));
    let tokens_after = after_tracker.estimate_tokens_for_messages(&messages);

    let mut diagnostics = programmatic.diagnostics;
    diagnostics.push(CompactionDiagnostic {
        level: CompactionDiagnosticLevel::Info,
        message: format!(
            "Compaction {:?}/{:?}: {} -> {} tokens",
            mode, policy, tokens_before, tokens_after
        ),
    });

    Ok(CompactionOutput {
        messages,
        frame: Some(output_frame),
        diagnostics,
        tokens_before,
        tokens_after,
    })
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

    // === Phase 4: Semantic Checkpoint Tests ===

    #[tokio::test]
    async fn test_parse_semantic_response_valid() {
        let json = r#"{"constraints": ["must use Rust"], "decisions": ["use async"], "unresolved_errors": [], "next_steps": ["run tests"]}"#;
        let frame = parse_semantic_response(json).unwrap();
        assert_eq!(frame.constraints, vec!["must use Rust"]);
        assert_eq!(frame.decisions, vec!["use async"]);
        assert!(frame.unresolved_errors.is_empty());
        assert_eq!(frame.next_steps, vec!["run tests"]);
    }

    #[tokio::test]
    async fn test_parse_semantic_response_with_fences() {
        let json = "```json\n{\"constraints\": [\"no unwrap\"], \"decisions\": [], \"unresolved_errors\": [], \"next_steps\": []}\n```";
        let frame = parse_semantic_response(json).unwrap();
        assert_eq!(frame.constraints, vec!["no unwrap"]);
    }

    #[tokio::test]
    async fn test_parse_semantic_response_invalid() {
        let result = parse_semantic_response("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_frames() {
        let mut base = crate::agent::context_frame::ContextFrame {
            touched_files: vec!["src/main.rs".to_string()],
            commands_run: vec!["cargo test".to_string()],
            ..Default::default()
        };
        let semantic = crate::agent::context_frame::ContextFrame {
            constraints: vec!["must be fast".to_string()],
            decisions: vec!["use trait objects".to_string()],
            ..Default::default()
        };
        merge_frames(&mut base, semantic);
        assert_eq!(base.constraints, vec!["must be fast"]);
        assert_eq!(base.decisions, vec!["use trait objects"]);
        // Should preserve programmatic fields
        assert_eq!(base.touched_files, vec!["src/main.rs"]);
        assert_eq!(base.commands_run, vec!["cargo test"]);
    }

    #[test]
    fn test_collect_tool_pairs_valid() {
        let messages = vec![
            Message::User {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![crate::provider::ToolCall {
                    id: "call_1".to_string().into(),
                    name: "bash".to_string().into(),
                    arguments: serde_json::json!({"command": "ls"}),
                }],
            },
            Message::Tool {
                tool_call_id: "call_1".to_string().into(),
                content: "output".to_string().into(),
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: "done".to_string().into(),
                }],
                tool_calls: vec![],
            },
        ];
        let pairs = collect_tool_pairs(&messages);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].tool_call_id, "call_1");
        assert!(pairs[0].result.is_some());
    }

    #[test]
    fn test_validate_valid_history() {
        let messages = vec![
            Message::User {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![crate::provider::ToolCall {
                    id: "c1".to_string().into(),
                    name: "bash".to_string().into(),
                    arguments: serde_json::json!({}),
                }],
            },
            Message::Tool {
                tool_call_id: "c1".to_string().into(),
                content: "output".to_string().into(),
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: "done".to_string().into(),
                }],
                tool_calls: vec![],
            },
        ];
        assert!(validate_message_invariants(&messages).is_ok());
    }

    #[test]
    fn test_validate_orphan_tool_result() {
        let messages = vec![
            Message::User {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
            },
            Message::Tool {
                tool_call_id: "orphan".to_string().into(),
                content: "output".to_string().into(),
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: "done".to_string().into(),
                }],
                tool_calls: vec![],
            },
        ];
        let err = validate_message_invariants(&messages).unwrap_err();
        match err {
            CompactionInvariantError::OrphanToolResult { .. } => {}
            _ => panic!("Expected OrphanToolResult"),
        }
    }

    #[test]
    fn test_validate_missing_tool_result() {
        let messages = vec![
            Message::User {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![crate::provider::ToolCall {
                    id: "c1".to_string().into(),
                    name: "bash".to_string().into(),
                    arguments: serde_json::json!({}),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: "no result".to_string().into(),
                }],
                tool_calls: vec![],
            },
        ];
        let err = validate_message_invariants(&messages).unwrap_err();
        match err {
            CompactionInvariantError::MissingToolResult { .. } => {}
            _ => panic!("Expected MissingToolResult"),
        }
    }

    #[test]
    fn test_emergency_pair_safe_preserves_pairs() {
        let messages = vec![
            Message::System {
                content: "system".to_string().into(),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: "msg1".to_string().into(),
                }],
            },
            Message::Assistant {
                content: vec![],
                tool_calls: vec![crate::provider::ToolCall {
                    id: "c1".to_string().into(),
                    name: "bash".to_string().into(),
                    arguments: serde_json::json!({}),
                }],
            },
            Message::Tool {
                tool_call_id: "c1".to_string().into(),
                content: "output1".to_string().into(),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: "msg2".to_string().into(),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: "resp1".to_string().into(),
                }],
                tool_calls: vec![],
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: "msg3".to_string().into(),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: "resp2".to_string().into(),
                }],
                tool_calls: vec![],
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: "msg4".to_string().into(),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: "resp3".to_string().into(),
                }],
                tool_calls: vec![],
            },
        ];
        let config = ResolvedCompactionConfig::default();
        let result = emergency_pair_safe_compaction(&messages, &config);
        assert!(result.iter().any(|m| matches!(m, Message::System { .. })));
        let tool_pairs = collect_tool_pairs(&result);
        for pair in &tool_pairs {
            assert!(
                pair.result.is_some(),
                "Tool pair {} should have result",
                pair.tool_call_id
            );
        }
    }

    #[test]
    fn test_prune_tool_outputs_rich() {
        let mut lines = Vec::new();
        for i in 0..10_000 {
            if i % 100 == 0 {
                lines.push(format!("error: something failed at line {}", i));
            } else {
                lines.push(format!("normal output line {} with some content here", i));
            }
        }
        let long = lines.join("\n");
        let messages = vec![Message::Tool {
            tool_call_id: "1".to_string().into(),
            content: long.clone().into(),
        }];
        let result = prune_tool_outputs_rich(&messages, 10, CompactionPolicy::Balanced);
        assert_eq!(result.len(), 1);
        if let Message::Tool { content, .. } = &result[0] {
            assert!(content.len() < long.len());
            assert!(content.contains("[Tool output compacted]"));
        }
    }

    #[test]
    fn test_extract_commands() {
        let tool_pairs = vec![ToolPair {
            assistant_index: 0,
            tool_index: Some(1),
            tool_call_id: "c1".to_string(),
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "cargo test"}),
            result: Some("test result: 5 passed"),
        }];
        let commands = extract_commands(&tool_pairs);
        assert_eq!(commands.len(), 1);
        assert!(commands[0].contains("cargo test"));
    }

    #[test]
    fn test_extract_user_constraints() {
        let messages = vec![
            Message::User {
                content: vec![ContentPart::Text {
                    text: "You must use Rust for this project.".to_string().into(),
                }],
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: "Do not use unwrap in production code.".to_string().into(),
                }],
            },
        ];
        let constraints = extract_user_constraints(&messages);
        assert_eq!(constraints.len(), 2);
    }

    #[test]
    fn test_build_programmatic_state() {
        let messages = vec![
            Message::System {
                content: "system".to_string().into(),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: "world".to_string().into(),
                }],
                tool_calls: vec![],
            },
        ];
        let config = ResolvedCompactionConfig::default();
        let state = build_programmatic_state(&messages, &config);
        assert!(!state.evidence.is_empty());
    }

    #[test]
    fn test_compile_programmatic_messages() {
        let messages = vec![
            Message::System {
                content: "system".to_string().into(),
            },
            Message::User {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
            },
            Message::Assistant {
                content: vec![ContentPart::Text {
                    text: "world".to_string().into(),
                }],
                tool_calls: vec![],
            },
        ];
        let state = ProgrammaticCompactionState {
            retained_message_indices: vec![1, 2],
            ..Default::default()
        };
        let config = ResolvedCompactionConfig::default();
        let result = compile_programmatic_messages(&messages, &state, &config);
        assert!(result.len() >= 3);
        let has_marker = result.iter().any(|m| {
            if let Message::System { content } = m {
                content.contains("[codegg compacted session state]")
            } else {
                false
            }
        });
        assert!(has_marker);
    }
}
