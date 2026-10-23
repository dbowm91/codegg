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

/// The expandable part that owns a row.
///
/// The row producer records which rows belong to which part, so a viewport
/// row resolves to the thing under the cursor rather than to the message
/// that carries it. A message holding both a reasoning block and a tool call
/// used to make the tool call unreachable: the click asked "does this
/// message have reasoning?" first, and the reasoning arm won every time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowPart {
    /// A tool call's disclosure: its header row, the status row a pending
    /// or running call draws under it, and the body rows an expanded call
    /// shows.
    ToolCall { part_idx: usize },
    /// A reasoning block's disclosure: its header row and the body rows an
    /// expanded block shows.
    Reasoning { part_idx: usize },
}

/// A run of rows one part owns, in the producer's own pre-collapse
/// coordinates.
///
/// The producer emits the rows and says which run belongs to which part; the
/// cache collapses blank runs and translates the spans into the coordinates
/// of the rows that survive, so the recorded boundary is the boundary of
/// what is drawn and not of what was emitted.
#[derive(Clone, Copy, Debug)]
pub struct PartSpan {
    /// The part the run belongs to.
    pub part: RowPart,
    /// Index of the part's first row among the message's emitted rows.
    pub start: usize,
    /// How many rows the part emitted.
    pub count: usize,
}

#[derive(Clone, Default)]
pub struct MessageLayoutCache {
    /// Flattened, blank-collapsed rows in message order.
    rows: Vec<Line<'static>>,
    /// `(msg_idx, first row index, row count)` per message, in message order.
    message_offsets: Vec<(usize, usize, usize)>,
    /// `(msg_idx, part, first row index, row count)` per expandable part, in
    /// row order. A row no entry covers belongs to no expandable part: prose,
    /// a timestamp, a separator.
    part_offsets: Vec<(usize, RowPart, usize, usize)>,
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
        // The parts recorded for the dropped messages start at or after the
        // first dropped row, so the same watermark drops them.
        self.part_offsets
            .retain(|(_, _, start, _)| *start < new_rows);
        self.total_lines = new_rows;
        self.prev_blank = self.rows.last().is_some_and(is_blank_line);
    }

    /// Append the rows emitted for the message at `msg_idx`, collapsing runs
    /// of blank rows against whatever precedes them.
    ///
    /// `spans` says which run of the emitted rows belongs to which
    /// expandable part. The collapse can drop rows, so the spans are
    /// translated into the coordinates of the rows that survive: a part's
    /// recorded range is the range of what is drawn, which is the range a
    /// click has to resolve against.
    pub fn push_message(
        &mut self,
        msg_idx: usize,
        msg_rows: Vec<Line<'static>>,
        spans: Vec<PartSpan>,
    ) {
        let start = self.total_lines;
        let mut count = 0usize;
        // The row index each emitted row landed at, or `None` when the
        // collapse dropped it.
        let mut landed: Vec<Option<usize>> = Vec::with_capacity(msg_rows.len());
        for line in msg_rows {
            let blank = is_blank_line(&line);
            if blank && self.prev_blank {
                landed.push(None);
                continue;
            }
            self.prev_blank = blank;
            self.rows.push(line);
            landed.push(Some(start + count));
            count += 1;
        }
        self.message_offsets.push((msg_idx, start, count));
        self.total_lines += count;

        for span in spans {
            let first = span.start.min(landed.len());
            let end = span.start.saturating_add(span.count).min(landed.len());
            let kept: Vec<usize> = landed[first..end]
                .iter()
                .enumerate()
                .filter_map(|(offset, row)| row.map(|_| first + offset))
                .collect();
            let Some(&first_row) = kept.first() else {
                // Every row of this part was dropped by the collapse, so the
                // part owns nothing that can be clicked.
                continue;
            };
            let last_row = *kept.last().expect("kept is not empty");
            let part_start = landed[first_row].expect("a kept row landed");
            let part_end = landed[last_row].expect("a kept row landed");
            self.part_offsets
                .push((msg_idx, span.part, part_start, part_end - part_start + 1));
        }
    }

    /// Rows for the absolute row range `start..end`, clamped to the buffer.
    pub fn rows_from(&self, start: usize, end: usize) -> Vec<Line<'static>> {
        let start = start.min(self.total_lines);
        let end = end.min(self.total_lines).max(start);
        self.rows[start..end].to_vec()
    }

    /// Rows belonging to one message, from the offsets recorded when it was
    /// emitted.
    ///
    /// Read the same rows the renderer drew rather than re-deriving them, so a
    /// test asserting on what a message looks like cannot pass while the
    /// rendered row says something else. Test-only: production scroll and
    /// selection work from absolute row ranges.
    #[cfg(test)]
    pub fn rows_of_message(&self, msg_idx: usize) -> Vec<Line<'static>> {
        match self
            .message_offsets
            .iter()
            .find(|(idx, _, _)| *idx == msg_idx)
        {
            Some(&(_, start, count)) => self.rows_from(start, start + count),
            None => Vec::new(),
        }
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

    /// The expandable part that owns `target`, as `(msg_idx, part)`.
    ///
    /// `None` when the row belongs to no expandable part — prose, a
    /// timestamp, a separator — and when the row is past the end of the
    /// buffer. A row with no part is a row with nothing to expand, which is
    /// a different thing from a row whose part is collapsed.
    pub fn find_part_at_line(&self, target: usize) -> Option<(usize, RowPart)> {
        if self.part_offsets.is_empty() || target >= self.total_lines {
            return None;
        }
        let mut left = 0;
        let mut right = self.part_offsets.len();
        while left < right {
            let mid = left + (right - left) / 2;
            let (_, _, start, count) = self.part_offsets[mid];
            if start + count <= target {
                left = mid + 1;
            } else {
                right = mid;
            }
        }
        if left >= self.part_offsets.len() {
            return None;
        }
        let (msg_idx, part, _start, _count) = self.part_offsets[left];
        Some((msg_idx, part))
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
