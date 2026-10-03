//! Additive, bounded `document.v1` editor-document wire contract.

use serde::{Deserialize, Serialize};

pub const DOCUMENT_PROTOCOL_CAPABILITY: &str = "document.v1";
pub const MAX_DOCUMENT_TEXT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_DOCUMENT_EDITS: usize = 256;
pub const MAX_DOCUMENT_INSERT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentTextRangeDto {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentTextEditDto {
    pub range: DocumentTextRangeDto,
    pub insert: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DocumentTransactionDto {
    pub edits: Vec<DocumentTextEditDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentSnapshotDto {
    pub document_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub relative_path: String,
    pub revision: u64,
    pub text: String,
    pub dirty: bool,
    pub conflicted: bool,
    pub writer: bool,
    pub disk_base_digest: String,
}
