//! How much work the TUI's render path does, counted rather than timed.
//!
//! The render path is a **cost** surface: what it recomputes per frame is
//! invisible until it is slow, and a wall-clock claim about a terminal
//! application is not reproducible on a shared machine. These counters are
//! the reproducible evidence: each one answers "how many times did the frame
//! do this expensive thing", which is a property of the code rather than of
//! the host it ran on.
//!
//! The counts are **per thread**. The render path runs on the thread that
//! draws the frame, and a test drives it on the test's own thread, so a
//! thread's counts are that thread's work — which is what makes the numbers
//! readable while the test binary runs its cases side by side. A process-wide
//! counter would answer "how many wraps happened while I was measuring",
//! which is a different question and an unanswerable one.
//!
//! The counters are not a benchmark. They count *operations*, not time, and
//! an operation count says nothing about how long the operation took — it
//! says how many of them there were, which is the thing a cache changes.

use std::cell::Cell;

thread_local! {
    /// Composer buffers wrapped.
    static COMPOSER_WRAPS: Cell<u64> = const { Cell::new(0) };
    /// Markdown documents parsed.
    static MARKDOWN_PARSES: Cell<u64> = const { Cell::new(0) };
    /// Syntax-highlight scans.
    static HIGHLIGHT_SCANS: Cell<u64> = const { Cell::new(0) };
    /// Task rows copied for the tasks overlay.
    static TASK_ROW_BUILDS: Cell<u64> = const { Cell::new(0) };
    /// Full task lists derived for the tasks overlay.
    static TASK_LIST_BUILDS: Cell<u64> = const { Cell::new(0) };
    /// Whole-frame buffers retained for a later drag.
    static FRAME_BUFFER_CLONES: Cell<u64> = const { Cell::new(0) };
    /// Sidebar projection populations.
    static SIDEBAR_POPULATIONS: Cell<u64> = const { Cell::new(0) };
    /// Sidebar content line models built.
    static SIDEBAR_LINE_BUILDS: Cell<u64> = const { Cell::new(0) };
    /// Subagent tabs cloned.
    static SUBAGENT_TAB_CLONES: Cell<u64> = const { Cell::new(0) };
    /// Composer hint lines built.
    static COMPOSER_HINT_BUILDS: Cell<u64> = const { Cell::new(0) };
    /// Status-bar summaries built.
    static STATUS_SUMMARY_BUILDS: Cell<u64> = const { Cell::new(0) };
    /// Transcript watermark scans.
    static WATERMARK_SCANS: Cell<u64> = const { Cell::new(0) };
    /// File-tree row vocabularies rebuilt (M002).
    static FILE_TREE_ROW_BUILDS: Cell<u64> = const { Cell::new(0) };
    /// File-tree row lists filtered (M002).
    static FILE_TREE_FILTER_PASSES: Cell<u64> = const { Cell::new(0) };
    /// Chat row models rebuilt (M003).
    static CHAT_ROW_BUILDS: Cell<u64> = const { Cell::new(0) };
    /// Chat message bodies wrapped (M003).
    static CHAT_WRAP_PASSES: Cell<u64> = const { Cell::new(0) };
    /// Chat unread counts derived by scanning the window (M003).
    static CHAT_UNREAD_SCANS: Cell<u64> = const { Cell::new(0) };
    /// Parsed hunks copied by the change review (M003).
    static REVIEW_HUNK_COPIES: Cell<u64> = const { Cell::new(0) };
    /// Re-assemblies of the streaming tail's rows (M004).
    static TAIL_ROW_COPIES: Cell<u64> = const { Cell::new(0) };
    /// Message-part rows copied into the transcript (M004).
    static PART_ROW_COPIES: Cell<u64> = const { Cell::new(0) };
    /// Payload parses that build a tool call's displayed body (M004).
    static TOOL_BODY_BUILDS: Cell<u64> = const { Cell::new(0) };
    /// Spans emitted by the code-block wrapper (M004).
    static CODE_BLOCK_SPANS: Cell<u64> = const { Cell::new(0) };
    /// Content reads of a visible editor row (M005).
    static EDITOR_ROW_READS: Cell<u64> = const { Cell::new(0) };
    /// Content bytes those reads examined (M005).
    static EDITOR_ROW_BYTES: Cell<u64> = const { Cell::new(0) };
    /// Steps the editor's semantic-token scan took (M005).
    static EDITOR_TOKEN_SCAN_STEPS: Cell<u64> = const { Cell::new(0) };
    /// Lines the highlight cache's prefix check re-read (M005).
    static HIGHLIGHT_PREFIX_LINES: Cell<u64> = const { Cell::new(0) };
    /// Spans the styled wrap's lookup examined (M006).
    static WRAP_SPAN_RESCANS: Cell<u64> = const { Cell::new(0) };
    /// Display-width passes the styled wrap's item builder made (M006).
    static WRAP_WIDTH_MEASURES: Cell<u64> = const { Cell::new(0) };
}

/// The number of expensive operations the render path has performed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderCost {
    /// Composer buffers wrapped.
    pub composer_wraps: u64,
    /// Markdown documents parsed.
    pub markdown_parses: u64,
    /// Syntax-highlight scans.
    pub highlight_scans: u64,
    /// Task rows copied for the tasks overlay.
    pub task_row_builds: u64,
    /// Full task lists derived for the tasks overlay.
    pub task_list_builds: u64,
    /// Whole-frame buffers retained for a later drag.
    pub frame_buffer_clones: u64,
    /// Sidebar projection populations.
    pub sidebar_populations: u64,
    /// Sidebar content line models built.
    pub sidebar_line_builds: u64,
    /// Subagent tabs cloned.
    pub subagent_tab_clones: u64,
    /// Composer hint lines built.
    pub composer_hint_builds: u64,
    /// Status-bar summaries built.
    pub status_summary_builds: u64,
    /// Transcript watermark scans.
    pub watermark_scans: u64,
    /// File-tree row vocabularies rebuilt: one per flatten of the listing
    /// into the row list a surface draws.
    pub file_tree_row_builds: u64,
    /// File-tree row lists filtered: one per pass over a row list with a
    /// surface's filter.
    pub file_tree_filter_passes: u64,
    /// Chat row models rebuilt: one per miss of a chat surface's row cache,
    /// which is one full rebuild of that channel's wrapped rows (M003).
    pub chat_row_builds: u64,
    /// Chat message bodies wrapped: one per non-redacted body a row-model
    /// build passes through the shared wrap engine (M003).
    pub chat_wrap_passes: u64,
    /// Chat unread counts derived by scanning the message window (M003).
    /// The count is memoized on the window and the read marker, so a frame
    /// that applied nothing derives none.
    pub chat_unread_scans: u64,
    /// Parsed hunks copied by the change review (M003): one per `DiffHunk`
    /// clone. The review borrows its state's patch, so a frame copies none.
    pub review_hunk_copies: u64,
    /// Re-assemblies of the streaming tail's rows (M004): one per assembly of
    /// the blocks a change could have touched. The rows an unchanged tail
    /// serves are the rows the last assembly produced, so a frame that extended
    /// nothing assembles none; a token re-assembles the suffix from the block
    /// it grew, not the whole response.
    pub tail_row_copies: u64,
    /// Message-part rows copied into the transcript (M004): one per part a
    /// rebuild appends, from the part cache's borrow. It was two — the cache
    /// cloned the rows and the caller copied them again — and three under a
    /// search match.
    pub part_row_copies: u64,
    /// Tool-call **bodies** built for the transcript (M004). The body is built
    /// only under the disclosure's `expanded` gate, so a collapsed call — the
    /// default — builds none.
    ///
    /// This counts the body, not every read of the payload. A collapsed
    /// *structured* payload still performs exactly one count-only structural
    /// read to keep the header's `(+N lines)` honest, because for a structured
    /// payload that number **is** the parsed outline's row count and no
    /// parse-free exact count exists. That read builds no rows and is
    /// deliberately outside this counter; the name says so rather than
    /// claiming a parse-free collapsed path the code does not have.
    pub tool_body_builds: u64,
    /// Spans emitted by the code-block wrapper (M004): one per **style run**
    /// per row. It was one per character, which is what made an 80x100 code
    /// block eight thousand spans.
    pub code_block_spans: u64,
    /// Content reads of a visible editor row (M005): one per row the draw
    /// path asks the replica for its bytes.
    ///
    /// Two passes read the visible window — the viewport's horizontal clamp
    /// measures every visible line, and the draw reads every row — so an
    /// unchanged frame used to pay the height twice. The clamp is memoized,
    /// so a frame that changed neither the revision, the first line, the
    /// height nor the width reads each row once.
    pub editor_row_reads: u64,
    /// Content bytes those reads examined (M005).
    ///
    /// The per-row read is the horizontal window's bytes, not the logical
    /// line's: a minified line is read as far as the columns the reader can
    /// see. `MAX_RENDER_LINE_BYTES` still caps one row and
    /// `MAX_FRAME_SOURCE_BYTES` still caps the frame, so this counter
    /// measures the reduction and not the bound.
    pub editor_row_bytes: u64,
    /// Steps the editor's semantic-token scan took (M005): the binary
    /// search's probes plus the tokens it visited on the row.
    ///
    /// The scan is `O(log tokens + tokens on the row)`. It was
    /// `O(tokens)` — a linear filter over the whole per-document token
    /// vector for **every** visible row.
    pub editor_token_scan_steps: u64,
    /// Lines the highlight cache's prefix check re-read and re-hashed
    /// (M005).
    ///
    /// The check starts at the first line an edit could have touched rather
    /// than at line 0, so editing line 4,900 of a 5,000-line document
    /// verifies one line, not 4,900. An edit the presentation cannot place
    /// (a daemon-side change, a reload) still scans from the top: a missed
    /// change above the edit is a wrong colour, because the cache is the
    /// only source of a row's spans.
    pub highlight_prefix_lines: u64,
    /// Spans the styled wrap's span lookup examined (M006).
    ///
    /// One per span the lookup looked at while resolving a word's parts or a
    /// separator's style. The lookup advances a cursor over the source's
    /// spans, and `tokenize` emits its segments in source order, so a wrap
    /// examines each span a bounded number of times — `O(spans + words)`,
    /// down from a rescan from index 0 for every word and every separator,
    /// which was `O(words x spans)`.
    pub wrap_span_rescans: u64,
    /// Display-width passes the styled item builder made over a word's
    /// pieces (M006).
    ///
    /// One per piece, which is one per word when a word is a single span.
    /// `chunk_word` used to measure each piece again — and again for every
    /// fragment when it hard-broke one — after the builder had already
    /// measured it; it now accumulates the widths from the character walk it
    /// already performs, so the builder's pass is the only one left.
    pub wrap_width_measures: u64,
}

impl RenderCost {
    /// The difference between two snapshots.
    ///
    /// A difference that underflows is a snapshot that went backwards, which
    /// cannot happen to a monotonic counter; it is reported as zero rather
    /// than as a wrapped number, because a wrapped number would be a lie
    /// about the work that was done.
    pub fn since(&self, earlier: &RenderCost) -> RenderCost {
        RenderCost {
            composer_wraps: self.composer_wraps.saturating_sub(earlier.composer_wraps),
            markdown_parses: self.markdown_parses.saturating_sub(earlier.markdown_parses),
            highlight_scans: self.highlight_scans.saturating_sub(earlier.highlight_scans),
            task_row_builds: self.task_row_builds.saturating_sub(earlier.task_row_builds),
            task_list_builds: self
                .task_list_builds
                .saturating_sub(earlier.task_list_builds),
            frame_buffer_clones: self
                .frame_buffer_clones
                .saturating_sub(earlier.frame_buffer_clones),
            sidebar_populations: self
                .sidebar_populations
                .saturating_sub(earlier.sidebar_populations),
            sidebar_line_builds: self
                .sidebar_line_builds
                .saturating_sub(earlier.sidebar_line_builds),
            subagent_tab_clones: self
                .subagent_tab_clones
                .saturating_sub(earlier.subagent_tab_clones),
            composer_hint_builds: self
                .composer_hint_builds
                .saturating_sub(earlier.composer_hint_builds),
            status_summary_builds: self
                .status_summary_builds
                .saturating_sub(earlier.status_summary_builds),
            watermark_scans: self.watermark_scans.saturating_sub(earlier.watermark_scans),
            file_tree_row_builds: self
                .file_tree_row_builds
                .saturating_sub(earlier.file_tree_row_builds),
            file_tree_filter_passes: self
                .file_tree_filter_passes
                .saturating_sub(earlier.file_tree_filter_passes),
            chat_row_builds: self.chat_row_builds.saturating_sub(earlier.chat_row_builds),
            chat_wrap_passes: self
                .chat_wrap_passes
                .saturating_sub(earlier.chat_wrap_passes),
            chat_unread_scans: self
                .chat_unread_scans
                .saturating_sub(earlier.chat_unread_scans),
            review_hunk_copies: self
                .review_hunk_copies
                .saturating_sub(earlier.review_hunk_copies),
            tail_row_copies: self.tail_row_copies.saturating_sub(earlier.tail_row_copies),
            part_row_copies: self.part_row_copies.saturating_sub(earlier.part_row_copies),
            tool_body_builds: self
                .tool_body_builds
                .saturating_sub(earlier.tool_body_builds),
            code_block_spans: self
                .code_block_spans
                .saturating_sub(earlier.code_block_spans),
            editor_row_reads: self
                .editor_row_reads
                .saturating_sub(earlier.editor_row_reads),
            editor_row_bytes: self
                .editor_row_bytes
                .saturating_sub(earlier.editor_row_bytes),
            editor_token_scan_steps: self
                .editor_token_scan_steps
                .saturating_sub(earlier.editor_token_scan_steps),
            highlight_prefix_lines: self
                .highlight_prefix_lines
                .saturating_sub(earlier.highlight_prefix_lines),
            wrap_span_rescans: self
                .wrap_span_rescans
                .saturating_sub(earlier.wrap_span_rescans),
            wrap_width_measures: self
                .wrap_width_measures
                .saturating_sub(earlier.wrap_width_measures),
        }
    }
}

/// The counts as they are right now, on this thread.
pub fn snapshot() -> RenderCost {
    RenderCost {
        composer_wraps: COMPOSER_WRAPS.get(),
        markdown_parses: MARKDOWN_PARSES.get(),
        highlight_scans: HIGHLIGHT_SCANS.get(),
        task_row_builds: TASK_ROW_BUILDS.get(),
        task_list_builds: TASK_LIST_BUILDS.get(),
        frame_buffer_clones: FRAME_BUFFER_CLONES.get(),
        sidebar_populations: SIDEBAR_POPULATIONS.get(),
        sidebar_line_builds: SIDEBAR_LINE_BUILDS.get(),
        subagent_tab_clones: SUBAGENT_TAB_CLONES.get(),
        composer_hint_builds: COMPOSER_HINT_BUILDS.get(),
        status_summary_builds: STATUS_SUMMARY_BUILDS.get(),
        watermark_scans: WATERMARK_SCANS.get(),
        file_tree_row_builds: FILE_TREE_ROW_BUILDS.get(),
        file_tree_filter_passes: FILE_TREE_FILTER_PASSES.get(),
        chat_row_builds: CHAT_ROW_BUILDS.get(),
        chat_wrap_passes: CHAT_WRAP_PASSES.get(),
        chat_unread_scans: CHAT_UNREAD_SCANS.get(),
        review_hunk_copies: REVIEW_HUNK_COPIES.get(),
        tail_row_copies: TAIL_ROW_COPIES.get(),
        part_row_copies: PART_ROW_COPIES.get(),
        tool_body_builds: TOOL_BODY_BUILDS.get(),
        code_block_spans: CODE_BLOCK_SPANS.get(),
        editor_row_reads: EDITOR_ROW_READS.get(),
        editor_row_bytes: EDITOR_ROW_BYTES.get(),
        editor_token_scan_steps: EDITOR_TOKEN_SCAN_STEPS.get(),
        highlight_prefix_lines: HIGHLIGHT_PREFIX_LINES.get(),
        wrap_span_rescans: WRAP_SPAN_RESCANS.get(),
        wrap_width_measures: WRAP_WIDTH_MEASURES.get(),
    }
}

/// Count one composer wrap.
pub(crate) fn count_composer_wrap() {
    COMPOSER_WRAPS.with(|count| count.set(count.get() + 1));
}

/// Count one Markdown parse.
pub(crate) fn count_markdown_parse() {
    MARKDOWN_PARSES.with(|count| count.set(count.get() + 1));
}

/// Count one syntax-highlight scan.
pub(crate) fn count_highlight_scan() {
    HIGHLIGHT_SCANS.with(|count| count.set(count.get() + 1));
}

/// Count the task rows copied for the tasks overlay.
pub(crate) fn count_task_row_build(rows: u64) {
    TASK_ROW_BUILDS.with(|count| count.set(count.get() + rows));
}

/// Count one full task-list derivation for the tasks overlay.
pub(crate) fn count_task_list_build() {
    TASK_LIST_BUILDS.with(|count| count.set(count.get() + 1));
}

/// Count one whole-frame buffer retained for a later drag.
pub(crate) fn count_frame_buffer_clone() {
    FRAME_BUFFER_CLONES.with(|count| count.set(count.get() + 1));
}

/// Count one sidebar projection population.
pub(crate) fn count_sidebar_population() {
    SIDEBAR_POPULATIONS.with(|count| count.set(count.get() + 1));
}

/// Count one sidebar content line model build.
pub(crate) fn count_sidebar_line_build() {
    SIDEBAR_LINE_BUILDS.with(|count| count.set(count.get() + 1));
}

/// Count one subagent tab clone.
pub(crate) fn count_subagent_tab_clone() {
    SUBAGENT_TAB_CLONES.with(|count| count.set(count.get() + 1));
}

/// Count one composer hint line build.
pub(crate) fn count_composer_hint_build() {
    COMPOSER_HINT_BUILDS.with(|count| count.set(count.get() + 1));
}

/// Count one status-bar summary build.
pub(crate) fn count_status_summary_build() {
    STATUS_SUMMARY_BUILDS.with(|count| count.set(count.get() + 1));
}

/// Count one transcript watermark scan.
pub(crate) fn count_watermark_scan() {
    WATERMARK_SCANS.with(|count| count.set(count.get() + 1));
}

/// Count one rebuild of the file tree's row vocabulary.
///
/// The build is the **flatten**: turning the tree's nodes and expansion set
/// into the row list a surface draws. A cache that copies an already-built
/// row list is not a build, and a frame that reads the cached slice does not
/// count one.
pub(crate) fn count_file_tree_row_build() {
    FILE_TREE_ROW_BUILDS.with(|count| count.set(count.get() + 1));
}

/// Count one filter pass over a file tree's row list.
///
/// A pass is one application of a surface's filter to a row list, whether it
/// narrowed the list or copied it whole. The pair with
/// [`count_file_tree_row_build`] is deliberate: a surface can filter rows it
/// did not flatten (the `/files` box filters the adopted structure), so a
/// single count could not tell the two costs apart.
pub(crate) fn count_file_tree_filter_pass() {
    FILE_TREE_FILTER_PASSES.with(|count| count.set(count.get() + 1));
}

/// Count one rebuild of a chat surface's row model.
///
/// A **build** is the whole flatten: every retained message of the channel
/// wrapped again and styled again. A cache that hands the already-built rows
/// back is not a build, and a frame that reads the cached rows does not count
/// one.
pub(crate) fn count_chat_row_build() {
    CHAT_ROW_BUILDS.with(|count| count.set(count.get() + 1));
}

/// Count one chat message body wrapped during a row-model build.
///
/// A redacted body is not wrapped — it is drawn as `[redacted]` — so a build
/// performs one wrap pass per non-redacted message, which is what separates
/// this counter from [`count_chat_row_build`].
pub(crate) fn count_chat_wrap_pass() {
    CHAT_WRAP_PASSES.with(|count| count.set(count.get() + 1));
}

/// Count one derivation of a chat channel's unread count by scanning the
/// message window.
///
/// The count is memoized on the window's sequence, length and the read
/// marker, so a frame that applied nothing performs no scan. A scan is the
/// operation, not the read: a memo hit is free and counts nothing.
pub(crate) fn count_chat_unread_scan() {
    CHAT_UNREAD_SCANS.with(|count| count.set(count.get() + 1));
}

/// Count one parsed hunk copied by the change review.
///
/// The producer is attached to [`DiffHunk`](crate::tui::components::diff::DiffHunk)'s
/// `Clone` rather than to a call site: the review used to copy its whole
/// parsed patch into a fresh viewer on every frame, and a copy that came back
/// anywhere on that path would be counted without a second place to forget.
/// A hunk list that is moved or borrowed copies nothing and counts nothing.
pub(crate) fn count_review_hunk_copy() {
    REVIEW_HUNK_COPIES.with(|count| count.set(count.get() + 1));
}

/// Count one assembly of the streaming tail's rows.
///
/// The producer sits in the tail cache's **assembly** — the step that folds
/// the rendered blocks into the rows a message draws — and not at the call site
/// that appends them into the transcript. That call site's copy is the single
/// materialization into the row index, and it is what remains after M004; the
/// work this counter measures is the assembly the fast path used to repeat on
/// every rebuild and no longer does.
pub(crate) fn count_tail_row_copy() {
    TAIL_ROW_COPIES.with(|count| count.set(count.get() + 1));
}

/// Count one copy of a message part's rows into the transcript.
///
/// One per part a rebuild appends, whether the part's rows came from the
/// cache or from a fresh parse: the cache owns its rows and the transcript
/// owns its own, so one copy per part is the floor. The counter exists to
/// catch the *second* copy — the cache handing back an owned `Vec` that the
/// caller then copied again — coming back.
pub(crate) fn count_part_row_copy() {
    PART_ROW_COPIES.with(|count| count.set(count.get() + 1));
}

/// Count one tool-call body built for the transcript.
///
/// A collapsed call draws no body, so it builds none; an expanded call builds
/// one. The **header's** line count is not this counter: it is a count-only
/// structural read that builds no formatted rows (see
/// `tool_output_body_line_count` in `components/messages.rs`), and it exists
/// because the disclosure model's displayed number must not change. For a
/// structured payload that read is a parse, so the collapsed path is
/// *body-free*, not parse-free — which is why this counter counts bodies.
pub(crate) fn count_tool_body_build() {
    TOOL_BODY_BUILDS.with(|count| count.set(count.get() + 1));
}

/// Count the spans a call to the code-block wrapper emitted.
///
/// The producer takes the count rather than incrementing once, because the
/// quantity the milestone is about is spans per block: one per style run per
/// row after M004, one per **character** before it.
pub(crate) fn count_code_block_spans(spans: u64) {
    CODE_BLOCK_SPANS.with(|count| count.set(count.get() + spans));
}

/// Count one content read of a visible editor row, and the bytes it examined.
///
/// The producer takes the read's byte length rather than incrementing once,
/// because the two quantities answer different halves of M005: a frame's
/// read *count* is what the clamp cache removes, and a row's read *size* is
/// what the window-bounded read removes. One call site cannot report the
/// second without the first.
pub(crate) fn count_editor_row_read(bytes: usize) {
    let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
    EDITOR_ROW_READS.with(|count| count.set(count.get() + 1));
    EDITOR_ROW_BYTES.with(|count| count.set(count.get().saturating_add(bytes)));
}

/// Count the steps one row's semantic-token scan took.
///
/// A step is one comparison against the binary search's sorted `line` field
/// or one token the row's range visited, so the count is the whole cost of
/// locating and walking the row's tokens — `O(log tokens + tokens on the
/// row)` — rather than only the part of it that touches a token.
pub(crate) fn count_editor_token_scan_steps(steps: u64) {
    EDITOR_TOKEN_SCAN_STEPS.with(|count| count.set(count.get() + steps));
}

/// Count the lines the highlight cache's prefix check re-read and re-hashed.
///
/// One per line the check examined, so the count is the prefix's length:
/// the distance from the seed the edit supplied to the first line whose
/// content differs.
pub(crate) fn count_highlight_prefix_lines(lines: u64) {
    HIGHLIGHT_PREFIX_LINES.with(|count| count.set(count.get() + lines));
}

/// Count one span the styled wrap's lookup examined.
///
/// A span is examined when the lookup reads its range to decide whether the
/// span covers the offset or range it is resolving. The producer sits in the
/// cursor's own advance, so a lookup that restarts from the front cannot be
/// counted as anything but the work it did.
pub(crate) fn count_wrap_span_rescan() {
    WRAP_SPAN_RESCANS.with(|count| count.set(count.get() + 1));
}

/// Count one display-width pass the styled wrap's item builder made.
///
/// One per piece it measures, not one per character: the producer wraps the
/// builder's single `visible_width` call, so the count falls exactly by the
/// passes a second measurement would have added.
pub(crate) fn count_wrap_width_measure() {
    WRAP_WIDTH_MEASURES.with(|count| count.set(count.get() + 1));
}
