//! Shared word and line geometry over a text buffer.
//!
//! The editor and the composer are two surfaces over the same idea: a text
//! buffer with a caret in it, and Helix's motions over that caret. Before this
//! module they each had their own idea of where a word ends, and the two
//! could disagree — the composer had no word motions at all, so "disagree"
//! meant "the composer had nothing". The first time a motion lands on both
//! surfaces, the rule has to be one rule, or the surfaces drift the moment
//! the policy changes.
//!
//! So the geometry lives here, over a [`TextSource`], and each surface
//! supplies an adapter:
//!
//! - the editor adapts [`DocumentSnapshot`], the daemon-authorized replica it
//!   reads on demand;
//! - the composer adapts its own `String`, which is a frontend-local buffer
//!   that is delivered only on submit.
//!
//! The adapters differ in every respect that matters — one is a replica with
//! revisions, the other is a plain string — and agree on the one thing that
//! matters: where the words are.
//!
//! ## The word policy
//!
//! [`is_word_byte`] is the tree's word-segmentation policy. It is the rule
//! Helix uses (alphanumerics and `_`, with every non-ASCII byte treated as
//! word content so a multi-byte character is never split), and it is here
//! rather than in the editor so the composer inherits it instead of
//! re-deciding it.
//!
//! The interactive terminal panel has a **different** rule on purpose: it
//! selects a whitespace-delimited *token* out of a build log, which is not
//! the same thing as a word. That rule lives in
//! [`is_whitespace_token_char`] next to this one, so the tree has one module
//! that owns word classification, and the two rules are visibly different on
//! purpose rather than accidentally divergent.

use codegg_document::DocumentSnapshot;

/// A text buffer the word and line geometry can be computed over.
///
/// The trait is deliberately small: it is the minimum a buffer has to be able
/// to answer for the geometry to work, and nothing more. A surface that
/// cannot answer these does not have a text buffer.
pub trait TextSource {
    /// Total length of the buffer in bytes.
    fn len_bytes(&self) -> usize;

    /// The bytes of `start..end`, or `None` when the range is out of bounds.
    fn read_range(&self, start: usize, end: usize) -> Option<String>;

    /// Whether `offset` sits on a UTF-8 character boundary.
    fn is_char_boundary(&self, offset: usize) -> bool;

    /// The number of lines in the buffer. Always at least one.
    fn len_lines(&self) -> usize;

    /// Byte offset where `line` starts, or `None` when there is no such line.
    fn line_start(&self, line: usize) -> Option<usize>;

    /// Byte offset just past the end of `line`, including its terminator.
    fn line_end(&self, line: usize) -> Option<usize>;

    /// The zero-based line containing `offset`.
    fn line_of(&self, offset: usize) -> usize;

    /// The byte at `offset`, or `None` past the end.
    ///
    /// The default reads a one-byte range. Sources that can answer without
    /// allocating should override it — the geometry calls this once per
    /// scanned byte.
    fn byte_at(&self, offset: usize) -> Option<u8> {
        self.read_range(offset, offset + 1)?
            .as_bytes()
            .first()
            .copied()
    }

    /// Byte offset of the end of `line`'s content, excluding its terminator.
    ///
    /// The terminator, if any, is `\n`, `\r`, or `\r\n` at the very end of the
    /// line.
    fn line_content_end(&self, line: usize) -> Option<usize> {
        let start = self.line_start(line)?;
        let end = self.line_end(line)?;
        if start >= end {
            return Some(start);
        }
        let mut tail_start = end.saturating_sub(2).max(start);
        while tail_start < end && !self.is_char_boundary(tail_start) {
            tail_start += 1;
        }
        let tail = self.read_range(tail_start, end)?;
        let terminator = if tail.ends_with("\r\n") {
            2
        } else if tail.ends_with('\n') || tail.ends_with('\r') {
            1
        } else {
            0
        };
        Some(end.saturating_sub(terminator))
    }
}

/// Whether `byte` is word content.
///
/// This is the tree's word-segmentation policy: ASCII alphanumerics, `_`, and
/// every byte `>= 0x80`. The last part is what makes the scan safe on
/// multi-byte characters — a byte inside a UTF-8 sequence is still word
/// content, so a byte-at-a-time walk cannot mistake the interior of a
/// character for a separator.
pub fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// Whether `ch` is part of a whitespace-delimited token.
///
/// The interactive terminal panel's rule, kept next to [`is_word_byte`] so the
/// tree has one module that owns word classification. It is a **different**
/// rule on purpose: the panel selects a token out of a build log, where
/// `foo-bar` is two tokens, while a word is `foo-bar` in one piece.
pub fn is_whitespace_token_char(ch: char) -> bool {
    !ch.is_whitespace()
}

/// Clamp `offset` into `0..=len_bytes` and onto a UTF-8 character boundary.
///
/// A non-boundary offset is at most three bytes past the preceding boundary
/// in valid UTF-8, so this walks back at most three steps.
pub fn clamp_offset<S: TextSource + ?Sized>(source: &S, offset: usize) -> usize {
    let mut candidate = offset.min(source.len_bytes());
    while candidate > 0 && !source.is_char_boundary(candidate) {
        candidate -= 1;
    }
    candidate
}

/// Next character boundary strictly after `offset`, or the end of the buffer.
///
/// Returns `None` only when `offset` is already the end.
pub fn next_char_boundary<S: TextSource + ?Sized>(source: &S, offset: usize) -> Option<usize> {
    let mut candidate = clamp_offset(source, offset).saturating_add(1);
    while candidate < source.len_bytes() {
        if source.is_char_boundary(candidate) {
            return Some(candidate);
        }
        candidate += 1;
    }
    None
}

/// Previous character boundary strictly before `offset`.
pub fn prev_char_boundary<S: TextSource + ?Sized>(source: &S, offset: usize) -> usize {
    let mut candidate = clamp_offset(source, offset);
    while candidate > 0 {
        candidate -= 1;
        if source.is_char_boundary(candidate) {
            return candidate;
        }
    }
    0
}

/// Zero-based line containing `offset`, clamped to a valid line.
pub fn cursor_line<S: TextSource + ?Sized>(source: &S, offset: usize) -> usize {
    let offset = clamp_offset(source, offset);
    source.line_of(offset)
}

/// Byte offset where `line` starts.
pub fn line_start<S: TextSource + ?Sized>(source: &S, line: usize) -> Option<usize> {
    source.line_start(line)
}

/// Byte offset of the end of `line`'s content, excluding its terminator.
///
/// The terminator, if any, is `\n`, `\r`, or `\r\n` at the very end of the
/// line.
pub fn line_content_end<S: TextSource + ?Sized>(source: &S, line: usize) -> Option<usize> {
    source.line_content_end(line)
}

/// Byte offset of the end of the last line, excluding any terminator.
pub fn buffer_content_end<S: TextSource + ?Sized>(source: &S) -> usize {
    let last = source.len_lines().saturating_sub(1);
    source
        .line_content_end(last)
        .unwrap_or_else(|| source.len_bytes())
}

/// Start of the next word at or after `offset`.
pub fn word_forward<S: TextSource + ?Sized>(source: &S, offset: usize) -> usize {
    let mut cursor = clamp_offset(source, offset);
    let end = source.len_bytes();
    while cursor < end {
        if !is_word_byte(source.byte_at(cursor).unwrap_or(0)) {
            cursor += 1;
            continue;
        }
        while cursor < end && is_word_byte(source.byte_at(cursor).unwrap_or(0)) {
            cursor = next_char_boundary(source, cursor).unwrap_or(end);
        }
        if cursor >= end {
            return buffer_content_end(source);
        }
        // Skip the separator run, then report the next word start.
        while cursor < end && !is_word_byte(source.byte_at(cursor).unwrap_or(0)) {
            cursor = next_char_boundary(source, cursor).unwrap_or(end);
        }
        return cursor;
    }
    buffer_content_end(source)
}

/// Start of the previous word strictly before `offset`.
pub fn word_backward<S: TextSource + ?Sized>(source: &S, offset: usize) -> usize {
    let mut cursor = clamp_offset(source, offset);
    while cursor > 0 {
        let previous = prev_char_boundary(source, cursor);
        if is_word_byte(source.byte_at(previous).unwrap_or(0)) {
            break;
        }
        cursor = previous;
    }
    while cursor > 0 {
        let previous = prev_char_boundary(source, cursor);
        if !is_word_byte(source.byte_at(previous).unwrap_or(0)) {
            break;
        }
        cursor = previous;
    }
    cursor
}

/// Byte offset of the last byte of the word at or after `offset`.
///
/// Helix `e`. When the cursor is not already on word bytes it first steps to
/// the next word, so `e` from whitespace lands on a word rather than standing
/// still.
///
/// It steps **a whole character at a time** rather than probing `offset + 1`.
/// The probe was the original design on the reasoning that [`is_word_byte`]
/// treats any byte `>= 0x80` as content, so walking through the interior of a
/// multi-byte character would still classify it correctly — that reasoning is
/// false, because a one-byte read inside a multi-byte character is not
/// readable at all, so the interior could never be classified. Worse, the
/// final `.unwrap_or(cursor)` then had nowhere to advance to on the last
/// character of a document, which is a spin.
pub fn word_end<S: TextSource + ?Sized>(source: &S, offset: usize) -> usize {
    let end = source.len_bytes();
    let mut cursor = clamp_offset(source, offset);
    if !is_word_byte(source.byte_at(cursor).unwrap_or(0)) {
        cursor = word_forward(source, cursor);
        if cursor >= end || !is_word_byte(source.byte_at(cursor).unwrap_or(0)) {
            return buffer_content_end(source);
        }
    }
    while let Some(next) = next_char_boundary(source, cursor) {
        match source.byte_at(next) {
            Some(byte) if is_word_byte(byte) => cursor = next,
            _ => break,
        }
    }
    cursor
}

/// Move `offset` by `delta` lines, preserving the display column as closely
/// as the target line allows.
///
/// `desired_column` is the column a vertical run should aim for, which is what
/// makes `j` onto a short line and back up return to the original column
/// instead of the short line's. `None` means "no run in progress", so the
/// current cursor column is used and becomes the target.
pub fn move_vertical<S: TextSource + ?Sized>(
    source: &S,
    offset: usize,
    delta: isize,
    desired_column: Option<usize>,
) -> usize {
    let offset = clamp_offset(source, offset);
    let last_line = source.len_lines().saturating_sub(1);
    let current = cursor_line(source, offset);
    let column = desired_column
        .unwrap_or_else(|| offset.saturating_sub(source.line_start(current).unwrap_or(0)));
    let target = if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as usize).min(last_line)
    };
    let start = source.line_start(target).unwrap_or(0);
    let end = source.line_content_end(target).unwrap_or(start);
    clamp_offset(source, (start + column).min(end))
}

// ── Adapters ──────────────────────────────────────────────────────────────

impl TextSource for DocumentSnapshot {
    fn len_bytes(&self) -> usize {
        self.len_bytes()
    }

    fn read_range(&self, start: usize, end: usize) -> Option<String> {
        self.read_bytes(start, end).ok()
    }

    fn is_char_boundary(&self, offset: usize) -> bool {
        self.is_byte_boundary(offset)
    }

    fn len_lines(&self) -> usize {
        self.len_lines()
    }

    fn line_start(&self, line: usize) -> Option<usize> {
        self.line_range(line).ok().map(|range| range.start)
    }

    fn line_end(&self, line: usize) -> Option<usize> {
        self.line_range(line).ok().map(|range| range.end)
    }

    fn line_of(&self, offset: usize) -> usize {
        self.byte_to_position(offset)
            .map(|position| position.line)
            .unwrap_or_else(|_| self.len_lines().saturating_sub(1))
    }

    /// The **first** byte of the character at `offset`, not the byte at
    /// `offset`.
    ///
    /// `read_bytes(offset, offset + 1)` needs a character boundary at *both*
    /// ends, so it is `Err` for the first byte of a multi-byte character as
    /// much as for its interior. Reading it that way classified every
    /// non-ASCII character as a separator, and a scan walked straight past
    /// every one of them: on `日本語 text`, `w` from byte 0 landed on the end
    /// of the document instead of on `text`. Reading the whole character and
    /// taking its first byte classifies it as word content, which is what
    /// `is_word_byte`'s `>= 0x80` clause is for.
    fn byte_at(&self, offset: usize) -> Option<u8> {
        if !self.is_byte_boundary(offset) {
            return None;
        }
        let end = next_char_boundary(self, offset).unwrap_or(self.len_bytes());
        self.read_bytes(offset, end)
            .ok()
            .and_then(|text| text.as_bytes().first().copied())
    }
}

impl TextSource for str {
    fn len_bytes(&self) -> usize {
        self.len()
    }

    fn read_range(&self, start: usize, end: usize) -> Option<String> {
        self.get(start..end).map(|slice| slice.to_string())
    }

    fn is_char_boundary(&self, offset: usize) -> bool {
        str::is_char_boundary(self, offset)
    }

    fn len_lines(&self) -> usize {
        // A buffer always has at least one line, and a trailing newline
        // opens one more — the same count the composer's widget reports.
        self.split('\n').count()
    }

    fn line_start(&self, line: usize) -> Option<usize> {
        if line == 0 {
            return Some(0);
        }
        if line >= self.len_lines() {
            return None;
        }
        let mut seen = 0usize;
        for (index, byte) in self.bytes().enumerate() {
            if byte == b'\n' {
                seen += 1;
                if seen == line {
                    return Some(index + 1);
                }
            }
        }
        None
    }

    fn line_end(&self, line: usize) -> Option<usize> {
        if line >= self.len_lines() {
            return None;
        }
        let start = self.line_start(line)?;
        let rest = &self[start..];
        match rest.find('\n') {
            Some(index) => Some(start + index + 1),
            None => Some(self.len()),
        }
    }

    fn line_of(&self, offset: usize) -> usize {
        let offset = offset.min(self.len());
        self.as_bytes()[..offset]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
    }

    fn byte_at(&self, offset: usize) -> Option<u8> {
        // The byte at `offset`, but only when `offset` is on a character
        // boundary — the same rule the `DocumentSnapshot` adapter follows,
        // because a one-byte read *inside* a multi-byte character is not
        // readable at all. The two surfaces have to classify the same
        // offsets the same way, or "where is a word" gets two answers.
        if !str::is_char_boundary(self, offset) {
            return None;
        }
        self.as_bytes().get(offset).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The composer's buffer and the editor's replica must agree on where
    /// the words are. This is the whole point of the module: the same text,
    /// the same boundaries, whichever surface holds it.
    #[test]
    fn a_string_and_a_snapshot_agree_on_word_boundaries() {
        let text = "fn main() { let x = hello_world; }";
        let buffer = codegg_document::DocumentBuffer::new(text);
        let snapshot = buffer.snapshot();

        for offset in 0..=text.len() {
            assert_eq!(
                word_forward(text, offset),
                word_forward(&snapshot, offset),
                "word_forward disagreed at offset {offset}"
            );
            assert_eq!(
                word_backward(text, offset),
                word_backward(&snapshot, offset),
                "word_backward disagreed at offset {offset}"
            );
            assert_eq!(
                word_end(text, offset),
                word_end(&snapshot, offset),
                "word_end disagreed at offset {offset}"
            );
        }
    }

    /// The same agreement, on text that is **not** ASCII.
    ///
    /// The one-byte read is `Err` for the first byte of a multi-byte character
    /// as much as for its interior, so a scanner that read one byte at a time
    /// classified every non-ASCII character as a separator and walked straight
    /// past it. ASCII single-byte characters are the only kind that reads
    /// back, so the case was invisible to every test that existed.
    #[test]
    fn a_string_and_a_snapshot_agree_on_word_boundaries_in_non_ascii_text() {
        let text = "日本語 text café naïve";
        let buffer = codegg_document::DocumentBuffer::new(text);
        let snapshot = buffer.snapshot();

        for offset in 0..=text.len() {
            assert_eq!(
                word_forward(text, offset),
                word_forward(&snapshot, offset),
                "word_forward disagreed at offset {offset}"
            );
            assert_eq!(
                word_backward(text, offset),
                word_backward(&snapshot, offset),
                "word_backward disagreed at offset {offset}"
            );
            assert_eq!(
                word_end(text, offset),
                word_end(&snapshot, offset),
                "word_end disagreed at offset {offset}"
            );
        }
    }

    /// A multi-byte character is **word content**, not a separator.
    ///
    /// `w` from the start of `日本語 text` must land on `text`, not on the end
    /// of the document: the classifier reads the character's *first* byte,
    /// which is `>= 0x80` and therefore word content.
    #[test]
    fn a_multi_byte_character_is_a_word_and_not_a_separator() {
        let text = "日本語 text";
        // The three CJK characters are nine bytes and the space is the tenth,
        // so `text` starts at byte 10 — which is where `w` from byte 0 must
        // land. Before the fix it landed on the end of the document.
        assert_eq!(word_forward(text, 0), 10);
        assert_eq!(&text[word_forward(text, 0)..], "text");
        // And the same answer from the editor's replica.
        let buffer = codegg_document::DocumentBuffer::new(text);
        let snapshot = buffer.snapshot();
        assert_eq!(word_forward(&snapshot, 0), 10);
    }

    #[test]
    fn a_string_and_a_snapshot_agree_on_line_geometry() {
        let text = "alpha\nbeta\ngamma\n";
        let buffer = codegg_document::DocumentBuffer::new(text);
        let snapshot = buffer.snapshot();

        assert_eq!(text.len_lines(), snapshot.len_lines());
        for line in 0..text.len_lines() {
            assert_eq!(
                line_start(text, line),
                line_start(&snapshot, line),
                "line_start disagreed for line {line}"
            );
            assert_eq!(
                text.line_content_end(line),
                snapshot.line_content_end(line),
                "line_content_end disagreed for line {line}"
            );
        }
        for offset in 0..=text.len() {
            assert_eq!(
                cursor_line(text, offset),
                cursor_line(&snapshot, offset),
                "cursor_line disagreed at offset {offset}"
            );
        }
    }

    #[test]
    fn word_motions_find_the_same_boundaries_on_a_plain_string() {
        let text = "one two three";
        assert_eq!(word_forward(text, 0), 4);
        assert_eq!(word_forward(text, 4), 8);
        assert_eq!(word_backward(text, 8), 4);
        assert_eq!(word_backward(text, 4), 0);
        assert_eq!(word_end(text, 0), 2);
        assert_eq!(word_end(text, 4), 6);
    }

    #[test]
    fn an_underscore_is_word_content_and_a_separator_is_not() {
        let text = "foo_bar baz";
        // `foo_bar` is one word: the underscore is word content. The next
        // word starts after the space, at byte 8.
        assert_eq!(word_forward(text, 0), 8);
        assert_eq!(word_forward(text, 8), 11);
        assert_eq!(word_backward(text, 11), 8);
        // A hyphen is a separator under this policy, so it splits the word.
        assert_eq!(word_forward("foo-bar", 0), 4);
    }

    #[test]
    fn a_multibyte_character_is_never_split() {
        let text = "naïve café";
        // The `ï` is two bytes; the walk must land on the character, not
        // inside it. `naïve` is six bytes, and the space after it is the
        // seventh, so the next word starts at byte 7.
        let after = word_forward(text, 0);
        assert!(text.is_char_boundary(after), "the walk split a character");
        assert_eq!(after, 7);
        assert_eq!(&text[after..], "café");
    }

    #[test]
    fn vertical_movement_keeps_the_column_where_it_can() {
        let text = "abcdef\nghijkl\nmnopqr";
        // Down one line, keeping the column.
        assert_eq!(move_vertical(text, 2, 1, None), 9);
        // Up one line, keeping the column again.
        assert_eq!(move_vertical(text, 9, -1, None), 2);
        // And on to the third line.
        assert_eq!(move_vertical(text, 9, 1, None), 16);
    }

    #[test]
    fn vertical_movement_clamps_to_a_shorter_target_line() {
        let text = "long line here\nx\nanother line";
        // The first line is 14 bytes, so the second starts at 15 and holds a
        // single character. Column 5 does not exist there, so the caret lands
        // at that line's end — byte 16 — instead of past it.
        assert_eq!(move_vertical(text, 5, 1, None), 16);
    }

    #[test]
    fn a_desired_column_overrides_the_cursors_own_column() {
        let text = "abcdef\nghijkl\nmnopqr";
        // The caret is at column 0 of the first line, but a run in progress
        // wants column 3 — so the target is column 3 of the second line (which
        // starts at byte 7), and coming back lands on column 3 of the first
        // rather than on column 0.
        assert_eq!(move_vertical(text, 0, 1, Some(3)), 10);
        assert_eq!(move_vertical(text, 10, -1, Some(3)), 3);
    }

    #[test]
    fn the_terminal_panels_token_rule_is_a_different_rule_on_purpose() {
        // The panel's rule splits on whitespace, so `foo-bar` is one token.
        // The word policy treats the hyphen as a separator, so it is two
        // words. Both rules are the tree's; they are simply not the same
        // rule, and the panel selects tokens out of a build log.
        assert!(is_whitespace_token_char('-'));
        assert!(!is_word_byte(b'-'));
        assert!(is_word_byte(b'_'));
    }
}
