mod code_actions;
mod completion;
mod formatting;
mod navigation;
mod overlay_ops;
pub(crate) mod rename;
mod semantic_tokens;
mod signature;

pub use code_actions::{
    select_source_action_edit, CodeActionPreview, CodeActionSummary, SourceActionPreviewKind,
    CODE_ACTION_SUMMARY_DEFAULT_MAX,
};
pub use completion::{completion_kind_to_string, CompletionCandidate, COMPLETION_DETAIL_MAX_CHARS};
pub use formatting::{
    document_end_position_utf16, sha256_hex, FormattingPreview, VersionedFileEvidence,
    FORMATTING_PREVIEW_MAX_DIFF_BYTES,
};
pub use navigation::{normalize_goto_response, normalize_workspace_symbol_response};
pub use overlay_ops::HierarchyDirection;
pub use rename::{
    PrepareRenameResult, RenamePreview, RENAME_PREVIEW_MAX_EDITS, RENAME_PREVIEW_MAX_FILES,
};
pub use semantic_tokens::{decode_semantic_tokens, DecodedSemanticToken};
pub use signature::{
    SignatureHelpSummary, SignatureInfoSummary, SignatureParameterSummary, SIGNATURE_DOC_MAX_CHARS,
};

use std::sync::Arc;

use crate::service::LspService;

pub struct LspOperations {
    pub(crate) service: Arc<LspService>,
}

impl LspOperations {
    pub fn new(service: Arc<LspService>) -> Self {
        Self { service }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::LspError;
    use lsp_types::*;

    #[test]
    fn test_hierarchy_direction_parse() {
        use overlay_ops::HierarchyDirection;
        assert_eq!(
            HierarchyDirection::parse(Some("incoming")).unwrap(),
            HierarchyDirection::Incoming
        );
        assert_eq!(
            HierarchyDirection::parse(Some("outgoing")).unwrap(),
            HierarchyDirection::Outgoing
        );
        assert_eq!(
            HierarchyDirection::parse(Some("both")).unwrap(),
            HierarchyDirection::Both
        );
        assert_eq!(
            HierarchyDirection::parse(None).unwrap(),
            HierarchyDirection::Both
        );
        assert!(HierarchyDirection::parse(Some("invalid")).is_err());
    }

    #[test]
    fn test_hierarchy_direction_parse_error() {
        use overlay_ops::HierarchyDirection;
        let err = HierarchyDirection::parse(Some("bad")).unwrap_err();
        match err {
            LspError::RequestFailed(msg) => {
                assert!(msg.contains("unsupported hierarchy direction"));
            }
            other => panic!("expected RequestFailed, got {other:?}"),
        }
    }
}
