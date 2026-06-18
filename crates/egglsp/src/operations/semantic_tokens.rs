use lsp_types::*;

use crate::capability::SemanticTokenLegendSnapshot;
use crate::error::LspError;

/// Decoded semantic-token DTO returned to model-facing surfaces.
///
/// `line` and `start` are absolute (not delta-encoded). `start` and
/// `length` are measured in UTF-16 code units, matching the LSP
/// specification. `token_type` is the legend-resolved name; if the
/// server reports an out-of-range index [`decode_semantic_tokens`]
/// returns a structured [`LspError::RequestFailed`] instead of
/// silently dropping the token.
///
/// `modifiers` is a `Vec<String>` of resolved legend names — bit `i`
/// in the wire `token_modifiers_bitset` corresponds to legend
/// position `i`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DecodedSemanticToken {
    pub line: u32,
    pub start: u32,
    pub length: u32,
    pub token_type: String,
    pub modifiers: Vec<String>,
}

/// Pure helper: decode an LSP delta-encoded semantic-token stream
/// against a server-supplied legend.
///
/// Decoding rules (per LSP §3.16 semanticTokens):
/// - The first token's `line` is `delta_line` (no previous token).
/// - `line = previous.line + delta_line` for subsequent tokens.
/// - If `delta_line == 0`, the token is on the same line as the
///   previous one and `start = previous.start + delta_start`.
/// - Otherwise, the token is on a new line and `start = delta_start`
///   (absolute on that line).
///
/// Returns [`LspError::RequestFailed`] when a token reports a
/// `token_type` index that exceeds the legend, or when delta
/// arithmetic overflows.
pub fn decode_semantic_tokens(
    tokens: &[SemanticToken],
    legend: &SemanticTokenLegendSnapshot,
) -> Result<Vec<DecodedSemanticToken>, LspError> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut prev_line: u32 = 0;
    let mut prev_start: u32 = 0;
    for (i, tok) in tokens.iter().enumerate() {
        let line = prev_line.checked_add(tok.delta_line).ok_or_else(|| {
            LspError::RequestFailed(format!(
                "semantic token line overflow at index {i}: \
                 prev_line={prev_line} + delta_line={}",
                tok.delta_line
            ))
        })?;
        let start = if i == 0 || tok.delta_line != 0 {
            tok.delta_start
        } else {
            prev_start.checked_add(tok.delta_start).ok_or_else(|| {
                LspError::RequestFailed(format!(
                    "semantic token start overflow at index {i}: \
                     prev_start={prev_start} + delta_start={}",
                    tok.delta_start
                ))
            })?
        };
        let token_type_idx = tok.token_type as usize;
        let token_type = legend
            .token_types
            .get(token_type_idx)
            .ok_or_else(|| {
                LspError::RequestFailed(format!(
                    "semantic token_type index {token_type_idx} out of range \
                     (legend has {} types)",
                    legend.token_types.len()
                ))
            })?
            .clone();
        let mut modifiers = Vec::new();
        let bitset = tok.token_modifiers_bitset;
        // Modifier bits beyond the legend length are silently ignored.
        // The LSP spec says the client should ignore bits it doesn't
        // recognize, so this is correct behavior.
        for (bit, name) in legend.token_modifiers.iter().enumerate() {
            if bit >= 32 {
                break;
            }
            if bitset & (1u32 << bit) != 0 {
                modifiers.push(name.clone());
            }
        }
        out.push(DecodedSemanticToken {
            line,
            start,
            length: tok.length,
            token_type,
            modifiers,
        });
        prev_line = line;
        prev_start = start;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{LspCapabilitySnapshot, LspSemanticOperation};
    use lsp_types::ServerCapabilities;

    fn legend(types: &[&str], modifiers: &[&str]) -> SemanticTokenLegendSnapshot {
        SemanticTokenLegendSnapshot {
            token_types: types.iter().map(|s| s.to_string()).collect(),
            token_modifiers: modifiers.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn decode_semantic_tokens_empty_returns_empty_vec() {
        let tokens: Vec<SemanticToken> = Vec::new();
        let l = legend(&["function", "variable"], &["declaration", "deprecated"]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert!(decoded.is_empty());
    }

    #[test]
    fn decode_semantic_tokens_single_token_uses_absolute_deltas() {
        let tokens = vec![SemanticToken {
            delta_line: 5,
            delta_start: 12,
            length: 4,
            token_type: 0,
            token_modifiers_bitset: 0,
        }];
        let l = legend(&["function"], &[]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].line, 5);
        assert_eq!(decoded[0].start, 12);
        assert_eq!(decoded[0].length, 4);
        assert_eq!(decoded[0].token_type, "function");
        assert!(decoded[0].modifiers.is_empty());
    }

    #[test]
    fn decode_semantic_tokens_multiple_on_same_line_accumulates_start() {
        let tokens = vec![
            SemanticToken {
                delta_line: 10,
                delta_start: 3,
                length: 5,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 0,
                delta_start: 4,
                length: 2,
                token_type: 1,
                token_modifiers_bitset: 0,
            },
        ];
        let l = legend(&["function", "variable"], &[]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].line, 10);
        assert_eq!(decoded[0].start, 3);
        assert_eq!(decoded[1].line, 10);
        assert_eq!(decoded[1].start, 7);
        assert_eq!(decoded[1].token_type, "variable");
    }

    #[test]
    fn decode_semantic_tokens_multiple_on_different_lines_uses_absolute_start() {
        let tokens = vec![
            SemanticToken {
                delta_line: 4,
                delta_start: 8,
                length: 3,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 2,
                delta_start: 1,
                length: 6,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
        ];
        let l = legend(&["function"], &[]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].line, 4);
        assert_eq!(decoded[0].start, 8);
        assert_eq!(decoded[1].line, 6);
        assert_eq!(decoded[1].start, 1);
    }

    #[test]
    fn decode_semantic_tokens_resolves_modifier_bitset() {
        let tokens = vec![SemanticToken {
            delta_line: 0,
            delta_start: 0,
            length: 1,
            token_type: 0,
            token_modifiers_bitset: 0b101,
        }];
        let l = legend(&["function"], &["declaration", "readonly", "deprecated"]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert_eq!(decoded[0].modifiers, vec!["declaration", "deprecated"]);
    }

    #[test]
    fn decode_semantic_tokens_out_of_range_token_type_returns_structured_error() {
        let tokens = vec![SemanticToken {
            delta_line: 0,
            delta_start: 0,
            length: 1,
            token_type: 2,
            token_modifiers_bitset: 0,
        }];
        let l = legend(&["function", "variable"], &[]);
        let err = decode_semantic_tokens(&tokens, &l).expect_err("must fail");
        match err {
            LspError::RequestFailed(msg) => {
                assert!(msg.contains("token_type"));
                assert!(msg.contains("index 2"));
                assert!(msg.contains("2 types"));
            }
            other => panic!("expected RequestFailed, got {other:?}"),
        }
    }

    #[test]
    fn decode_semantic_tokens_later_out_of_range_still_returns_error() {
        let tokens = vec![
            SemanticToken {
                delta_line: 0,
                delta_start: 0,
                length: 1,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 0,
                delta_start: 2,
                length: 1,
                token_type: 7,
                token_modifiers_bitset: 0,
            },
        ];
        let l = legend(&["function"], &[]);
        assert!(decode_semantic_tokens(&tokens, &l).is_err());
    }

    #[test]
    fn decode_semantic_tokens_three_token_chain_accumulates_correctly() {
        let tokens = vec![
            SemanticToken {
                delta_line: 2,
                delta_start: 4,
                length: 3,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 0,
                delta_start: 5,
                length: 4,
                token_type: 1,
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 1,
                delta_start: 0,
                length: 6,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
        ];
        let l = legend(&["function", "variable"], &[]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert_eq!(decoded.len(), 3);
        assert_eq!((decoded[0].line, decoded[0].start), (2, 4));
        assert_eq!((decoded[1].line, decoded[1].start), (2, 9));
        assert_eq!((decoded[2].line, decoded[2].start), (3, 0));
        assert_eq!(decoded[0].token_type, "function");
        assert_eq!(decoded[1].token_type, "variable");
        assert_eq!(decoded[2].token_type, "function");
    }

    #[test]
    fn decode_semantic_tokens_line_overflow_returns_error() {
        let tokens = vec![
            SemanticToken {
                delta_line: u32::MAX,
                delta_start: 0,
                length: 1,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 1,
                delta_start: 0,
                length: 1,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
        ];
        let l = legend(&["function"], &[]);
        let err = decode_semantic_tokens(&tokens, &l).expect_err("must fail");
        match err {
            LspError::RequestFailed(msg) => {
                assert!(
                    msg.contains("overflow"),
                    "expected 'overflow' in error: {msg}"
                );
            }
            other => panic!("expected RequestFailed, got {other:?}"),
        }
    }

    #[test]
    fn decode_semantic_tokens_start_overflow_returns_error() {
        let tokens = vec![
            SemanticToken {
                delta_line: 5,
                delta_start: 10,
                length: 1,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 0,
                delta_start: u32::MAX,
                length: 1,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
        ];
        let l = legend(&["function"], &[]);
        let err = decode_semantic_tokens(&tokens, &l).expect_err("must fail");
        match err {
            LspError::RequestFailed(msg) => {
                assert!(
                    msg.contains("overflow"),
                    "expected 'overflow' in error: {msg}"
                );
            }
            other => panic!("expected RequestFailed, got {other:?}"),
        }
    }

    #[test]
    fn decode_semantic_tokens_zero_length_token() {
        let tokens = vec![SemanticToken {
            delta_line: 1,
            delta_start: 0,
            length: 0,
            token_type: 0,
            token_modifiers_bitset: 0,
        }];
        let l = legend(&["function"], &[]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].length, 0);
    }

    #[test]
    fn decode_semantic_tokens_modifier_bits_beyond_legend_are_ignored() {
        let tokens = vec![SemanticToken {
            delta_line: 0,
            delta_start: 0,
            length: 1,
            token_type: 0,
            token_modifiers_bitset: 0b100101,
        }];
        let l = legend(&["function"], &["declaration", "readonly"]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert_eq!(decoded[0].modifiers, vec!["declaration"]);
    }

    // ---- capability gating: semantic tokens ----

    #[test]
    fn capability_snapshot_reports_semantic_tokens_as_unavailable_when_unset() {
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("pylsp"), Some("python"));
        assert!(!snap.supports(LspSemanticOperation::SemanticTokens));
        let u = snap
            .unavailable(LspSemanticOperation::SemanticTokens)
            .expect("unavailable");
        assert_eq!(u.operation, "semanticTokens");
        assert!(u.reason.contains("pylsp"));
    }
}
