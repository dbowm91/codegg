//! Token counting and context packing primitives.
//!
//! Codegg keeps compaction policy and model-driven compaction in
//! `agent/compaction.rs`. `eggcontext` provides deterministic token
//! accounting that can be tested without booting Codegg.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EggcontextError {
    #[error("tokenizer not available: {0}")]
    Tokenizer(String),
}

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

/// Estimate the number of tokens in `text` for the given model hint.
/// When `model` is `None` the `Cl100kBase` tokenizer is used.
pub fn estimate_tokens_sync(text: &str, model: Option<&str>) -> usize {
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

        tiktoken::encoding_for_model(model_name)
            .or_else(|| tiktoken::encoding_for_model("gpt-3.5-turbo"))
            .map(|enc| enc.encode(text).len())
            .unwrap_or_else(|| {
                tiktoken::get_encoding("cl100k_base")
                    .map(|enc| enc.encode(text).len())
                    .unwrap_or(0)
            })
    } else {
        tiktoken::get_encoding("cl100k_base")
            .map(|enc| enc.encode(text).len())
            .unwrap_or(0)
    };

    let multiplier = tokenizer_type.multiplier();
    (base_tokens as f64 * multiplier) as usize
}

/// Convenience wrapper that estimates tokens using the default tokenizer.
pub fn estimate_tokens(text: &str) -> usize {
    estimate_tokens_sync(text, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_for_model_claude() {
        assert_eq!(TokenizerType::for_model("claude-3-opus"), TokenizerType::Claude);
    }

    #[test]
    fn tokenizer_for_model_gemini() {
        assert_eq!(TokenizerType::for_model("gemini-pro"), TokenizerType::Gemini);
    }

    #[test]
    fn tokenizer_for_model_o200k() {
        assert_eq!(TokenizerType::for_model("o200k-base"), TokenizerType::O200kBase);
    }

    #[test]
    fn tokenizer_for_model_default() {
        assert_eq!(
            TokenizerType::for_model("gpt-4o"),
            TokenizerType::Cl100kBase
        );
    }

    #[test]
    fn multiplier_values() {
        assert_eq!(TokenizerType::Cl100kBase.multiplier(), 1.0);
        assert_eq!(TokenizerType::Claude.multiplier(), 1.4);
        assert_eq!(TokenizerType::Gemini.multiplier(), 1.2);
        assert_eq!(TokenizerType::O200kBase.multiplier(), 1.0);
    }

    #[test]
    fn estimate_empty_text() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_short_text() {
        let tokens = estimate_tokens("hello world");
        assert!(tokens > 0);
        assert!(tokens < 10);
    }

    #[test]
    fn estimate_sync_with_model() {
        let a = estimate_tokens_sync("hello", Some("gpt-4"));
        let b = estimate_tokens_sync("hello", Some("gpt-3.5-turbo"));
        assert!(a > 0);
        assert!(b > 0);
    }

    #[test]
    fn estimate_sync_none_uses_default() {
        let tokens = estimate_tokens_sync("hello", None);
        assert!(tokens > 0);
    }

    #[test]
    fn claude_multiplier_scales() {
        let baseline = estimate_tokens_sync("hello world", Some("gpt-4"));
        let claude = estimate_tokens_sync("hello world", Some("claude-3-opus"));
        // Claude has a 1.4x multiplier, so it should be >= baseline.
        assert!(claude >= baseline);
    }
}
