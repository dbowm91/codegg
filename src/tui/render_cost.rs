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

/// Count one task row copied.
pub(crate) fn count_task_row_build() {
    TASK_ROW_BUILDS.with(|count| count.set(count.get() + 1));
}
