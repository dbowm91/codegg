use thiserror::Error;

pub type Result<T, E = DocumentError> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DocumentError {
    #[error("edit range starts after it ends")]
    ReversedRange,
    #[error("edit range is outside the document")]
    OutOfBounds,
    #[error("edit range is not on UTF-8 character boundaries")]
    InvalidBoundary,
    #[error("transaction edits overlap or are not in ascending order")]
    OverlappingEdits,
    #[error("transaction exceeds the edit count limit")]
    TooManyEdits,
    #[error("transaction exceeds the inserted byte limit")]
    TooMuchInsertedText,
    #[error("document exceeds the byte limit")]
    DocumentTooLarge,
    #[error("document revision is exhausted")]
    RevisionExhausted,
    #[error("line or byte column is out of bounds")]
    InvalidPosition,
}
