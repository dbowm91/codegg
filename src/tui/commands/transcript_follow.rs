//! M003 — the delegated run's **live transcript follow**.
//!
//! The tab used to be a boundary read: it showed whatever the session
//! projection carried when it opened, and it never changed again. This
//! module makes it a follow, without turning it into a poll.
//!
//! # What drives a read
//!
//! Exactly three things, and none of them is a timer:
//!
//! 1. **A body-free hint.** The daemon publishes a body-free
//!    transcript-advanced advisory (the `AgentRunTranscriptAdvanced` variant
//!    of the daemon's core event enum) once a row is committed. The
//!    projection reducer keeps it as a per-run watermark, and this module
//!    reads that watermark to learn *that* the run advanced — never *what*
//!    it recorded. The rows themselves come from the authorized page read.
//! 2. **A tab boundary.** Opening or focusing a tab catches up from the
//!    authoritative cursor, so a tab opened on a run that has been working
//!    for a while shows the rows that were recorded while it was closed.
//! 3. **A terminal run.** A run that finished still owes its last rows.
//!
//! # What bounds the work
//!
//! - One outstanding request per tab. A second hint **coalesces** with the
//!   request already in flight instead of issuing another one.
//! - A strict page budget per catch-up cycle
//!   ([`MAX_TRANSCRIPT_PAGES_PER_PASS`]). A long transcript is walked over
//!   several cycles, not in one burst.
//! - A fair share across open tabs
//!   ([`MAX_TRANSCRIPT_FETCHES_PER_PASS`]). Only the **focused** tab reads
//!   eagerly; a background tab remembers that it is dirty and reads when it
//!   is focused. There is therefore no recurrent traffic for a hidden tab.
//! - A bounded cache per tab ([`MAX_TRANSCRIPT_CACHE_ENTRIES`]), which keeps
//!   a tail and says that earlier entries were not loaded.
//!
//! # What a stale answer may not do
//!
//! Every completion is fenced by the existing mechanisms, in this order:
//! the per-tab request id, the live route token (project, workspace,
//! session, reconnect epoch), the tab's own existence, the run id the cache
//! follows, and the cursor the read asked for. A page that arrives after a
//! session switch, a project switch, a reconnect, a tab close, a task-id
//! reuse or a run replacement is **dropped**, not rendered.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::protocol::projection::dto::AgentRunTranscriptPageDto;
use crate::tui::app::state::routing::{RouteCheck, UiRouteToken};
use crate::tui::app::state::subagent_tabs::SubagentTabId;
use crate::tui::app::state::transcript_cache::{
    PageOutcome, TranscriptCache, TranscriptFollowStatus, MAX_TRANSCRIPT_FETCHES_PER_PASS,
    MAX_TRANSCRIPT_GAP_RECOVERIES, MAX_TRANSCRIPT_PAGES_PER_PASS,
};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_scoped_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// The error codes that mean "the peer does not serve this request".
///
/// An older daemon answers a request it has never heard of with one of
/// these (or with a transport error carrying the same meaning). That is a
/// **fact about the peer**, not a failure of the read, and the tab says so
/// rather than claiming the run recorded nothing.
const UNSUPPORTED_CODES: &[&str] = &[
    "unsupported_request",
    "unknown_request",
    "unsupported_capability",
    "invalid_request",
    "method_not_found",
    "not_implemented",
];

/// The live routing identity, as the fence compares it.
fn live_route_check(app: &App) -> RouteCheck {
    app.routing_registry.check_for(
        app.project_tabs.active_tab_id(),
        app.active_project_id(),
        app.active_workspace_id(),
        app.active_session_id(),
        app.view_switch.active_view_epoch,
    )
}

/// The session identity the authorized projection vouches for.
///
/// The session id in the request is the **caller's claim**; the daemon
/// re-resolves the run's own owning session from the durable task origin
/// before it reads a row. A claim that is wrong is refused, not served.
fn projected_session_identity(app: &App) -> Option<(String, String, Option<String>)> {
    let (_, snapshot) = app.projection_client.active_snapshot()?;
    if snapshot.primary_session_id.is_empty() {
        return None;
    }
    Some((
        snapshot.primary_session_id.clone(),
        snapshot.project_id.clone(),
        Some(snapshot.workspace_id.clone()).filter(|id| !id.is_empty()),
    ))
}

/// Whether the run's recorded status is terminal.
///
/// The status comes from the authoritative lifecycle projection, never from
/// a transcript row: a row that says "tool call" is not evidence that the
/// run finished.
fn is_terminal_status(status: &str) -> bool {
    let status = status.trim().to_ascii_lowercase();
    if status.is_empty() {
        return false;
    }
    !(status.contains("running")
        || status.contains("in_progress")
        || status.contains("in-progress")
        || status.contains("active")
        || status.contains("pending")
        || status.contains("queued")
        || status.contains("started"))
}

/// Make sure the tab has a live-follow cache for the run it currently shows.
///
/// Returns whether a cache is present. A tab whose row carries no durable
/// run identity gets **no** cache: there is nothing to read, and the tab
/// stays the run inspector it was. Nothing is ever joined by display name
/// or by a numeric task id.
///
/// A cache whose run id no longer matches the row is **replaced**: a run
/// that was replaced under the same tab id is a different conversation, and
/// keeping the old rows under the new run's name would be a lie.
pub(crate) fn ensure_cache(app: &mut App, tab_id: &SubagentTabId) -> bool {
    let Some(run_id) = app
        .ui_state
        .subagents
        .tabs
        .get(tab_id)
        .and_then(|tab| tab.row.run_id.clone())
    else {
        // The run has no durable identity, so there is nothing to read. The
        // tab says so rather than showing an empty pane.
        if let Some(tab) = app.ui_state.subagents.tabs.get_mut(tab_id) {
            tab.transcript = None;
            tab.follow_unavailable =
                Some("this run has no durable identity, so there is nothing to follow".to_string());
        }
        return false;
    };
    let terminal = app
        .ui_state
        .subagents
        .tabs
        .get(tab_id)
        .map(|tab| is_terminal_status(&tab.row.status))
        .unwrap_or(false);
    // An existing cache that already follows this run is kept, even when
    // the projection snapshot is momentarily absent: the cache holds the
    // rows the daemon authorized, and the cursor it holds is still the
    // authoritative one.
    if let Some(cache) = app
        .ui_state
        .subagents
        .tabs
        .get(tab_id)
        .and_then(|tab| tab.transcript.as_ref())
    {
        if cache.run_id == run_id {
            if let Some(cache) = app
                .ui_state
                .subagents
                .tabs
                .get_mut(tab_id)
                .and_then(|tab| tab.transcript.as_mut())
            {
                cache.set_run_terminal(terminal);
            }
            if let Some(tab) = app.ui_state.subagents.tabs.get_mut(tab_id) {
                tab.follow_unavailable = None;
            }
            return true;
        }
    }
    let Some((session_id, project_id, workspace_id)) = projected_session_identity(app) else {
        return false;
    };
    let Some(tab) = app.ui_state.subagents.tabs.get_mut(tab_id) else {
        return false;
    };
    tab.transcript = Some(TranscriptCache::new(
        run_id,
        session_id,
        project_id,
        workspace_id,
    ));
    tab.follow_unavailable = None;
    true
}

/// The number of transcript reads outstanding across the open tabs.
fn outstanding_reads(app: &App) -> usize {
    app.ui_state
        .subagents
        .tabs
        .ordered()
        .iter()
        .filter(|tab| tab.transcript.as_ref().is_some_and(|cache| cache.in_flight))
        .count()
}

/// Start one bounded catch-up cycle for `tab_id`.
///
/// Safe to call as often as the event loop likes: a tab with nothing to
/// read, a tab with a request already in flight, and a tab that has spent
/// its page budget all return without issuing anything.
pub(crate) fn start_follow(app: &mut App, tab_id: &SubagentTabId) {
    if !ensure_cache(app, tab_id) {
        return;
    }
    let Some(cache) = app
        .ui_state
        .subagents
        .tabs
        .get(tab_id)
        .and_then(|tab| tab.transcript.as_ref())
    else {
        return;
    };
    if cache.in_flight {
        // Coalesce: the request already in flight will pick up whatever the
        // hint raised, because the cursor it reads from is the cache's.
        return;
    }
    if !cache.wants_read() {
        return;
    }
    // The fair total limit across the open tabs. One busy child must not
    // spend the whole budget while the others wait — and a read that is
    // already outstanding counts against it.
    if outstanding_reads(app) >= MAX_TRANSCRIPT_FETCHES_PER_PASS {
        return;
    }
    if cache.pages_in_pass >= MAX_TRANSCRIPT_PAGES_PER_PASS as u32 {
        // The cycle's page budget is spent. The tab says it is catching up
        // and the next cycle continues from the authoritative cursor.
        if let Some(cache) = app
            .ui_state
            .subagents
            .tabs
            .get_mut(tab_id)
            .and_then(|tab| tab.transcript.as_mut())
        {
            if cache.status.is_following() {
                cache.status = TranscriptFollowStatus::CatchingUp;
            }
        }
        return;
    }
    issue_page_read(app, tab_id);
}

/// Issue one authorized page read for `tab_id`.
fn issue_page_read(app: &mut App, tab_id: &SubagentTabId) {
    let Some(cache) = app
        .ui_state
        .subagents
        .tabs
        .get(tab_id)
        .and_then(|tab| tab.transcript.as_ref())
    else {
        return;
    };
    let run_id = cache.run_id.clone();
    let session_id = cache.session_id.clone();
    let from_ordinal = cache.next_ordinal;
    let limit = cache.page_rows();
    let epoch = app.routing_registry.reconnect_epoch;
    let request_id = {
        let Some(cache) = app
            .ui_state
            .subagents
            .tabs
            .get_mut(tab_id)
            .and_then(|tab| tab.transcript.as_mut())
        else {
            return;
        };
        cache.begin_request(epoch)
    };
    {
        // The first read of a run shows as a read in flight; a later read
        // keeps the count of rows it already holds on screen.
        let Some(cache) = app
            .ui_state
            .subagents
            .tabs
            .get_mut(tab_id)
            .and_then(|tab| tab.transcript.as_mut())
        else {
            return;
        };
        if cache.entry_count() == 0 {
            cache.status = TranscriptFollowStatus::Fetching;
        }
    }
    let route = UiRouteToken::new(
        app.project_tabs.active_tab_id().cloned(),
        app.active_project_id().map(str::to_string),
        app.active_workspace_id().map(str::to_string),
        app.active_session_id().map(str::to_string),
        // The active-view epoch is deliberately not part of the match: the
        // transcript belongs to the run, not to the panel that shows it.
        0,
        epoch,
        request_id,
    );
    let Some(client) = app.core_client.clone() else {
        let Some(cache) = app
            .ui_state
            .subagents
            .tabs
            .get_mut(tab_id)
            .and_then(|tab| tab.transcript.as_mut())
        else {
            return;
        };
        cache.fail_request(request_id, "the daemon is not connected".to_string());
        return;
    };
    let Some(tx) = app.tui_cmd_tx.clone() else {
        let Some(cache) = app
            .ui_state
            .subagents
            .tabs
            .get_mut(tab_id)
            .and_then(|tab| tab.transcript.as_mut())
        else {
            return;
        };
        cache.fail_request(request_id, "the TUI event loop is unavailable".to_string());
        return;
    };
    if tokio::runtime::Handle::try_current().is_err() {
        let Some(cache) = app
            .ui_state
            .subagents
            .tabs
            .get_mut(tab_id)
            .and_then(|tab| tab.transcript.as_mut())
        else {
            return;
        };
        cache.fail_request(request_id, "live follow needs the TUI runtime".to_string());
        return;
    }
    let tab_key = tab_id.as_str().to_string();
    spawn_scoped_registered_tui_task(
        Some(tx),
        &mut app.task_registry,
        TuiTaskKind::Command,
        "subagent_transcript_page",
        None,
        Some(session_id.clone()),
        None,
        async move {
            let request = crate::core::new_request(
                uuid::Uuid::new_v4().to_string(),
                CoreRequest::AgentRunTranscriptRead {
                    session_id,
                    run_id: run_id.clone(),
                    from_ordinal,
                    limit,
                },
            );
            let response = client.request(request).await;
            let (page, error, error_code) = match response {
                Ok(CoreResponse::AgentRunTranscriptRead { page }) => (Some(page), None, None),
                Ok(CoreResponse::Error { code, message }) => (None, Some(message), Some(code)),
                Ok(_) => (
                    None,
                    Some("the daemon answered with an unexpected response".to_string()),
                    None,
                ),
                Err(error) => (None, Some(error.to_string()), None),
            };
            Some(TuiCommand::SubagentTranscriptPage {
                request_id,
                route,
                tab_id: tab_key,
                run_id,
                from_ordinal,
                page,
                error,
                error_code,
            })
        },
    );
}

/// One completed delegated-transcript page read.
///
/// The completion carries the fence the read was issued under, so the applier
/// can recognise a stale answer without re-deriving anything.
pub(crate) struct TranscriptPageCompletion {
    /// The per-tab request id the read was issued under.
    pub request_id: u64,
    /// The route the read was issued under.
    pub route: UiRouteToken,
    /// The tab the read was issued for.
    pub tab_id: String,
    /// The run the read was issued for.
    pub run_id: String,
    /// The cursor the read asked from.
    pub from_ordinal: i64,
    /// The page, when the read succeeded.
    pub page: Option<AgentRunTranscriptPageDto>,
    /// The failure message, when the read did not succeed.
    pub error: Option<String>,
    /// The daemon's error code, when it named one.
    pub error_code: Option<String>,
}

/// Apply one completed page read.
///
/// Every check here is a fence. A completion that fails any of them is
/// dropped: it is not rendered, it does not move a cursor, and it does not
/// change a status.
pub(crate) fn apply_page(app: &mut App, completion: TranscriptPageCompletion) {
    let TranscriptPageCompletion {
        request_id,
        route,
        tab_id,
        run_id,
        from_ordinal,
        page,
        error,
        error_code,
    } = completion;
    let tab_key = SubagentTabId::new(tab_id);
    // The tab must still exist: a tab closed while the read was in flight
    // takes its cache with it.
    //
    // Every fence below is read-only, and every one of them has to be
    // checked **before** the cache is borrowed mutably: a check that reads
    // the live route while the cache is borrowed would be a borrow the
    // compiler (rightly) refuses.
    {
        let Some(tab) = app.ui_state.subagents.tabs.get(&tab_key) else {
            return;
        };
        let Some(cache) = tab.transcript.as_ref() else {
            return;
        };
        // The request must be the one this cache is waiting on.
        if !cache.is_current(request_id) {
            return;
        }
        // The live route must still be the route the read was issued under.
        if !route.matches(&live_route_check(app)) {
            return;
        }
        // The cache must still be following the run the page names.
        if cache.run_id != run_id {
            return;
        }
        // The cache must still be at the cursor the read asked from.
        if cache.next_ordinal != from_ordinal {
            return;
        }
        // The live session must still be the session the cache claims. When
        // the app does not know a live session there is nothing to compare,
        // and the run id plus the request id still fence the answer.
        if let Some(live_session) = app.active_session_id() {
            if live_session != cache.session_id {
                return;
            }
        }
        if let Some(live_project) = app.active_project_id() {
            if live_project != cache.project_id {
                return;
            }
        }
    }
    if page.is_none() {
        let Some(message) = error else {
            return;
        };
        let status = status_for_error(error_code.as_deref(), &message);
        let bound_exceeded = error_code.as_deref() == Some("transcript_bound_exceeded");
        let unsupported = status == TranscriptFollowStatus::Unsupported;
        {
            let Some(cache) = app
                .ui_state
                .subagents
                .tabs
                .get_mut(&tab_key)
                .and_then(|tab| tab.transcript.as_mut())
            else {
                return;
            };
            if !cache.fail_request(request_id, message.clone()) {
                return;
            }
            cache.status = status;
            if bound_exceeded {
                // The daemon refused the page rather than shortening it.
                // Asking for fewer rows is the recovery it names, so this
                // one re-arms itself.
                cache.reduce_page_rows();
                cache.dirty = true;
            } else {
                // The follow waits for an **explicit** retry. Re-arming it
                // here would turn the projection's own updates into a retry
                // storm against a daemon that is refusing the read.
                cache.blocked = true;
                cache.dirty = false;
            }
        }
        if unsupported {
            // The daemon does not serve the typed page read. Fall back to
            // the session snapshot the tab used to read, so an older peer
            // still shows whatever it can vouch for.
            app.refresh_delegated_transcript();
        }
        return;
    }
    let page = page.expect("the page is present when no error was reported");
    let Some(cache) = app
        .ui_state
        .subagents
        .tabs
        .get_mut(&tab_key)
        .and_then(|tab| tab.transcript.as_mut())
    else {
        return;
    };
    if !cache.finish_request(request_id) {
        return;
    }
    let outcome = cache.apply_page(&page);
    cache.pages_in_pass = cache.pages_in_pass.saturating_add(1);
    match outcome {
        PageOutcome::WrongRun => {
            // A page that names another run is never merged. The cache keeps
            // its cursor and asks again.
            cache.dirty = true;
            cache.status = TranscriptFollowStatus::Failed(
                "the daemon answered with a page for a different run".to_string(),
            );
        }
        PageOutcome::Gap { expected, .. } => {
            if cache.gap_recoveries >= MAX_TRANSCRIPT_GAP_RECOVERIES {
                cache.status = TranscriptFollowStatus::Failed(format!(
                    "the store's rows are not contiguous at ordinal {expected}"
                ));
                cache.dirty = false;
            } else {
                cache.status = TranscriptFollowStatus::CatchingUp;
                cache.dirty = true;
            }
        }
        PageOutcome::Appended { .. } | PageOutcome::Duplicate => {
            cache.dirty = cache.has_more;
            if !cache.dirty {
                cache.pages_in_pass = 0;
            }
        }
    }
    if cache.has_more && cache.pages_in_pass < MAX_TRANSCRIPT_PAGES_PER_PASS as u32 {
        // The store holds rows beyond this page. Continue the cycle from the
        // authoritative cursor.
        start_follow(app, &tab_key);
    }
}

/// The status a failed read maps to.
fn status_for_error(code: Option<&str>, message: &str) -> TranscriptFollowStatus {
    match code {
        Some(code) if UNSUPPORTED_CODES.contains(&code) => TranscriptFollowStatus::Unsupported,
        Some("agent_run_not_found") => TranscriptFollowStatus::Denied,
        Some("transcript_unavailable") => TranscriptFollowStatus::Disconnected,
        _ => TranscriptFollowStatus::Failed(message.to_string()),
    }
}

/// Apply the projection's transcript watermarks to the open tabs.
///
/// The watermark is **advisory**: it says the run's recording reached an
/// ordinal, not that the tab holds those rows, and never that the run
/// finished. It raises the desired high-water and leaves the cache dirty;
/// the rows come from the authorized page read.
pub(crate) fn on_snapshot_watermarks(app: &mut App) {
    let Some((_, snapshot)) = app.projection_client.active_snapshot() else {
        return;
    };
    let watermarks: Vec<(String, i64, bool)> = snapshot
        .transcript_watermarks
        .iter()
        .map(|watermark| {
            (
                watermark.run_id.clone(),
                watermark.ordinal,
                watermark.recording_stopped,
            )
        })
        .collect();
    if watermarks.is_empty() {
        return;
    }
    let active_tab = app.ui_state.subagents.tabs.active_tab_id().cloned();
    let tab_ids: Vec<SubagentTabId> = app
        .ui_state
        .subagents
        .tabs
        .ordered()
        .into_iter()
        .map(|tab| tab.tab_id.clone())
        .collect();
    for tab_id in tab_ids {
        let Some(cache) = app
            .ui_state
            .subagents
            .tabs
            .get(&tab_id)
            .and_then(|tab| tab.transcript.as_ref())
        else {
            continue;
        };
        let run_id = cache.run_id.clone();
        let Some((_, ordinal, recording_stopped)) = watermarks
            .iter()
            .find(|(candidate, _, _)| candidate == &run_id)
        else {
            continue;
        };
        let (ordinal, recording_stopped) = (*ordinal, *recording_stopped);
        if let Some(cache) = app
            .ui_state
            .subagents
            .tabs
            .get_mut(&tab_id)
            .and_then(|tab| tab.transcript.as_mut())
        {
            cache.raise_high_water(ordinal, recording_stopped);
        }
    }
    // Only the focused tab reads eagerly. A background tab stays dirty and
    // reads when it is focused, so a hidden tab generates no traffic.
    if let Some(active_tab) = active_tab {
        start_follow(app, &active_tab);
    }
}

/// Catch a tab up when it gains focus.
pub(crate) fn on_tab_focused(app: &mut App, tab_id: &SubagentTabId) {
    start_follow(app, tab_id);
}

/// Re-read the tab's transcript from its own cursor.
///
/// This is the explicit retry for a follow that broke. It re-reads from the
/// tab's **own** cursor, so it never re-fetches what the tab already holds
/// and it never re-reads a run the tab is not following.
///
/// A follow that is working is not re-issued: a retry that re-reads a
/// healthy tab is a request nobody asked for.
pub(crate) fn retry(app: &mut App, tab_id: &SubagentTabId) {
    let Some(cache) = app
        .ui_state
        .subagents
        .tabs
        .get(tab_id)
        .and_then(|tab| tab.transcript.as_ref())
    else {
        return;
    };
    if !cache.status.is_retryable() {
        return;
    }
    // The retry starts a fresh cycle from the authoritative cursor, so the
    // previous cycle's page budget does not apply to it.
    if let Some(cache) = app
        .ui_state
        .subagents
        .tabs
        .get_mut(tab_id)
        .and_then(|tab| tab.transcript.as_mut())
    {
        cache.pages_in_pass = 0;
        cache.dirty = true;
        cache.blocked = false;
        cache.last_error = None;
        cache.status = TranscriptFollowStatus::Idle;
    }
    start_follow(app, tab_id);
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CoreClient;
    use crate::error::AppError;
    use crate::protocol::core::{CoreEvent, EventEnvelope, RequestEnvelope};
    use crate::protocol::projection::dto::{
        AgentRunTranscriptEntryProjection, AgentRunTranscriptPageDto, AgentRunTranscriptPageStatus,
        AgentRunTranscriptSourceState,
    };
    use crate::tui::app::state::transcript_cache::MAX_TRANSCRIPT_PAGE_ROWS;
    use std::sync::{Arc, Mutex};

    const RUN_ID: &str = "run-follow-1";

    /// A fake M002 page provider.
    ///
    /// It serves the pages it was given and records every read it was asked
    /// for, so a test can assert **how many** reads a hint caused and **from
    /// which cursor** — the two facts the bounds are about.
    #[derive(Default)]
    struct FakePageClient {
        pages: Mutex<Vec<AgentRunTranscriptPageDto>>,
        reads: Mutex<Vec<(String, i64, u32)>>,
        error: Mutex<Option<(String, String)>>,
    }

    impl FakePageClient {
        fn with_page(page: AgentRunTranscriptPageDto) -> Arc<Self> {
            Arc::new(Self {
                pages: Mutex::new(vec![page]),
                ..Default::default()
            })
        }

        fn with_error(code: &str, message: &str) -> Arc<Self> {
            Arc::new(Self {
                error: Mutex::new(Some((code.to_string(), message.to_string()))),
                ..Default::default()
            })
        }

        fn reads(&self) -> Vec<(String, i64, u32)> {
            self.reads.lock().expect("the read log").clone()
        }
    }

    #[async_trait::async_trait]
    impl CoreClient for FakePageClient {
        async fn request(
            &self,
            request: RequestEnvelope<CoreRequest>,
        ) -> Result<CoreResponse, AppError> {
            match request.payload {
                CoreRequest::AgentRunTranscriptRead {
                    run_id,
                    from_ordinal,
                    limit,
                    ..
                } => {
                    self.reads.lock().expect("the read log").push((
                        run_id.clone(),
                        from_ordinal,
                        limit,
                    ));
                    if let Some((code, message)) =
                        self.error.lock().expect("the error slot").clone()
                    {
                        return Ok(CoreResponse::Error { code, message });
                    }
                    let page = self
                        .pages
                        .lock()
                        .expect("the page list")
                        .iter()
                        .find(|page| page.run_id == run_id && page.next_ordinal > from_ordinal)
                        .cloned()
                        .unwrap_or(AgentRunTranscriptPageDto {
                            run_id,
                            status: AgentRunTranscriptPageStatus::Absent,
                            entries: Vec::new(),
                            next_ordinal: from_ordinal,
                            has_more: false,
                            source: AgentRunTranscriptSourceState {
                                recording_stopped: false,
                                total_entries: from_ordinal,
                            },
                        });
                    Ok(CoreResponse::AgentRunTranscriptRead { page })
                }
                _ => Ok(CoreResponse::Error {
                    code: "unexpected_request".to_string(),
                    message: "the fake only serves transcript reads".to_string(),
                }),
            }
        }

        fn subscribe(&self) -> tokio::sync::mpsc::Receiver<EventEnvelope<CoreEvent>> {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            rx
        }
    }

    fn entry(ordinal: i64, body: &str) -> AgentRunTranscriptEntryProjection {
        AgentRunTranscriptEntryProjection {
            ordinal,
            role: crate::protocol::projection::dto::AgentRunTranscriptRole::Orchestrator,
            kind: crate::protocol::projection::dto::AgentRunTranscriptKind::ModelMessage,
            body: body.to_string(),
            byte_len: body.len() as i64,
            created_at: 1_700_000_000_000 + ordinal,
        }
    }

    fn page(
        entries: Vec<AgentRunTranscriptEntryProjection>,
        next_ordinal: i64,
    ) -> AgentRunTranscriptPageDto {
        AgentRunTranscriptPageDto {
            run_id: RUN_ID.to_string(),
            status: AgentRunTranscriptPageStatus::Present,
            entries,
            next_ordinal,
            has_more: false,
            source: AgentRunTranscriptSourceState {
                recording_stopped: false,
                total_entries: next_ordinal,
            },
        }
    }

    /// The session the fixture's runs belong to.
    const SESSION_ID: &str = "session-follow";

    /// An app whose projection vouches for one delegated run.
    fn app_with_run(run_id: &str, session_id: &str) -> App {
        use crate::protocol::projection::dto::{AgentRunSummary, AgentTreeNodeProjection};
        use crate::protocol::projection::fixtures::active_turn_snapshot;
        use crate::protocol::projection::replay::{
            ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
            ProjectionSubscriptionId,
        };

        let mut app = App::new_for_testing("/tmp".into());
        let mut snapshot = active_turn_snapshot();
        let mut turn = snapshot
            .active_turn
            .expect("the fixture has an active turn");
        turn.agent_tree.push_back(AgentTreeNodeProjection {
            child_instance_id: String::new(),
            originating_turn_id: None,
            task_id: 42,
            agent: "explore".into(),
            description: "map the subsystem".into(),
            status: crate::protocol::projection::dto::AgentTreeStatus::Running,
            parent_task_id: None,
            created_at: 1,
            completed_at: None,
            result_summary: None,
            display_name: Some("Sally".into()),
            profile: None,
            usefulness: None,
            run_id: Some(run_id.into()),
        });
        snapshot.active_turn = Some(turn);
        snapshot.primary_session_id = session_id.to_string();
        snapshot.agent_runs.push_back(AgentRunSummary {
            run_id: run_id.into(),
            task_id: "task-42".into(),
            project_id: snapshot.project_id.clone(),
            workspace_id: snapshot.workspace_id.clone(),
            agent: "explore".into(),
            status: "running".into(),
            depth: 1,
            control_status: "none".into(),
            updated_at: 3,
            ..Default::default()
        });
        let descriptor = ProjectionStreamDescriptor {
            stream_id: ProjectionStreamId::new("stream-sess").expect("valid stream id"),
            kind: ProjectionStreamKind::Session,
            project_id: snapshot.project_id.clone(),
            workspace_id: Some(snapshot.workspace_id.clone()),
            session_id: Some(snapshot.primary_session_id.clone()),
            projection_version: 1,
            retention_floor_seq: 0,
            high_water_seq: 0,
            latest_checkpoint_seq: None,
        };
        app.projection_client
            .controller_mut()
            .unsubscribe(&ProjectionSubscriptionId::new("sub-1"));
        app.projection_client.controller_mut().install_subscription(
            ProjectionSubscriptionId::new("sub-1"),
            descriptor,
            snapshot,
        );
        render_once(&mut app);
        app
    }

    /// An app whose projection vouches for **two** delegated runs, and whose
    /// snapshot carries a transcript watermark for the first one.
    ///
    /// The watermark is what a body-free hint reduces to on the TUI side, so
    /// it is the only way to exercise the hint path without a live daemon.
    fn app_with_two_runs(run_id: &str, other_run_id: &str) -> App {
        use crate::protocol::projection::dto::{
            AgentRunSummary, AgentRunTranscriptWatermarkProjection, AgentTreeNodeProjection,
        };
        use crate::protocol::projection::fixtures::active_turn_snapshot;
        use crate::protocol::projection::replay::{
            ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
            ProjectionSubscriptionId,
        };

        let mut app = App::new_for_testing("/tmp".into());
        let mut snapshot = active_turn_snapshot();
        let mut turn = snapshot
            .active_turn
            .expect("the fixture has an active turn");
        for (task_id, run) in [(42u64, run_id), (43u64, other_run_id)] {
            turn.agent_tree.push_back(AgentTreeNodeProjection {
                child_instance_id: String::new(),
                originating_turn_id: None,
                task_id,
                agent: "explore".into(),
                description: "map the subsystem".into(),
                status: crate::protocol::projection::dto::AgentTreeStatus::Running,
                parent_task_id: None,
                created_at: 1,
                completed_at: None,
                result_summary: None,
                display_name: Some("Sally".into()),
                profile: None,
                usefulness: None,
                run_id: Some(run.to_string()),
            });
            snapshot.agent_runs.push_back(AgentRunSummary {
                run_id: run.to_string(),
                task_id: format!("task-{task_id}"),
                project_id: snapshot.project_id.clone(),
                workspace_id: snapshot.workspace_id.clone(),
                agent: "explore".into(),
                status: "running".into(),
                depth: 1,
                control_status: "none".into(),
                updated_at: 3,
                ..Default::default()
            });
        }
        snapshot.active_turn = Some(turn);
        snapshot
            .transcript_watermarks
            .push_back(AgentRunTranscriptWatermarkProjection {
                run_id: run_id.to_string(),
                ordinal: 4,
                recording_stopped: false,
            });
        let descriptor = ProjectionStreamDescriptor {
            stream_id: ProjectionStreamId::new("stream-sess").expect("valid stream id"),
            kind: ProjectionStreamKind::Session,
            project_id: snapshot.project_id.clone(),
            workspace_id: Some(snapshot.workspace_id.clone()),
            session_id: Some(snapshot.primary_session_id.clone()),
            projection_version: 1,
            retention_floor_seq: 0,
            high_water_seq: 0,
            latest_checkpoint_seq: None,
        };
        app.projection_client
            .controller_mut()
            .unsubscribe(&ProjectionSubscriptionId::new("sub-1"));
        app.projection_client.controller_mut().install_subscription(
            ProjectionSubscriptionId::new("sub-1"),
            descriptor,
            snapshot,
        );
        render_once(&mut app);
        app
    }

    /// Render one frame.
    ///
    /// The subagent rows are **derived during rendering**, exactly as they
    /// are in the real TUI, so a fixture that wants to open a tab has to
    /// render first. This is the same order the shipped render tests use.
    fn render_once(app: &mut App) {
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).expect("a test terminal");
        terminal
            .draw(|frame| {
                app.render(frame);
            })
            .expect("draw the app");
    }

    /// Wire an app to a fake page provider and a command channel the test
    /// can drain.
    fn wire(app: &mut App, client: Arc<FakePageClient>) -> tokio::sync::mpsc::Receiver<TuiCommand> {
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        app.tui_cmd_tx = Some(tx);
        app.set_core_client(client);
        rx
    }

    /// The active tab's cache.
    fn cache(app: &App) -> TranscriptCache {
        let tab_id = app
            .ui_state
            .subagents
            .tabs
            .active_tab_id()
            .cloned()
            .expect("a delegated tab is open");
        app.ui_state
            .subagents
            .tabs
            .get(&tab_id)
            .expect("the delegated tab")
            .transcript
            .clone()
            .expect("a live-follow cache")
    }

    #[tokio::test]
    async fn a_tab_that_opens_reads_its_transcript_and_holds_the_rows() {
        let client = FakePageClient::with_page(page(
            vec![
                entry(0, "the first recorded row"),
                entry(1, "the second recorded row"),
            ],
            2,
        ));
        let mut app = app_with_run(RUN_ID, SESSION_ID);
        let mut rx = wire(&mut app, client.clone());
        assert!(
            app.open_subagent_tab("42"),
            "the delegated run's tab must open from its projection row"
        );

        // The read is issued from the tab's own cursor — the store's
        // authoritative cursor, which starts at zero.
        let command = rx.recv().await.expect("a page read is issued");
        assert_eq!(
            client.reads(),
            vec![(RUN_ID.to_string(), 0, MAX_TRANSCRIPT_PAGE_ROWS)],
            "the first read must start at the cursor the store named"
        );
        crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, command).await;

        let cache = cache(&app);
        assert_eq!(
            cache.entry_count(),
            2,
            "the tab must hold the rows the page read returned"
        );
        assert_eq!(
            cache.next_ordinal, 2,
            "the cursor must advance to the page's authoritative next ordinal"
        );
        assert_eq!(
            cache.status,
            TranscriptFollowStatus::Following,
            "a run that may still record must be following, not complete"
        );
    }

    #[tokio::test]
    async fn a_second_hint_coalesces_with_the_read_that_is_already_in_flight() {
        let client = FakePageClient::with_page(page(vec![entry(0, "a row")], 1));
        let mut app = app_with_run(RUN_ID, SESSION_ID);
        let mut rx = wire(&mut app, client.clone());
        app.open_subagent_tab("42");
        let _first = rx.recv().await.expect("a page read is issued");

        // A hint arrives while the first read is still outstanding.
        let tab_id = app
            .ui_state
            .subagents
            .tabs
            .active_tab_id()
            .cloned()
            .expect("a delegated tab is open");
        app.ui_state
            .subagents
            .tabs
            .get_mut(&tab_id)
            .expect("the delegated tab")
            .transcript
            .as_mut()
            .expect("a live-follow cache")
            .raise_high_water(4, false);
        start_follow(&mut app, &tab_id);

        assert_eq!(
            client.reads().len(),
            1,
            "a hint while a read is in flight must not issue a second read"
        );
    }

    #[tokio::test]
    async fn a_completion_for_a_superseded_request_is_discarded() {
        let client = FakePageClient::with_page(page(vec![entry(0, "a row")], 1));
        let mut app = app_with_run(RUN_ID, SESSION_ID);
        let mut rx = wire(&mut app, client.clone());
        app.open_subagent_tab("42");
        let command = rx.recv().await.expect("a page read is issued");
        let TuiCommand::SubagentTranscriptPage { request_id, .. } = command else {
            panic!("the command must be the transcript page completion");
        };

        // A newer request supersedes the one that is in flight.
        let tab_id = app
            .ui_state
            .subagents
            .tabs
            .active_tab_id()
            .cloned()
            .expect("a delegated tab is open");
        app.ui_state
            .subagents
            .tabs
            .get_mut(&tab_id)
            .expect("the delegated tab")
            .transcript
            .as_mut()
            .expect("a live-follow cache")
            .begin_request(99);

        crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, command).await;
        assert_eq!(
            cache(&app).entry_count(),
            0,
            "a completion for a superseded request must not be merged"
        );
        assert_eq!(
            request_id, 1,
            "the first request of a cache is numbered one"
        );
    }

    #[tokio::test]
    async fn a_daemon_that_does_not_serve_the_read_marks_the_follow_unsupported() {
        let client = FakePageClient::with_error("unsupported_request", "not this daemon");
        let mut app = app_with_run(RUN_ID, SESSION_ID);
        let mut rx = wire(&mut app, client.clone());
        app.open_subagent_tab("42");
        let command = rx.recv().await.expect("a page read is issued");
        crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, command).await;

        assert_eq!(
            cache(&app).status,
            TranscriptFollowStatus::Unsupported,
            "an older peer must be reported as not serving the read, not as an empty transcript"
        );
    }

    #[tokio::test]
    async fn a_refused_read_marks_the_follow_denied_and_offers_a_retry() {
        let client = FakePageClient::with_error("agent_run_not_found", "delegated run not found");
        let mut app = app_with_run(RUN_ID, SESSION_ID);
        let mut rx = wire(&mut app, client.clone());
        app.open_subagent_tab("42");
        let command = rx.recv().await.expect("a page read is issued");
        crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, command).await;

        let cache = cache(&app);
        assert_eq!(
            cache.status,
            TranscriptFollowStatus::Denied,
            "a refused read must be reported as refused"
        );
        assert!(
            cache.status.is_retryable(),
            "a refused read must offer the user a retry"
        );
    }

    #[tokio::test]
    async fn a_background_tab_keeps_only_bounded_dirty_state_and_reads_on_focus() {
        const OTHER_RUN_ID: &str = "run-follow-2";
        let client = FakePageClient::with_page(page(vec![entry(0, "a row")], 1));
        let mut app = app_with_two_runs(RUN_ID, OTHER_RUN_ID);
        let mut rx = wire(&mut app, client.clone());

        // Two tabs. The second one opened last, so it is the focused one.
        app.open_subagent_tab("42");
        let _first = rx.recv().await.expect("a page read is issued");
        app.open_subagent_tab("43");
        let _second = rx.recv().await.expect("a page read is issued");
        let reads_before = client.reads().len();

        // A hint arrives for the **background** tab's run.
        on_snapshot_watermarks(&mut app);

        assert_eq!(
            client.reads().len(),
            reads_before,
            "a hint must not make a hidden tab generate traffic"
        );
        let background = app
            .ui_state
            .subagents
            .tabs
            .get(&SubagentTabId::new("42"))
            .expect("the background tab")
            .transcript
            .clone()
            .expect("a live-follow cache");
        assert!(
            background.dirty,
            "a hidden tab remembers that it is behind, in bounded state"
        );
        assert_eq!(
            background.high_water, 4,
            "the hidden tab's high-water still follows the hint"
        );
    }

    #[tokio::test]
    async fn a_retry_re_reads_from_the_tabs_own_cursor() {
        let client = FakePageClient::with_page(page(vec![entry(0, "a row")], 1));
        let mut app = app_with_run(RUN_ID, SESSION_ID);
        let mut rx = wire(&mut app, client.clone());
        app.open_subagent_tab("42");
        let command = rx.recv().await.expect("a page read is issued");
        crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, command).await;
        assert_eq!(cache(&app).next_ordinal, 1);

        // The follow breaks, and the user retries.
        let tab_id = app
            .ui_state
            .subagents
            .tabs
            .active_tab_id()
            .cloned()
            .expect("a delegated tab is open");
        {
            let cache = app
                .ui_state
                .subagents
                .tabs
                .get_mut(&tab_id)
                .expect("the delegated tab")
                .transcript
                .as_mut()
                .expect("a live-follow cache");
            cache.status = TranscriptFollowStatus::Disconnected;
        }
        retry(&mut app, &tab_id);
        let _retry_command = rx.recv().await.expect("the retry issues a read");

        assert_eq!(
            client.reads().last().cloned(),
            Some((RUN_ID.to_string(), 1, MAX_TRANSCRIPT_PAGE_ROWS)),
            "a retry must re-read from the tab's own cursor, not from the start"
        );
    }

    /// The end-to-end proof: a **real** daemon over a **real** SQLite store,
    /// reached through the production `CoreClient`, read by the production
    /// TUI path, and painted by the production renderer.
    ///
    /// The rows are in the store before the tab opens, so the assertion is
    /// about the whole path rather than about a fake. The run is still
    /// *running*: the tab shows its recorded rows while the child has not
    /// finished, which is the claim the milestone makes.
    #[tokio::test]
    async fn the_tab_reads_a_real_daemons_real_store_and_paints_the_recorded_row() {
        use crate::core::runtime_deps::CoreRuntimeDeps;
        use crate::core::InprocCoreClient;
        use codegg_core::agent_run::{
            AgentRunBudget, AgentRunStore, NewAgentRun, NewAgentTask, SqliteAgentRunStore,
        };
        use codegg_core::agent_run_transcript::{
            AgentRunTranscriptStore, SqliteAgentRunTranscriptStore, TranscriptKind, TranscriptRole,
        };
        use codegg_core::identity::{AgentRunId, AgentTaskId, ProjectId};
        use codegg_core::workspace::WorkspaceId;

        const RECORDED: &str = "the row the real store held";

        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("migrate the test pool");

        // A delegated run owned by `SESSION_ID`, with a recorded row.
        let run_id = AgentRunId::new();
        let workspace_id = WorkspaceId::new();
        let task = NewAgentTask {
            task_id: AgentTaskId::new(),
            parent_task_id: None,
            originating_session_id: SESSION_ID.to_string(),
            originating_turn_id: Some("turn-follow".to_string()),
            project_id: ProjectId::new(),
            repository_id: None,
            workspace_id: workspace_id.clone(),
            requested_agent: "explore".into(),
            delegation_key: format!("delegation-{run_id}"),
            request_fingerprint: format!("fingerprint-{run_id}"),
            description: "a delegated run".into(),
        };
        let run = NewAgentRun {
            run_id: run_id.clone(),
            parent_run_id: None,
            depth: 1,
            workspace_id,
            agent_name: "explore".into(),
            agent_digest: Some("digest".into()),
            provider: "openai".into(),
            model: "model".into(),
            authority_digest: "authority".into(),
            budget: AgentRunBudget::default(),
            shared_scratch: Vec::new(),
        };
        SqliteAgentRunStore::new(pool.clone())
            .create_or_get(task, run)
            .await
            .expect("start the delegated run");
        SqliteAgentRunTranscriptStore::new(pool.clone())
            .append(
                &run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                RECORDED,
            )
            .await
            .expect("append the recorded row");

        // The production client, over the production daemon, over the pool
        // the row was just written to.
        let deps = CoreRuntimeDeps::with_jobs(pool, None, None);
        let client = InprocCoreClient::with_deps(deps, crate::config::schema::Config::default());

        let mut app = app_with_run(run_id.as_ref(), SESSION_ID);
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);
        app.set_core_client(Arc::new(client));
        assert!(
            app.open_subagent_tab("42"),
            "the delegated run's tab must open from its projection row"
        );

        let command = rx.recv().await.expect("a page read is issued");
        crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, command).await;

        assert_eq!(
            cache(&app).entry_count(),
            1,
            "the tab must hold the row the real store held"
        );

        // And the renderer paints it.
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).expect("a test terminal");
        terminal
            .draw(|frame| {
                app.render(frame);
            })
            .expect("draw the tab");
        let buffer = terminal.backend().buffer().clone();
        let mut painted = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                painted.push(buffer[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            painted.push('\n');
        }
        assert!(
            painted.contains(RECORDED),
            "the recorded row must reach the buffer: {painted}"
        );
    }
}
