use std::fmt;
use std::ops::Deref;

use serde::{Deserialize, Serialize};

use super::artifact::{compute_content_hash, estimate_tokens, stable_hash_hex};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ContextBlockId(pub String);

impl fmt::Display for ContextBlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for ContextBlockId {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextBlockKind {
    SystemPrompt,
    ModelProfile,
    ToolDefinitions,
    ProjectInstructions,
    SessionFrame,
    GoalContext,
    MemoryContext,
    ActiveWorkingSet,
    UserMessage,
    AssistantMessage,
    ToolResult,
    ControlInstruction,
    TodoReminder,
    ArtifactSummary,
}

impl ContextBlockKind {
    pub fn tier(self) -> CacheClass {
        match self {
            Self::SystemPrompt | Self::ModelProfile | Self::ProjectInstructions => {
                CacheClass::StablePrefix
            }
            Self::ToolDefinitions | Self::GoalContext | Self::MemoryContext => {
                CacheClass::SlowChanging
            }
            Self::SessionFrame
            | Self::ActiveWorkingSet
            | Self::UserMessage
            | Self::AssistantMessage
            | Self::ToolResult
            | Self::TodoReminder
            | Self::ArtifactSummary => CacheClass::Volatile,
            Self::ControlInstruction => CacheClass::NeverCache,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheClass {
    StablePrefix,
    SlowChanging,
    Volatile,
    NeverCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lossiness {
    Lossless,
    ProjectedRecoverable,
    SummaryOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBlock {
    pub id: ContextBlockId,
    pub kind: ContextBlockKind,
    pub text: String,
    pub content_hash: String,
    pub estimated_tokens: usize,
    pub priority: u32,
    pub required: bool,
    pub lossiness: Lossiness,
    pub source: String,
    #[serde(default)]
    pub source_handle: Option<String>,
}

impl ContextBlock {
    pub fn new(
        kind: ContextBlockKind,
        source: &str,
        text: String,
        priority: u32,
        required: bool,
        lossiness: Lossiness,
        source_handle: Option<String>,
    ) -> Self {
        let content_hash = compute_content_hash(&text);
        let estimated_tokens = estimate_tokens(&text);
        let id = ContextBlockId(compute_block_id(kind, source));
        Self {
            id,
            kind,
            text,
            content_hash,
            estimated_tokens,
            priority,
            required,
            lossiness,
            source: source.to_string(),
            source_handle,
        }
    }
}

pub fn compute_block_id(kind: ContextBlockKind, source: &str) -> String {
    // Stable hash of (kind-as-str + ":" + source) using the common sha256 primitive.
    // Full 64-char lowercase hex for correctness (plan: "at least 32 hex").
    let input = format!("{:?}:{}", kind, source);
    stable_hash_hex(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_block(text: &str) -> ContextBlock {
        ContextBlock::new(
            ContextBlockKind::SystemPrompt,
            "sys",
            text.to_string(),
            100,
            true,
            Lossiness::Lossless,
            None,
        )
    }

    #[test]
    fn stable_id_for_identical_content() {
        let a = ContextBlock::new(
            ContextBlockKind::SystemPrompt,
            "src1",
            "hello".into(),
            100,
            true,
            Lossiness::Lossless,
            None,
        );
        let b = ContextBlock::new(
            ContextBlockKind::SystemPrompt,
            "src1",
            "hello".into(),
            100,
            true,
            Lossiness::Lossless,
            None,
        );
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn stable_hash_for_identical_content() {
        let a = sample_block("hello world");
        let b = sample_block("hello world");
        assert_eq!(a.content_hash, b.content_hash);
    }

    #[test]
    fn hash_changes_when_text_changes() {
        let a = sample_block("hello");
        let b = sample_block("world");
        assert_ne!(a.content_hash, b.content_hash);
    }

    #[test]
    fn different_source_produces_different_id() {
        let a = ContextBlock::new(
            ContextBlockKind::SystemPrompt,
            "src1",
            "text".into(),
            100,
            true,
            Lossiness::Lossless,
            None,
        );
        let b = ContextBlock::new(
            ContextBlockKind::SystemPrompt,
            "src2",
            "text".into(),
            100,
            true,
            Lossiness::Lossless,
            None,
        );
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn different_kind_produces_different_id() {
        let a = ContextBlock::new(
            ContextBlockKind::SystemPrompt,
            "src",
            "text".into(),
            100,
            true,
            Lossiness::Lossless,
            None,
        );
        let b = ContextBlock::new(
            ContextBlockKind::ToolDefinitions,
            "src",
            "text".into(),
            100,
            true,
            Lossiness::Lossless,
            None,
        );
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn block_id_deterministic() {
        let id1 = compute_block_id(ContextBlockKind::ToolDefinitions, "src/foo.rs");
        let id2 = compute_block_id(ContextBlockKind::ToolDefinitions, "src/foo.rs");
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 64);
    }

    #[test]
    fn changing_source_or_kind_changes_block_id() {
        let id1 = compute_block_id(ContextBlockKind::SystemPrompt, "s1");
        let id2 = compute_block_id(ContextBlockKind::SystemPrompt, "s2");
        let id3 = compute_block_id(ContextBlockKind::ToolDefinitions, "s1");
        assert_ne!(id1, id2);
        assert_ne!(id1, id3);
        assert_eq!(id1.len(), 64);
        assert_eq!(id2.len(), 64);
    }

    #[test]
    fn estimated_tokens_nonzero_for_nonempty() {
        let block = sample_block("hello world foo bar");
        assert!(block.estimated_tokens > 0);
    }

    #[test]
    fn serialization_roundtrip() {
        let block = sample_block("test content");
        let json = serde_json::to_string(&block).unwrap();
        let back: ContextBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, block.id);
        assert_eq!(back.kind, block.kind);
        assert_eq!(back.text, block.text);
        assert_eq!(back.content_hash, block.content_hash);
        assert_eq!(back.estimated_tokens, block.estimated_tokens);
        assert_eq!(back.priority, block.priority);
        assert_eq!(back.required, block.required);
        assert_eq!(back.source_handle, block.source_handle);
    }

    #[test]
    fn serialization_roundtrip_includes_source_handle() {
        let block = ContextBlock::new(
            ContextBlockKind::ArtifactSummary,
            "artifacts:s1:1",
            "summary".to_string(),
            20,
            false,
            Lossiness::SummaryOnly,
            Some("ctx://tool/sess/1/call_abc".to_string()),
        );
        let json = serde_json::to_string(&block).unwrap();
        let back: ContextBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.source_handle,
            Some("ctx://tool/sess/1/call_abc".to_string())
        );
        assert_eq!(back.source_handle, block.source_handle);
    }

    #[test]
    fn non_artifact_blocks_default_source_handle_to_none() {
        let sys = ContextBlock::new(
            ContextBlockKind::SystemPrompt,
            "sys:m",
            "sys".to_string(),
            100,
            true,
            Lossiness::Lossless,
            None,
        );
        assert_eq!(sys.source_handle, None);

        let tools = ContextBlock::new(
            ContextBlockKind::ToolDefinitions,
            "tools:hash",
            "tools".to_string(),
            80,
            true,
            Lossiness::Lossless,
            None,
        );
        assert_eq!(tools.source_handle, None);

        let ctrl = ContextBlock::new(
            ContextBlockKind::ControlInstruction,
            "ctrl:s",
            "ctrl".to_string(),
            30,
            false,
            Lossiness::SummaryOnly,
            None,
        );
        assert_eq!(ctrl.source_handle, None);
    }

    #[test]
    fn artifact_block_can_carry_ctx_source_handle() {
        let art = ContextBlock::new(
            ContextBlockKind::ArtifactSummary,
            "artifacts:s1:3",
            "sum".to_string(),
            20,
            false,
            Lossiness::SummaryOnly,
            Some("ctx://tool/s1/0/c42".to_string()),
        );
        assert_eq!(art.source_handle, Some("ctx://tool/s1/0/c42".to_string()));
        assert_eq!(art.kind, ContextBlockKind::ArtifactSummary);
    }

    #[test]
    fn kind_tier_mapping() {
        assert_eq!(
            ContextBlockKind::SystemPrompt.tier(),
            CacheClass::StablePrefix
        );
        assert_eq!(
            ContextBlockKind::ToolDefinitions.tier(),
            CacheClass::SlowChanging
        );
        assert_eq!(ContextBlockKind::UserMessage.tier(), CacheClass::Volatile);
        assert_eq!(
            ContextBlockKind::ControlInstruction.tier(),
            CacheClass::NeverCache
        );
    }
}
