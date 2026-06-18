use lsp_types::*;

/// Default cap on signature-help / signature documentation strings.
///
/// The LSP spec allows documentation to be a full Markdown blob; for
/// tool output we bound it so a misbehaving server cannot blow up the
/// payload size.
pub const SIGNATURE_DOC_MAX_CHARS: usize = 2000;

/// Compact signature help DTO returned to model-facing surfaces.
///
/// Truncates documentation strings to [`SIGNATURE_DOC_MAX_CHARS`]
/// characters per item. Parameter offsets (`[start, end]` ranges into
/// the signature label) are resolved to substrings of `label`, matching
/// the behavior of [`format_signature_help`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignatureHelpSummary {
    pub active_signature: Option<u32>,
    pub active_parameter: Option<u32>,
    pub signatures: Vec<SignatureInfoSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignatureInfoSummary {
    pub label: String,
    pub documentation: Option<String>,
    pub parameters: Vec<SignatureParameterSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignatureParameterSummary {
    pub label: String,
    pub documentation: Option<String>,
}

impl SignatureHelpSummary {
    /// Build a normalized summary from a raw `SignatureHelp`. Returns
    /// `None` when `help` has no signatures.
    pub fn from_signature_help(help: &SignatureHelp) -> Option<Self> {
        if help.signatures.is_empty() {
            return None;
        }
        let signatures = help
            .signatures
            .iter()
            .map(|sig| SignatureInfoSummary {
                label: sig.label.clone(),
                documentation: sig.documentation.as_ref().map(format_documentation_clamped),
                parameters: sig
                    .parameters
                    .as_ref()
                    .map(|params| {
                        params
                            .iter()
                            .map(|p| SignatureParameterSummary {
                                label: resolve_parameter_label(&sig.label, &p.label),
                                documentation: p
                                    .documentation
                                    .as_ref()
                                    .map(format_documentation_clamped),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect();
        Some(Self {
            active_signature: help.active_signature,
            active_parameter: help.active_parameter,
            signatures,
        })
    }
}

/// Truncate a documentation string to [`SIGNATURE_DOC_MAX_CHARS`].
pub fn truncate_doc(input: &str, max: usize) -> String {
    if input.len() <= max {
        return input.to_string();
    }
    // Walk char boundaries so we never split a UTF-8 codepoint.
    let mut end = max;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + 16);
    out.push_str(&input[..end]);
    out.push('…');
    out
}

pub(crate) fn format_documentation_clamped(doc: &Documentation) -> String {
    let raw = format_documentation(doc);
    truncate_doc(&raw, SIGNATURE_DOC_MAX_CHARS)
}

/// Convert an LSP position offset (in UTF-16 code units) to a Rust
/// byte offset within the given string. Returns `None` when the
/// offset exceeds the string's length in UTF-16 code units or
/// doesn't land on a char boundary.
pub(crate) fn lsp_units_to_byte_offset(text: &str, units: u32) -> Option<usize> {
    if units == 0 {
        return Some(0);
    }
    let mut byte_offset = 0;
    let mut unit_offset = 0;
    for c in text.chars() {
        let char_units = c.len_utf16() as u32;
        // If the target is within this character, it's not on a boundary.
        if unit_offset + char_units > units {
            return None;
        }
        unit_offset += char_units;
        byte_offset += c.len_utf8();
        if unit_offset == units {
            return Some(byte_offset);
        }
    }
    None
}

pub(crate) fn resolve_parameter_label(sig_label: &str, label: &ParameterLabel) -> String {
    match label {
        ParameterLabel::Simple(s) => s.clone(),
        ParameterLabel::LabelOffsets([start, end]) => {
            let s_byte = match lsp_units_to_byte_offset(sig_label, *start) {
                Some(b) => b,
                None => return String::new(),
            };
            let e_byte = match lsp_units_to_byte_offset(sig_label, *end) {
                Some(b) => b,
                None => return String::new(),
            };
            if s_byte <= e_byte && e_byte <= sig_label.len() {
                sig_label[s_byte..e_byte].to_string()
            } else {
                String::new()
            }
        }
    }
}

pub(crate) fn format_documentation(doc: &Documentation) -> String {
    match doc {
        Documentation::String(s) => s.clone(),
        Documentation::MarkupContent(mc) => mc.value.clone(),
    }
}

pub(crate) fn format_signature_help_typed(help: &SignatureHelpSummary) -> String {
    let mut result = String::new();
    for (i, sig) in help.signatures.iter().enumerate() {
        if i > 0 {
            result.push_str("\n---\n");
        }
        result.push_str(&sig.label);
        if let Some(doc) = &sig.documentation {
            result.push_str("\n\n");
            result.push_str(doc);
        }
        for (j, param) in sig.parameters.iter().enumerate() {
            let doc_str = param.documentation.as_deref().unwrap_or("");
            result.push_str(&format!("\n  {}. {}: {}", j + 1, param.label, doc_str));
        }
    }
    result
}

pub(crate) fn format_hover_contents(contents: &HoverContents) -> String {
    match contents {
        HoverContents::Scalar(s) => match s {
            MarkedString::String(s) => s.clone(),
            MarkedString::LanguageString(ls) => {
                format!("```{}\n{}\n```", ls.language, ls.value)
            }
        },
        HoverContents::Array(arr) => arr
            .iter()
            .map(|s| match s {
                MarkedString::String(s) => s.clone(),
                MarkedString::LanguageString(ls) => {
                    format!("```{}\n{}\n```", ls.language, ls.value)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::Markup(mc) => mc.value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{LspCapabilitySnapshot, LspSemanticOperation};
    use lsp_types::{MarkupContent, MarkupKind, Uri};
    use std::str::FromStr;

    fn uri(s: &str) -> Uri {
        Uri::from_str(s).expect("valid uri")
    }

    #[test]
    fn truncate_doc_short_strings_pass_through() {
        assert_eq!(truncate_doc("hello", 100), "hello");
        assert_eq!(truncate_doc("", 100), "");
    }

    #[test]
    fn truncate_doc_caps_at_max() {
        let s = "a".repeat(SIGNATURE_DOC_MAX_CHARS + 50);
        let out = truncate_doc(&s, SIGNATURE_DOC_MAX_CHARS);
        assert!(out.ends_with('…'));
        assert!(out.len() <= SIGNATURE_DOC_MAX_CHARS + 4);
        assert!(s.starts_with(out.trim_end_matches('…')));
    }

    #[test]
    fn truncate_doc_respects_utf8_boundaries() {
        let mut s = String::new();
        for _ in 0..(SIGNATURE_DOC_MAX_CHARS / 2) {
            s.push_str("a");
        }
        s.push('🦀');
        s.push_str("rest");
        let _ = truncate_doc(&s, SIGNATURE_DOC_MAX_CHARS);
    }

    // ---- SignatureHelpSummary ----

    #[test]
    fn signature_help_summary_extracts_simple_label() {
        let help = SignatureHelp {
            signatures: vec![SignatureInformation {
                label: "fn foo(a: i32, b: i32) -> i32".to_string(),
                documentation: Some(Documentation::String("Sums two ints.".to_string())),
                parameters: Some(vec![ParameterInformation {
                    label: ParameterLabel::Simple("a: i32".to_string()),
                    documentation: None,
                }]),
                active_parameter: None,
            }],
            active_signature: Some(0),
            active_parameter: Some(0),
        };
        let summary = SignatureHelpSummary::from_signature_help(&help).unwrap();
        assert_eq!(summary.active_signature, Some(0));
        assert_eq!(summary.active_parameter, Some(0));
        assert_eq!(summary.signatures.len(), 1);
        assert_eq!(summary.signatures[0].label, "fn foo(a: i32, b: i32) -> i32");
        assert_eq!(
            summary.signatures[0].documentation.as_deref(),
            Some("Sums two ints.")
        );
        assert_eq!(summary.signatures[0].parameters.len(), 1);
        assert_eq!(summary.signatures[0].parameters[0].label, "a: i32");
        assert!(summary.signatures[0].parameters[0].documentation.is_none());
    }

    #[test]
    fn signature_help_summary_resolves_label_offsets() {
        let help = SignatureHelp {
            signatures: vec![SignatureInformation {
                label: "fn add(x: u32, y: u32) -> u32".to_string(),
                documentation: None,
                parameters: Some(vec![ParameterInformation {
                    label: ParameterLabel::LabelOffsets([7, 13]),
                    documentation: Some(Documentation::String("first".to_string())),
                }]),
                active_parameter: None,
            }],
            active_signature: None,
            active_parameter: None,
        };
        let summary = SignatureHelpSummary::from_signature_help(&help).unwrap();
        assert_eq!(summary.signatures[0].parameters[0].label, "x: u32");
        assert_eq!(
            summary.signatures[0].parameters[0].documentation.as_deref(),
            Some("first")
        );
    }

    #[test]
    fn signature_help_summary_truncates_long_documentation() {
        let huge = "x".repeat(SIGNATURE_DOC_MAX_CHARS * 3);
        let help = SignatureHelp {
            signatures: vec![SignatureInformation {
                label: "fn huge()".to_string(),
                documentation: Some(Documentation::String(huge)),
                parameters: None,
                active_parameter: None,
            }],
            active_signature: None,
            active_parameter: None,
        };
        let summary = SignatureHelpSummary::from_signature_help(&help).unwrap();
        let doc = summary.signatures[0]
            .documentation
            .as_deref()
            .expect("doc present");
        assert!(doc.ends_with('…'));
        assert!(doc.chars().count() <= SIGNATURE_DOC_MAX_CHARS + 1);
    }

    #[test]
    fn signature_help_summary_uses_markup_content_value() {
        let help = SignatureHelp {
            signatures: vec![SignatureInformation {
                label: "fn bar()".to_string(),
                documentation: Some(Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**bold** doc".to_string(),
                })),
                parameters: None,
                active_parameter: None,
            }],
            active_signature: None,
            active_parameter: None,
        };
        let summary = SignatureHelpSummary::from_signature_help(&help).unwrap();
        assert_eq!(
            summary.signatures[0].documentation.as_deref(),
            Some("**bold** doc")
        );
    }

    #[test]
    fn signature_help_summary_returns_none_when_empty() {
        let help = SignatureHelp {
            signatures: Vec::new(),
            active_signature: None,
            active_parameter: None,
        };
        assert!(SignatureHelpSummary::from_signature_help(&help).is_none());
    }

    #[test]
    fn signature_help_summary_handles_offset_out_of_bounds() {
        let help = SignatureHelp {
            signatures: vec![SignatureInformation {
                label: "fn z()".to_string(),
                documentation: None,
                parameters: Some(vec![ParameterInformation {
                    label: ParameterLabel::LabelOffsets([1000, 2000]),
                    documentation: None,
                }]),
                active_parameter: None,
            }],
            active_signature: None,
            active_parameter: None,
        };
        let summary = SignatureHelpSummary::from_signature_help(&help).unwrap();
        assert_eq!(summary.signatures[0].parameters[0].label, "");
    }

    // ---- format_signature_help_typed ----

    #[test]
    fn format_signature_help_typed_renders_label_and_documentation() {
        let summary = SignatureHelpSummary {
            active_signature: Some(0),
            active_parameter: Some(0),
            signatures: vec![SignatureInfoSummary {
                label: "fn add(a: i32, b: i32) -> i32".to_string(),
                documentation: Some("Adds two ints.".to_string()),
                parameters: vec![
                    SignatureParameterSummary {
                        label: "a: i32".to_string(),
                        documentation: Some("first".to_string()),
                    },
                    SignatureParameterSummary {
                        label: "b: i32".to_string(),
                        documentation: None,
                    },
                ],
            }],
        };
        let out = format_signature_help_typed(&summary);
        assert!(out.contains("fn add(a: i32, b: i32) -> i32"));
        assert!(out.contains("Adds two ints."));
        assert!(out.contains("1. a: i32: first"));
        assert!(out.contains("2. b: i32: "));
    }

    #[test]
    fn format_signature_help_typed_separates_signatures_with_dashes() {
        let summary = SignatureHelpSummary {
            active_signature: None,
            active_parameter: None,
            signatures: vec![
                SignatureInfoSummary {
                    label: "sig1".to_string(),
                    documentation: None,
                    parameters: Vec::new(),
                },
                SignatureInfoSummary {
                    label: "sig2".to_string(),
                    documentation: None,
                    parameters: Vec::new(),
                },
            ],
        };
        let out = format_signature_help_typed(&summary);
        assert!(out.contains("sig1"));
        assert!(out.contains("\n---\n"));
        assert!(out.contains("sig2"));
    }

    // ---- lsp_units_to_byte_offset ----

    #[test]
    fn lsp_units_to_byte_offset_ascii() {
        let text = "hello world";
        assert_eq!(lsp_units_to_byte_offset(text, 0), Some(0));
        assert_eq!(lsp_units_to_byte_offset(text, 5), Some(5));
        assert_eq!(lsp_units_to_byte_offset(text, 11), Some(11));
        assert_eq!(lsp_units_to_byte_offset(text, 12), None);
    }

    #[test]
    fn lsp_units_to_byte_offset_empty_string() {
        assert_eq!(lsp_units_to_byte_offset("", 0), Some(0));
        assert_eq!(lsp_units_to_byte_offset("", 1), None);
    }

    #[test]
    fn lsp_units_to_byte_offset_non_ascii() {
        let text = "café";
        assert_eq!(lsp_units_to_byte_offset(text, 0), Some(0));
        assert_eq!(lsp_units_to_byte_offset(text, 2), Some(2));
        assert_eq!(lsp_units_to_byte_offset(text, 3), Some(3));
        assert_eq!(lsp_units_to_byte_offset(text, 4), Some(5));
    }

    #[test]
    fn lsp_units_to_byte_offset_cjk() {
        let text = "漢字";
        assert_eq!(lsp_units_to_byte_offset(text, 0), Some(0));
        assert_eq!(lsp_units_to_byte_offset(text, 1), Some(3));
        assert_eq!(lsp_units_to_byte_offset(text, 2), Some(6));
    }

    #[test]
    fn lsp_units_to_byte_offset_mixed_ascii_and_non_ascii() {
        let text = "fn café(x: i32)";
        assert_eq!(lsp_units_to_byte_offset(text, 0), Some(0));
        assert_eq!(lsp_units_to_byte_offset(text, 6), Some(6));
        assert_eq!(lsp_units_to_byte_offset(text, 7), Some(8));
        assert_eq!(lsp_units_to_byte_offset(text, 8), Some(9));
    }

    #[test]
    fn lsp_units_to_byte_offset_offset_in_middle_of_multibyte_char() {
        let text = "é";
        assert_eq!(lsp_units_to_byte_offset(text, 0), Some(0));
        assert_eq!(lsp_units_to_byte_offset(text, 1), Some(2));
    }

    // ---- resolve_parameter_label with non-ASCII ----

    #[test]
    fn resolve_parameter_label_with_non_ascii() {
        let sig = "fn café(x: i32)";
        let label = ParameterLabel::LabelOffsets([8, 9]);
        assert_eq!(resolve_parameter_label(sig, &label), "x");
    }

    #[test]
    fn resolve_parameter_label_simple() {
        let label = ParameterLabel::Simple("x: i32".to_string());
        assert_eq!(resolve_parameter_label("any", &label), "x: i32");
    }

    #[test]
    fn resolve_parameter_label_offsets_beyond_end_returns_empty() {
        let label = ParameterLabel::LabelOffsets([100, 200]);
        assert_eq!(resolve_parameter_label("short", &label), "");
    }

    #[test]
    fn resolve_parameter_label_offsets_zero_length() {
        let label = ParameterLabel::LabelOffsets([3, 3]);
        assert_eq!(resolve_parameter_label("hello", &label), "");
    }

    #[test]
    fn resolve_parameter_label_offsets_ascii_passthrough() {
        let sig = "fn add(x: u32, y: u32) -> u32";
        let label = ParameterLabel::LabelOffsets([7, 13]);
        assert_eq!(resolve_parameter_label(sig, &label), "x: u32");
    }

    // ---- LspUnavailable display ----

    #[test]
    fn lsp_unavailable_display_includes_server_and_language_when_known() {
        let u = crate::capability::LspUnavailable::new(LspSemanticOperation::Declaration, "no provider")
            .with_server("rust-analyzer")
            .with_language_id("rust");
        let s = u.to_string();
        assert!(s.contains("declaration"));
        assert!(s.contains("rust-analyzer"));
        assert!(s.contains("rust"));
        assert!(s.contains("no provider"));
    }

    #[test]
    fn lsp_unavailable_display_falls_back_when_unknown() {
        let u = crate::capability::LspUnavailable::new(LspSemanticOperation::Implementation, "no provider");
        let s = u.to_string();
        assert!(s.contains("implementation"));
        assert!(s.contains("no provider"));
        assert!(!s.contains("("));
    }
}
