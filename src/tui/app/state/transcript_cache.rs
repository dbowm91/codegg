//! Bounded per-tab cache for a delegated run's recorded transcript.
//!
//! The cache is the TUI's half of the M003 live follow. It holds exactly
//! what the daemon authorized for **one** durable run:
//!
//! - the canonical durable run id, the session that claims it, and the
//!   project/workspace that session belongs to;
//! - the store's own durable cursor (`next_ordinal`) and the run's
//!   committed high-water;
//! - the completeness facts — whether the run's own recording stopped at the
//!   store's per-run bound, and whether the store holds rows beyond the
//!   page that was last read;
//! - a bounded deque of **redacted** entries, merged by `(run_id, ordinal)`;
//! - the in-flight request id/epoch and the follow status.
//!
//! Three separations are load-bearing and are enforced by the shape here:
//!
//! 1. **Inspector metadata is not paging state.** The last-known row (agent
//!    name, task text, status) lives on the tab; the cursor, the entries and
//!    the request identity live here. A snapshot that arrives late therefore
//!    cannot overwrite fresher fetched entries.
//! 2. **Source truncation is not viewport truncation.** `recording_stopped`
//!    is the *store's* per-run bound: the rows that are missing were never
//!    written. The renderer's own display bound and the cache's own
//!    eviction cap are different facts and are counted separately
//!    ([`TranscriptCache::evicted_entries`]).
//! 3. **A hint is not a body.** The high-water is advisory. The canonical
//!    cursor always comes from the authorized page's `next_ordinal`, never
//!    from the hint and never from a count of painted rows.

use std::collections::VecDeque;

use crate::protocol::projection::dto::{
    AgentRunTranscriptEntryProjection, AgentRunTranscriptPageDto, AgentRunTranscriptPageStatus,
};

/// The maximum number of redacted entries one tab keeps in memory.
///
/// A transcript that outgrows this keeps its **tail**: the oldest rows are
/// evicted and the tab says so, so a scrolled view never claims to be the
/// whole conversation.
pub const MAX_TRANSCRIPT_CACHE_ENTRIES: usize = 512;

/// The number of rows one page read asks for.
///
/// The daemon clamps this to its own declared page ceiling; asking for the
/// ceiling means a full page is a full page and not a silent short read.
pub const MAX_TRANSCRIPT_PAGE_ROWS: u32 = 64;

/// The maximum number of pages one follow pass may walk.
///
/// A pass is a catch-up, not a drain: the store's own cursor decides what is
/// next, and this bound is what keeps a long transcript from turning one
/// hint into an unbounded burst of reads.
pub const MAX_TRANSCRIPT_PAGES_PER_PASS: usize = 4;

/// The maximum number of page reads one pass may start across all tabs.
///
/// The fair share across open tabs. Without it, one busy child could spend
/// the whole pass budget and starve the other fifteen.
pub const MAX_TRANSCRIPT_FETCHES_PER_PASS: usize = 4;

/// The maximum number of consecutive gap recoveries one cache may attempt.
///
/// A gap is recovered by re-reading from the authoritative cursor. A store
/// whose rows are genuinely not contiguous would otherwise retry forever, so
/// the recovery is bounded and the gap is then reported as a failure.
pub const MAX_TRANSCRIPT_GAP_RECOVERIES: u32 = 3;

/// What the tab's live follow is doing, and why it is not doing more.
///
/// These are deliberately distinct: "the daemon is older" is not "the run
/// recorded nothing", and "the read failed" is not "the run finished".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptFollowStatus {
    /// Nothing has been read for this run yet.
    Idle,
    /// A page read is in flight.
    Fetching,
    /// Recorded rows are shown and the run may still record more.
    Following,
    /// The tab is behind the run and is catching up.
    ///
    /// This is a distinct state from [`TranscriptFollowStatus::Fetching`]:
    /// a read *is* in flight, and the tab is also behind. It is what a user
    /// sees when the transcript outgrew one catch-up cycle's page budget.
    CatchingUp,
    /// Every row the store holds is shown and the run has finished.
    Complete,
    /// The run's own recording stopped at the store's per-run bound, so the
    /// rows that are missing were never written.
    RecordingStopped,
    /// The store holds no recorded transcript for this run.
    Absent,
    /// The run has no durable identity, so there is nothing to read.
    Unavailable,
    /// The connected daemon does not serve the delegated transcript read.
    Unsupported,
    /// The daemon refused the read.
    Denied,
    /// The daemon is temporarily unreachable.
    Disconnected,
    /// The read failed for a reason the daemon named.
    Failed(String),
}

impl TranscriptFollowStatus {
    /// Whether this status is an error the user can retry.
    ///
    /// "Older daemon" and "no durable identity" are **not** retryable: they
    /// are facts about the peer and about the run, and offering a retry for
    /// them would be offering a button that cannot work.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            TranscriptFollowStatus::Denied
                | TranscriptFollowStatus::Disconnected
                | TranscriptFollowStatus::Failed(_)
        )
    }

    /// Whether this status means the tab is not following the run.
    pub fn is_following(&self) -> bool {
        matches!(
            self,
            TranscriptFollowStatus::Following | TranscriptFollowStatus::Complete
        )
    }
}

/// What applying one authorized page did to the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageOutcome {
    /// The page carried rows that were appended to the cache.
    Appended {
        /// How many rows were appended.
        added: usize,
    },
    /// The page carried nothing the cache did not already hold.
    Duplicate,
    /// The page's rows did not start where the cursor said they would.
    ///
    /// The cache keeps the cursor at the gap so the next read recovers it
    /// from the authoritative reader rather than skipping the missing rows.
    Gap {
        /// The ordinal the gap starts at.
        expected: i64,
        /// How many rows were appended before the gap was seen.
        added: usize,
    },
    /// The page named a different run.
    ///
    /// Such a page is never merged: it would put one run's rows under
    /// another run's name.
    WrongRun,
}

/// One tab's bounded view of a delegated run's recorded transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptCache {
    /// The canonical durable run id this cache follows.
    pub run_id: String,
    /// The session that claims the run.
    ///
    /// This is the caller's **claim**. The daemon re-resolves the run's own
    /// owning session from the durable task origin before reading a row, so
    /// a wrong claim is refused rather than served.
    pub session_id: String,
    /// The project the claiming session belongs to.
    pub project_id: String,
    /// The workspace the claiming session belongs to.
    pub workspace_id: Option<String>,
    /// The store's own durable cursor: the ordinal the next page starts at.
    pub next_ordinal: i64,
    /// The run's committed high-water, as last known.
    pub high_water: i64,
    /// The high-water the next read should try to reach.
    pub desired_high_water: i64,
    /// Whether the run's own recording stopped at the store's per-run bound.
    pub recording_stopped: bool,
    /// Whether the store holds rows beyond the page that was last read.
    pub has_more: bool,
    /// How many entries the store has committed for this run.
    pub total_entries: i64,
    /// The bounded deque of redacted entries, ascending by ordinal.
    pub entries: VecDeque<AgentRunTranscriptEntryProjection>,
    /// How many entries the cache's own cap has evicted.
    pub evicted_entries: i64,
    /// The monotonic id of the last request this cache started.
    pub request_id: u64,
    /// The route epoch of the last request this cache started.
    pub request_epoch: u64,
    /// Whether a request is in flight for this cache.
    pub in_flight: bool,
    /// Whether a hint arrived that has not been read yet.
    pub dirty: bool,
    /// Whether a read has ever been attempted for this cache.
    ///
    /// A cache that has never read wants to read: the store may already
    /// hold rows for the run, and a tab that opened on a long-running child
    /// has to show them.
    pub has_read: bool,
    /// Whether the follow is blocked on a failed read.
    ///
    /// A failed read waits for an **explicit** retry or for the next
    /// successful page — it does not re-issue itself. A read that keeps
    /// failing would otherwise become a retry storm driven by the
    /// projection's own updates, which is a load the user did not ask for.
    pub blocked: bool,
    /// How many consecutive gap recoveries this cache has attempted.
    pub gap_recoveries: u32,
    /// Whether the run has reached a terminal status.
    pub run_terminal: bool,
    /// How many pages the current catch-up cycle has read.
    pub pages_in_pass: u32,
    /// How many rows the next page read asks for.
    ///
    /// The daemon refuses an over-bound page rather than shortening it, so a
    /// refusal is recovered by asking for fewer rows.
    pub page_rows: u32,
    /// The follow status.
    pub status: TranscriptFollowStatus,
    /// The bounded last error text.
    pub last_error: Option<String>,
}

impl TranscriptCache {
    /// Build a cache for `run_id` under the claiming session.
    pub fn new(
        run_id: impl Into<String>,
        session_id: impl Into<String>,
        project_id: impl Into<String>,
        workspace_id: Option<String>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            session_id: session_id.into(),
            project_id: project_id.into(),
            workspace_id,
            next_ordinal: 0,
            high_water: 0,
            desired_high_water: 0,
            recording_stopped: false,
            has_more: false,
            total_entries: 0,
            entries: VecDeque::new(),
            evicted_entries: 0,
            request_id: 0,
            request_epoch: 0,
            in_flight: false,
            dirty: false,
            has_read: false,
            blocked: false,
            gap_recoveries: 0,
            run_terminal: false,
            pages_in_pass: 0,
            page_rows: MAX_TRANSCRIPT_PAGE_ROWS,
            status: TranscriptFollowStatus::Idle,
            last_error: None,
        }
    }

    /// How many rows the next page read asks for.
    pub fn page_rows(&self) -> u32 {
        self.page_rows
    }

    /// Ask for fewer rows on the next read.
    ///
    /// The daemon refuses an over-bound page rather than shipping a short
    /// one, so the recovery is to ask for less. The floor is one row: a
    /// single row always fits.
    pub fn reduce_page_rows(&mut self) {
        self.page_rows = (self.page_rows / 2).max(1);
    }

    /// The number of entries the cache currently holds.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache's own cap has evicted earlier entries.
    ///
    /// This is the "earlier entries not loaded" fact. It is distinct from
    /// the store's own per-run truncation, and a scrolled view must not
    /// claim a complete history when it is true.
    pub fn has_evicted_entries(&self) -> bool {
        self.evicted_entries > 0
    }

    /// Start a page read. Returns the new request id.
    ///
    /// The id is monotonic per cache, so a completion that arrives after a
    /// newer read started is recognisably stale.
    pub fn begin_request(&mut self, epoch: u64) -> u64 {
        self.request_id = self.request_id.saturating_add(1);
        self.request_epoch = epoch;
        self.in_flight = true;
        self.has_read = true;
        self.last_error = None;
        self.request_id
    }

    /// Whether `request_id` is the request this cache is currently waiting on.
    pub fn is_current(&self, request_id: u64) -> bool {
        self.request_id == request_id
    }

    /// Finish the request `request_id`. Returns whether it was the current one.
    pub fn finish_request(&mut self, request_id: u64) -> bool {
        if !self.is_current(request_id) {
            return false;
        }
        self.in_flight = false;
        true
    }

    /// Record that the request `request_id` failed.
    ///
    /// Returns whether it was the current one; a stale failure is dropped
    /// rather than overwriting a fresher status.
    pub fn fail_request(&mut self, request_id: u64, error: String) -> bool {
        if !self.finish_request(request_id) {
            return false;
        }
        self.status = TranscriptFollowStatus::Failed(error.clone());
        self.last_error = Some(error);
        true
    }

    /// Raise the desired high-water from a body-free hint.
    ///
    /// The hint is advisory: it says the run's recording reached `ordinal`,
    /// not that the tab now holds those rows. A hint that does not advance
    /// the high-water is ignored, and a hint that does leaves the cache
    /// **dirty** so a read is scheduled.
    pub fn raise_high_water(&mut self, ordinal: i64, recording_stopped: bool) -> bool {
        let advanced = ordinal > self.high_water;
        if advanced {
            self.high_water = ordinal;
            self.desired_high_water = self.desired_high_water.max(ordinal);
            self.dirty = true;
            // A new hint starts a new catch-up cycle, so the previous
            // cycle's page budget no longer applies to it.
            self.pages_in_pass = 0;
        }
        if recording_stopped {
            self.recording_stopped = true;
        }
        advanced
    }

    /// Whether the cache still wants to read.
    ///
    /// A blocked cache does not want to read: it is waiting for an explicit
    /// retry.
    pub fn wants_read(&self) -> bool {
        !self.blocked && (self.dirty || self.has_more || !self.has_read)
    }

    /// Merge one authorized page into the cache.
    pub fn apply_page(&mut self, page: &AgentRunTranscriptPageDto) -> PageOutcome {
        if page.run_id != self.run_id {
            return PageOutcome::WrongRun;
        }
        // A page that arrived is a read that worked, so the follow is no
        // longer blocked on an earlier failure.
        self.blocked = false;
        let mut added = 0usize;
        let mut gap = None;
        for entry in &page.entries {
            if entry.ordinal < self.next_ordinal {
                // A row the cursor already passed: a duplicate or a stale
                // envelope. It is ignored rather than appended twice.
                continue;
            }
            if entry.ordinal > self.next_ordinal {
                gap = Some(self.next_ordinal);
                break;
            }
            self.push_entry(entry.clone());
            // The cursor advances with the row, so the next row is compared
            // against where this one left it. Without this the second row of
            // a page looks like a gap.
            self.next_ordinal = entry.ordinal + 1;
            added += 1;
        }
        if let Some(expected) = gap {
            // Keep the cursor at the gap so the next read recovers it from
            // the authoritative reader instead of skipping the missing rows.
            self.next_ordinal = expected;
            self.dirty = true;
            self.gap_recoveries = self.gap_recoveries.saturating_add(1);
            return PageOutcome::Gap { expected, added };
        }
        self.gap_recoveries = 0;
        // The page's own cursor is authoritative — never the hint, never a
        // count of the rows that were painted.
        self.next_ordinal = self.next_ordinal.max(page.next_ordinal);
        self.has_more = page.has_more;
        self.total_entries = page.source.total_entries;
        if page.source.recording_stopped
            || page.status == AgentRunTranscriptPageStatus::RecordingStopped
        {
            self.recording_stopped = true;
        }
        self.status = self.status_for(page.status);
        if added == 0 {
            PageOutcome::Duplicate
        } else {
            PageOutcome::Appended { added }
        }
    }

    /// The status the cache takes after a page with `page_status`.
    fn status_for(&self, page_status: AgentRunTranscriptPageStatus) -> TranscriptFollowStatus {
        if page_status == AgentRunTranscriptPageStatus::Absent {
            return TranscriptFollowStatus::Absent;
        }
        if self.recording_stopped {
            return TranscriptFollowStatus::RecordingStopped;
        }
        if self.run_terminal && !self.has_more {
            return TranscriptFollowStatus::Complete;
        }
        TranscriptFollowStatus::Following
    }

    /// Append one entry, evicting the oldest once the cap is reached.
    fn push_entry(&mut self, entry: AgentRunTranscriptEntryProjection) {
        self.entries.push_back(entry);
        while self.entries.len() > MAX_TRANSCRIPT_CACHE_ENTRIES {
            if self.entries.pop_front().is_some() {
                self.evicted_entries = self.evicted_entries.saturating_add(1);
            }
        }
    }

    /// Record that the run reached a terminal status.
    ///
    /// A terminal run triggers a final catch-up: the last rows are still
    /// committed rows, and a tab that stopped reading at the last hint would
    /// miss them.
    pub fn set_run_terminal(&mut self, terminal: bool) {
        self.run_terminal = terminal;
        if terminal {
            self.dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::projection::dto::{AgentRunTranscriptKind, AgentRunTranscriptRole};

    fn entry(ordinal: i64) -> AgentRunTranscriptEntryProjection {
        AgentRunTranscriptEntryProjection {
            ordinal,
            role: AgentRunTranscriptRole::Orchestrator,
            kind: AgentRunTranscriptKind::ModelMessage,
            body: format!("row {ordinal}"),
            byte_len: 8,
            created_at: 1_700_000_000_000 + ordinal,
        }
    }

    fn page(
        run_id: &str,
        entries: Vec<AgentRunTranscriptEntryProjection>,
        next_ordinal: i64,
        has_more: bool,
    ) -> AgentRunTranscriptPageDto {
        AgentRunTranscriptPageDto {
            run_id: run_id.to_string(),
            status: AgentRunTranscriptPageStatus::Present,
            entries,
            next_ordinal,
            has_more,
            source: crate::protocol::projection::dto::AgentRunTranscriptSourceState {
                recording_stopped: false,
                total_entries: next_ordinal,
            },
        }
    }

    fn cache() -> TranscriptCache {
        TranscriptCache::new(
            "run-1",
            "session-1",
            "project-1",
            Some("workspace-1".into()),
        )
    }

    #[test]
    fn a_page_appends_its_rows_and_advances_the_authoritative_cursor() {
        let mut cache = cache();
        let outcome = cache.apply_page(&page("run-1", vec![entry(0), entry(1)], 2, false));
        assert_eq!(outcome, PageOutcome::Appended { added: 2 });
        assert_eq!(cache.entry_count(), 2);
        assert_eq!(cache.next_ordinal, 2);
        assert_eq!(cache.status, TranscriptFollowStatus::Following);
    }

    #[test]
    fn a_duplicate_page_adds_nothing_and_does_not_move_the_cursor_backwards() {
        let mut cache = cache();
        cache.apply_page(&page("run-1", vec![entry(0), entry(1)], 2, false));
        let outcome = cache.apply_page(&page("run-1", vec![entry(0), entry(1)], 2, false));
        assert_eq!(outcome, PageOutcome::Duplicate);
        assert_eq!(cache.entry_count(), 2);
        assert_eq!(cache.next_ordinal, 2);
    }

    #[test]
    fn a_gap_keeps_the_cursor_at_the_gap_so_the_next_read_recovers_it() {
        let mut cache = cache();
        let outcome = cache.apply_page(&page("run-1", vec![entry(0), entry(3)], 4, false));
        assert_eq!(
            outcome,
            PageOutcome::Gap {
                expected: 1,
                added: 1
            }
        );
        assert_eq!(cache.next_ordinal, 1);
        assert!(cache.dirty, "a gap leaves the cache wanting another read");
    }

    #[test]
    fn a_page_for_another_run_is_never_merged() {
        let mut cache = cache();
        let outcome = cache.apply_page(&page("run-2", vec![entry(0)], 1, false));
        assert_eq!(outcome, PageOutcome::WrongRun);
        assert_eq!(cache.entry_count(), 0);
        assert_eq!(cache.next_ordinal, 0);
    }

    #[test]
    fn the_cache_cap_keeps_a_bounded_tail_and_counts_what_it_evicted() {
        let mut cache = cache();
        let total = MAX_TRANSCRIPT_CACHE_ENTRIES + 10;
        for ordinal in 0..total as i64 {
            cache.push_entry(entry(ordinal));
        }
        assert_eq!(cache.entry_count(), MAX_TRANSCRIPT_CACHE_ENTRIES);
        assert_eq!(cache.evicted_entries, 10);
        assert!(cache.has_evicted_entries());
        assert_eq!(
            cache.entries.front().map(|row| row.ordinal),
            Some(10),
            "the oldest rows are the ones evicted"
        );
    }

    #[test]
    fn a_hint_raises_the_high_water_and_leaves_the_cache_dirty() {
        let mut cache = cache();
        assert!(cache.raise_high_water(7, false));
        assert_eq!(cache.high_water, 7);
        assert!(cache.dirty);
        assert!(
            !cache.raise_high_water(3, false),
            "a stale hint does not rewind"
        );
        assert_eq!(cache.high_water, 7);
    }

    #[test]
    fn a_stale_completion_cannot_finish_a_newer_request() {
        let mut cache = cache();
        let first = cache.begin_request(1);
        let second = cache.begin_request(2);
        assert!(!cache.finish_request(first));
        assert!(cache.finish_request(second));
        assert!(!cache.in_flight);
    }

    #[test]
    fn an_absent_page_is_absent_and_not_a_failure() {
        let mut cache = cache();
        let mut absent = page("run-1", Vec::new(), 0, false);
        absent.status = AgentRunTranscriptPageStatus::Absent;
        cache.apply_page(&absent);
        assert_eq!(cache.status, TranscriptFollowStatus::Absent);
        assert!(!cache.status.is_retryable());
    }

    #[test]
    fn a_terminal_run_that_has_been_caught_up_is_complete() {
        let mut cache = cache();
        cache.set_run_terminal(true);
        cache.apply_page(&page("run-1", vec![entry(0)], 1, false));
        assert_eq!(cache.status, TranscriptFollowStatus::Complete);
    }

    #[test]
    fn a_source_that_stopped_recording_is_not_complete() {
        let mut cache = cache();
        cache.set_run_terminal(true);
        let mut stopped = page("run-1", vec![entry(0)], 1, false);
        stopped.source.recording_stopped = true;
        cache.apply_page(&stopped);
        assert_eq!(cache.status, TranscriptFollowStatus::RecordingStopped);
    }
}
