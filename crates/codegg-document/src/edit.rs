use crop::Rope;

use crate::{DocumentError, DocumentRevision, DocumentSnapshot, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentLimits {
    pub max_document_bytes: usize,
    pub max_edits: usize,
    pub max_inserted_bytes: usize,
}

impl Default for DocumentLimits {
    fn default() -> Self {
        Self {
            max_document_bytes: usize::MAX,
            max_edits: usize::MAX,
            max_inserted_bytes: usize::MAX,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: TextRange,
    pub insert: String,
}

impl TextEdit {
    pub fn new(range: std::ops::Range<usize>, insert: impl Into<String>) -> Self {
        Self {
            range: TextRange {
                start: range.start,
                end: range.end,
            },
            insert: insert.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextTransaction {
    pub edits: Vec<TextEdit>,
}

impl TextTransaction {
    pub fn new(edits: Vec<TextEdit>) -> Self {
        Self { edits }
    }

    pub(crate) fn validate(&self, text: &Rope, limits: DocumentLimits) -> Result<()> {
        if self.edits.len() > limits.max_edits {
            return Err(DocumentError::TooManyEdits);
        }
        if text.byte_len() > limits.max_document_bytes {
            return Err(DocumentError::DocumentTooLarge);
        }
        let mut inserted = 0usize;
        let mut prior_end = 0usize;
        for (index, edit) in self.edits.iter().enumerate() {
            let range = edit.range;
            if range.start > range.end {
                return Err(DocumentError::ReversedRange);
            }
            if range.end > text.byte_len() {
                return Err(DocumentError::OutOfBounds);
            }
            if !text.is_char_boundary(range.start) || !text.is_char_boundary(range.end) {
                return Err(DocumentError::InvalidBoundary);
            }
            if index > 0 && range.start < prior_end {
                return Err(DocumentError::OverlappingEdits);
            }
            prior_end = range.end;
            inserted = inserted
                .checked_add(edit.insert.len())
                .ok_or(DocumentError::TooMuchInsertedText)?;
        }
        if inserted > limits.max_inserted_bytes {
            return Err(DocumentError::TooMuchInsertedText);
        }
        let removed = self
            .edits
            .iter()
            .try_fold(0usize, |total, edit| {
                total.checked_add(edit.range.end - edit.range.start)
            })
            .ok_or(DocumentError::DocumentTooLarge)?;
        let final_size = text
            .byte_len()
            .checked_sub(removed)
            .and_then(|n| n.checked_add(inserted))
            .ok_or(DocumentError::DocumentTooLarge)?;
        if final_size > limits.max_document_bytes {
            return Err(DocumentError::DocumentTooLarge);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct AppliedTransaction {
    pub revision: DocumentRevision,
    pub snapshot: DocumentSnapshot,
    pub inverse: TextTransaction,
}
