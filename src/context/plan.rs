//! The canonical, provider-facing context plan.
//!
//! A plan is deliberately lossless in full mode: it describes the request
//! that will be sent, while retaining tiered diagnostics for cache analysis.
//! Provider message chronology is never reconstructed from the tier lists.

use super::artifact::stable_hash_hex;
use super::block::{CacheClass, ContextBlock, ContextBlockKind, Lossiness};
use super::tool_hash::tool_definitions_hash;
use crate::provider::{ChatRequest, ContentPart, Message, ToolDefinition};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextPlanMode {
    Full,
    Observation,
    ConservativeOptional,
}

#[derive(Debug, Clone)]
pub struct PlannedMessage {
    pub sequence: usize,
    pub message: Message,
    pub cache_class: CacheClass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheIdentity {
    pub provider: String,
    pub model: String,
    pub adapter: String,
    pub compiler: String,
    pub tool_surface: String,
    pub mode: ContextPlanMode,
}

impl CacheIdentity {
    pub fn key(&self) -> String {
        format!(
            "provider={};model={};adapter={};compiler={};tools={};mode={:?}",
            self.provider, self.model, self.adapter, self.compiler, self.tool_surface, self.mode
        )
    }
}

#[derive(Debug, Clone)]
pub struct ContextPlan {
    pub stable_blocks: Vec<ContextBlock>,
    pub slow_blocks: Vec<ContextBlock>,
    pub messages: Vec<PlannedMessage>,
    pub control_blocks: Vec<ContextBlock>,
    pub tool_definitions: Vec<ToolDefinition>,
    pub tools_present: bool,
    pub omissions: Vec<super::packer::OmittedContextBlock>,
    pub stable_prefix_hash: String,
    pub tool_surface_hash: String,
    pub adapter_fingerprint: String,
    pub plan_fingerprint: String,
    pub cache_identity: CacheIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPlanDiagnostics {
    pub plan_fingerprint: String,
    pub cache_key: String,
    pub stable_blocks: usize,
    pub slow_blocks: usize,
    pub volatile_messages: usize,
    pub control_blocks: usize,
    pub omitted_blocks: usize,
}

impl ContextPlan {
    /// Build a complete plan from the request. This is intentionally not the
    /// lossy packer: required protocol messages remain in their input order.
    pub fn from_request(
        request: &ChatRequest,
        provider: &str,
        adapter_fingerprint: &str,
        compiler_fingerprint: &str,
        mode: ContextPlanMode,
    ) -> Result<Self, String> {
        validate_tool_protocol(&request.messages)?;
        let tools_present = request.tools.is_some();
        let tools = request.tools.clone().unwrap_or_default();
        let tool_surface_hash = tool_definitions_hash(&tools);
        let mut stable_blocks = Vec::new();
        let mut slow_blocks = Vec::new();
        let mut control_blocks = Vec::new();
        let mut planned = Vec::with_capacity(request.messages.len());

        for (sequence, message) in request.messages.iter().enumerate() {
            let (kind, text, required) = message_block(message);
            let block = ContextBlock::new(
                kind,
                &format!("message:{sequence}"),
                text,
                100,
                required,
                Lossiness::Lossless,
                None,
            );
            match block.kind.tier() {
                CacheClass::StablePrefix => stable_blocks.push(block),
                CacheClass::SlowChanging => slow_blocks.push(block),
                CacheClass::NeverCache => control_blocks.push(block),
                CacheClass::Volatile => {}
            }
            planned.push(PlannedMessage {
                sequence,
                message: message.clone(),
                cache_class: kind.tier(),
            });
        }

        if !tools.is_empty() {
            let tool_text = tools
                .iter()
                .map(|tool| format!("{}:{}", tool.name, tool.description))
                .collect::<Vec<_>>()
                .join("\n");
            slow_blocks.push(ContextBlock::new(
                ContextBlockKind::ToolDefinitions,
                &format!("tools:{tool_surface_hash}"),
                tool_text,
                90,
                true,
                Lossiness::Lossless,
                None,
            ));
        }

        let stable_prefix_hash = stable_hash_hex(
            stable_blocks
                .iter()
                .map(|block| format!("{}:{}", block.content_hash, block.kind as u8))
                .collect::<Vec<_>>()
                .join("|"),
        );
        let plan_fingerprint = stable_hash_hex(request_fingerprint(
            request,
            &planned,
            &tool_surface_hash,
            adapter_fingerprint,
        ));
        let cache_identity = CacheIdentity {
            provider: provider.to_string(),
            model: request.model.clone(),
            adapter: adapter_fingerprint.to_string(),
            compiler: compiler_fingerprint.to_string(),
            tool_surface: tool_surface_hash.clone(),
            mode,
        };

        Ok(Self {
            stable_blocks,
            slow_blocks,
            messages: planned,
            control_blocks,
            tool_definitions: tools,
            tools_present,
            omissions: Vec::new(),
            stable_prefix_hash,
            tool_surface_hash,
            adapter_fingerprint: adapter_fingerprint.to_string(),
            plan_fingerprint,
            cache_identity,
        })
    }

    pub fn cache_key(&self) -> String {
        self.cache_identity.key()
    }

    /// Return bounded diagnostics. Content and private reasoning never leave
    /// the plan through this summary.
    pub fn diagnostics(&self) -> ContextPlanDiagnostics {
        ContextPlanDiagnostics {
            plan_fingerprint: self.plan_fingerprint.clone(),
            cache_key: self.cache_key(),
            stable_blocks: self.stable_blocks.len(),
            slow_blocks: self.slow_blocks.len(),
            volatile_messages: self
                .messages
                .iter()
                .filter(|message| message.cache_class == CacheClass::Volatile)
                .count(),
            control_blocks: self.control_blocks.len(),
            omitted_blocks: self.omissions.len(),
        }
    }

    /// Materialize the same typed inputs for diagnostic packing. Message
    /// blocks are emitted in transcript order here; the legacy packer may
    /// still sort these diagnostic candidates, but provider requests never
    /// consume that sorted representation.
    pub fn packing_blocks(&self) -> Vec<ContextBlock> {
        let mut blocks = self.stable_blocks.clone();
        blocks.extend(self.slow_blocks.clone());
        blocks.extend(self.control_blocks.clone());
        for planned in &self.messages {
            if planned.cache_class == CacheClass::Volatile {
                let (kind, text, required) = message_block(&planned.message);
                blocks.push(ContextBlock::new(
                    kind,
                    &format!("message:{}", planned.sequence),
                    text,
                    100,
                    required,
                    Lossiness::Lossless,
                    None,
                ));
            }
        }
        blocks
    }

    /// Apply the plan as the provider request source. No sorting or tier-based
    /// reconstruction occurs here; chronology is the authoritative sequence.
    pub fn apply_to_request(&self, request: &mut ChatRequest) {
        request.messages = self
            .messages
            .iter()
            .map(|planned| planned.message.clone())
            .collect();
        request.tools = self.tools_present.then(|| self.tool_definitions.clone());
    }
}

fn message_block(message: &Message) -> (ContextBlockKind, String, bool) {
    match message {
        Message::System { content } => (ContextBlockKind::SystemPrompt, content.to_string(), true),
        Message::User { content } => (
            ContextBlockKind::UserMessage,
            visible_content(content),
            true,
        ),
        Message::Assistant { content, .. } => (
            ContextBlockKind::AssistantMessage,
            visible_content(content),
            true,
        ),
        Message::Tool {
            tool_call_id,
            content,
        } => (
            ContextBlockKind::ToolResult,
            format!("tool_call_id={tool_call_id}\n{content}"),
            true,
        ),
    }
}

fn visible_content(parts: &[ContentPart]) -> String {
    parts
        .iter()
        .map(|part| match part {
            ContentPart::Text { text } => text.to_string(),
            ContentPart::Image { .. } => "[image]".to_string(),
            ContentPart::Reasoning { .. } => "[private reasoning]".to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn request_fingerprint(
    request: &ChatRequest,
    messages: &[PlannedMessage],
    tools: &str,
    adapter: &str,
) -> String {
    let mut value = format!(
        "{}|{}|{}|{}",
        request.model,
        tools,
        adapter,
        request.messages.len()
    );
    for planned in messages {
        let (_, text, _) = message_block(&planned.message);
        value.push_str(&format!(
            "|{}:{:?}:{}",
            planned.sequence,
            planned.cache_class,
            stable_hash_hex(text)
        ));
    }
    value
}

fn validate_tool_protocol(messages: &[Message]) -> Result<(), String> {
    let mut pending = BTreeSet::new();
    for message in messages {
        match message {
            Message::Assistant { tool_calls, .. } => {
                for call in tool_calls {
                    pending.insert(call.id.to_string());
                }
            }
            Message::Tool { tool_call_id, .. } if !pending.remove(tool_call_id.as_ref()) => {
                return Err(format!("tool result has no preceding call: {tool_call_id}"));
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatRequest, ToolCall};
    use serde_json::json;

    fn request() -> ChatRequest {
        ChatRequest {
            messages: vec![
                Message::System {
                    content: std::sync::Arc::new("system".to_string()),
                },
                Message::User {
                    content: vec![ContentPart::Text {
                        text: std::sync::Arc::new("question".to_string()),
                    }],
                },
                Message::Assistant {
                    content: vec![],
                    tool_calls: vec![ToolCall {
                        id: std::sync::Arc::new("c1".to_string()),
                        name: std::sync::Arc::new("read".to_string()),
                        arguments: json!({}),
                    }],
                },
                Message::Tool {
                    tool_call_id: std::sync::Arc::new("c1".to_string()),
                    content: std::sync::Arc::new("result".to_string()),
                },
            ],
            model: "m".into(),
            tools: Some(vec![]),
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
        }
    }

    #[test]
    fn plan_preserves_tool_chronology_and_is_deterministic() {
        let req = request();
        let a = ContextPlan::from_request(&req, "p", "a", "c", ContextPlanMode::Full).unwrap();
        let b = ContextPlan::from_request(&req, "p", "a", "c", ContextPlanMode::Full).unwrap();
        assert_eq!(a.plan_fingerprint, b.plan_fingerprint);
        assert_eq!(a.messages.len(), 4);
        assert!(matches!(a.messages[2].message, Message::Assistant { .. }));
        assert!(matches!(a.messages[3].message, Message::Tool { .. }));
    }

    #[test]
    fn invalid_tool_pairing_is_rejected() {
        let mut req = request();
        req.messages.remove(2);
        assert!(ContextPlan::from_request(&req, "p", "a", "c", ContextPlanMode::Full).is_err());
    }
}
