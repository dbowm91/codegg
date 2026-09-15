//! Token counting and context packing primitives.
//!
//! This crate provides deterministic token accounting that can be tested
//! without booting any host application. One consumer is CodeGG's
//! compaction policy (`agent/compaction.rs`), which stays outside this
//! crate.
//!
//! ## Two layers
//!
//! - **Deterministic tokenizer layer** (stable): pick a [`TokenizerType`]
//!   explicitly and count with [`count_with_tokenizer`] or
//!   [`estimate_for_tokenizer`]. `Cl100kBase` and `O200kBase` run the
//!   public tiktoken BPE encoding for those vocabularies exactly;
//!   `Claude` and `Gemini` run `cl100k_base` and apply a documented
//!   per-family multiplier, so they are heuristic (see below). This layer
//!   never parses model names.
//! - **Volatile model-name policy layer** (convenience, replaceable):
//!   [`TokenizerType::for_model`] maps a model-name hint to a
//!   `TokenizerType`, and [`estimate_tokens_sync`]/[`estimate_tokens`]/
//!   [`estimate_with_provenance`] combine that mapping with the
//!   deterministic layer. Model vendors revise tokenizers without notice,
//!   so treat the mapping and multipliers as policy: pin or replace
//!   `for_model` when exact accounting matters.
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

    /// Stable identifier for this tokenizer selection.
    ///
    /// `cl100k_base` and `o200k_base` are exact BPE vocabularies;
    /// `claude` and `gemini` are heuristic estimates built on
    /// `cl100k_base` (see [`is_approximate`](Self::is_approximate)).
    /// This string is suitable for logs and persisted provenance;
    /// do not parse vendor model names from it.
    pub fn as_str(self) -> &'static str {
        match self {
            TokenizerType::Cl100kBase => "cl100k_base",
            TokenizerType::O200kBase => "o200k_base",
            TokenizerType::Claude => "claude",
            TokenizerType::Gemini => "gemini",
        }
    }

    /// Underlying tiktoken BPE encoding used for the count.
    ///
    /// `Claude` and `Gemini` share `cl100k_base`; their heuristic nature
    /// comes from the [`multiplier`](Self::multiplier), not from a
    /// vendor-exact encoding.
    pub fn encoding_name(self) -> &'static str {
        match self {
            TokenizerType::Cl100kBase | TokenizerType::Claude | TokenizerType::Gemini => {
                "cl100k_base"
            }
            TokenizerType::O200kBase => "o200k_base",
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
///
/// This is the volatile model-policy layer: it maps `model` through
/// [`TokenizerType::for_model`] and then counts with
/// [`estimate_for_tokenizer`]. To bypass model-name parsing, call
/// [`count_with_tokenizer`] with an explicit [`TokenizerType`].
pub fn estimate_tokens_sync(text: &str, model: Option<&str>) -> usize {
    estimate_with_provenance(text, model).tokens
}

/// Convenience wrapper that estimates tokens using the default tokenizer.
pub fn estimate_tokens(text: &str) -> usize {
    estimate_with_provenance(text, None).tokens
}

/// Count tokens with an explicitly selected [`TokenizerType`].
///
/// This is the deterministic layer: no model-name parsing occurs.
/// `Cl100kBase`/`O200kBase` are exact BPE counts; `Claude`/`Gemini`
/// are `cl100k_base` × multiplier heuristics.
pub fn count_with_tokenizer(text: &str, tokenizer: TokenizerType) -> usize {
    estimate_for_tokenizer(text, tokenizer).tokens
}

/// Estimate tokens with full provenance metadata for an explicitly
/// selected [`TokenizerType`].
///
/// This is the deterministic layer behind [`estimate_with_provenance`].
/// The returned [`TokenEstimate::approximate`] field is `true` for
/// Claude and Gemini (heuristic multiplier) and `false` for
/// `cl100k_base` / `o200k_base` (exact BPE).
pub fn estimate_for_tokenizer(text: &str, tokenizer: TokenizerType) -> TokenEstimate {
    let base_tokens = match tokenizer {
        TokenizerType::Cl100kBase => tiktoken::get_encoding("cl100k_base")
            .map(|enc| enc.encode(text).len())
            .unwrap_or(0),
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

    let multiplier = tokenizer.multiplier();
    // Saturating conversion: on 32-bit targets a plain `as usize`
    // silently truncates near u32::MAX.
    let tokens = (base_tokens as f64 * multiplier) as u64;
    let tokens = usize::try_from(tokens).unwrap_or(usize::MAX);
    TokenEstimate {
        tokens,
        tokenizer,
        approximate: tokenizer.is_approximate(),
    }
}

/// Estimate tokens with full provenance metadata. The returned
/// `TokenEstimate::approximate` field is `true` for Claude and
/// Gemini (heuristic multiplier) and `false` for cl100k_base /
/// o200k_base (exact BPE).
///
/// This combines the volatile [`TokenizerType::for_model`] mapping with
/// the deterministic [`estimate_for_tokenizer`] layer; behavior is
/// identical to mapping first and then calling `estimate_for_tokenizer`.
pub fn estimate_with_provenance(text: &str, model: Option<&str>) -> TokenEstimate {
    // Preserve the historical GPT-4 model-name pinning for the exact
    // `cl100k_base` path: `gpt-4` used the `gpt-4` tiktoken model entry
    // while other cl100k names used `gpt-3.5-turbo`. Both resolve to the
    // same `cl100k_base` vocabulary today, but keep the lookup order so
    // byte-identical counts are preserved through the refactor.
    if let Some(model_name) = model {
        let mapped = TokenizerType::for_model(model_name);
        if mapped == TokenizerType::Cl100kBase {
            let pinned = if model_name.to_lowercase().contains("gpt-4") {
                "gpt-4"
            } else {
                "gpt-3.5-turbo"
            };
            let base_tokens = tiktoken::encoding_for_model(pinned)
                .or_else(|| tiktoken::encoding_for_model("gpt-3.5-turbo"))
                .map(|enc| enc.encode(text).len())
                .unwrap_or_else(|| {
                    tiktoken::get_encoding("cl100k_base")
                        .map(|enc| enc.encode(text).len())
                        .unwrap_or(0)
                });
            return TokenEstimate {
                tokens: base_tokens,
                tokenizer: TokenizerType::Cl100kBase,
                approximate: false,
            };
        }
        return estimate_for_tokenizer(text, mapped);
    }
    estimate_for_tokenizer(text, TokenizerType::Cl100kBase)
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

    #[test]
    fn explicit_tokenizer_bypasses_model_mapping() {
        // Deterministic layer: no model-name parsing.
        let direct = count_with_tokenizer("hello world", TokenizerType::Cl100kBase);
        let via_model = estimate_tokens_sync("hello world", Some("gpt-3.5-turbo"));
        assert_eq!(direct, via_model);

        let direct_o200k = count_with_tokenizer("hello world", TokenizerType::O200kBase);
        let via_o3 = estimate_tokens_sync("hello world", Some("o3-mini"));
        assert_eq!(direct_o200k, via_o3);
    }

    #[test]
    fn estimate_for_tokenizer_matches_model_provenance() {
        for (model, tokenizer) in [
            ("gpt-4", TokenizerType::Cl100kBase),
            ("claude-3-opus", TokenizerType::Claude),
            ("gemini-pro", TokenizerType::Gemini),
            ("o3-mini", TokenizerType::O200kBase),
        ] {
            let via_model = estimate_with_provenance("hello world", Some(model));
            let direct = estimate_for_tokenizer("hello world", tokenizer);
            assert_eq!(via_model, direct, "model {model}");
        }
    }

    #[test]
    fn tokenizer_identifiers_are_stable() {
        assert_eq!(TokenizerType::Cl100kBase.as_str(), "cl100k_base");
        assert_eq!(TokenizerType::O200kBase.as_str(), "o200k_base");
        assert_eq!(TokenizerType::Claude.as_str(), "claude");
        assert_eq!(TokenizerType::Gemini.as_str(), "gemini");
        assert_eq!(TokenizerType::Cl100kBase.encoding_name(), "cl100k_base");
        assert_eq!(TokenizerType::O200kBase.encoding_name(), "o200k_base");
        // Approximate families share the cl100k BPE; the heuristic is the multiplier.
        assert_eq!(TokenizerType::Claude.encoding_name(), "cl100k_base");
        assert_eq!(TokenizerType::Gemini.encoding_name(), "cl100k_base");
    }
}
