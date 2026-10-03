use crop::Rope;

use crate::{DocumentError, DocumentLimits, Result, TextTransaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct DocumentRevision(u64);

impl DocumentRevision {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone)]
pub struct DocumentSnapshot {
    pub(crate) text: Rope,
    revision: DocumentRevision,
}

impl std::fmt::Debug for DocumentSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentSnapshot")
            .field("revision", &self.revision)
            .field("len_bytes", &self.len_bytes())
            .finish()
    }
}

impl DocumentSnapshot {
    pub fn revision(&self) -> DocumentRevision {
        self.revision
    }
    pub fn len_bytes(&self) -> usize {
        self.text.byte_len()
    }
    pub fn len_lines(&self) -> usize {
        self.text.line_len().saturating_add(1)
    }
    pub fn is_empty(&self) -> bool {
        self.len_bytes() == 0
    }
    pub fn to_string(&self) -> String {
        self.text.to_string()
    }
    pub fn is_byte_boundary(&self, offset: usize) -> bool {
        offset <= self.len_bytes() && self.text.is_char_boundary(offset)
    }

    pub fn read_bytes(&self, start: usize, end: usize) -> Result<String> {
        if start > end {
            return Err(DocumentError::ReversedRange);
        }
        if end > self.len_bytes() {
            return Err(DocumentError::OutOfBounds);
        }
        if !self.is_byte_boundary(start) || !self.is_byte_boundary(end) {
            return Err(DocumentError::InvalidBoundary);
        }
        Ok(self.text.byte_slice(start..end).to_string())
    }

    pub fn line_range(&self, line: usize) -> Result<std::ops::Range<usize>> {
        if line >= self.len_lines() {
            return Err(DocumentError::InvalidPosition);
        }
        let start = self.text.byte_of_line(line);
        let end = if line + 1 > self.text.line_len() {
            self.len_bytes()
        } else {
            self.text.byte_of_line(line + 1)
        };
        Ok(start..end)
    }

    pub fn byte_to_position(&self, offset: usize) -> Result<crate::BytePosition> {
        if offset > self.len_bytes() {
            return Err(DocumentError::OutOfBounds);
        }
        if !self.is_byte_boundary(offset) {
            return Err(DocumentError::InvalidBoundary);
        }
        let line = self.text.line_of_byte(offset);
        Ok(crate::BytePosition {
            line,
            byte_column: offset - self.text.byte_of_line(line),
        })
    }

    pub fn position_to_byte(&self, position: crate::BytePosition) -> Result<usize> {
        if position.line >= self.len_lines() {
            return Err(DocumentError::InvalidPosition);
        }
        let range = self.line_range(position.line)?;
        let line_text = self.text.byte_slice(range.clone()).to_string();
        let content_end = line_text
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .len();
        if position.byte_column > content_end {
            return Err(DocumentError::InvalidPosition);
        }
        let offset = range.start + position.byte_column;
        if !self.is_byte_boundary(offset) {
            return Err(DocumentError::InvalidBoundary);
        }
        Ok(offset)
    }
}

#[derive(Clone, Debug)]
pub struct DocumentBuffer {
    text: Rope,
    revision: DocumentRevision,
}

impl DocumentBuffer {
    pub fn new(text: impl AsRef<str>) -> Self {
        Self {
            text: Rope::from(text.as_ref()),
            revision: DocumentRevision::default(),
        }
    }
    pub fn revision(&self) -> DocumentRevision {
        self.revision
    }
    pub fn snapshot(&self) -> DocumentSnapshot {
        DocumentSnapshot {
            text: self.text.clone(),
            revision: self.revision,
        }
    }

    pub fn apply(
        &mut self,
        transaction: &TextTransaction,
        limits: DocumentLimits,
    ) -> Result<crate::AppliedTransaction> {
        transaction.validate(&self.text, limits)?;
        if transaction.edits.is_empty() {
            return Ok(crate::AppliedTransaction {
                revision: self.revision,
                snapshot: self.snapshot(),
                inverse: TextTransaction::default(),
            });
        }
        let next_revision = self
            .revision
            .0
            .checked_add(1)
            .ok_or(DocumentError::RevisionExhausted)?;
        let before = self.text.clone();
        let mut inverse = Vec::with_capacity(transaction.edits.len());
        let mut shift: isize = 0;
        for edit in &transaction.edits {
            let removed = before
                .byte_slice(edit.range.start..edit.range.end)
                .to_string();
            let post_start = edit
                .range
                .start
                .checked_add_signed(shift)
                .ok_or(DocumentError::OutOfBounds)?;
            let post_end = post_start
                .checked_add(edit.insert.len())
                .ok_or(DocumentError::OutOfBounds)?;
            inverse.push(crate::TextEdit::new(post_start..post_end, removed));
            let delta = isize::try_from(edit.insert.len())
                .map_err(|_| DocumentError::OutOfBounds)?
                - isize::try_from(edit.range.end - edit.range.start)
                    .map_err(|_| DocumentError::OutOfBounds)?;
            shift = shift.checked_add(delta).ok_or(DocumentError::OutOfBounds)?;
        }
        for edit in transaction.edits.iter().rev() {
            self.text
                .replace(edit.range.start..edit.range.end, &edit.insert);
        }
        self.revision = super::DocumentRevision::new(next_revision);
        let snapshot = self.snapshot();
        Ok(crate::AppliedTransaction {
            revision: self.revision,
            snapshot,
            inverse: TextTransaction { edits: inverse },
        })
    }
}
