//! Protocol DTOs for controlled LSP mutation application.

use serde::{Deserialize, Serialize};

/// One already-normalized text patch in a reviewed LSP preview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LspPreviewPatchDto {
    pub path: String,
    pub patch: String,
    pub original_hash: String,
}

/// Explicitly authorized request to apply one reviewed LSP preview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LspPreviewApplyRequestDto {
    pub preview_id: String,
    pub preview_revision: u64,
    pub preview_digest: String,
    pub kind: String,
    pub title: String,
    pub provenance: String,
    pub workspace_id: String,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub patches: Vec<LspPreviewPatchDto>,
}

/// Result of a controlled LSP preview application.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LspPreviewApplyResultDto {
    pub preview_id: String,
    pub preview_revision: u64,
    pub preview_digest: String,
    pub kind: String,
    pub title: String,
    pub written_files: Vec<String>,
    pub checkpoint_id: String,
    #[serde(default)]
    pub synchronization_errors: Vec<String>,
}
