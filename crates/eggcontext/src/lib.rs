//! Token counting and context packing primitives.
//!
//! Codegg keeps compaction policy and model-driven compaction in
//! `agent/compaction.rs`. `eggcontext` provides deterministic token
//! accounting that can be tested without booting Codegg.
//!
//! ## Approximation
//!
//! `estimate_tokens_sync` (and its companion `estimate_tokens`) are
//! **approximate** for Claude and Gemini model families. We
//! encode text with the public `cl100k_base` BPE tokenizer (which
//! is a faithful fit for GPT-3.5/GPT-4-style models) and then
//! apply a per-family multiplier. The multiplier is a documented
//! heuristic, not a measured value from the actual vendor
//! tokenizer, so callers must treat the resulting count as an
//! upper-bound estimate. The richer `estimate_with_provenance`
//! API returns a `TokenEstimate` whose `approximate` field
//! reflects whether the chosen path was an exact BPE count or a
//! heuristic.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EggcontextError {
    #[error("tokenizer not available: {0}")]
    Tokenizer(String),
}

/// Which tokenizer was used to produce a `TokenEstimate`.
///
/// `Cl100kBase` and `O200kBase` are exact: we run the public
/// tiktoken BPE encoding for those vocabularies. `Claude` and
/// `Gemini` are approximate — we use the cl100k_base encoder
/// and apply a per-family multiplier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenizerType {
    Cl100kBase,
    Claude,
    Gemini,
    O200kBase,
}

impl TokenizerType {
    /// Map a model name hint to a `TokenizerType`. Matching is
    /// case-insensitive and uses substring containment so values
    /// like `claude-3-5-sonnet-20241022`, `claude-3-opus`, and
    /// `claude-sonnet-4-5` all resolve to `Claude`.
    pub fn for_model(model: &str) -> Self {
        let lower = model.to_lowercase();
        if lower.contains("claude") {
            TokenizerType::Claude
        } else if lower.contains("gemini") {
            TokenizerType::Gemini
        } else if lower.contains("o200k") || lower.contains("o3") || lower.contains("gpt-4.1") {
            // o3-mini, o1, gpt-4.1, and explicit o200k hints all
            // use the newer o200k_base vocabulary.
            TokenizerType::O200kBase
        } else {
            // Default: cl100k_base (gpt-3.5 / gpt-4 family).
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

    /// Whether the token count for this family is exact or a
    /// heuristic estimate.
    pub fn is_approximate(self) -> bool {
        matches!(self, TokenizerType::Claude | TokenizerType::Gemini)
    }
}

/// A token count with provenance metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenEstimate {
    /// The estimated token count, rounded from the BPE
    /// encoder (exact) or the BPE encoder × multiplier
    /// (approximate).
    pub tokens: usize,
    /// Which tokenizer was used.
    pub tokenizer: TokenizerType,
    /// True when the count is derived from a per-family
    /// multiplier rather than an exact BPE encoding.
    pub approximate: bool,
}

/// Estimate tokens for `text` for a given `model` hint, returning
/// just the count. **This count is approximate for Claude and
/// Gemini**; use `estimate_with_provenance` if you need to
/// distinguish exact from approximate.
pub fn estimate_tokens_sync(text: &str, model: Option<&str>) -> usize {
    estimate_with_provenance(text, model).tokens
}

/// Convenience wrapper that estimates tokens using the default tokenizer.
pub fn estimate_tokens(text: &str) -> usize {
    estimate_with_provenance(text, None).tokens
}

/// Estimate tokens with full provenance metadata. The returned
/// `TokenEstimate::approximate` field is `true` for Claude and
/// Gemini (heuristic multiplier) and `false` for cl100k_base /
/// o200k_base (exact BPE).
pub fn estimate_with_provenance(text: &str, model: Option<&str>) -> TokenEstimate {
    let tokenizer_type = model
        .map(TokenizerType::for_model)
        .unwrap_or(TokenizerType::Cl100kBase);

    let base_tokens = match tokenizer_type {
        TokenizerType::Cl100kBase => {
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
        }
        TokenizerType::O200kBase => tiktoken::get_encoding("o200k_base")
            .map(|enc| enc.encode(text).len())
            .unwrap_or_else(|| {
                tiktoken::get_encoding("cl100k_base")
                    .map(|enc| enc.encode(text).len())
                    .unwrap_or(0)
            }),
        TokenizerType::Claude | TokenizerType::Gemini => tiktoken::get_encoding("cl100k_base")
            .map(|enc| enc.encode(text).len())
            .unwrap_or(0),
    };

    let multiplier = tokenizer_type.multiplier();
    let tokens = (base_tokens as f64 * multiplier) as usize;
    TokenEstimate {
        tokens,
        tokenizer: tokenizer_type,
        approximate: tokenizer_type.is_approximate(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_for_model_claude() {
        assert_eq!(
            TokenizerType::for_model("claude-3-opus"),
            TokenizerType::Claude
        );
    }

    #[test]
    fn tokenizer_for_model_claude_sonnet_4_5() {
        assert_eq!(
            TokenizerType::for_model("claude-sonnet-4-5"),
            TokenizerType::Claude
        );
    }

    #[test]
    fn tokenizer_for_model_gemini() {
        assert_eq!(
            TokenizerType::for_model("gemini-pro"),
            TokenizerType::Gemini
        );
    }

    #[test]
    fn tokenizer_for_model_o200k() {
        assert_eq!(
            TokenizerType::for_model("o200k-base"),
            TokenizerType::O200kBase
        );
    }

    #[test]
    fn tokenizer_for_model_o3() {
        assert_eq!(
            TokenizerType::for_model("o3-mini"),
            TokenizerType::O200kBase
        );
    }

    #[test]
    fn tokenizer_for_model_gpt4_1() {
        assert_eq!(
            TokenizerType::for_model("gpt-4.1"),
            TokenizerType::O200kBase
        );
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
    fn approximate_flag_is_set_correctly() {
        assert!(!TokenizerType::Cl100kBase.is_approximate());
        assert!(!TokenizerType::O200kBase.is_approximate());
        assert!(TokenizerType::Claude.is_approximate());
        assert!(TokenizerType::Gemini.is_approximate());
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

    #[test]
    fn provenance_marks_approximate_for_claude() {
        let est = estimate_with_provenance("hello world", Some("claude-3-opus"));
        assert!(est.approximate);
        assert_eq!(est.tokenizer, TokenizerType::Claude);
        assert!(est.tokens > 0);
    }

    #[test]
    fn provenance_marks_exact_for_cl100k() {
        let est = estimate_with_provenance("hello world", Some("gpt-4"));
        assert!(!est.approximate);
        assert_eq!(est.tokenizer, TokenizerType::Cl100kBase);
    }

    #[test]
    fn provenance_marks_exact_for_o200k() {
        let est = estimate_with_provenance("hello world", Some("o3-mini"));
        assert!(!est.approximate);
        assert_eq!(est.tokenizer, TokenizerType::O200kBase);
    }

    #[test]
    fn estimate_sync_matches_with_provenance() {
        let n = estimate_tokens_sync("hello", Some("gpt-4"));
        let p = estimate_with_provenance("hello", Some("gpt-4"));
        assert_eq!(n, p.tokens);
    }
}
