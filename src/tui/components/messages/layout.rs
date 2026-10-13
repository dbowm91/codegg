//! Row index for the chat viewport.
//!
//! The cache holds the rows the renderer actually emits and derives the
//! scroll offsets from them. Row counts used to be re-derived by
//! `estimate_msg_lines`, a second implementation of the same rules that had
//! drifted out of agreement with the emit loops: it counted wrapped rows for
//! collapsed tool output but only logical rows for `read` output, which is
//! rendered as pre-numbered code. Because the disagreement is per message it
//! accumulated down the buffer, so the further back you scrolled the further
//! the cached offset sat from the row actually drawn — long transcripts
//! clipped off the edge and most of the history became unreachable.
//!
//! Rows are cached flat rather than per message so that a partial rebuild
//! (streaming appends to the last message) can splice the rebuilt suffix in
//! place instead of re-emitting every earlier message.

use ratatui::text::Line;

#[derive(Clone, Default)]
pub struct MessageLayoutCache {
    /// Flattened, blank-collapsed rows in message order.
    rows: Vec<Line<'static>>,
    /// `(msg_idx, first row index, row count)` per message, in message order.
    message_offsets: Vec<(usize, usize, usize)>,
    total_lines: usize,
    /// Whether the row before the next appended row draws nothing. Carried
    /// across a message boundary so a trailing blank and a leading blank
    /// cannot stack into two.
    prev_blank: bool,
}

impl MessageLayoutCache {
    /// Drop every message at or after `msg_idx`, so the suffix can be rebuilt
    /// from fresh output. Rows before it are unaffected and stay valid.
    pub fn truncate_from(&mut self, msg_idx: usize) {
        let keep = msg_idx.min(self.message_offsets.len());
        let new_rows = if keep == 0 {
            0
        } else {
            self.message_offsets[keep - 1].1 + self.message_offsets[keep - 1].2
        };
        self.rows.truncate(new_rows);
        self.message_offsets.truncate(keep);
        self.total_lines = new_rows;
        self.prev_blank = self.rows.last().is_some_and(is_blank_line);
    }

    /// Append the rows emitted for the message at `msg_idx`, collapsing runs
    /// of blank rows against whatever precedes them.
    pub fn push_message(&mut self, msg_idx: usize, msg_rows: Vec<Line<'static>>) {
        let start = self.total_lines;
        let mut count = 0usize;
        for line in msg_rows {
            let blank = is_blank_line(&line);
            if blank && self.prev_blank {
                continue;
            }
            self.prev_blank = blank;
            self.rows.push(line);
            count += 1;
        }
        self.message_offsets.push((msg_idx, start, count));
        self.total_lines += count;
    }

    /// Rows for the absolute row range `start..end`, clamped to the buffer.
    pub fn rows_from(&self, start: usize, end: usize) -> Vec<Line<'static>> {
        let start = start.min(self.total_lines);
        let end = end.min(self.total_lines).max(start);
        self.rows[start..end].to_vec()
    }

    /// The message whose rows cover `target`, as `(msg_idx, row within msg)`.
    pub fn find_message_at_line(&self, target: usize) -> Option<(usize, usize)> {
        if self.message_offsets.is_empty() || target >= self.total_lines {
            return None;
        }
        let mut left = 0;
        let mut right = self.message_offsets.len();
        while left < right {
            let mid = left + (right - left) / 2;
            let (_, start, count) = self.message_offsets[mid];
            if start + count <= target {
                left = mid + 1;
            } else {
                right = mid;
            }
        }
        if left >= self.message_offsets.len() {
            return None;
        }
        let (msg_idx, start, _count) = self.message_offsets[left];
        Some((msg_idx, target - start))
    }

    pub fn total_lines(&self) -> usize {
        self.total_lines
    }

    pub fn get_offset(&self, msg_idx: usize) -> Option<usize> {
        self.message_offsets
            .get(msg_idx)
            .filter(|(idx, _, _)| *idx == msg_idx)
            .map(|(_, start, _)| *start)
    }
}

/// True when a row draws nothing. Shared with the row builders so the two
/// cannot disagree about what counts as blank.
pub(super) fn is_blank_line(line: &Line) -> bool {
    line.spans.is_empty()
        || line
            .spans
            .iter()
            .all(|s| s.content.is_empty() || s.content.chars().all(|c| c == ' '))
}
