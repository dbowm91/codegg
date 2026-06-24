use std::path::Path;

use lsp_types::*;
use tracing::trace;

use crate::client::url_to_uri;
use crate::error::LspError;

use super::semantic_tokens::DecodedSemanticToken;
use super::signature::truncate_doc;
use super::LspOperations;

/// Default cap on per-completion `detail` strings and `insertText`
/// previews exposed through [`CompletionCandidate`].
pub const COMPLETION_DETAIL_MAX_CHARS: usize = 200;

/// Bounded completion candidate DTO returned to model-facing surfaces.
///
/// Strips raw completion-item edit payloads (`textEdit`,
/// `additionalTextEdits`, `command`) — this surface is read-only and
/// must never apply edits. `detail` and `insert_text_preview` are
/// truncated to [`COMPLETION_DETAIL_MAX_CHARS`] characters each.
///
/// `kind` is the LSP `CompletionItemKind` rendered as a lowercase
/// stable name (`"function"`, `"variable"`, …) for known kinds and as
/// `"kind(N)"` for unknown / custom kinds. Server order is preserved;
/// no client-side sorting is applied.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompletionCandidate {
    pub label: String,
    pub detail: Option<String>,
    pub kind: Option<String>,
    pub sort_text: Option<String>,
    pub filter_text: Option<String>,
    pub insert_text_preview: Option<String>,
    pub deprecated: bool,
}

impl CompletionCandidate {
    /// Build a bounded candidate from a raw `CompletionItem`. Pure
    /// conversion — does not touch the network or apply edits.
    pub fn from_completion_item(item: &CompletionItem) -> Self {
        Self {
            label: item.label.clone(),
            detail: item
                .detail
                .as_ref()
                .map(|d| truncate_doc(d, COMPLETION_DETAIL_MAX_CHARS)),
            kind: item.kind.map(completion_kind_to_string),
            sort_text: item.sort_text.clone(),
            filter_text: item.filter_text.clone(),
            insert_text_preview: item
                .insert_text
                .as_ref()
                .map(|t| truncate_doc(t, COMPLETION_DETAIL_MAX_CHARS)),
            deprecated: item.deprecated.unwrap_or(false),
        }
    }
}

/// Render an `CompletionItemKind` as a stable lowercase name. Known
/// LSP kinds map to `"function"`, `"variable"`, …; unknown / custom
/// kinds render as `"kind(<formatted_debug>)"` so the surface stays
/// informative without crashing on server-defined extensions.
pub fn completion_kind_to_string(kind: CompletionItemKind) -> String {
    if kind == CompletionItemKind::TEXT {
        return "text".to_string();
    }
    if kind == CompletionItemKind::METHOD {
        return "method".to_string();
    }
    if kind == CompletionItemKind::FUNCTION {
        return "function".to_string();
    }
    if kind == CompletionItemKind::CONSTRUCTOR {
        return "constructor".to_string();
    }
    if kind == CompletionItemKind::FIELD {
        return "field".to_string();
    }
    if kind == CompletionItemKind::VARIABLE {
        return "variable".to_string();
    }
    if kind == CompletionItemKind::CLASS {
        return "class".to_string();
    }
    if kind == CompletionItemKind::INTERFACE {
        return "interface".to_string();
    }
    if kind == CompletionItemKind::MODULE {
        return "module".to_string();
    }
    if kind == CompletionItemKind::PROPERTY {
        return "property".to_string();
    }
    if kind == CompletionItemKind::UNIT {
        return "unit".to_string();
    }
    if kind == CompletionItemKind::VALUE {
        return "value".to_string();
    }
    if kind == CompletionItemKind::ENUM {
        return "enum".to_string();
    }
    if kind == CompletionItemKind::KEYWORD {
        return "keyword".to_string();
    }
    if kind == CompletionItemKind::SNIPPET {
        return "snippet".to_string();
    }
    if kind == CompletionItemKind::COLOR {
        return "color".to_string();
    }
    if kind == CompletionItemKind::FILE {
        return "file".to_string();
    }
    if kind == CompletionItemKind::REFERENCE {
        return "reference".to_string();
    }
    if kind == CompletionItemKind::FOLDER {
        return "folder".to_string();
    }
    if kind == CompletionItemKind::ENUM_MEMBER {
        return "enum_member".to_string();
    }
    if kind == CompletionItemKind::CONSTANT {
        return "constant".to_string();
    }
    if kind == CompletionItemKind::STRUCT {
        return "struct".to_string();
    }
    if kind == CompletionItemKind::EVENT {
        return "event".to_string();
    }
    if kind == CompletionItemKind::OPERATOR {
        return "operator".to_string();
    }
    if kind == CompletionItemKind::TYPE_PARAMETER {
        return "type_parameter".to_string();
    }
    // Unknown / custom kind — fall back to the Debug representation
    // (which the `lsp_enum!` macro renders as `CompletionItemKind(N)`
    // for unrecognized integer values).
    format!("kind({kind:?})")
}

impl LspOperations {
    pub async fn completion(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
        trigger_kind: Option<CompletionTriggerKind>,
        trigger_char: Option<String>,
    ) -> Result<Vec<CompletionItem>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = url_to_uri(&url::Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: trigger_kind.map(|kind| CompletionContext {
                trigger_kind: kind,
                trigger_character: trigger_char,
            }),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/completion", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<CompletionItem> =
            match serde_json::from_value::<CompletionList>(resp.clone()) {
                Ok(list) => list.items,
                Err(_) => serde_json::from_value(resp).unwrap_or_default(),
            };
        Ok(items)
    }

    /// Bounded, read-only `textDocument/completion` returning typed
    /// [`CompletionCandidate`] DTOs. Capability-gated: returns
    /// [`LspError::Unavailable`] when the server does not advertise a
    /// completion provider.
    ///
    /// The output is truncated to at most `max_candidates` items,
    /// preserving server order (no client-side sort). Raw completion
    /// edit payloads (`textEdit`, `additionalTextEdits`, `command`)
    /// are stripped — this surface is read-only and must never apply
    /// edits. `detail` and `insert_text_preview` are each capped at
    /// [`COMPLETION_DETAIL_MAX_CHARS`] characters.
    pub async fn completion_bounded(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
        trigger_kind: Option<CompletionTriggerKind>,
        trigger_char: Option<String>,
        max_candidates: usize,
    ) -> Result<Vec<CompletionCandidate>, LspError> {
        self.require_capability(
            file_path,
            crate::capability::LspSemanticOperation::Completion,
        )
        .await?;
        let items = self
            .completion(file_path, line, column, trigger_kind, trigger_char)
            .await?;
        Ok(items
            .iter()
            .take(max_candidates)
            .map(CompletionCandidate::from_completion_item)
            .collect())
    }

    /// Read-only `textDocument/semanticTokens/full` returning typed
    /// [`DecodedSemanticToken`] DTOs with legend-resolved type and
    /// modifier names. Capability-gated: returns
    /// [`LspError::Unavailable`] when the server does not advertise a
    /// semantic-tokens provider.
    ///
    /// The output is truncated to at most `max_tokens` decoded
    /// tokens. Token type / modifier indexes that exceed the legend
    /// are reported as [`LspError::RequestFailed`]; this is the
    /// structured fallback for misbehaving servers rather than a
    /// silent drop.
    pub async fn semantic_tokens(
        &self,
        file_path: &Path,
        max_tokens: usize,
    ) -> Result<Vec<DecodedSemanticToken>, LspError> {
        self.require_capability(
            file_path,
            crate::capability::LspSemanticOperation::SemanticTokens,
        )
        .await?;
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = url_to_uri(&url::Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/semanticTokens/full", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let tokens: SemanticTokens = match serde_json::from_value(resp) {
            Ok(t) => t,
            Err(e) => {
                trace!("failed to parse semanticTokens response: {}", e);
                return Err(LspError::RequestFailed(format!(
                    "malformed semanticTokens response: {e}"
                )));
            }
        };

        if tokens.data.is_empty() {
            return Ok(Vec::new());
        }

        let legend = self
            .capability_snapshot_for_file(file_path)
            .await
            .and_then(|snap| snap.details.semantic_token_legend)
            .ok_or_else(|| {
                LspError::RequestFailed(
                    "semantic token legend unavailable for this client".to_string(),
                )
            })?;

        let decoded = super::semantic_tokens::decode_semantic_tokens(&tokens.data, &legend)?;
        Ok(decoded.into_iter().take(max_tokens).collect())
    }

    /// Read-only `textDocument/signatureHelp` returning a typed
    /// [`SignatureHelpSummary`] DTO. Capability-gated.
    pub async fn signature_help_typed(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Option<super::signature::SignatureHelpSummary>, LspError> {
        self.require_capability(
            file_path,
            crate::capability::LspSemanticOperation::SignatureHelp,
        )
        .await?;
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = url_to_uri(&url::Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            context: None,
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/signatureHelp", params)
            .await?;

        if resp.is_null() {
            return Ok(None);
        }

        let help: SignatureHelp = match serde_json::from_value(resp) {
            Ok(h) => h,
            Err(e) => {
                trace!("failed to parse signature help response: {}", e);
                return Ok(None);
            }
        };
        Ok(super::signature::SignatureHelpSummary::from_signature_help(
            &help,
        ))
    }

    /// Backwards-compatible string rendering of signature help.
    pub async fn signature_help(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Option<String>, LspError> {
        match self.signature_help_typed(file_path, line, column).await? {
            Some(summary) => Ok(Some(super::signature::format_signature_help_typed(
                &summary,
            ))),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completion_item(label: &str, kind: CompletionItemKind) -> CompletionItem {
        CompletionItem {
            label: label.to_string(),
            kind: Some(kind),
            ..Default::default()
        }
    }

    #[test]
    fn completion_candidate_preserves_label_kind_sort_filter() {
        let mut item = completion_item("foo", CompletionItemKind::FUNCTION);
        item.detail = Some("fn foo()".to_string());
        item.sort_text = Some("0001".to_string());
        item.filter_text = Some("foo".to_string());
        item.insert_text = Some("foo()".to_string());

        let cand = CompletionCandidate::from_completion_item(&item);
        assert_eq!(cand.label, "foo");
        assert_eq!(cand.kind.as_deref(), Some("function"));
        assert_eq!(cand.detail.as_deref(), Some("fn foo()"));
        assert_eq!(cand.sort_text.as_deref(), Some("0001"));
        assert_eq!(cand.filter_text.as_deref(), Some("foo"));
        assert_eq!(cand.insert_text_preview.as_deref(), Some("foo()"));
        assert!(!cand.deprecated);
    }

    #[test]
    fn completion_candidate_truncates_long_detail() {
        let mut item = completion_item("foo", CompletionItemKind::FUNCTION);
        item.detail = Some("x".repeat(COMPLETION_DETAIL_MAX_CHARS * 2));
        let cand = CompletionCandidate::from_completion_item(&item);
        let detail = cand.detail.expect("detail present");
        assert!(detail.ends_with('…'));
        assert!(detail.chars().count() <= COMPLETION_DETAIL_MAX_CHARS + 1);
    }

    #[test]
    fn completion_candidate_truncates_long_insert_text() {
        let mut item = completion_item("foo", CompletionItemKind::FUNCTION);
        item.insert_text = Some("y".repeat(COMPLETION_DETAIL_MAX_CHARS * 2));
        let cand = CompletionCandidate::from_completion_item(&item);
        let preview = cand.insert_text_preview.expect("preview present");
        assert!(preview.ends_with('…'));
        assert!(preview.chars().count() <= COMPLETION_DETAIL_MAX_CHARS + 1);
    }

    #[test]
    fn completion_candidate_deprecated_defaults_false_and_respects_true() {
        let item_none = completion_item("a", CompletionItemKind::FUNCTION);
        let cand_none = CompletionCandidate::from_completion_item(&item_none);
        assert!(!cand_none.deprecated);

        let mut item_true = completion_item("b", CompletionItemKind::FUNCTION);
        item_true.deprecated = Some(true);
        let cand_true = CompletionCandidate::from_completion_item(&item_true);
        assert!(cand_true.deprecated);

        let mut item_false = completion_item("c", CompletionItemKind::FUNCTION);
        item_false.deprecated = Some(false);
        let cand_false = CompletionCandidate::from_completion_item(&item_false);
        assert!(!cand_false.deprecated);
    }

    #[test]
    fn completion_bounded_truncates_to_max_candidates_preserving_order() {
        let items: Vec<CompletionItem> = vec![
            completion_item("alpha", CompletionItemKind::FUNCTION),
            completion_item("beta", CompletionItemKind::VARIABLE),
            completion_item("gamma", CompletionItemKind::CLASS),
            completion_item("delta", CompletionItemKind::METHOD),
            completion_item("epsilon", CompletionItemKind::ENUM),
        ];
        let max_candidates = 3;
        let bounded: Vec<CompletionCandidate> = items
            .iter()
            .take(max_candidates)
            .map(CompletionCandidate::from_completion_item)
            .collect();
        assert_eq!(bounded.len(), 3);
        assert_eq!(bounded[0].label, "alpha");
        assert_eq!(bounded[1].label, "beta");
        assert_eq!(bounded[2].label, "gamma");
        assert_eq!(bounded[0].kind.as_deref(), Some("function"));
        assert_eq!(bounded[1].kind.as_deref(), Some("variable"));
        assert_eq!(bounded[2].kind.as_deref(), Some("class"));
    }

    #[test]
    #[allow(clippy::useless_vec)]
    fn completion_bounded_max_candidates_zero_yields_empty() {
        let items = vec![
            completion_item("alpha", CompletionItemKind::FUNCTION),
            completion_item("beta", CompletionItemKind::VARIABLE),
        ];
        let bounded: Vec<CompletionCandidate> = items
            .iter()
            .take(0)
            .map(CompletionCandidate::from_completion_item)
            .collect();
        assert!(bounded.is_empty());
    }

    #[test]
    #[allow(clippy::useless_vec, clippy::iter_out_of_bounds)]
    fn completion_bounded_max_candidates_larger_than_items_returns_all() {
        let items = vec![completion_item("alpha", CompletionItemKind::FUNCTION)];
        let bounded: Vec<CompletionCandidate> = items
            .iter()
            .take(100)
            .map(CompletionCandidate::from_completion_item)
            .collect();
        assert_eq!(bounded.len(), 1);
    }

    // ---- completion_kind_to_string ----

    #[test]
    fn completion_kind_to_string_maps_known_kinds_lowercase() {
        assert_eq!(
            completion_kind_to_string(CompletionItemKind::FUNCTION),
            "function"
        );
        assert_eq!(
            completion_kind_to_string(CompletionItemKind::VARIABLE),
            "variable"
        );
        assert_eq!(
            completion_kind_to_string(CompletionItemKind::ENUM_MEMBER),
            "enum_member"
        );
        assert_eq!(
            completion_kind_to_string(CompletionItemKind::TYPE_PARAMETER),
            "type_parameter"
        );
        assert_eq!(completion_kind_to_string(CompletionItemKind::TEXT), "text");
    }

    #[test]
    fn completion_kind_to_string_falls_back_for_custom_kind() {
        let custom: CompletionItemKind =
            serde_json::from_str("9999").expect("deserialize custom kind");
        let s = completion_kind_to_string(custom);
        assert!(s.starts_with("kind("));
        assert!(s.ends_with(')'));
    }
}
