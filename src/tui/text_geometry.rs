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
//! [`classify_byte`] is the tree's word-segmentation policy, and it is
//! **three-way**: word content, punctuation, and whitespace. That split is
//! Helix's, and upstream's own test names it — *"alphanumeric.!,and.?=
//! punctuation are considered 'words' for the purposes of word motion"*. A
//! two-way "is this a word byte" predicate cannot express it: it has nowhere
//! to put punctuation, so punctuation reads as a separator and `e` stalls on
//! it. Word content is alphanumerics and `_`, with every non-ASCII byte
//! treated as word content so a multi-byte character is never split.
//!
//! The geometry lives here rather than in the editor so the composer inherits
//! the policy instead of re-deciding it.
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

/// The category a byte belongs to, for word motion.
///
/// Helix sorts characters into three buckets and treats a **category change**
/// as the word boundary. Upstream's own test is named for the rule: *"the
/// alphanumeric `.!,` and `.?=` punctuation are considered 'words' for the
/// purposes of word motion"*.
///
/// The split matters because punctuation is neither word content nor
/// whitespace. Under a two-way predicate it reads as a separator, so `e`
/// steps over a whole `.b.` region and then stalls on the letter it landed
/// on. Under three buckets a punctuation run is a word in its own right, and
/// `e` walks it like any other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordClass {
    /// Word content: ASCII alphanumerics, `_`, and every byte `>= 0x80`.
    Word,
    /// Punctuation: everything that is not word content and not whitespace.
    Punctuation,
    /// ASCII whitespace.
    Whitespace,
}

/// Classify `byte` for word motion.
///
/// The `>= 0x80` clause is what keeps a multi-byte character in one piece:
/// every byte of a UTF-8 sequence is word content, so a byte-at-a-time walk
/// cannot mistake the interior of a character for a boundary.
pub fn classify_byte(byte: u8) -> WordClass {
    if byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80 {
        WordClass::Word
    } else if byte.is_ascii_whitespace() {
        WordClass::Whitespace
    } else {
        WordClass::Punctuation
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
    classify_byte(byte) == WordClass::Word
}

/// A maximal run of one [`WordClass`] over the buffer.
///
/// The three offsets are distinct on purpose. `start` and `last` are
/// character boundaries a caret may sit on; `end` is the **exclusive** byte
/// end of the run and is a boundary too, but the caret may not rest there
/// when the run's last character is multi-byte — `last` is that character's
/// own start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WordRun {
    /// First byte of the run.
    pub start: usize,
    /// Start byte of the run's **last character**.
    pub last: usize,
    /// Exclusive end of the run.
    pub end: usize,
    /// The class every byte in the run belongs to.
    pub class: WordClass,
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

/// The run of one class that contains `offset`.
///
/// The run is expanded on both sides, so the answer is the same for every
/// offset inside it — which is what lets a word motion ask "which word am I
/// in" without also asking "where exactly am I".
fn category_run_at<S: TextSource + ?Sized>(source: &S, offset: usize) -> WordRun {
    let offset = clamp_offset(source, offset);
    let len = source.len_bytes();
    let class = classify_byte(source.byte_at(offset).unwrap_or(b'\n'));
    // Expand left over the same class.
    let mut start = offset;
    while start > 0 {
        let previous = prev_char_boundary(source, start);
        if previous >= start || classify_byte(source.byte_at(previous).unwrap_or(b'\n')) != class {
            break;
        }
        start = previous;
    }
    // Expand right over the same class, remembering the last character's start.
    //
    // `end` is the start of the first character **after** the run, so the
    // character that starts there is the one to test next. Advancing `end` to
    // the *end* of the last matching character instead would step over the
    // character that ends the run — on `日本語 text` that swallowed the space
    // and merged the two words into one run.
    let mut last = offset;
    let mut end = next_char_boundary(source, offset).unwrap_or(len);
    while end < len {
        if classify_byte(source.byte_at(end).unwrap_or(b'\n')) != class {
            break;
        }
        last = end;
        end = next_char_boundary(source, end).unwrap_or(len);
    }
    WordRun {
        start,
        last,
        end,
        class,
    }
}

/// The word run containing `offset`.
///
/// `None` when `offset` is on whitespace or past the end of the buffer — a
/// whitespace run is not a word, and a caret past the end is nowhere.
pub fn word_run_at<S: TextSource + ?Sized>(source: &S, offset: usize) -> Option<WordRun> {
    let offset = clamp_offset(source, offset);
    if offset >= source.len_bytes() {
        return None;
    }
    let run = category_run_at(source, offset);
    (run.class != WordClass::Whitespace).then_some(run)
}

/// The extent of the word a word motion landed on.
///
/// `(start, end)` is the run's byte span with `end` **exclusive**, so a caller
/// that wants the selected bytes asks for `start..end` and a caller that
/// wants a caret position asks for `start` or the run's `last`.
///
/// This is the helper the auto-selection needs: the motion already knows
/// which run it landed on, and the selection is that run's extent. One
/// function answers both, so the caret and the highlight can never disagree
/// about which word was jumped to.
pub fn word_bounds<S: TextSource + ?Sized>(source: &S, offset: usize) -> Option<(usize, usize)> {
    word_run_at(source, offset).map(|run| (run.start, run.end))
}

/// The maximal run of **word content** containing `offset`.
///
/// `(start, end)` is the run's byte span with `end` **exclusive**.
///
/// This is the word the `w` **text object** names, and it is *not* the word
/// the `w` **motion** moves over. The motion's word is a run of word content
/// **or punctuation** — Helix's category model — so `a.b` is one
/// motion-word. The object's word is word content only, so `a.b` is two
/// object-words, and taking one of them does not take the dot with it.
///
/// `None` when `offset` is not on word content.
pub fn word_content_run_at<S: TextSource + ?Sized>(
    source: &S,
    offset: usize,
) -> Option<(usize, usize)> {
    let offset = clamp_offset(source, offset);
    if offset >= source.len_bytes() || !is_word_byte(source.byte_at(offset).unwrap_or(0)) {
        return None;
    }
    // Expand left over word content.
    let mut start = offset;
    while start > 0 {
        let previous = prev_char_boundary(source, start);
        if previous >= start || !is_word_byte(source.byte_at(previous).unwrap_or(0)) {
            break;
        }
        start = previous;
    }
    // Expand right over word content.
    let len = source.len_bytes();
    let mut end = next_char_boundary(source, offset).unwrap_or(len);
    while end < len {
        if !is_word_byte(source.byte_at(end).unwrap_or(0)) {
            break;
        }
        end = next_char_boundary(source, end).unwrap_or(len);
    }
    Some((start, end))
}

/// The next run of **word content** at or after `offset`.
///
/// `None` when the buffer holds no word content from `offset` onwards.
///
/// The text object's counterpart to [`word_content_run_at`]: a caret on a
/// separator names the next word, the same rule the `e` motion uses to step
/// onto one.
pub fn next_word_content_run<S: TextSource + ?Sized>(
    source: &S,
    offset: usize,
) -> Option<(usize, usize)> {
    let len = source.len_bytes();
    let mut cursor = clamp_offset(source, offset);
    while cursor < len {
        if is_word_byte(source.byte_at(cursor).unwrap_or(0)) {
            return word_content_run_at(source, cursor);
        }
        cursor = next_char_boundary(source, cursor).unwrap_or(len);
    }
    None
}

/// The first word run that begins strictly after `offset`.
///
/// A caret inside a run skips the rest of that run, so the answer is the
/// *next* word rather than the one the caret is already in. That is what
/// makes `w` advance.
pub fn next_word_run<S: TextSource + ?Sized>(source: &S, offset: usize) -> Option<WordRun> {
    let offset = clamp_offset(source, offset);
    let len = source.len_bytes();
    if offset >= len {
        return None;
    }
    // A caret on word content or punctuation continues past the rest of its
    // run; a caret on whitespace has nothing to skip.
    let mut cursor = match word_run_at(source, offset) {
        Some(run) => run.end,
        None => offset,
    };
    while cursor < len {
        let class = classify_byte(source.byte_at(cursor).unwrap_or(b'\n'));
        if class != WordClass::Whitespace {
            let run = category_run_at(source, cursor);
            return Some(run);
        }
        cursor = next_char_boundary(source, cursor).unwrap_or(len);
    }
    None
}

/// The last word run that begins strictly before `offset`.
///
/// A caret inside a run is answered with **that** run, so `b` lands on the
/// start of the word the caret is in. A caret at a run's start — or on
/// whitespace — is answered with the run before it, so `b` keeps walking
/// backwards through the words.
pub fn prev_word_run<S: TextSource + ?Sized>(source: &S, offset: usize) -> Option<WordRun> {
    let offset = clamp_offset(source, offset);
    if offset == 0 {
        return None;
    }
    let mut cursor = offset;
    while cursor > 0 {
        let previous = prev_char_boundary(source, cursor);
        if previous >= cursor {
            break;
        }
        cursor = previous;
        let class = classify_byte(source.byte_at(cursor).unwrap_or(b'\n'));
        if class != WordClass::Whitespace {
            return Some(category_run_at(source, cursor));
        }
    }
    None
}

/// The word run a Helix `e` lands on.
///
/// The rule is Helix's: `e` moves to the **end** of the next word. A caret
/// inside a word is answered with that word; a caret already on a word's
/// last byte — or on whitespace — is answered with the word **after** it.
///
/// The second case is the whole of the "e stalls" defect. A motion that
/// returned the caret's own offset whenever it was already on the last byte
/// of a word froze there forever, and a held `e` stopped skipping. Upstream
/// does not stall: its own test asserts *"Basic forward motion from the end
/// of a word to the end of the next"*.
pub fn word_end_run<S: TextSource + ?Sized>(source: &S, offset: usize) -> Option<WordRun> {
    let offset = clamp_offset(source, offset);
    match word_run_at(source, offset) {
        // Strictly inside the run: this is the word `e` ends on.
        Some(run) if offset < run.last => Some(run),
        // Already on its last byte: `e` still has to advance, so the next
        // word is the target.
        Some(run) => next_word_run(source, run.last),
        // On whitespace: the next word is the target.
        None => next_word_run(source, offset),
    }
}

/// Start of the next word at or after `offset`.
///
/// Helix `w` — "Move next word start". The word is a run of word content or
/// punctuation, and whitespace runs are stepped over rather than stopped on.
pub fn word_forward<S: TextSource + ?Sized>(source: &S, offset: usize) -> usize {
    match next_word_run(source, offset) {
        Some(run) => run.start,
        None => buffer_content_end(source),
    }
}

/// Start of the previous word strictly before `offset`.
///
/// Helix `b` — "Move previous word start".
pub fn word_backward<S: TextSource + ?Sized>(source: &S, offset: usize) -> usize {
    match prev_word_run(source, offset) {
        Some(run) => run.start,
        // Nowhere to go: the caret stays where it is, which is Helix's
        // "a failed motion does not modify the range".
        None => clamp_offset(source, offset),
    }
}

/// Byte offset of the last character of the word `e` lands on.
///
/// Helix `e` — "Move next word end". The caret rests on the word's **last
/// character**, which is what makes the auto-selection read as "this word is
/// selected" rather than "this word and the character after it".
///
/// The word is a run of word content **or punctuation**, and the motion
/// always advances: a caret already on a word's last byte moves on to the
/// next word instead of standing still.
///
/// It steps **a whole character at a time** rather than probing `offset + 1`.
/// The probe was the original design on the reasoning that [`is_word_byte`]
/// treats any byte `>= 0x80` as content, so walking through the interior of a
/// multi-byte character would still classify it correctly — that reasoning is
/// false, because a one-byte read inside a multi-byte character is not
/// readable at all, so the interior could never be classified. Worse, the
/// final `.unwrap_or(cursor)` then had nowhere to advance to on the last
/// character of a document, which was a spin.
pub fn word_end<S: TextSource + ?Sized>(source: &S, offset: usize) -> usize {
    match word_end_run(source, offset) {
        Some(run) => run.last,
        // No word left to land on: the caret stays where it is.
        None => clamp_offset(source, offset),
    }
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
    fn an_underscore_is_word_content_and_a_punctuation_run_is_a_word_of_its_own() {
        let text = "foo_bar baz";
        // `foo_bar` is one word: the underscore is word content. The next
        // word starts after the space, at byte 8.
        assert_eq!(word_forward(text, 0), 8);
        assert_eq!(word_forward(text, 8), 11);
        assert_eq!(word_backward(text, 11), 8);
        // A hyphen is **punctuation**, and Helix counts a punctuation run as a
        // word of its own — so `w` lands on it rather than stepping over it.
        assert_eq!(word_forward("foo-bar", 0), 3);
        // And the run after the hyphen is the next word.
        assert_eq!(word_forward("foo-bar", 3), 4);
    }

    #[test]
    fn a_word_end_motion_never_stands_still() {
        // The defect: `e` returned the cursor's own offset whenever the cursor
        // already sat on a word's last byte, so a held `e` froze there.
        let text = "one two three";
        // From inside "one" it lands on the last character of "one".
        assert_eq!(word_end(text, 0), 2);
        // From that last character it must still advance — to the last
        // character of the *next* word.
        assert_eq!(word_end(text, 2), 6);
        assert_eq!(word_end(text, 6), 12);
        // And from whitespace it steps onto the next word.
        assert_eq!(word_end(text, 3), 6);
        // At the end of the buffer there is nowhere left to go.
        assert_eq!(word_end(text, 13), 13);
    }

    #[test]
    fn a_punctuation_run_is_one_word() {
        // Helix's own rule, named by upstream's test: punctuation counts as a
        // word for the purposes of word motion.
        let text = "a.b.c";
        // `e` walks the punctuation runs rather than stalling on them.
        assert_eq!(word_end(text, 0), 1);
        assert_eq!(word_end(text, 1), 2);
        assert_eq!(word_end(text, 2), 3);
        assert_eq!(word_end(text, 3), 4);
        // `w` lands on the start of the next run.
        assert_eq!(word_forward(text, 0), 1);
        assert_eq!(word_forward(text, 1), 2);
        // And a punctuation run's extent is the whole run, not one character.
        assert_eq!(word_bounds(text, 3), Some((3, 4)));
    }

    #[test]
    fn a_word_motion_lands_on_a_character_boundary() {
        // The run's last character is two bytes wide, so the caret has to rest
        // on the character's *start* rather than on the run's last byte.
        let text = "café au";
        assert_eq!(word_end(text, 0), 3);
        assert!(text.is_char_boundary(word_end(text, 0)));
        assert_eq!(word_bounds(text, 3), Some((0, 5)));
    }

    #[test]
    fn a_word_backward_motion_lands_on_the_start_of_a_word() {
        let text = "one two three";
        // From inside "two" it lands on the start of "two" ...
        assert_eq!(word_backward(text, 5), 4);
        // ... and from that start it moves on to the word before.
        assert_eq!(word_backward(text, 4), 0);
        // From whitespace it skips back to the word before the gap.
        assert_eq!(word_backward(text, 3), 0);
        // And at the very start there is nowhere to go.
        assert_eq!(word_backward(text, 0), 0);
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
