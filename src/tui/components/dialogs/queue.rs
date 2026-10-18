//! The `/queue` box: the session's waiting prompts, one per row, in the order
//! the daemon ordered them.
//!
//! M004 makes this workstream's **first user-operable** milestone. What is on
//! screen here is `SessionState::user_prompt_queue` verbatim — this widget
//! never sorts, filters, or reorders. That is not a stylistic choice: the
//! daemon owns the ordinal, and a locally re-sorted list would claim an order
//! the queue does not have.
//!
//! The projection carries `pending` **and** `delivering` rows on purpose
//! (ADR-0038). A `delivering` row is the one the user most needs to see — it is
//! the prompt whose turn is running right now — so the status is rendered from
//! the DTO rather than assumed, and the entry stays listed when a delete is
//! refused for it.
//!
//! M005 adds authoring: `a` queues a new prompt, `e` corrects one in place, and
//! `[`/`]` reorder. That makes this box both a navigator and a text field, which
//! is the whole hazard of the milestone — see [`QueueDialog::is_text_step`].
//!
//! M006 adds send-now: `s` hands a queued prompt to the turn already running.
//!
//! M007 adds delayed delivery. `a` and `e` are now **two** steps — the text,
//! then how long to wait — and every row carries a due marker so a scheduled
//! entry is distinguishable from an immediate one without relying on colour.
//! The box's module doc claimed there was no wakeup to ride; planning M007
//! falsified that, and ADR-0039 is what replaced the claim.
//!
//! Still absent: adoption and transfer (ADR-0037's explicit audited
//! operations). Naming them here would teach a key this widget does not
//! implement, which is the specific dishonesty the milestone plan forbids.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::super::component::{Component, DialogType};
use super::super::scroll::CenteredScroll;
use crate::protocol::core::UserPromptQueueEntryDto;
use crate::tui::app::TuiMsg;
use crate::tui::theme::Theme;

/// Who owns this render's chrome.
///
/// Same per-widget convention as `tree.rs`: [`Chrome::Owned`] is the modal box
/// and draws its own border, heading, and keymap footer; [`Chrome::Hosted`] is
/// a column in a side panel where the host already painted all three into the
/// same rect. There is no shared helper for this — panel-convergence M003
/// deferred one and never opened it — so this widget duplicates the shape
/// rather than inventing a second abstraction.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chrome {
    Owned,
    Hosted,
}

/// The verbs this box implements while it is only navigating.
///
/// `handle_key` below is the implementation and this is the advertisement, so
/// the two cannot drift. `Esc` is deliberately absent even though `handle_key`
/// answers it, for the same reason `TreeDialog` omits it: the modal host
/// claims `Esc` as its universal exit before consulting the widget, so
/// advertising it here would teach a key that never reaches this code.
///
/// `Enter` is absent for a different reason and must stay that way: it is this
/// box's **confirm** key inside a text step, so naming it here would describe a
/// verb that does not apply to what is on screen.
const HINT_BROWWSING: &str =
    "j/k move · [ ] reorder · a add · e edit · d delete · s send now · r refresh";

/// The verbs while a text step has the keyboard.
///
/// Two keys, both of which the box itself answers. This is the one place a
/// stray `d` does real damage — see [`QueueDialog::is_text_step`].
const HINT_TEXT: &str = "Enter next · Esc cancel";

/// The verbs while the duration step has the keyboard.
///
/// Names the grammar inline because the box is the only place it is stated, and
/// a user who has to leave the box to learn `1h` has already lost the entry
/// they were composing. "Enter with nothing" is stated because it is the common
/// case, not an edge case — most prompts are immediate.
const HINT_SCHEDULE: &str = "Enter set · 30s/5min/2h/1d · empty = next turn · Esc back";

/// Row height budget before the list starts paging.
const DEFAULT_VISIBLE_HEIGHT: usize = 10;

/// Width reserved on the right of every row for the status tag, including the
/// separator and its trailing space.
const STATUS_COLUMN_WIDTH: usize = 12;

/// Longest prompt text drawn before truncation, as a backstop for absurdly
/// narrow terminals where the width-derived bound would still be enormous.
const MAX_TEXT_CHARS: usize = 512;

/// Local guard on the compose buffer, in bytes.
///
/// This is a **resource** bound, not the queue's rule: the daemon's own limit
/// is `USER_PROMPT_QUEUE_MAX_ENTRY_BYTES` (64 KiB) and it refuses over-bound
/// text as `user_prompt_queue_entry_too_large`, which is the refusal a user
/// actually hits. This constant sits deliberately above it so the daemon's
/// typed refusal is what anyone sees in practice, and this only bounds a
/// pathological paste.
///
/// It is reported, never silently applied: an append past the bound is refused
/// with [`QueueStep::BufferFull`] so the user learns the text stopped rather
/// than finding a prompt quietly shorter than what they typed. That is the
/// "text is never silently truncated" invariant, and it is why this is a
/// refuse rather than a `truncate`.
const MAX_COMPOSE_BYTES: usize = 256 * 1024;

/// The empty state, verbatim.
///
/// It names `a` from M005 onward, because `a` now exists. Before M005 it named
/// no verb for creating an entry precisely because naming one that does nothing
/// is worse than saying nothing.
const EMPTY_STATE: &str = "no prompts queued";
/// M006 revised this line. It used to promise the only outcome that existed —
/// "it waits until the current turn finishes" — which send-now made false: a
/// queued prompt can now be handed to the turn that is running *right now*.
/// Leaving the old wording would have taught a user to press nothing.
const EMPTY_HINT: &str =
    "a adds a prompt — it waits for the next turn, or s hands it to the one running";

/// Which step the box is in.
///
/// M005 makes this box two different surfaces behind one key handler, and the
/// enum is what keeps them from bleeding into each other: [`Self::is_text_step`]
/// is the single predicate every action letter is guarded by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueStep {
    /// Navigating the list. Every action letter is live.
    Browsing,
    /// Composing a new prompt. No action letter is live.
    Adding,
    /// Revising the selected prompt's text in place. No action letter is live.
    Editing,
    /// M007: typing how long until this prompt should run.
    ///
    /// A **separate step** rather than an inline suffix on the text field.
    /// Prompt text is user data the box must not reinterpret, and any inline
    /// delimiter is a character the user cannot type — so `ship the docs | 1h`
    /// and `ship the docs | 1h | check the 1h build` are not distinguishable.
    /// A second step makes the due time a first-class, editable field instead.
    ///
    /// It is still inside [`QueueDialog::is_text_step`], so it keeps every
    /// guarantee the text step has: action letters are inert and the composer
    /// row stays reserved.
    Scheduling,
    /// The compose buffer is at [`MAX_COMPOSE_BYTES`]; further characters are
    /// refused. Reachability only — nothing routes here today, so an append
    /// past the bound is reported in place instead of latched.
    BufferFull,
}

/// Why a reorder produced no mutation.
///
/// Carried as a value rather than a formatted string so the message is chosen
/// by the input layer, where toasts live, and so a test can assert *which*
/// refusal happened rather than matching prose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueReorderRefusal {
    /// The selected entry is mid-delivery, so the daemon will not move it.
    Delivering,
    /// Already the first operable row.
    AlreadyEarliest,
    /// Already the last operable row.
    AlreadyLatest,
}

/// M006: why a send-now produced no request at all.
///
/// Split by *which side* decided, because the two are not the same promise.
/// [`Self::Delivering`] is a fact about the row: it is already running as its
/// own turn, and no amount of retrying changes that. [`Self::NoActiveTurn`] is a
/// fact about the box's own view of the session: the queued prompt simply has
/// nowhere to go until a turn starts, which is what the queue already does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueSendRefusal {
    /// The selected entry is mid-delivery, so it is already running on its own.
    Delivering,
    /// No turn is running, so there is nothing to hand the prompt to.
    NoActiveTurn,
}

#[derive(Clone)]
pub struct QueueDialog {
    pub theme: Arc<Theme>,
    entries: Vec<UserPromptQueueEntryDto>,
    selected: usize,
    scroll: CenteredScroll,
    visible_height: usize,
    /// M005: which surface has the keyboard right now.
    step: QueueStep,
    /// M005: the text being composed. Lives in the box rather than in
    /// `DialogState` so it is dropped by the same `clone`/`close` path that
    /// unmounts the box — there is no window in which a later open can commit
    /// text the user abandoned.
    compose: String,
    /// M005: the entry an [`QueueStep::Editing`] step is revising.
    ///
    /// Carried by identity, not by cursor position, for the same reason
    /// `selected_entry_id` exists: a projection that lands mid-edit must not
    /// silently retarget the edit at whichever row the cursor now sits on.
    editing_entry_id: Option<String>,
    /// M007: the duration being typed in [`QueueStep::Scheduling`].
    ///
    /// Separate from `compose` because the two are different fields with
    /// different meanings, and a single buffer holding both would make "go back
    /// and fix the text" ambiguous.
    schedule: String,
    /// M007: the entry's **existing** due time, captured when the edit step
    /// opened.
    ///
    /// Held so the duration step can be seeded with what the entry already
    /// carries, and so an edit that changes nothing still writes back the
    /// schedule it had rather than silently clearing it.
    editing_run_at_ms: Option<i64>,
}

impl QueueDialog {
    pub fn new(theme: Arc<Theme>, entries: Vec<UserPromptQueueEntryDto>) -> Self {
        let mut dialog = Self {
            theme,
            entries,
            selected: 0,
            scroll: CenteredScroll::new(),
            visible_height: DEFAULT_VISIBLE_HEIGHT,
            step: QueueStep::Browsing,
            compose: String::new(),
            editing_entry_id: None,
            schedule: String::new(),
            editing_run_at_ms: None,
        };
        dialog.clamp_selection();
        dialog
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn entries(&self) -> &[UserPromptQueueEntryDto] {
        &self.entries
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Adopt a fresh daemon projection.
    ///
    /// Selection follows the **entry id**, not the index. A projection that
    /// arrives after a delete, a delivery, or a second attached frontend's
    /// mutation will not have the same row count — keying on the index would
    /// silently move the cursor onto a different prompt, and deleting there
    /// would remove something the user never selected.
    ///
    /// When the previously selected entry is gone (it was the one deleted) the
    /// cursor clamps to the nearest surviving row, which is what stops a stale
    /// row from leaving a permanently unselectable index behind.
    pub fn set_entries(&mut self, entries: Vec<UserPromptQueueEntryDto>) {
        let previously_selected = self.selected_entry_id().map(str::to_owned);
        self.entries = entries;
        self.selected = previously_selected
            .and_then(|id| self.entries.iter().position(|entry| entry.id == id))
            .unwrap_or(self.selected);
        self.clamp_selection();
    }

    pub fn selected_entry_id(&self) -> Option<&str> {
        self.entries
            .get(self.selected)
            .map(|entry| entry.id.as_str())
    }

    /// The selected row's status, normalised the same way the renderer reads it.
    ///
    /// M006. Absent when the box is empty, which the caller must treat as "no
    /// row to act on" rather than as a `pending` row.
    fn selected_entry_status(&self) -> Option<&str> {
        self.entries.get(self.selected).map(Self::status_of)
    }

    fn clamp_selection(&mut self) {
        if self.entries.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.entries.len() {
            self.selected = self.entries.len() - 1;
        }
        self.scroll
            .clamp(self.selected, self.entries.len(), self.visible_height);
    }

    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.clamp_selection();
        }
    }

    pub fn select_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
            self.clamp_selection();
        }
    }

    // ── M005: the text step ──────────────────────────────────────────
    //
    // A box that navigates and accepts typed text swallows letters unless the
    // two are separated. Typing `d` into a prompt must not delete a prompt.
    //
    // There is **no** shared `captures_text` predicate on `App` — the only
    // precedent is `ConnectDialog::is_text_input_step`, one method on one
    // dialog. This is that method again, and the plan's stop condition is
    // explicit that opening a shared `App`-level predicate is out of scope.
    //
    // The guard is structural rather than per-letter: the whole match below
    // branches on this predicate once, so a new action letter added to the
    // browsing arm cannot accidentally be live in a text step. That is the
    // failure the plan calls the single most likely defect, and the shape that
    // makes it impossible is "text step first, as a separate match", not
    // "remember the guard on every arm".

    /// Whether a text field currently owns the keyboard.
    ///
    /// M007 adds [`QueueStep::Scheduling`] to this set rather than giving it a
    /// sibling predicate. The duration step has exactly the same relationship to
    /// the browsing arms as the text step — it swallows letters — so treating it
    /// as anything else is how `d` would end up deleting a prompt while someone
    /// was typing `2d`.
    pub fn is_text_step(&self) -> bool {
        matches!(
            self.step,
            QueueStep::Adding | QueueStep::Editing | QueueStep::Scheduling | QueueStep::BufferFull
        )
    }

    /// Whether the duration field is the one under the cursor.
    pub fn is_schedule_step(&self) -> bool {
        self.step == QueueStep::Scheduling
    }

    /// The duration being typed, empty outside the duration step.
    pub fn schedule(&self) -> &str {
        &self.schedule
    }

    pub fn step(&self) -> QueueStep {
        self.step
    }

    /// The text being composed. Empty unless [`Self::is_text_step`].
    pub fn compose(&self) -> &str {
        &self.compose
    }

    /// The entry an edit step is revising, by identity.
    pub fn editing_entry_id(&self) -> Option<&str> {
        self.editing_entry_id.as_deref()
    }

    /// `a`: start composing a new prompt.
    pub fn begin_add(&mut self) {
        self.compose.clear();
        self.editing_entry_id = None;
        self.schedule.clear();
        self.editing_run_at_ms = None;
        self.step = QueueStep::Adding;
    }

    /// `e`: start revising the selected prompt in place, seeded with its text.
    ///
    /// Seeds from the entry's **current** text, so an edit starts from what the
    /// user can see on the row rather than from empty. An empty queue, or a
    /// selection that vanished between the keystroke and this call, does
    /// nothing at all — there is nothing to revise.
    pub fn begin_edit(&mut self) -> bool {
        let Some(entry) = self.entries.get(self.selected) else {
            return false;
        };
        self.compose = entry.text.clone();
        self.editing_entry_id = Some(entry.id.clone());
        // M007: seed the duration step from what the row already carries, as
        // *remaining* time rather than the original offset. An entry scheduled
        // an hour ago for 09:00 should open editing at `30m`, not `1h` — showing
        // the original offset would let a user "keep" 1h and silently push the
        // entry a full hour further out than they meant.
        self.schedule = remaining_duration_text(entry.run_at_ms);
        self.editing_run_at_ms = entry.run_at_ms;
        self.step = QueueStep::Editing;
        true
    }

    /// Abandon the text step without committing.
    ///
    /// Discarding rather than restoring is deliberate: the buffer was already
    /// the user-visible content, so there is nothing to go back to, and holding
    /// a "pre-edit" copy would be text this box has no other reason to keep.
    pub fn cancel_text_step(&mut self) {
        self.compose.clear();
        self.editing_entry_id = None;
        self.schedule.clear();
        self.editing_run_at_ms = None;
        self.step = QueueStep::Browsing;
    }

    /// Leave the duration step and go **back** to the text step.
    ///
    /// `Esc` here is not a cancel. The text the user already typed is still in
    /// the box and still worth keeping, and making `Esc` discard it would mean a
    /// mistyped `2d` costs the whole prompt. This is the only back-navigation in
    /// the box, and it is one level deep by construction.
    fn back_to_text_step(&mut self) {
        self.schedule.clear();
        self.step = if self.editing_entry_id.is_some() {
            QueueStep::Editing
        } else {
            QueueStep::Adding
        };
    }

    /// Take the composed text and the finished step, for the caller to send.
    ///
    /// The text is handed over rather than left here so the box is never in a
    /// state where the same characters could be committed twice. Returns `None`
    /// outside a text step or on whitespace-only text — the daemon refuses that
    /// too, but there is no point spending a round trip to learn it.
    pub fn take_compose(&mut self) -> Option<(Option<String>, String, Option<i64>)> {
        if !self.is_text_step() {
            return None;
        }
        let entry_id = self.editing_entry_id.take();
        let run_at_ms = self.editing_run_at_ms.take();
        let text = std::mem::take(&mut self.compose);
        self.schedule.clear();
        self.step = QueueStep::Browsing;
        if text.trim().is_empty() {
            return None;
        }
        Some((entry_id, text, run_at_ms))
    }

    /// `Enter` in a text step: move on to the duration step.
    ///
    /// M007. This no longer commits — it advances, because the due time is part
    /// of what the user is specifying and guessing "immediate" for them would
    /// silently drop a schedule they meant to set.
    ///
    /// The composed text **stays in the buffer** across the duration step so
    /// `Esc` there can return to it. It is taken only by [`Self::commit_schedule_step`],
    /// which is the single point where a compose turns into a request.
    fn commit_text_step(&mut self) -> Option<TuiMsg> {
        if self.compose.trim().is_empty() {
            // Same rule as before, moved here: an all-whitespace prompt is
            // refused locally rather than spending a round trip on it.
            self.cancel_text_step();
            return None;
        }
        self.step = QueueStep::Scheduling;
        None
    }

    /// `Enter` in the duration step: parse, and commit if it parses.
    ///
    /// An **empty** buffer is the ordinary "no schedule" answer and commits with
    /// `None`. A buffer that does not parse is refused *here*, in the box,
    /// without a request: the grammar is a UI-level fact the box already owns,
    /// and sending a string to the daemon to be told "that is not a duration"
    /// would spend a round trip to deliver a message the box can give first.
    ///
    /// The buffer is left exactly as typed on a refusal, so the fix is editing
    /// it rather than retyping it.
    fn commit_schedule_step(&mut self) -> Option<TuiMsg> {
        let raw = self.schedule.trim().to_string();
        let run_at_ms: Option<i64> = if raw.is_empty() {
            None
        } else {
            let Some(duration) = crate::background_task_migration::parse_duration(&raw) else {
                return Some(TuiMsg::QueueScheduleRejected { input: raw });
            };
            // `as_millis` is `u128`; the clamp keeps a multi-century duration
            // from wrapping into a due time in the past, which would be the one
            // value that turns "schedule it later" into "run it now".
            let millis = duration.as_millis().min(i64::MAX as u128) as i64;
            Some(chrono::Utc::now().timestamp_millis().saturating_add(millis))
        };

        let (entry_id, text, _previous_run_at) = self.take_compose()?;
        Some(match entry_id {
            // An edit reuses the entry's existing identity. Minting a new id
            // here would turn "correct this prompt" into "add a second prompt
            // and orphan the first", which is the opposite of the verb.
            Some(id) => TuiMsg::QueueEditCommitted {
                entry_id: id,
                text,
                run_at_ms,
            },
            None => TuiMsg::QueueAddCommitted { text, run_at_ms },
        })
    }

    /// Put text back after a refusal, so the user can shorten it.
    ///
    /// This is the "a refusal is shown, not absorbed" invariant in code: a
    /// bounds refusal (`entry_too_large`, `too_many_entries`) arrives with the
    /// user's text still composed, and dropping it would make the fix —
    /// shorten the prompt — impossible without retyping from nothing.
    pub fn restore_compose(&mut self, entry_id: Option<String>, text: String) {
        self.step = if entry_id.is_some() {
            QueueStep::Editing
        } else {
            QueueStep::Adding
        };
        self.compose = text;
        self.editing_entry_id = entry_id;
        self.schedule.clear();
        self.editing_run_at_ms = None;
    }

    /// Append one character to the compose buffer.
    ///
    /// Refuses rather than truncates at [`MAX_COMPOSE_BYTES`], and says so.
    pub fn insert_char(&mut self, c: char) -> bool {
        if self.compose.len() + c.len_utf8() > MAX_COMPOSE_BYTES {
            self.step = QueueStep::BufferFull;
            return false;
        }
        self.compose.push(c);
        true
    }

    /// M007: append one character to the duration buffer.
    ///
    /// Bounded by the same local ceiling as the prompt buffer, but without the
    /// `BufferFull` latch — a duration field cannot plausibly reach 64 KiB, and
    /// routing it through [`QueueStep::BufferFull`] would silently drop the user
    /// out of the step they were in.
    fn push_schedule_char(&mut self, c: char) {
        if self.schedule.len() + c.len_utf8() <= MAX_COMPOSE_BYTES {
            self.schedule.push(c);
        }
    }

    /// Delete the last character. A no-op on an empty buffer.
    pub fn backspace(&mut self) {
        self.compose.pop();
    }

    /// Clear a [`QueueStep::BufferFull`] latch once the user backspaces.
    fn clear_buffer_full(&mut self) {
        if self.step == QueueStep::BufferFull {
            let entry_id = self.editing_entry_id.clone();
            let text = std::mem::take(&mut self.compose);
            self.restore_compose(entry_id, text);
        }
    }

    // ── M005: reorder ────────────────────────────────────────────────

    /// Whether a row is the one the daemon will let a mutation act on.
    ///
    /// `pending` is not a display concern: `reorder`'s permutation is checked
    /// against `OPERABLE_STATUS_PREDICATE` (`status = 'pending'`), so this is
    /// exactly the set a reorder may name. An empty `status` reads as `pending`
    /// because that is what the projection's own renderer assumes.
    fn is_operable(entry: &UserPromptQueueEntryDto) -> bool {
        matches!(entry.status.as_str(), "pending" | "")
    }

    /// Build the permutation `UserPromptQueueReorder` expects.
    ///
    /// This is the milestone's sharpest edge. The projection the box renders
    /// contains `pending` **and** `delivering` rows
    /// (`list_projected_in`, `LIVE_STATUS_PREDICATE`), but `reorder` validates
    /// against `list_live_in`, whose `WHERE` is `OPERABLE_STATUS_PREDICATE`
    /// (`status = 'pending'`). `is_permutation` requires **exact length
    /// equality**, so sending every id the box rendered is refused with
    /// `order_mismatch` the moment a single entry is mid-delivery.
    ///
    /// So the permutation is built from the operable rows only, and `entry_id`
    /// is moved within that subset. The relative order of every other row is
    /// preserved exactly — this is a move, not a resort, and it never
    /// re-derives an ordinal (the daemon owns those).
    ///
    /// Returns `None` when the move is not expressible: the selection is
    /// `delivering`, or the selected entry is the first/last operable row, so
    /// there is no neighbour to swap with.
    pub fn pending_permutation(&self, entry_id: &str, earlier: bool) -> Option<Vec<String>> {
        let operable: Vec<&UserPromptQueueEntryDto> = self
            .entries
            .iter()
            .filter(|entry| QueueDialog::is_operable(entry))
            .collect();

        let from = operable
            .iter()
            .position(|entry| entry.id == entry_id)
            .map(|index| (index, operable[index].id.clone()))?;
        // The neighbour we swap with. `checked_sub`/`checked_add` rather than
        // saturating arithmetic so "already first" is a `None` (nothing to do)
        // instead of an off-by-one that swaps the row with itself.
        let neighbour = if earlier {
            from.0.checked_sub(1)
        } else {
            from.0.checked_add(1)
        }?;
        if neighbour >= operable.len() {
            return None;
        }

        let mut ids: Vec<String> = operable.iter().map(|entry| entry.id.clone()).collect();
        ids.swap(from.0, neighbour);
        Some(ids)
    }

    /// Emit a reorder for the selected entry, or explain why there is none.
    fn reorder_selected(&self, earlier: bool) -> Option<TuiMsg> {
        let entry_id = self.selected_entry_id().map(str::to_owned)?;
        // Checked before the permutation because the two failures read very
        // differently to the user: "that one is being delivered" is about the
        // entry, "already at the front" is about the position.
        if !self.selected_is_reorderable() {
            return Some(TuiMsg::QueueReorderUnavailable {
                reason: QueueReorderRefusal::Delivering,
            });
        }
        match self.pending_permutation(&entry_id, earlier) {
            Some(entry_ids) => Some(TuiMsg::QueueReorderSelected { entry_ids }),
            None => Some(TuiMsg::QueueReorderUnavailable {
                reason: if earlier {
                    QueueReorderRefusal::AlreadyEarliest
                } else {
                    QueueReorderRefusal::AlreadyLatest
                },
            }),
        }
    }
    /// Whether the selected row can be reordered at all.
    ///
    /// Separate from [`Self::pending_permutation`] so the *reason* a move is
    /// refused can be told apart: a `delivering` selection and an
    /// already-first/last selection are different answers to the user, and
    /// only the first one is about the entry's state.
    pub fn selected_is_reorderable(&self) -> bool {
        self.selected_entry_id()
            .is_some_and(|id| QueueDialog::is_operable(self.entry(id)))
    }

    fn entry(&self, id: &str) -> &UserPromptQueueEntryDto {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .expect("caller resolved the id from this list")
    }

    /// Put the cursor on a named entry, if it is in the list.
    ///
    /// Used after an add, so the cursor lands on the prompt the user just wrote
    /// rather than wherever the previous selection happened to be. Returns
    /// whether the entry was found — a refused add never moves the cursor.
    pub fn select_entry_id(&mut self, id: &str) -> bool {
        let Some(index) = self.entries.iter().position(|entry| entry.id == id) else {
            return false;
        };
        self.selected = index;
        self.clamp_selection();
        true
    }

    /// The rows that fit on screen after the current scroll offset.
    fn window(&self) -> &[UserPromptQueueEntryDto] {
        let start = self.scroll.get().min(self.entries.len());
        let end = (start + self.visible_height).min(self.entries.len());
        &self.entries[start..end]
    }

    fn title(&self) -> String {
        if self.entries.is_empty() {
            " Prompt Queue ".to_string()
        } else {
            format!(" Prompt Queue ({}) ", self.entries.len())
        }
    }

    /// The status tag for a row, read from the DTO.
    ///
    /// Never hardcoded to `pending`: the projection deliberately contains
    /// `delivering` rows, and a row whose turn is running is precisely the one
    /// the user must be able to see.
    fn status_of(entry: &UserPromptQueueEntryDto) -> &str {
        let status = entry.status.as_str();
        if status.is_empty() {
            "pending"
        } else {
            status
        }
    }

    /// One row, bounded to `text_width` columns with an announced truncation.
    ///
    /// Truncation is marked rather than silent: a quietly shortened prompt
    /// reads as the whole prompt, which would let the user act on text they
    /// never actually saw.
    fn bounded_text(text: &str, text_width: usize) -> String {
        if text_width == 0 {
            return String::new();
        }
        let mut out = String::new();
        let mut used = 0usize;
        // Reserve one column for the ellipsis unless the whole text fits.
        for ch in text.chars().take(MAX_TEXT_CHARS) {
            let width = unicode_width_char(ch);
            if used + width > text_width.saturating_sub(1) {
                out.push('…');
                return out;
            }
            out.push(ch);
            used += width;
        }
        if used < text_width {
            // Room left over; nothing is hidden so nothing is announced.
            return out;
        }
        out
    }

    fn row_style(&self, is_selected: bool) -> Style {
        if is_selected {
            Style::default()
                .fg(self.theme.background)
                .bg(self.theme.primary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.theme.foreground)
        }
    }

    fn detail_style(&self, is_selected: bool) -> Style {
        if is_selected {
            Style::default().fg(self.theme.background)
        } else {
            Style::default().fg(self.theme.muted)
        }
    }

    fn render_body(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>, chrome: Chrome) {
        let (rows_area, compose_area, hint_area) = match chrome {
            Chrome::Owned => {
                let block = Block::default()
                    .title(self.title())
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .style(Style::default().bg(theme.background));
                let inner = block.inner(area);
                frame.render_widget(Paragraph::new("").block(block), area);
                // M005: the composer only takes a row when a text step is
                // actually running. Reserving it unconditionally would reflow
                // the list on every `a` and `Esc`, which is the same class of
                // defect as the working strip's reserved row 0.
                let mut constraints = vec![ratatui::layout::Constraint::Min(1)];
                if self.is_text_step() {
                    constraints.push(ratatui::layout::Constraint::Length(1));
                }
                constraints.push(ratatui::layout::Constraint::Length(1));
                let chunks = ratatui::layout::Layout::vertical(constraints).split(inner);
                let hint = chunks[chunks.len() - 1];
                let compose = if chunks.len() > 2 {
                    Some(chunks[chunks.len() - 2])
                } else {
                    None
                };
                (chunks[0], compose, Some(hint))
            }
            Chrome::Hosted => {
                // The host has already painted a border, a title, and the focus
                // marker into this rect. A second border overwrites all three
                // and a focused column renders like an unfocused one.
                let inner = area.inner(Margin::new(1, 1));
                (inner, None, None)
            }
        };

        if rows_area.height == 0 || rows_area.width == 0 {
            return;
        }

        // Re-clamp against the height actually available. `visible_height` is a
        // nominal budget set at construction; a short terminal gives fewer rows
        // than that, and paging computed against the larger number would show
        // a half-empty window instead of scrolling.
        self.visible_height = rows_area.height as usize;
        self.clamp_selection();

        let mut lines: Vec<Line> = Vec::new();
        if self.entries.is_empty() {
            lines.push(Line::from(Span::styled(
                EMPTY_STATE,
                Style::default().fg(theme.muted),
            )));
            lines.push(Line::from(Span::styled(
                EMPTY_HINT,
                Style::default().fg(theme.muted),
            )));
        } else {
            let text_width = (rows_area.width as usize)
                .saturating_sub(STATUS_COLUMN_WIDTH)
                .saturating_sub(3);
            let start = self.scroll.get();
            let now_ms = chrono::Utc::now().timestamp_millis();
            for (offset, entry) in self.window().iter().enumerate() {
                let index = start + offset;
                let is_selected = index == self.selected;
                let marker = if is_selected { ">" } else { " " };
                let ordinal = format!("{:>3}", entry.ordinal);
                let body = QueueDialog::bounded_text(&entry.text, text_width);
                // M007: a scheduled row is marked by a **character**, not a
                // colour, so the distinction survives a monochrome terminal and
                // a colourblind reader.
                let due = due_marker(entry.run_at_ms, now_ms);
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{marker} {ordinal}{due} "),
                        self.row_style(is_selected),
                    ),
                    Span::styled(body, self.row_style(is_selected)),
                    Span::styled(
                        format!("  {}", QueueDialog::status_of(entry)),
                        self.detail_style(is_selected),
                    ),
                ]));
            }
        }

        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), rows_area);

        if let Some(compose_area) = compose_area {
            if compose_area.height > 0 && compose_area.width > 0 {
                // M007: the composer renders whichever field owns the cursor.
                // Showing the prompt text while the user types a duration would
                // make the box look like it had dropped their input.
                let field = if self.is_schedule_step() {
                    &self.schedule
                } else {
                    &self.compose
                };
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            compose_label(self.step),
                            Style::default().fg(theme.accent_text),
                        ),
                        Span::styled(
                            QueueDialog::bounded_text(field, compose_area.width as usize - 8),
                            Style::default().fg(theme.foreground),
                        ),
                    ])),
                    compose_area,
                );
            }
        }

        if let Some(hint_area) = hint_area {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(" {} ", self.hint()),
                    Style::default().fg(theme.muted),
                ))),
                hint_area,
            );
        }
    }

    /// The hint for the step actually on screen.
    ///
    /// Two constants rather than one composed string: the browsing verbs are
    /// *inert* in a text step, so advertising them while a composer is open
    /// teaches keys that would not work. Rendering the browsing hint under an
    /// active text field is the same lie as advertising an unimplemented key.
    fn hint(&self) -> &'static str {
        if self.is_schedule_step() {
            HINT_SCHEDULE
        } else if self.is_text_step() {
            HINT_TEXT
        } else {
            HINT_BROWWSING
        }
    }
}

// ── M007: due-time display ────────────────────────────────────────
//
// Deliberately *not* a relative-time framework. The plan puts "a human-readable
// relative-time renderer" out of scope, so this is the one helper that rounds a
// remaining duration to a coarse unit and nothing more. Anything that needs a
// calendar, a timezone, or "in 3 days 4 hours" belongs in a milestone that says
// so.

/// Render a remaining duration as `30m`, `2h`, `3d` — the largest whole unit.
///
/// **Rounds up**, which is the direction a "time until" display needs. Flooring
/// shows an entry due in 59m59s as `59m`, so a prompt the user scheduled "for an
/// hour" reads back as "59m" and looks like something went wrong. Rounding up
/// makes the worst case an entry due in 60s read as `2m`, which is harmless —
/// and `now` is still reserved for under a minute, where it is the truth.
fn format_remaining(remaining_ms: i64) -> String {
    let seconds = remaining_ms.max(0) / 1000;
    // Ceiling division, spelled out: `i64::div_ceil` is still unstable, and
    // `(a + b - 1) / b` is the idiom for it.
    let ceil = |unit: i64| (seconds + unit - 1) / unit;
    if seconds < 60 {
        return "now".to_string();
    }
    if seconds >= 86_400 {
        return format!("{}d", ceil(86_400));
    }
    if seconds >= 3_600 {
        return format!("{}h", ceil(3_600));
    }
    let minutes = ceil(60);
    // Round *up into the next unit* rather than printing "60m". An entry due in
    // 59m59s reads as "1h" on a row that otherwise speaks in whole units;
    // printing "60m" makes a correct value look like an off-by-one.
    if minutes >= 60 {
        format!("{}h", ceil(3_600))
    } else {
        format!("{}m", minutes)
    }
}

/// The two-character due tag for a row, or the empty string.
///
/// Three states, all distinguishable without colour:
///
/// - `"  "` — no due time: the entry has been eligible since it was queued.
/// - `"@3h"` — scheduled and still waiting.
/// - `"@·"` — due, i.e. the time has arrived and it is waiting for the next
///   turn rather than for the clock.
///
/// The last state is the one that matters: "scheduled for 09:00" and "09:00 has
/// passed and it still has not run" look identical otherwise, and they are very
/// different states for a user deciding whether to press `s`.
fn due_marker(run_at_ms: Option<i64>, now_ms: i64) -> String {
    match run_at_ms {
        None => "  ".to_string(),
        Some(run_at) if run_at <= now_ms => "@·".to_string(),
        Some(run_at) => format!("@{}", format_remaining(run_at - now_ms)),
    }
}

/// Seed text for the duration step from an entry's **remaining** time.
///
/// Returns `""` for an unscheduled entry, so an edit opens on an empty duration
/// field and the user can press `Enter` to keep it immediate. A due time already
/// in the past renders as `"now"`, which parses back to a due time that has
/// already passed — and the store accepts that, because a time the user has
/// already reached is eligible immediately rather than an error.
fn remaining_duration_text(run_at_ms: Option<i64>) -> String {
    match run_at_ms {
        None => String::new(),
        Some(run_at) => {
            format_remaining(run_at.saturating_sub(chrono::Utc::now().timestamp_millis()))
        }
    }
}

/// The `> ` prefix for the composer line, by step.
fn compose_label(step: QueueStep) -> String {
    match step {
        QueueStep::Browsing => "> ".to_string(),
        QueueStep::Adding => "+ ".to_string(),
        QueueStep::Editing => "~ ".to_string(),
        QueueStep::Scheduling => "@ ".to_string(),
        // Says *why* the text stopped instead of leaving the user to wonder
        // why their keystroke did nothing.
        QueueStep::BufferFull => "! ".to_string(),
    }
}

/// Terminal cell width for the characters this box renders.
///
/// A prompt is user text, so it can be non-ASCII. Counting `char`s would
/// mis-measure a CJK or emoji prompt and let the status column wrap out of
/// alignment; one cell per wide char is the conservative answer.
fn unicode_width_char(ch: char) -> usize {
    if ch.is_ascii() {
        1
    } else {
        2
    }
}

impl Component for QueueDialog {
    /// A plain viewer box: no composer, no filter buffer, no mode of its own.
    ///
    /// `App::dock_target` has no queue arm, so the panel docking gesture is
    /// refused for this box either way; declaring the capability keeps the
    /// claim honest rather than accidental.
    fn docks_as_viewer(&self) -> bool {
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        // M005: the text step is matched **first and separately**, so no action
        // letter can be live in it by accident. The alternative — a guard on
        // each action arm — is the shape that produces the bug this milestone
        // exists to avoid: add a sixth action, forget one guard, and `d`
        // deletes a prompt the user was typing. Here the browsing arm is simply
        // unreachable from a text step.
        //
        // This branch also answers keys the browsing arm owns. `Esc` cancels
        // the step rather than closing the box, so abandoning half-typed text
        // does not also unmount the list the user was working in.
        if self.is_text_step() {
            // M007: the duration step is a distinct field inside the same
            // guarded branch, so it inherits every property that keeps a stray
            // `d` from deleting a prompt. Its `Esc` goes *back* rather than
            // cancelling, because the prompt text is still in the buffer.
            return match key.code {
                KeyCode::Esc if self.is_schedule_step() => {
                    self.back_to_text_step();
                    None
                }
                KeyCode::Esc => {
                    self.cancel_text_step();
                    None
                }
                KeyCode::Enter if self.is_schedule_step() => self.commit_schedule_step(),
                KeyCode::Enter => self.commit_text_step(),
                KeyCode::Backspace => {
                    self.clear_buffer_full();
                    if self.is_schedule_step() {
                        self.schedule.pop();
                    } else {
                        self.backspace();
                    }
                    None
                }
                // SHIFT is accepted because uppercase is ordinary text; every
                // other modifier is a chord, not a character.
                KeyCode::Char(c)
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    self.clear_buffer_full();
                    if self.is_schedule_step() {
                        self.push_schedule_char(c);
                    } else {
                        self.insert_char(c);
                    }
                    None
                }
                _ => None,
            };
        }

        match key.code {
            KeyCode::Esc => Some(TuiMsg::CloseDialog),
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_up();
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_down();
                None
            }
            KeyCode::Char('r') if key.modifiers == KeyModifiers::NONE => {
                Some(TuiMsg::QueueRefreshRequested)
            }
            KeyCode::Char('a') if key.modifiers == KeyModifiers::NONE => {
                self.begin_add();
                None
            }
            KeyCode::Char('e') if key.modifiers == KeyModifiers::NONE => {
                self.begin_edit();
                None
            }
            // `delivering` is refused by the daemon; the box still offers the
            // key so the refusal is the daemon's answer to a real attempt
            // rather than a silent no-op the user cannot interpret.
            KeyCode::Char('d') if key.modifiers == KeyModifiers::NONE => self
                .selected_entry_id()
                .map(|id| TuiMsg::QueueDeleteSelected {
                    entry_id: id.to_string(),
                }),
            // M006. `s` is refused **locally** on a `delivering` row for the
            // same reason reorder is: that entry is already running as its own
            // turn, so the daemon could only answer "no", and a refusal the
            // daemon had to be asked for would cost a round trip to deliver a
            // fact the box already knows.
            //
            // There is no local check for "is a turn running" that would be
            // trustworthy — `active_turn_id` is a snapshot — so that one is
            // decided by the input layer, not here, and the daemon is the
            // authority when the two disagree.
            KeyCode::Char('s') if key.modifiers == KeyModifiers::NONE => {
                let entry_id = self.selected_entry_id()?;
                if self.selected_entry_status() == Some("delivering") {
                    return Some(TuiMsg::QueueSendNowUnavailable {
                        reason: QueueSendRefusal::Delivering,
                    });
                }
                Some(TuiMsg::QueueSendNowSelected {
                    entry_id: entry_id.to_string(),
                })
            }
            // `[`/`]` rather than `Shift+K`/`Shift+J`: the box already uses
            // `j`/`k` for movement, and a shifted variant of the movement key
            // to do something *other* than move is the kind of pairing people
            // misfire. `panels::direction_of` only claims bare `H`/`J`/`K`/`L`,
            // and the whole docking-steal branch is additionally dead for this
            // box because `App::dock_target` has no `DialogType::Queue` arm —
            // asserted by the host-first test rather than assumed here.
            //
            // A `delivering` selection is refused **locally**, which is the one
            // place this box departs from "let the daemon refuse". It has to:
            // the permutation would otherwise be built from the pending subset,
            // succeed, and reorder nothing — a silent no-op rather than an
            // honest refusal. There is no daemon call that can express "this
            // particular row is not yours to move".
            KeyCode::Char('[') | KeyCode::Char(']') if key.modifiers == KeyModifiers::NONE => {
                self.reorder_selected(key.code == KeyCode::Char('['))
            }
            _ => None,
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    /// The verbs this box owns, spelled the way this box spells them.
    fn keymap_hint(&self) -> Option<String> {
        Some(self.hint().to_string())
    }

    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let inner = area.inner(Margin::new(1, 1));
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        self.render_body(frame, inner, theme, Chrome::Hosted);
    }

    #[allow(clippy::incompatible_msrv)]
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, Chrome::Owned);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Queue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn entry(id: &str, ordinal: i64, text: &str, status: &str) -> UserPromptQueueEntryDto {
        UserPromptQueueEntryDto {
            id: id.to_string(),
            ordinal,
            text: text.to_string(),
            text_bytes: text.len(),
            status: status.to_string(),
            created_at_ms: 0,
            expires_at_ms: 0,
            run_at_ms: None,
        }
    }

    fn dialog(entries: Vec<UserPromptQueueEntryDto>) -> QueueDialog {
        QueueDialog::new(Arc::new(Theme::dark()), entries)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn rendered(app_entries: Vec<UserPromptQueueEntryDto>, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut dialog = dialog(app_entries);
        terminal
            .draw(|frame| dialog.render(frame, frame.area(), &Arc::new(Theme::dark())))
            .expect("draw");
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn user_prompt_queue_selection_moves_with_jk_and_arrows_and_clamps() {
        let mut d = dialog(vec![
            entry("a", 0, "first", "pending"),
            entry("b", 1, "second", "pending"),
        ]);

        assert_eq!(d.handle_key(key(KeyCode::Char('j'))), None);
        assert_eq!(d.selected_index(), 1);
        assert_eq!(d.handle_key(key(KeyCode::Down)), None);
        assert_eq!(d.selected_index(), 1, "must clamp at the last row");
        assert_eq!(d.handle_key(key(KeyCode::Char('k'))), None);
        assert_eq!(d.selected_index(), 0);
        assert_eq!(d.handle_key(key(KeyCode::Up)), None);
        assert_eq!(d.selected_index(), 0, "must clamp at the first row");
    }

    #[test]
    fn user_prompt_queue_delete_names_the_selected_entry() {
        let mut d = dialog(vec![
            entry("a", 0, "first", "pending"),
            entry("b", 1, "second", "delivering"),
        ]);
        d.handle_key(key(KeyCode::Char('j')));
        assert_eq!(
            d.handle_key(key(KeyCode::Char('d'))),
            Some(TuiMsg::QueueDeleteSelected {
                entry_id: "b".to_string()
            })
        );
    }

    /// A `delivering` row is offered to the daemon and refused there. The box
    /// must not pre-empt that: the refusal is the authority's answer, and a
    /// local "you can't delete that" would be a second, weaker one.
    #[test]
    fn user_prompt_queue_delete_is_still_offered_for_a_delivering_row() {
        let mut d = dialog(vec![entry("a", 0, "first", "delivering")]);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('d'))),
            Some(TuiMsg::QueueDeleteSelected {
                entry_id: "a".to_string()
            })
        );
    }

    #[test]
    fn user_prompt_queue_delete_on_an_empty_queue_is_not_an_action() {
        let mut d = dialog(Vec::new());
        assert_eq!(d.handle_key(key(KeyCode::Char('d'))), None);
    }

    /// Selection follows the entry id across a projection that dropped a
    /// different row. Keying on the index would silently slide the cursor onto
    /// a prompt the user never looked at.
    #[test]
    fn user_prompt_queue_selection_follows_the_entry_id_not_the_index() {
        let mut d = dialog(vec![
            entry("a", 0, "first", "pending"),
            entry("b", 1, "second", "pending"),
        ]);
        d.handle_key(key(KeyCode::Char('j')));
        assert_eq!(d.selected_entry_id(), Some("b"));

        // `a` was delivered and left the projection; `b` moved up a row.
        d.set_entries(vec![entry("b", 0, "second", "pending")]);

        assert_eq!(
            d.selected_entry_id(),
            Some("b"),
            "the cursor must stay on the entry the user selected"
        );
        assert_eq!(d.selected_index(), 0);
    }

    /// The deleted row is the one that disappears, so selection has to land on
    /// something real rather than leave an index pointing past the end.
    #[test]
    fn user_prompt_queue_selection_clamps_when_the_selected_entry_is_gone() {
        let mut d = dialog(vec![
            entry("a", 0, "first", "pending"),
            entry("b", 1, "second", "pending"),
            entry("c", 2, "third", "pending"),
        ]);
        d.handle_key(key(KeyCode::Char('j')));
        d.handle_key(key(KeyCode::Char('j')));
        assert_eq!(d.selected_entry_id(), Some("c"));

        d.set_entries(vec![
            entry("a", 0, "first", "pending"),
            entry("b", 1, "second", "pending"),
        ]);

        assert_eq!(d.selected_entry_id(), Some("b"));
        assert_eq!(d.selected_index(), 1);
    }

    #[test]
    fn user_prompt_queue_empty_projection_selects_nothing_and_deletes_nothing() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        d.set_entries(Vec::new());
        assert_eq!(d.selected_entry_id(), None);
        assert_eq!(d.handle_key(key(KeyCode::Char('d'))), None);
    }

    /// M005 inverts M004's version of this test. Before M005 the empty state named
    /// **no** verb for creating an entry, because naming one that does nothing
    /// is worse than saying nothing. Now `a` exists, so the empty state must
    /// name it — and must still not name a verb this box does not implement.
    #[test]
    fn user_prompt_queue_empty_state_names_add_and_nothing_it_cannot_do() {
        let text = rendered(Vec::new(), 80, 12);
        assert!(text.contains(EMPTY_STATE), "got {text:?}");
        assert!(text.contains(EMPTY_HINT), "got {text:?}");
        assert!(
            text.contains('a'),
            "an empty queue must now say how to put something in it: {text:?}"
        );
        // M005 wrote this list to forbid verbs the box did not implement. Two
        // have since been implemented — M006 added `s send now`, M007 added the
        // duration step — and leaving them here would have failed on the very
        // hint they taught. The list is now "what this box still cannot do".
        //
        // This was a stale M006 assertion, not an M007 regression: `s send now`
        // reached `HINT_BROWWSING` and this test continued to forbid it.
        for forbidden in ["adopt", "transfer", "timer", "later"] {
            assert!(
                !text.contains(forbidden),
                "empty state advertised {forbidden:?}, which this box does not implement: {text:?}"
            );
        }
        // The verbs that *are* implemented must be advertised.
        assert!(
            text.contains("send now"),
            "M006 teaches `s send now` in the browsing hint; the empty state must not hide it: {text:?}"
        );
    }

    #[test]
    fn user_prompt_queue_empty_state_does_not_promise_a_close_verb() {
        let text = rendered(Vec::new(), 60, 12);
        assert!(
            !text.contains("Esc close") && !text.contains("Enter close"),
            "the box must not advertise a close verb the modal host answers for it: {text:?}"
        );
    }

    #[test]
    fn user_prompt_queue_renders_a_single_row_with_its_status() {
        let text = rendered(vec![entry("a", 0, "only one", "pending")], 60, 12);
        assert!(text.contains("only one"), "got {text:?}");
        assert!(text.contains("pending"), "got {text:?}");
    }

    /// The status column is the reason a `delivering` row is visible at all.
    #[test]
    fn user_prompt_queue_renders_the_delivering_status_from_the_dto() {
        let text = rendered(vec![entry("a", 0, "running now", "delivering")], 60, 12);
        assert!(
            text.contains("delivering"),
            "a row whose turn is running must say so: {text:?}"
        );
    }

    #[test]
    fn user_prompt_queue_truncates_a_long_prompt_and_says_so() {
        let long = "x".repeat(400);
        let text = rendered(vec![entry("a", 0, &long, "pending")], 60, 12);
        assert!(
            text.contains('…'),
            "a quietly shortened prompt reads as the whole prompt: {text:?}"
        );
        assert!(
            !text.contains(&"x".repeat(300)),
            "the row must actually be bounded: {text:?}"
        );
    }

    /// Rows must page rather than silently clip: a queue of 40 with a 10-row
    /// window that never scrolls makes entries past the tenth unreachable.
    #[test]
    fn user_prompt_queue_pages_when_rows_exceed_the_visible_height() {
        let entries: Vec<UserPromptQueueEntryDto> = (0..40)
            .map(|i| {
                entry(
                    &format!("e{i}"),
                    i as i64,
                    &format!("prompt-{i}"),
                    "pending",
                )
            })
            .collect();
        let mut d = dialog(entries);
        for _ in 0..20 {
            d.handle_key(key(KeyCode::Char('j')));
        }
        let text = {
            let backend = TestBackend::new(60, 14);
            let mut terminal = Terminal::new(backend).expect("terminal");
            terminal
                .draw(|frame| d.render(frame, frame.area(), &Arc::new(Theme::dark())))
                .expect("draw");
            let buf = terminal.backend().buffer().clone();
            (0..buf.area.height)
                .map(|y| {
                    (0..buf.area.width)
                        .map(|x| buf[(x, y)].symbol().to_string())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(
            text.contains("prompt-20"),
            "the cursor's row must be on screen after paging: {text:?}"
        );
    }

    #[test]
    fn user_prompt_queue_keymap_hint_names_only_keys_it_handles() {
        let d = dialog(vec![entry("a", 0, "first", "pending")]);
        let hint = d.keymap_hint().expect("hint");
        assert_eq!(hint, HINT_BROWWSING);
        for named in ["j", "k", "r", "d", "a", "e", "[", "]"] {
            assert!(hint.contains(named), "hint is missing {named}: {hint:?}");
        }
        // `Esc` is answered by the host before this box; `Enter` is this box's
        // **confirm** key, which is not a browsing verb. Neither belongs in the
        // browsing hint, and advertising either teaches a key that either never
        // arrives or does something other than what the browsing line implies.
        for forbidden in ["Esc", "Enter", "move up", "move down"] {
            assert!(
                !hint.contains(forbidden),
                "hint advertised {forbidden:?}, which this box does not implement here: {hint:?}"
            );
        }
    }

    /// The hint must describe the step that is actually on screen.
    ///
    /// Advertising `d delete` above an open composer is the same lie as
    /// advertising an unimplemented key: the user reads a promise the box does
    /// not keep until they press the key.
    #[test]
    fn user_prompt_queue_hint_follows_the_step() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        assert_eq!(d.keymap_hint().as_deref(), Some(HINT_BROWWSING));

        d.begin_add();
        assert_eq!(
            d.keymap_hint().as_deref(),
            Some(HINT_TEXT),
            "a text step must not advertise the browsing verbs, which are inert there"
        );
        assert!(
            !HINT_TEXT.contains('d'),
            "the text hint must not contain a letter that is an action key: {HINT_TEXT:?}"
        );

        d.cancel_text_step();
        assert_eq!(d.keymap_hint().as_deref(), Some(HINT_BROWWSING));
    }

    /// Every key the hint names must actually reach an action through
    /// `handle_key`. Non-vacuous: `z` is asserted to do nothing.
    #[test]
    fn user_prompt_queue_every_hinted_key_reaches_an_action() {
        let mut d = dialog(vec![
            entry("a", 0, "first", "pending"),
            entry("b", 1, "second", "pending"),
        ]);
        assert_eq!(d.handle_key(key(KeyCode::Char('j'))), None);
        assert_eq!(d.selected_index(), 1, "`j` must move the cursor");
        assert_eq!(d.handle_key(key(KeyCode::Char('k'))), None);
        assert_eq!(d.selected_index(), 0, "`k` must move the cursor");
        assert_eq!(
            d.handle_key(key(KeyCode::Char('d'))),
            Some(TuiMsg::QueueDeleteSelected {
                entry_id: "a".to_string()
            }),
            "`d` must ask for the delete"
        );
        assert_eq!(
            d.handle_key(key(KeyCode::Char('r'))),
            Some(TuiMsg::QueueRefreshRequested),
            "`r` must ask for the re-read"
        );

        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('z'))),
            None,
            "an unhinted key must do nothing, or the hint is describing a fiction"
        );
    }

    #[test]
    fn user_prompt_queue_esc_closes() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        assert_eq!(d.handle_key(key(KeyCode::Esc)), Some(TuiMsg::CloseDialog));
    }

    #[test]
    fn user_prompt_queue_dialog_type_is_the_queue_box() {
        let d = dialog(Vec::new());
        assert_eq!(d.dialog_type(), DialogType::Queue);
        assert!(d.is_modal());
    }

    // ── M005: the text step ───────────────────────────────────────────

    /// Type into an open composer through the real key path.
    fn type_text(d: &mut QueueDialog, text: &str) {
        for c in text.chars() {
            d.handle_key(key(KeyCode::Char(c)));
        }
    }

    /// WP-A's acceptance sweep: every action letter, typed into a text step,
    /// must land in the buffer and trigger nothing.
    ///
    /// This is the milestone's headline hazard. `d` deleting a prompt because
    /// the user was typing one, or `e` opening an editor mid-word, is the
    /// failure the whole `is_text_step` seam exists to prevent.
    #[test]
    fn user_prompt_queue_text_step_swallows_every_action_letter() {
        // Each of these is a verb in the browsing step. If any reaches its
        // action here, the guard is missing.
        for letter in ['d', 'e', 'a', 'j', 'k', 'r', '[', ']'] {
            let mut d = dialog(vec![
                entry("a", 0, "first", "pending"),
                entry("b", 1, "second", "pending"),
            ]);
            d.handle_key(key(KeyCode::Char('a')));
            assert!(d.is_text_step());
            let before = d.selected_index();

            let msg = d.handle_key(key(KeyCode::Char(letter)));

            assert_eq!(
                msg, None,
                "typing {letter:?} into a text step fired an action: {msg:?}"
            );
            assert_eq!(d.compose(), letter.to_string(), "{letter:?} must be text");
            assert_eq!(
                d.selected_index(),
                before,
                "typing {letter:?} must not move the cursor"
            );
            assert!(
                d.is_text_step(),
                "typing {letter:?} must not leave the text step"
            );
        }
    }

    /// Non-vacuity for the sweep above, and the reason the sweep means
    /// anything: the *same* keys are live verbs outside a text step. If these
    /// also did nothing, the sweep would be satisfied by a box implementing no
    /// keys at all.
    #[test]
    fn user_prompt_queue_action_letters_are_live_outside_a_text_step() {
        let mut d = dialog(vec![
            entry("a", 0, "first", "pending"),
            entry("b", 1, "second", "pending"),
        ]);
        // `j` moves the cursor — no message, but a real effect.
        assert_eq!(d.handle_key(key(KeyCode::Char('j'))), None);
        assert_eq!(d.selected_index(), 1);

        // `d` and `r` emit their messages.
        assert!(matches!(
            d.handle_key(key(KeyCode::Char('d'))),
            Some(TuiMsg::QueueDeleteSelected { .. })
        ));
        assert!(matches!(
            d.handle_key(key(KeyCode::Char('r'))),
            Some(TuiMsg::QueueRefreshRequested)
        ));

        // `a` and `e` enter their text steps.
        assert_eq!(d.handle_key(key(KeyCode::Char('a'))), None);
        assert_eq!(d.step(), QueueStep::Adding);
        d.cancel_text_step();
        assert_eq!(d.handle_key(key(KeyCode::Char('e'))), None);
        assert_eq!(d.step(), QueueStep::Editing);
    }

    /// Backspace edits text; it never deletes a row. The two meanings sit one
    /// keystroke apart while this box is authoring.
    #[test]
    fn user_prompt_queue_backspace_edits_text_and_never_deletes() {
        let mut d = dialog(vec![
            entry("a", 0, "first", "pending"),
            entry("b", 1, "second", "pending"),
        ]);
        d.begin_add();
        type_text(&mut d, "delete this");
        d.handle_key(key(KeyCode::Backspace));
        assert_eq!(d.compose(), "delete thi");
        assert_eq!(d.entries().len(), 2, "backspace must not delete a row");
    }

    // ── M005: add ─────────────────────────────────────────────────────

    #[test]
    fn user_prompt_queue_add_commits_text_without_an_entry_id() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        d.handle_key(key(KeyCode::Char('a')));
        type_text(&mut d, "ship it");
        // M007: the text step now *advances* to the duration step; committing
        // is the second Enter. M005's single-Enter commit is superseded, and
        // this test still asserts the same contract — the add carries the
        // composed text and no entry id — one step later.
        assert!(
            d.handle_key(key(KeyCode::Enter)).is_none(),
            "Enter on the text step advances rather than committing"
        );
        assert_eq!(d.step(), QueueStep::Scheduling);
        let msg = d.handle_key(key(KeyCode::Enter)).expect("Enter commits");
        assert!(
            matches!(&msg, TuiMsg::QueueAddCommitted { text, .. } if text == "ship it"),
            "add must carry the composed text: {msg:?}"
        );
        assert_eq!(
            d.step(),
            QueueStep::Browsing,
            "the step closes on commit so the box is navigable while the write flies"
        );
    }

    /// Whitespace is refused without spending a round trip: the daemon would
    /// refuse it too, but this box already knows.
    #[test]
    fn user_prompt_queue_add_of_blank_text_is_not_a_commit() {
        let mut d = dialog(Vec::new());
        d.begin_add();
        type_text(&mut d, "   ");
        assert_eq!(d.handle_key(key(KeyCode::Enter)), None);
    }

    /// Two identical prompts are two entries. Idempotency is entry identity,
    /// never text equality — this is the invariant that stops "add the same
    /// thing twice" from being quietly deduplicated into one.
    #[test]
    fn user_prompt_queue_identical_text_is_not_a_duplicate() {
        let mut d = dialog(Vec::new());
        for id in ["mint-1", "mint-2"] {
            d.begin_add();
            type_text(&mut d, "same words");
            d.handle_key(key(KeyCode::Enter)); // M007: advance to duration
            let msg = d.handle_key(key(KeyCode::Enter)).expect("commits");
            let TuiMsg::QueueAddCommitted { text, .. } = msg else {
                panic!("expected an add, got {msg:?}");
            };
            assert_eq!(text, "same words");
            let mut next = d.entries().to_vec();
            next.push(entry(id, next.len() as i64, "same words", "pending"));
            d.set_entries(next);
        }
        assert_eq!(d.entries().len(), 2, "equal text is not equal identity");
        assert_ne!(
            d.entries()[0].id,
            d.entries()[1].id,
            "two entries with the same text must still be distinct rows"
        );
    }

    // ── M005: edit ────────────────────────────────────────────────────

    #[test]
    fn user_prompt_queue_edit_is_seeded_with_the_selected_text() {
        let mut d = dialog(vec![
            entry("a", 0, "first", "pending"),
            entry("b", 1, "second prompt", "pending"),
        ]);
        d.handle_key(key(KeyCode::Char('j')));
        assert!(d.begin_edit());
        assert_eq!(d.step(), QueueStep::Editing);
        assert_eq!(d.editing_entry_id(), Some("b"));
        assert_eq!(
            d.compose(),
            "second prompt",
            "an edit starts from what the user can see on the row"
        );
    }

    /// An edit reuses the entry's identity. Minting a new id would turn
    /// "correct this prompt" into "add a second prompt and orphan the first".
    #[test]
    fn user_prompt_queue_edit_commits_the_same_entry_id() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        assert!(d.begin_edit());
        d.handle_key(key(KeyCode::Backspace));
        type_text(&mut d, "!");
        d.handle_key(key(KeyCode::Enter)); // M007: advance to duration
        let msg = d.handle_key(key(KeyCode::Enter)).expect("Enter commits");
        assert!(
            matches!(&msg, TuiMsg::QueueEditCommitted { entry_id, .. } if entry_id == "a"),
            "an edit must name the existing entry: {msg:?}"
        );
    }

    /// An empty box has nothing to revise, so `e` is not a commit.
    #[test]
    fn user_prompt_queue_edit_on_an_empty_queue_does_nothing() {
        let mut d = dialog(Vec::new());
        assert!(!d.begin_edit());
        assert_eq!(d.step(), QueueStep::Browsing);
    }

    /// A bounds refusal must leave the user's words in the composer — the fix
    /// is to shorten them, and that is impossible if they were dropped.
    #[test]
    fn user_prompt_queue_a_refusal_restores_the_composed_text() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        d.begin_edit();
        type_text(&mut d, "much too long");
        let taken = d.take_compose().expect("a commit consumes the text");
        assert!(d.compose().is_empty(), "commit empties the buffer");

        d.restore_compose(taken.0, taken.1);
        assert_eq!(d.compose(), "firstmuch too long");
        assert_eq!(d.step(), QueueStep::Editing);
        assert_eq!(d.editing_entry_id(), Some("a"));
    }

    /// Restoring an *add* returns to the add step, not the edit step — the step
    /// is what the box renders and what the next `Enter` means.
    #[test]
    fn user_prompt_queue_restoring_an_add_returns_to_the_add_step() {
        let mut d = dialog(Vec::new());
        d.begin_add();
        type_text(&mut d, "typed then refused");
        let taken = d.take_compose().expect("a commit consumes the text");
        d.restore_compose(taken.0, taken.1);
        assert_eq!(d.step(), QueueStep::Adding);
        assert_eq!(d.editing_entry_id(), None);
    }

    // ── M005: reorder ─────────────────────────────────────────────────

    /// §3(a), the milestone's sharpest edge.
    ///
    /// The rendered projection contains `pending` **and** `delivering`; the
    /// daemon's `reorder` checks the permutation against pending rows only,
    /// with exact length equality. A client that sends everything it rendered
    /// is refused the moment one row is mid-delivery — so the permutation
    /// built here must name the pending rows alone.
    #[test]
    fn user_prompt_queue_permutation_omits_delivering_rows() {
        let d = dialog(vec![
            entry("a", 0, "first", "pending"),
            entry("b", 1, "second", "delivering"),
            entry("c", 2, "third", "pending"),
        ]);

        let ids = d
            .pending_permutation("c", true)
            .expect("`c` has a neighbour to swap with");

        assert_eq!(
            ids,
            vec!["c".to_string(), "a".to_string()],
            "the permutation is the pending subset only, permuted — naming `b` \
             would be answered with order_mismatch"
        );
        assert!(
            !ids.iter().any(|id| id == "b"),
            "a delivering row must never be named: got {ids:?}"
        );
    }

    /// The complement of the filter: the move is a swap with the immediate
    /// neighbour — never a resort, and never a locally invented ordinal.
    #[test]
    fn user_prompt_queue_permutation_is_a_neighbour_swap_not_a_resort() {
        let d = dialog(vec![
            entry("a", 0, "alpha", "pending"),
            entry("b", 1, "beta", "pending"),
            entry("c", 2, "gamma", "pending"),
            entry("d", 3, "delta", "pending"),
        ]);

        assert_eq!(
            d.pending_permutation("c", true),
            Some(vec!["a".into(), "c".into(), "b".into(), "d".into()]),
            "moving `c` earlier swaps it with `b` and leaves everything else alone"
        );
        assert_eq!(
            d.pending_permutation("b", false),
            Some(vec!["a".into(), "c".into(), "b".into(), "d".into()]),
            "moving `b` later is the same swap read the other way"
        );
        assert_eq!(
            d.pending_permutation("a", false),
            Some(vec!["b".into(), "a".into(), "c".into(), "d".into()]),
            "one step, not a jump to the end"
        );
    }

    /// At the ends there is nothing to swap with, which is a refusal rather
    /// than a no-op move that reports success.
    #[test]
    fn user_prompt_queue_permutation_at_the_ends_is_refused() {
        let d = dialog(vec![
            entry("a", 0, "alpha", "pending"),
            entry("b", 1, "beta", "pending"),
        ]);
        assert_eq!(d.pending_permutation("a", true), None, "already first");
        assert_eq!(d.pending_permutation("b", false), None, "already last");
    }

    /// A `delivering` row cannot be reordered, and this box says so itself.
    ///
    /// This is the one place M005 refuses locally rather than letting the
    /// daemon answer, and it has to: the permutation would otherwise be built
    /// from the pending subset, succeed, and reorder nothing — a silent no-op.
    #[test]
    fn user_prompt_queue_reorder_of_a_delivering_row_is_refused_locally() {
        let mut d = dialog(vec![
            entry("a", 0, "first", "pending"),
            entry("b", 1, "second", "delivering"),
        ]);
        d.handle_key(key(KeyCode::Char('j')));
        assert!(!d.selected_is_reorderable());
        for key_code in [KeyCode::Char('['), KeyCode::Char(']')] {
            assert_eq!(
                d.handle_key(key(key_code)),
                Some(TuiMsg::QueueReorderUnavailable {
                    reason: QueueReorderRefusal::Delivering
                }),
                "{key_code:?} must refuse a delivering row"
            );
        }
    }

    /// The refusal names *which* end the row is at, because "already first" and
    /// "being delivered" call for different user actions.
    #[test]
    fn user_prompt_queue_reorder_at_an_end_names_which_end() {
        let mut d = dialog(vec![
            entry("a", 0, "alpha", "pending"),
            entry("b", 1, "beta", "pending"),
        ]);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('['))),
            Some(TuiMsg::QueueReorderUnavailable {
                reason: QueueReorderRefusal::AlreadyEarliest
            })
        );
        d.handle_key(key(KeyCode::Char('j')));
        assert_eq!(
            d.handle_key(key(KeyCode::Char(']'))),
            Some(TuiMsg::QueueReorderUnavailable {
                reason: QueueReorderRefusal::AlreadyLatest
            })
        );
    }

    /// A `delivering` row *elsewhere* in the queue must not block a reorder of
    /// a pending one. This is the counterpart to the filter above: the row is
    /// excluded, not fatal.
    #[test]
    fn user_prompt_queue_a_delivering_row_elsewhere_does_not_block_a_reorder() {
        let mut d = dialog(vec![
            entry("a", 0, "alpha", "delivering"),
            entry("b", 1, "beta", "pending"),
            entry("c", 2, "gamma", "pending"),
        ]);
        d.handle_key(key(KeyCode::Char('j')));
        d.handle_key(key(KeyCode::Char('j')));
        assert_eq!(d.selected_entry_id(), Some("c"));
        assert!(d.selected_is_reorderable());
        assert_eq!(
            d.handle_key(key(KeyCode::Char('['))),
            Some(TuiMsg::QueueReorderSelected {
                entry_ids: vec!["c".into(), "b".into()]
            })
        );
    }

    /// A key the box does not bind still does nothing, so the assertions above
    /// about `[`/`]` reaching an action are not vacuous.
    #[test]
    fn user_prompt_queue_an_unbound_key_still_does_nothing() {
        let mut d = dialog(vec![
            entry("a", 0, "alpha", "pending"),
            entry("b", 1, "beta", "pending"),
        ]);
        assert_eq!(d.handle_key(key(KeyCode::Char('z'))), None);
        assert_eq!(d.selected_index(), 0);
    }

    // ── M005: lifecycle ───────────────────────────────────────────────

    /// Acceptance criterion "closing the box with unsaved text adds nothing and
    /// edits nothing".
    #[test]
    fn user_prompt_queue_escaping_a_text_step_discards_it() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        d.begin_edit();
        type_text(&mut d, "half typed");
        assert!(d.is_text_step());
        d.handle_key(key(KeyCode::Esc));
        assert!(!d.is_text_step());
        assert_eq!(d.step(), QueueStep::Browsing);
        assert!(d.compose().is_empty(), "abandoned text is not retained");
        assert_eq!(d.editing_entry_id(), None);
    }

    /// `Esc` cancels the composer; it does not close the list underneath it.
    /// Whether the host claims `Esc` before this widget is a host question, and
    /// the box must be safe either way — so it never answers `CloseDialog`
    /// from a text step.
    #[test]
    fn user_prompt_queue_escape_from_a_text_step_is_never_a_close() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        d.begin_add();
        type_text(&mut d, "typing");
        assert_eq!(
            d.handle_key(key(KeyCode::Esc)),
            None,
            "Esc inside a composer must not answer CloseDialog"
        );
    }

    /// And from browsing it is a close, as M004 defined it.
    #[test]
    fn user_prompt_queue_escape_while_browsing_still_closes() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        assert_eq!(d.handle_key(key(KeyCode::Esc)), Some(TuiMsg::CloseDialog));
    }

    /// A buffer at its bound refuses the next character and says so, rather
    /// than quietly shortening the prompt.
    #[test]
    fn user_prompt_queue_a_full_buffer_refuses_rather_than_truncating() {
        let mut d = dialog(Vec::new());
        d.begin_add();
        d.compose = "x".repeat(MAX_COMPOSE_BYTES - 1);
        assert!(d.insert_char('x'), "one byte short of the bound must fit");
        assert_eq!(d.compose().len(), MAX_COMPOSE_BYTES);

        assert!(
            !d.insert_char('y'),
            "an append past the bound must be refused, not dropped"
        );
        assert_eq!(
            d.compose().len(),
            MAX_COMPOSE_BYTES,
            "the refused character must not have been silently truncated away"
        );
        assert_eq!(d.step(), QueueStep::BufferFull);

        // Backspacing clears the latch and resumes composing.
        d.handle_key(key(KeyCode::Backspace));
        assert_eq!(d.step(), QueueStep::Adding);
        assert!(d.insert_char('y'));
    }

    /// The local bound must sit above the daemon's own, or this box would be
    /// the thing refusing prompts the queue would have accepted.
    #[test]
    fn user_prompt_queue_the_local_buffer_bound_is_above_the_daemon_limit() {
        let daemon = codegg_core::session::user_prompt_queue::USER_PROMPT_QUEUE_MAX_ENTRY_BYTES;
        assert!(
            MAX_COMPOSE_BYTES > daemon,
            "the local guard ({MAX_COMPOSE_BYTES}) must sit above the daemon's \
             limit ({daemon}), or the box refuses text the queue would accept"
        );
    }

    /// Selection is keyed on identity, so an add that lands a new row can move
    /// the cursor onto it — and a row that is not there fails rather than
    /// clamping onto a different prompt.
    #[test]
    fn user_prompt_queue_select_entry_id_moves_the_cursor_by_identity() {
        let mut d = dialog(vec![entry("a", 0, "alpha", "pending")]);
        assert!(d.select_entry_id("a"));
        assert_eq!(d.selected_index(), 0);
        d.set_entries(vec![
            entry("a", 0, "alpha", "pending"),
            entry("new", 1, "just added", "pending"),
        ]);
        assert!(d.select_entry_id("new"));
        assert_eq!(d.selected_entry_id(), Some("new"));
        assert!(
            !d.select_entry_id("never-existed"),
            "selecting an absent row must fail rather than land on another prompt"
        );
        assert_eq!(d.selected_entry_id(), Some("new"));
    }

    /// A projection landing mid-edit must not retarget the edit or clear the
    /// buffer: the edit names an id, and the buffer is the seed plus whatever
    /// the user has typed since.
    #[test]
    fn user_prompt_queue_a_projection_mid_edit_preserves_the_buffer() {
        let mut d = dialog(vec![
            entry("a", 0, "alpha", "pending"),
            entry("b", 1, "beta", "pending"),
        ]);
        d.handle_key(key(KeyCode::Char('j')));
        d.begin_edit();
        type_text(&mut d, " being typed");
        let edit_id = d.editing_entry_id().map(str::to_owned);
        let compose = d.compose().to_owned();

        // `b` is delivered and leaves the projection while the edit is open.
        d.set_entries(vec![entry("a", 0, "alpha", "pending")]);

        assert_eq!(
            d.editing_entry_id(),
            edit_id.as_deref(),
            "the edit keeps its target"
        );
        assert_eq!(d.compose(), compose, "unsaved text survives");
        assert!(d.is_text_step());
    }

    // ── M007 (ADR-0039): the duration step ──────────────────────────

    /// A scheduled entry helper, so a test can express "due in an hour"
    /// without repeating the DTO literal.
    fn scheduled(id: &str, ordinal: i64, text: &str, run_at_ms: i64) -> UserPromptQueueEntryDto {
        UserPromptQueueEntryDto {
            run_at_ms: Some(run_at_ms),
            ..entry(id, ordinal, text, "pending")
        }
    }

    fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    /// Adding is two steps: text, then duration. `Enter` on the text step must
    /// NOT commit, or a schedule could never be expressed.
    #[test]
    fn user_prompt_queue_add_advances_to_a_duration_step_before_committing() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        d.begin_add();
        type_text(&mut d, "ship it");
        assert!(
            d.handle_key(key(KeyCode::Enter)).is_none(),
            "Enter on the text step must advance, not commit"
        );
        assert_eq!(d.step(), QueueStep::Scheduling);
        assert_eq!(
            d.compose(),
            "ship it",
            "the text stays in the buffer across the duration step so Esc can go back"
        );
        assert_eq!(d.schedule(), "", "a fresh add starts with no duration");

        let msg = d.handle_key(key(KeyCode::Enter));
        assert!(
            matches!(&msg, Some(TuiMsg::QueueAddCommitted { text, run_at_ms: None })
                if text == "ship it"),
            "an empty duration commits an immediate entry: {msg:?}"
        );
    }

    /// A parsed duration becomes an absolute due time, not a string — and not a
    /// duration that would be re-resolved at submit time.
    #[test]
    fn user_prompt_queue_duration_step_resolves_a_parsed_delay_to_an_instant() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        let before = now_ms();
        d.begin_add();
        type_text(&mut d, "later");
        d.handle_key(key(KeyCode::Enter));
        type_text(&mut d, "2h");
        let msg = d.handle_key(key(KeyCode::Enter)).expect("commits");
        let TuiMsg::QueueAddCommitted { run_at_ms, .. } = msg else {
            panic!("expected an add, got {msg:?}");
        };
        let run_at = run_at_ms.expect("2h must resolve to a due instant");
        let two_hours = 2 * 60 * 60 * 1000;
        assert!(
            run_at >= before + two_hours && run_at <= now_ms() + two_hours,
            "2h must resolve to roughly two hours out"
        );
    }

    /// An unparseable duration is refused **in the box**, with no request, and
    /// the input is preserved so the fix is an edit rather than a retype.
    #[test]
    fn user_prompt_queue_unparseable_duration_is_refused_locally_and_preserved() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        d.begin_add();
        type_text(&mut d, "later");
        d.handle_key(key(KeyCode::Enter));
        type_text(&mut d, "1 hr");
        let msg = d.handle_key(key(KeyCode::Enter)).expect("refuses");
        assert!(
            matches!(&msg, TuiMsg::QueueScheduleRejected { input } if input == "1 hr"),
            "an unparseable duration must be refused by the box: {msg:?}"
        );
        assert_eq!(d.schedule(), "1 hr", "the refused input must be preserved");
        assert_eq!(
            d.step(),
            QueueStep::Scheduling,
            "still in the duration step"
        );
        assert_eq!(
            d.compose(),
            "later",
            "the prompt text must survive a rejected duration"
        );
    }

    /// The plan's §3.2 rule, asserted where the user meets it: a bare number is
    /// refused rather than guessed, and `1 hr` is not a spelling.
    #[test]
    fn user_prompt_queue_a_bare_number_is_refused_rather_than_guessed() {
        for input in ["5", "1 hr", "2 hours", "1h30m", ""] {
            let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
            d.begin_add();
            type_text(&mut d, "prompt");
            d.handle_key(key(KeyCode::Enter));
            if input.is_empty() {
                // Empty is the legitimate "no schedule" answer, not a refusal.
                assert!(
                    matches!(
                        d.handle_key(key(KeyCode::Enter)),
                        Some(TuiMsg::QueueAddCommitted { .. })
                    ),
                    "an empty duration must commit, not refuse"
                );
                continue;
            }
            type_text(&mut d, input);
            let msg = d.handle_key(key(KeyCode::Enter)).expect("refuses");
            assert!(
                matches!(&msg, TuiMsg::QueueScheduleRejected { .. }),
                "{input:?} must be refused, not guessed: {msg:?}"
            );
        }
    }

    /// `Esc` in the duration step goes **back**, not out. Discarding the prompt
    /// a mistyped `2d` would be the wrong cost.
    #[test]
    fn user_prompt_queue_escape_in_the_duration_step_returns_to_the_text() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        d.begin_add();
        type_text(&mut d, "keep me");
        d.handle_key(key(KeyCode::Enter));
        type_text(&mut d, "2d");
        assert!(
            d.handle_key(key(KeyCode::Esc)).is_none(),
            "Esc in the duration step is not a commit"
        );
        assert_eq!(d.step(), QueueStep::Adding);
        assert_eq!(
            d.compose(),
            "keep me",
            "the prompt survives a back-navigation"
        );
        assert_eq!(d.schedule(), "", "the abandoned duration is cleared");

        // A second Esc, from the text step, does cancel.
        d.handle_key(key(KeyCode::Esc));
        assert_eq!(d.step(), QueueStep::Browsing);
    }

    /// Editing a due time is first-class: it opens seeded with what the entry
    /// already carries, as **remaining** time, and commits a new one.
    #[test]
    fn user_prompt_queue_edit_opens_seeded_with_the_entries_remaining_time() {
        let mut d = dialog(vec![scheduled("a", 0, "scheduled", now_ms() + 3_600_000)]);
        assert!(d.begin_edit());
        assert_eq!(d.step(), QueueStep::Editing);
        let seeded = d.schedule().to_string();
        assert!(
            seeded.starts_with('1') && seeded.ends_with('h'),
            "an hour out must seed as roughly 1h, got {seeded:?}"
        );
        type_text(&mut d, "!");

        // Enter on the text, then change the duration, then commit.
        d.handle_key(key(KeyCode::Enter));
        for _ in 0..seeded.len() {
            d.handle_key(key(KeyCode::Backspace));
        }
        type_text(&mut d, "5min");
        let msg = d.handle_key(key(KeyCode::Enter)).expect("commits");
        let TuiMsg::QueueEditCommitted {
            entry_id,
            run_at_ms,
            ..
        } = msg
        else {
            panic!("expected an edit, got {msg:?}");
        };
        assert_eq!(entry_id, "a", "an edit keeps the entry's identity");
        let run_at = run_at_ms.expect("a duration was typed");
        let ahead = run_at - now_ms();
        assert!(
            (295_000..=305_000).contains(&ahead),
            "5min must commit a due time about five minutes out, got {ahead}ms"
        );
    }

    /// Editing an unscheduled entry opens on an empty duration, and committing
    /// it empty keeps the entry immediate rather than scheduling it.
    #[test]
    fn user_prompt_queue_editing_an_unscheduled_entry_stays_immediate() {
        let mut d = dialog(vec![entry("a", 0, "plain", "pending")]);
        assert!(d.begin_edit());
        assert_eq!(
            d.schedule(),
            "",
            "an unscheduled entry seeds an empty duration"
        );
        type_text(&mut d, "!");
        d.handle_key(key(KeyCode::Enter));
        let msg = d.handle_key(key(KeyCode::Enter)).expect("commits");
        assert!(
            matches!(
                &msg,
                TuiMsg::QueueEditCommitted {
                    run_at_ms: None,
                    ..
                }
            ),
            "an untouched duration must not invent a schedule: {msg:?}"
        );
    }

    /// The duration step is inside `is_text_step`, so the browsing verbs stay
    /// inert while a duration is being typed. `d` typing into a duration must
    /// not delete anything.
    #[test]
    fn user_prompt_queue_browsing_actions_are_inert_in_the_duration_step() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        d.begin_add();
        type_text(&mut d, "prompt");
        d.handle_key(key(KeyCode::Enter));
        assert!(d.is_text_step(), "the duration step owns the keyboard");

        for code in [
            KeyCode::Char('d'),
            KeyCode::Char('a'),
            KeyCode::Char('e'),
            KeyCode::Char('s'),
            KeyCode::Char('r'),
        ] {
            assert!(
                d.handle_key(key(code)).is_none(),
                "{code:?} must not produce an action in the duration step"
            );
            assert_eq!(d.step(), QueueStep::Scheduling, "the step must not change");
        }
        assert_eq!(
            d.schedule(),
            "daesr",
            "every action letter must land in the duration field as text"
        );
        type_text(&mut d, "2d");
        assert_eq!(d.schedule(), "daesr2d");
        assert_eq!(d.entries().len(), 1, "nothing was deleted");
    }

    /// The hint follows the step, and the duration hint names the grammar
    /// rather than leaving the user to guess it.
    #[test]
    fn user_prompt_queue_duration_hint_names_the_grammar_and_the_empty_case() {
        let mut d = dialog(vec![entry("a", 0, "first", "pending")]);
        d.begin_add();
        type_text(&mut d, "prompt");
        d.handle_key(key(KeyCode::Enter));
        assert_eq!(d.step(), QueueStep::Scheduling);
        assert_eq!(
            d.keymap_hint().as_deref(),
            Some(HINT_SCHEDULE),
            "the duration step must advertise its own verbs"
        );
        for expected in ["30s", "5min", "2h", "1d", "empty"] {
            assert!(
                HINT_SCHEDULE.contains(expected),
                "the duration hint must state {expected}: {HINT_SCHEDULE:?}"
            );
        }
        // The browsing verbs are inert here, so the hint must not offer them.
        for inert in ["reorder", "delete", "send now"] {
            assert!(
                !HINT_SCHEDULE.contains(inert),
                "the duration hint must not advertise the inert verb {inert}"
            );
        }
    }

    /// A scheduled row is distinguishable without colour, and a due one is
    /// distinguishable from one still waiting.
    #[test]
    fn user_prompt_queue_scheduled_rows_are_marked_without_depending_on_colour() {
        let now = now_ms();
        let text = rendered(
            vec![
                entry("plain", 0, "immediate row", "pending"),
                scheduled("later", 1, "waiting row", now + 3_600_000),
                scheduled("due", 2, "due row", now - 1_000),
            ],
            80,
            12,
        );
        assert!(text.contains("immediate row"));
        assert!(
            text.contains("@1h") || text.contains("@59m"),
            "a waiting row must be marked with its remaining time: {text:?}"
        );
        assert!(
            text.contains("@·"),
            "a row whose time has arrived must be distinguishable from one still              waiting: {text:?}"
        );
    }

    /// The due marker distinguishes the three states and never invents a fourth.
    #[test]
    fn user_prompt_queue_due_marker_covers_exactly_three_states() {
        let now = 1_000_000i64;
        assert_eq!(due_marker(None, now), "  ", "no schedule is unmarked");
        assert_eq!(due_marker(Some(now + 3_600_000), now), "@1h");
        assert_eq!(due_marker(Some(now + 120_000), now), "@2m");
        assert_eq!(due_marker(Some(now + 86_400_000), now), "@1d");
        assert_eq!(
            due_marker(Some(now + 90_000_000), now),
            "@2d",
            "1.04 days rounds up, never down"
        );
        assert_eq!(
            due_marker(Some(now), now),
            "@·",
            "the instant itself is due"
        );
        assert_eq!(due_marker(Some(now - 1), now), "@·", "a passed time is due");
    }

    /// Remaining time renders coarsely and never below zero — an overdue row
    /// must not show a negative or a nonsensical value.
    #[test]
    fn user_prompt_queue_remaining_time_never_renders_negative() {
        assert_eq!(format_remaining(-5_000), "now");
        assert_eq!(format_remaining(0), "now");
        assert_eq!(format_remaining(30_000), "now", "under a minute is `now`");
        assert_eq!(format_remaining(60_000), "1m");
        assert_eq!(format_remaining(119_000), "2m", "rounds up, never down");
        assert_eq!(
            format_remaining(3_599_000),
            "1h",
            "an hour minus a second must not read as 59m"
        );
        assert_eq!(format_remaining(3_600_000), "1h");
        assert_eq!(format_remaining(86_400_000), "1d");
    }
}
