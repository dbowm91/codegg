//! Project-scoped chat projection (Project Collaboration M002).
//!
//! Daemon-owned durable channels/messages rendered as a bounded per-project
//! projection. The TUI owns no chat truth: every entry here is derived from
//! an authorized `chat.v1` round-trip through `CoreClient` (M001 contract),
//! keyed by canonical `project_id` / `channel_id`. Structured chat actions
//! (M003) render as bounded reference/status projections only; free text
//! stored or rendered here never executes privileged work — only explicit
//! `/chat-action-*` commands can create work through the daemon gate.
//!
//! Invariants:
//!
//! * Chat renders daemon state and owns no durable messages. All sends,
//!   edits, redactions, read markers, and composing leases round-trip
//!   through the daemon; the reducer only caches the bounded window.
//! * Chat routes by `ProjectId`/`ChannelId`. Rapid project switching
//!   cannot cross-route drafts or messages: every apply path verifies the
//!   completion's project/channel against the entry it mutates, and drafts
//!   are stored per project.
//! * Observer typing cannot become turn steering. The observer insert path
//!   (`crate::tui::commands::chat::route_observer_insert_to_chat`) issues
//!   only `Chat*` core requests; turn submit/steer/cancel and
//!   permission/question answers stay blocked by `ObserverState`.
//! * Unauthorized (`project_not_found`) and feature-absent projects render
//!   identically (`Unavailable` — hidden content, generic panel). Cached
//!   messages are cleared on authorization loss so hidden data is never
//!   rendered from a local cache.
//! * Large references remain handles: message references render as typed
//!   locators (`kind:target_id` plus an optional display hint). No file
//!   content, prompts, or secrets are fetched or stored here.
//! * Memory is bounded: at most [`MAX_CHAT_PROJECTS`] projects, daemon-
//!   bounded channels per project ([`MAX_CHAT_CHANNELS_PER_PROJECT`]), and
//!   a [`MAX_CHAT_MESSAGES_PER_CHANNEL`] sliding window per active
//!   channel. The panel renders at most [`MAX_CHAT_DISPLAY_MESSAGES`]
//!   rows with per-message display truncation.

use std::collections::{HashMap, VecDeque};

use crate::protocol::core::{ChatChannelDto, ChatComposingDto, ChatMessageDto};

// ── Bounds ───────────────────────────────────────────────────────────────

/// Maximum per-project chat entries retained (matches the presence bound
/// so inactive tabs stay bounded).
pub const MAX_CHAT_PROJECTS: usize = 16;
/// Maximum channels cached per project (matches the daemon bound).
pub const MAX_CHAT_CHANNELS_PER_PROJECT: usize = 16;
/// Maximum messages retained in memory per active channel window. The
/// daemon retains 1000/channel and pages 100; the TUI keeps a smaller
/// sliding window so multi-project memory stays bounded.
pub const MAX_CHAT_MESSAGES_PER_CHANNEL: usize = 100;
/// Maximum message rows rendered in the panel (bounded virtualization
/// window over the retained messages).
pub const MAX_CHAT_DISPLAY_MESSAGES: usize = 50;
/// Maximum display bytes for one message body row (char-boundary
/// truncated, full body stays daemon-side).
pub const MAX_CHAT_BODY_DISPLAY_LEN: usize = 500;
/// Maximum stored error bytes per project.
pub const MAX_CHAT_ERROR_LEN: usize = 256;
/// Maximum draft bytes retained per project (matches the 8 KiB daemon
/// body bound).
pub const MAX_CHAT_DRAFT_LEN: usize = 8192;
/// Maximum locator bytes for project/channel/message ids.
pub const MAX_CHAT_LOCATOR_LEN: usize = 128;
/// Maximum mentions extracted from one composed body.
pub const MAX_CHAT_MENTIONS: usize = 16;

// ── Helpers ──────────────────────────────────────────────────────────────

fn valid_locator(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_CHAT_LOCATOR_LEN {
        return false;
    }
    !value.bytes().any(|b| b == 0 || b.is_ascii_control())
}

fn truncate_display(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut cut = max;
    while cut > 0 && !value.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + 1);
    out.push_str(&value[..cut]);
    out.push('…');
    out
}

fn truncate_error(msg: &str) -> String {
    truncate_display(msg, MAX_CHAT_ERROR_LEN)
}

fn short_id(id: &str) -> String {
    const MAX: usize = 8;
    if id.len() <= MAX {
        return id.to_string();
    }
    let mut cut = MAX;
    while cut > 0 && !id.is_char_boundary(cut) {
        cut -= 1;
    }
    id[..cut].to_string()
}

/// Extract `@mention` tokens from a composed body. Purely a client-side
/// convenience so the daemon receives the same mention list the author
/// sees; the daemon re-validates and this never confers authority.
pub fn extract_mentions(body: &str) -> Vec<String> {
    let mut mentions = Vec::new();
    for token in body.split_whitespace().take(MAX_CHAT_MENTIONS * 4) {
        if mentions.len() >= MAX_CHAT_MENTIONS {
            break;
        }
        let handle = token.trim_matches(|c: char| {
            c == ',' || c == '.' || c == ';' || c == ':' || c == '!' || c == '?' || c == ')'
        });
        let Some(name) = handle.strip_prefix('@') else {
            continue;
        };
        let name = name.trim_matches(|c: char| c == '(' || c == '"' || c == '\'');
        if name.is_empty() || name.len() > 64 {
            continue;
        }
        if !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        {
            continue;
        }
        if !mentions.iter().any(|m: &String| m == name) {
            mentions.push(name.to_string());
        }
    }
    mentions
}

// ── Status ───────────────────────────────────────────────────────────────

/// Presentation status for one project's chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatStatus {
    /// Never fetched.
    Unknown,
    /// Fetch in flight.
    Loading,
    /// Authorized window applied (may be empty — see message count).
    Ready,
    /// Capability unsupported, unauthorized, or absent. Renders
    /// identically in all three cases; content is hidden.
    Unavailable,
    /// Transient failure with optional stale window retained.
    Error,
}

// ── Per-project projection ───────────────────────────────────────────────

/// Bounded chat projection for one project (active channel window only;
/// multi-channel windows are a future milestone — the channel list is
/// cached for display and the message window follows the active channel).
#[derive(Debug, Clone)]
pub struct ProjectChat {
    pub project_id: String,
    pub channels: Vec<ChatChannelDto>,
    pub channels_truncated: bool,
    pub active_channel_id: Option<String>,
    /// Ascending-`seq` sliding window for the active channel.
    pub messages: VecDeque<ChatMessageDto>,
    pub next_cursor: u64,
    pub retention_floor_seq: u64,
    pub resync_required: bool,
    pub history_truncated: bool,
    pub status: ChatStatus,
    pub last_error: Option<String>,
    /// Current in-flight history/sync request id (0 when idle).
    pub current_request_id: u64,
    /// Whether a re-fetch is required (reconnect, error, or hint).
    pub needs_resync: bool,
    /// Last daemon-confirmed read marker (forward-only).
    pub last_read_seq: u64,
    pub read_marker_known: bool,
    /// Content-free composing snapshot for the active channel.
    pub composing: Vec<ChatComposingDto>,
    /// Retained editable draft for this project (set on failed send so
    /// the author loses nothing; never cross-routes to another project).
    pub draft: String,
    /// Last typed send failure (secret-free, display-truncated).
    pub last_send_error: Option<String>,
    /// Local monotonic sequence bumped on each applied page/event.
    pub sequence: u64,
    /// M003: bounded structured-action projection for the active
    /// channel. Reference/status only (ids/kind/title/job/status);
    /// prompts stay daemon-side in the canonical job store.
    pub actions: Vec<crate::protocol::core::ChatActionDto>,
    /// M003: last action-submit failure (secret-free, display-truncated).
    pub last_action_error: Option<String>,
}

impl ProjectChat {
    fn fresh(project_id: String) -> Self {
        Self {
            project_id,
            channels: Vec::new(),
            channels_truncated: false,
            active_channel_id: None,
            messages: VecDeque::new(),
            next_cursor: 0,
            retention_floor_seq: 0,
            resync_required: false,
            history_truncated: false,
            status: ChatStatus::Unknown,
            last_error: None,
            current_request_id: 0,
            needs_resync: false,
            last_read_seq: 0,
            read_marker_known: false,
            composing: Vec::new(),
            draft: String::new(),
            last_send_error: None,
            sequence: 0,
            actions: Vec::new(),
            last_action_error: None,
        }
    }

    pub fn is_loading(&self) -> bool {
        self.status == ChatStatus::Loading
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Number of retained messages newer than the daemon read marker.
    /// Zero when the marker is unknown (fail closed: no unread badge
    /// without an authoritative marker).
    pub fn unread_count(&self) -> usize {
        if !self.read_marker_known {
            return 0;
        }
        self.messages
            .iter()
            .filter(|m| m.seq > self.last_read_seq)
            .count()
    }

    /// Composing entries whose lease has not expired at `now_ms`.
    pub fn active_composing(&self, now_ms: i64) -> Vec<&ChatComposingDto> {
        self.composing
            .iter()
            .filter(|c| c.expires_at_ms > now_ms)
            .collect()
    }

    /// Merge an ascending page into the bounded window: dedup by
    /// `message_id`, keep ascending `seq` order, enforce the retention
    /// window. Returns the number of newly inserted messages.
    fn merge_page(&mut self, page: Vec<ChatMessageDto>) -> usize {
        let mut inserted = 0;
        for msg in page {
            if self.messages.iter().any(|m| m.message_id == msg.message_id) {
                // Same id: replace in place (edit/redact revision bump).
                if let Some(slot) = self
                    .messages
                    .iter_mut()
                    .find(|m| m.message_id == msg.message_id)
                {
                    *slot = msg;
                }
                continue;
            }
            // Insert in ascending seq order (pages arrive ascending; live
            // events append at the end).
            let pos = self
                .messages
                .iter()
                .position(|m| m.seq > msg.seq)
                .unwrap_or(self.messages.len());
            self.messages.insert(pos, msg);
            inserted += 1;
        }
        while self.messages.len() > MAX_CHAT_MESSAGES_PER_CHANNEL {
            self.messages.pop_front();
        }
        inserted
    }

    fn push_channel(&mut self, channel: ChatChannelDto) {
        if let Some(slot) = self
            .channels
            .iter_mut()
            .find(|c| c.channel_id == channel.channel_id)
        {
            *slot = channel;
        } else {
            self.channels.push(channel);
            self.channels.sort_by(|a, b| a.name.cmp(&b.name));
            while self.channels.len() > MAX_CHAT_CHANNELS_PER_PROJECT {
                self.channels.remove(0);
            }
        }
        if self.active_channel_id.is_none() {
            self.active_channel_id = self.channels.first().map(|c| c.channel_id.clone());
        }
    }
}

// ── Chat state ───────────────────────────────────────────────────────────

/// Bounded per-project chat map. Derived frontend state only.
#[derive(Debug, Default)]
pub struct ChatState {
    entries: HashMap<String, ProjectChat>,
    /// Insertion order for LRU eviction of inactive projects.
    order: VecDeque<String>,
    /// `None` before the first capabilities round-trip.
    pub capability_supported: Option<bool>,
    /// Monotonic reconnect epoch. Bumped on transport reconnect so
    /// pre-reconnect completions are dropped.
    pub reconnect_epoch: u64,
    request_counter: u64,
    sequence_counter: u64,
}

impl ChatState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a capabilities round-trip. Toggling to `false` marks every
    /// cached project unavailable and clears cached messages without
    /// leaking which projects exist.
    pub fn set_capability(&mut self, supported: bool) {
        self.capability_supported = Some(supported);
        if !supported {
            for entry in self.entries.values_mut() {
                entry.messages.clear();
                entry.channels.clear();
                entry.active_channel_id = None;
                entry.composing.clear();
                entry.draft.clear();
                entry.last_send_error = None;
                entry.status = ChatStatus::Unavailable;
                entry.last_error = None;
                entry.needs_resync = false;
                entry.current_request_id = 0;
            }
        }
    }

    pub fn is_supported(&self) -> bool {
        self.capability_supported == Some(true)
    }

    pub fn get(&self, project_id: &str) -> Option<&ProjectChat> {
        self.entries.get(project_id)
    }

    pub fn get_mut(&mut self, project_id: &str) -> Option<&mut ProjectChat> {
        self.entries.get_mut(project_id)
    }

    pub fn project_count(&self) -> usize {
        self.entries.len()
    }

    /// Editable draft for `project_id` (empty when none). Drafts never
    /// cross-route: each project has its own slot.
    pub fn draft_for(&self, project_id: &str) -> &str {
        self.entries
            .get(project_id)
            .map(|e| e.draft.as_str())
            .unwrap_or("")
    }

    /// Store an editable draft for `project_id`, bounded to the daemon
    /// body bound. Invalid locators are ignored (fail closed).
    pub fn set_draft(&mut self, project_id: &str, draft: String) {
        if !valid_locator(project_id) {
            return;
        }
        let entry = self.ensure_project(project_id);
        let mut text = draft;
        if text.len() > MAX_CHAT_DRAFT_LEN {
            let mut cut = MAX_CHAT_DRAFT_LEN;
            while cut > 0 && !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        entry.draft = text;
    }

    fn ensure_project(&mut self, project_id: &str) -> &mut ProjectChat {
        self.entries
            .entry(project_id.to_string())
            .or_insert_with(|| ProjectChat::fresh(project_id.to_string()));
        self.touch_order(project_id);
        self.evict_if_needed(Some(project_id));
        self.entries
            .get_mut(project_id)
            .expect("chat entry just inserted")
    }

    /// Begin a channel-ensure for `project_id`. Returns the request id
    /// the completion must echo. Marks the entry loading.
    pub fn begin_ensure(&mut self, project_id: &str) -> Option<u64> {
        if !valid_locator(project_id) {
            return None;
        }
        self.request_counter = self.request_counter.saturating_add(1);
        let request_id = self.request_counter;
        let entry = self.ensure_project(project_id);
        entry.current_request_id = request_id;
        entry.status = ChatStatus::Loading;
        entry.needs_resync = false;
        Some(request_id)
    }

    /// Begin a history fetch for `channel_id` in `project_id`. Returns
    /// the request id the completion must echo, or `None` for invalid
    /// locators (fail closed, no state change).
    pub fn begin_history(&mut self, project_id: &str, channel_id: &str) -> Option<u64> {
        if !valid_locator(project_id) || !valid_locator(channel_id) {
            return None;
        }
        self.request_counter = self.request_counter.saturating_add(1);
        let request_id = self.request_counter;
        let entry = self.ensure_project(project_id);
        entry.current_request_id = request_id;
        entry.status = ChatStatus::Loading;
        entry.needs_resync = false;
        Some(request_id)
    }

    /// Whether `project_id` needs a (re)fetch: unknown, resync-flagged,
    /// or in an error state the user asked to retry. Loading entries
    /// never need a duplicate fetch (no polling storm).
    pub fn needs_refresh(&self, project_id: &str) -> bool {
        match self.entries.get(project_id) {
            None => true,
            Some(entry) => match entry.status {
                ChatStatus::Unknown => true,
                ChatStatus::Loading => false,
                ChatStatus::Ready => entry.needs_resync,
                ChatStatus::Unavailable => entry.needs_resync,
                ChatStatus::Error => true,
            },
        }
    }

    /// Apply an ensured channel. Drops stale completions (wrong request
    /// id or reconnect epoch). Returns `false` without mutating state.
    pub fn apply_channel_ensured(
        &mut self,
        request_id: u64,
        project_id: &str,
        channel: &ChatChannelDto,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        if channel.project_id != project_id {
            return false;
        }
        let Some(entry) = self.entries.get_mut(project_id) else {
            return false;
        };
        if entry.current_request_id != request_id {
            return false;
        }
        entry.push_channel(channel.clone());
        entry.active_channel_id = Some(channel.channel_id.clone());
        entry.current_request_id = 0;
        entry.last_error = None;
        if entry.status == ChatStatus::Loading {
            entry.status = ChatStatus::Ready;
        }
        entry.needs_resync = false;
        self.sequence_counter = self.sequence_counter.saturating_add(1);
        entry.sequence = self.sequence_counter;
        self.touch_order(project_id);
        true
    }

    /// Apply a channel listing. Same stale guards as ensure.
    pub fn apply_channel_list(
        &mut self,
        request_id: u64,
        project_id: &str,
        channels: &[ChatChannelDto],
        truncated: bool,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        let Some(entry) = self.entries.get_mut(project_id) else {
            return false;
        };
        if entry.current_request_id != request_id {
            return false;
        }
        // Routing guard: every listed channel must belong to this
        // project, otherwise the whole page is dropped (rapid-switch
        // leak prevention).
        if channels.iter().any(|c| c.project_id != project_id) {
            return false;
        }
        entry.channels = channels
            .iter()
            .take(MAX_CHAT_CHANNELS_PER_PROJECT)
            .cloned()
            .collect();
        entry.channels.sort_by(|a, b| a.name.cmp(&b.name));
        entry.channels_truncated = truncated;
        if entry.active_channel_id.is_none() {
            entry.active_channel_id = entry.channels.first().map(|c| c.channel_id.clone());
        }
        entry.current_request_id = 0;
        entry.last_error = None;
        if entry.status == ChatStatus::Loading {
            entry.status = ChatStatus::Ready;
        }
        entry.needs_resync = false;
        self.sequence_counter = self.sequence_counter.saturating_add(1);
        entry.sequence = self.sequence_counter;
        self.touch_order(project_id);
        true
    }

    /// Apply a history page: replaces the active-channel window (history
    /// is authoritative for its cursor range). Drops stale completions
    /// and cross-channel pages.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_history(
        &mut self,
        request_id: u64,
        project_id: &str,
        channel_id: &str,
        messages: Vec<ChatMessageDto>,
        next_cursor: u64,
        truncated: bool,
        retention_floor_seq: u64,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        let Some(entry) = self.entries.get_mut(project_id) else {
            return false;
        };
        if entry.current_request_id != request_id {
            return false;
        }
        // Routing guard: the page must belong to this project/channel.
        if messages
            .iter()
            .any(|m| m.project_id != project_id || m.channel_id != channel_id)
        {
            return false;
        }
        entry.active_channel_id = Some(channel_id.to_string());
        entry.messages = messages.into_iter().collect();
        entry.messages.make_contiguous().sort_by_key(|m| m.seq);
        while entry.messages.len() > MAX_CHAT_MESSAGES_PER_CHANNEL {
            entry.messages.pop_front();
        }
        entry.next_cursor = next_cursor;
        entry.retention_floor_seq = retention_floor_seq;
        entry.resync_required = false;
        entry.history_truncated = truncated;
        entry.status = ChatStatus::Ready;
        entry.last_error = None;
        entry.current_request_id = 0;
        entry.needs_resync = false;
        self.sequence_counter = self.sequence_counter.saturating_add(1);
        entry.sequence = self.sequence_counter;
        self.touch_order(project_id);
        true
    }

    /// Apply an incremental sync page: merges into the window, or
    /// replaces it when the daemon flags `resync_required` (expired
    /// cursor / retention floor moved past the local window).
    #[allow(clippy::too_many_arguments)]
    pub fn apply_sync(
        &mut self,
        request_id: u64,
        project_id: &str,
        channel_id: &str,
        messages: Vec<ChatMessageDto>,
        next_cursor: u64,
        resync_required: bool,
        retention_floor_seq: u64,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        let Some(entry) = self.entries.get_mut(project_id) else {
            return false;
        };
        if entry.current_request_id != request_id {
            return false;
        }
        if messages
            .iter()
            .any(|m| m.project_id != project_id || m.channel_id != channel_id)
        {
            return false;
        }
        entry.active_channel_id = Some(channel_id.to_string());
        if resync_required {
            entry.messages = messages.into_iter().collect();
            entry.messages.make_contiguous().sort_by_key(|m| m.seq);
            while entry.messages.len() > MAX_CHAT_MESSAGES_PER_CHANNEL {
                entry.messages.pop_front();
            }
        } else {
            entry.merge_page(messages);
        }
        entry.next_cursor = next_cursor;
        entry.retention_floor_seq = retention_floor_seq;
        entry.resync_required = false;
        entry.status = ChatStatus::Ready;
        entry.last_error = None;
        entry.current_request_id = 0;
        entry.needs_resync = false;
        self.sequence_counter = self.sequence_counter.saturating_add(1);
        entry.sequence = self.sequence_counter;
        self.touch_order(project_id);
        true
    }

    /// Apply a sent message (command completion path). The completion
    /// carries its own project/channel routing; only the matching entry
    /// is mutated. A successful send clears the retained draft and the
    /// send error; the message merges into the window when it targets
    /// the active channel.
    pub fn apply_sent(
        &mut self,
        project_id: &str,
        channel_id: &str,
        message: &ChatMessageDto,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        if message.project_id != project_id || message.channel_id != channel_id {
            return false;
        }
        if !valid_locator(project_id) {
            return false;
        }
        self.sequence_counter = self.sequence_counter.saturating_add(1);
        let sequence = self.sequence_counter;
        let entry = self.ensure_project(project_id);
        if entry.status == ChatStatus::Unavailable {
            return false;
        }
        if entry.active_channel_id.as_deref() == Some(channel_id) {
            entry.merge_page(vec![message.clone()]);
            entry.next_cursor = entry.next_cursor.max(message.seq.saturating_add(1));
        } else if entry.active_channel_id.is_none() {
            entry.active_channel_id = Some(channel_id.to_string());
            entry.merge_page(vec![message.clone()]);
            entry.next_cursor = entry.next_cursor.max(message.seq.saturating_add(1));
        }
        // Success clears the per-project draft/error (failure path uses
        // `note_failed_send` instead and retains both).
        entry.draft.clear();
        entry.last_send_error = None;
        if entry.status == ChatStatus::Unknown || entry.status == ChatStatus::Loading {
            entry.status = ChatStatus::Ready;
        }
        entry.sequence = sequence;
        true
    }

    /// Record a failed send: retains the editable draft plus the typed
    /// error. Nothing is fabricated into the message window.
    pub fn note_failed_send(&mut self, project_id: &str, draft: String, error: String) {
        if !valid_locator(project_id) {
            return;
        }
        let entry = self.ensure_project(project_id);
        let mut text = draft;
        if text.len() > MAX_CHAT_DRAFT_LEN {
            let mut cut = MAX_CHAT_DRAFT_LEN;
            while cut > 0 && !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        entry.draft = text;
        entry.last_send_error = Some(truncate_error(&error));
    }

    /// Apply a live committed-message event. No request id exists on the
    /// event path; routing is by project/channel match. Events for an
    /// unknown project only flag a resync hint (fail closed — no message
    /// stored until an authorized fetch confirms it). Events for a
    /// non-active channel flag resync without disturbing the active
    /// window.
    pub fn apply_event_committed(&mut self, message: &ChatMessageDto) -> bool {
        if !valid_locator(&message.project_id) || !valid_locator(&message.channel_id) {
            return false;
        }
        let Some(entry) = self.entries.get_mut(message.project_id.as_str()) else {
            return false;
        };
        if entry.status == ChatStatus::Unavailable {
            return false;
        }
        match entry.active_channel_id.as_deref() {
            Some(active) if active == message.channel_id => {
                entry.merge_page(vec![message.clone()]);
                entry.next_cursor = entry.next_cursor.max(message.seq.saturating_add(1));
                self.sequence_counter = self.sequence_counter.saturating_add(1);
                entry.sequence = self.sequence_counter;
                true
            }
            Some(_) => {
                entry.needs_resync = true;
                true
            }
            None => {
                entry.active_channel_id = Some(message.channel_id.clone());
                entry.merge_page(vec![message.clone()]);
                entry.next_cursor = entry.next_cursor.max(message.seq.saturating_add(1));
                self.sequence_counter = self.sequence_counter.saturating_add(1);
                entry.sequence = self.sequence_counter;
                true
            }
        }
    }

    /// Apply a live edited-message event (revision bump in place).
    pub fn apply_event_edited(&mut self, message: &ChatMessageDto) -> bool {
        if !valid_locator(&message.project_id) || !valid_locator(&message.channel_id) {
            return false;
        }
        let Some(entry) = self.entries.get_mut(message.project_id.as_str()) else {
            return false;
        };
        if entry.active_channel_id.as_deref() != Some(message.channel_id.as_str()) {
            entry.needs_resync = true;
            return true;
        }
        match entry
            .messages
            .iter_mut()
            .find(|m| m.message_id == message.message_id)
        {
            Some(slot) => {
                *slot = message.clone();
                self.sequence_counter = self.sequence_counter.saturating_add(1);
                entry.sequence = self.sequence_counter;
                true
            }
            None => {
                entry.needs_resync = true;
                true
            }
        }
    }

    /// Apply a live redaction event (identity + revision only; the body
    /// is already `[REDACTED]` daemon-side). Unknown messages flag a
    /// resync so the next authorized fetch converges.
    pub fn apply_event_redacted(
        &mut self,
        project_id: &str,
        channel_id: &str,
        message_id: &str,
        revision: u64,
    ) -> bool {
        if !valid_locator(project_id) || !valid_locator(channel_id) || !valid_locator(message_id) {
            return false;
        }
        let Some(entry) = self.entries.get_mut(project_id) else {
            return false;
        };
        if entry.active_channel_id.as_deref() != Some(channel_id) {
            entry.needs_resync = true;
            return true;
        }
        match entry
            .messages
            .iter_mut()
            .find(|m| m.message_id == message_id)
        {
            Some(slot) => {
                slot.body = codegg_core::collaboration::REDACTED_BODY.to_string();
                slot.redacted = true;
                slot.revision = revision;
                self.sequence_counter = self.sequence_counter.saturating_add(1);
                entry.sequence = self.sequence_counter;
                true
            }
            None => {
                entry.needs_resync = true;
                true
            }
        }
    }

    /// Apply a failure for `project_id`.
    ///
    /// * `unauthorized` covers both `project_not_found` denials (which
    ///   are indistinguishable from absent) and genuinely absent
    ///   projects: cached messages are cleared and the entry renders
    ///   identically to feature-absent.
    /// * `unsupported` covers older daemons without the chat capability:
    ///   same rendering as unauthorized.
    /// * Transient errors retain the stale window (if any) and flag
    ///   `needs_resync` so the next refresh replaces it.
    pub fn apply_error(
        &mut self,
        request_id: u64,
        project_id: &str,
        error: String,
        unauthorized: bool,
        unsupported: bool,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        let Some(entry) = self.entries.get_mut(project_id) else {
            return false;
        };
        if entry.current_request_id != request_id {
            return false;
        }
        entry.current_request_id = 0;
        if unauthorized || unsupported {
            entry.messages.clear();
            entry.channels.clear();
            entry.active_channel_id = None;
            entry.composing.clear();
            entry.draft.clear();
            entry.last_send_error = None;
            entry.status = ChatStatus::Unavailable;
            entry.last_error = None;
            entry.needs_resync = false;
            if unsupported {
                self.capability_supported = Some(false);
            }
        } else {
            entry.status = ChatStatus::Error;
            entry.last_error = Some(truncate_error(&error));
            entry.needs_resync = true;
        }
        true
    }

    /// Advance the daemon read marker (forward-only; stale writers cannot
    /// regress it). Unknown projects/channels are ignored.
    pub fn apply_read_marker(&mut self, project_id: &str, channel_id: &str, last_read_seq: u64) {
        let Some(entry) = self.entries.get_mut(project_id) else {
            return;
        };
        if entry.active_channel_id.as_deref() != Some(channel_id) {
            return;
        }
        if !entry.read_marker_known || last_read_seq > entry.last_read_seq {
            entry.last_read_seq = last_read_seq;
            entry.read_marker_known = true;
        }
    }

    /// Replace the composing snapshot for the active channel
    /// (content-free leases). Unknown projects are ignored.
    pub fn apply_composing(
        &mut self,
        project_id: &str,
        channel_id: &str,
        composing: Vec<ChatComposingDto>,
    ) {
        let Some(entry) = self.entries.get_mut(project_id) else {
            return;
        };
        if entry.active_channel_id.as_deref() != Some(channel_id)
            && entry.active_channel_id.is_some()
        {
            return;
        }
        entry.active_channel_id = Some(channel_id.to_string());
        entry.composing = composing.into_iter().take(32).collect();
    }

    /// Clear composing leases (observer target disconnect / explicit
    /// stop leaves chat usable but drops ephemeral typing state).
    pub fn clear_composing(&mut self, project_id: &str) {
        if let Some(entry) = self.entries.get_mut(project_id) {
            entry.composing.clear();
        }
    }

    // ── M003: structured chat actions ────────────────────────────────

    /// Maximum actions retained per project (bounded projection;
    /// full history stays daemon-side).
    pub const MAX_ACTIONS: usize = 50;

    /// Begin an action submit/list round-trip. Returns the request id
    /// the completion must echo. Reuses the chat generation so stale
    /// completions (tab switch, stop, reconnect) drop at apply time.
    pub fn begin_action(&mut self, project_id: &str) -> Option<u64> {
        if !valid_locator(project_id) {
            return None;
        }
        self.request_counter = self.request_counter.saturating_add(1);
        let request_id = self.request_counter;
        let entry = self.ensure_project(project_id);
        entry.current_request_id = request_id;
        Some(request_id)
    }

    /// Apply an action-submit completion. Merges the durable projection
    /// by `action_id` when it targets the cached active channel;
    /// cross-channel pages are ignored (fail closed). Duplicate retries
    /// converge (no second row). Returns `false` for stale completions.
    pub fn apply_action_submitted(
        &mut self,
        request_id: u64,
        project_id: &str,
        channel_id: &str,
        action: &crate::protocol::core::ChatActionDto,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        if !valid_locator(project_id) || !valid_locator(channel_id) {
            return false;
        }
        if action.project_id != project_id || action.channel_id != channel_id {
            return false;
        }
        let Some(entry) = self.entries.get_mut(project_id) else {
            return false;
        };
        if entry.current_request_id != 0 && entry.current_request_id != request_id {
            return false;
        }
        entry.current_request_id = 0;
        if entry.status == ChatStatus::Unavailable {
            return false;
        }
        if entry
            .active_channel_id
            .as_deref()
            .is_some_and(|active| active != channel_id)
        {
            return false;
        }
        entry.active_channel_id = Some(channel_id.to_string());
        Self::merge_action(entry, action.clone());
        entry.last_action_error = None;
        self.sequence_counter = self.sequence_counter.saturating_add(1);
        entry.sequence = self.sequence_counter;
        true
    }

    /// Record a failed action submit: retains the typed error, fabricates
    /// nothing into the action window.
    pub fn note_failed_action(&mut self, project_id: &str, error: String) {
        if !valid_locator(project_id) {
            return;
        }
        let entry = self.ensure_project(project_id);
        entry.current_request_id = 0;
        entry.last_action_error = Some(truncate_error(&error));
    }

    /// Apply a bounded action-list page: replaces the cached projection
    /// for the active channel (merge-or-replace by `action_id`).
    /// Returns `false` for stale completions.
    pub fn apply_action_list(
        &mut self,
        request_id: u64,
        project_id: &str,
        channel_id: &str,
        actions: Vec<crate::protocol::core::ChatActionDto>,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        if !valid_locator(project_id) || !valid_locator(channel_id) {
            return false;
        }
        let Some(entry) = self.entries.get_mut(project_id) else {
            return false;
        };
        if entry.current_request_id != 0 && entry.current_request_id != request_id {
            return false;
        }
        entry.current_request_id = 0;
        if entry.status == ChatStatus::Unavailable {
            return false;
        }
        if entry
            .active_channel_id
            .as_deref()
            .is_some_and(|active| active != channel_id)
        {
            return false;
        }
        entry.active_channel_id = Some(channel_id.to_string());
        let mut merged: Vec<crate::protocol::core::ChatActionDto> = Vec::new();
        for action in actions {
            if action.project_id != project_id || action.channel_id != channel_id {
                continue;
            }
            if merged.iter().any(|a| a.action_id == action.action_id) {
                continue;
            }
            merged.push(action);
            if merged.len() >= Self::MAX_ACTIONS {
                break;
            }
        }
        entry.actions = merged;
        entry.last_action_error = None;
        self.sequence_counter = self.sequence_counter.saturating_add(1);
        entry.sequence = self.sequence_counter;
        true
    }

    /// Apply a live action event. Routing is by project/channel match;
    /// events for unknown projects are ignored (fail closed — no action
    /// stored until an authorized list confirms it).
    pub fn apply_event_action(&mut self, action: &crate::protocol::core::ChatActionDto) -> bool {
        if !valid_locator(&action.project_id) || !valid_locator(&action.channel_id) {
            return false;
        }
        let Some(entry) = self.entries.get_mut(action.project_id.as_str()) else {
            return false;
        };
        if entry.status == ChatStatus::Unavailable {
            return false;
        }
        if entry
            .active_channel_id
            .as_deref()
            .is_some_and(|active| active != action.channel_id)
        {
            entry.needs_resync = true;
            return true;
        }
        Self::merge_action(entry, action.clone());
        self.sequence_counter = self.sequence_counter.saturating_add(1);
        entry.sequence = self.sequence_counter;
        true
    }

    fn merge_action(entry: &mut ProjectChat, action: crate::protocol::core::ChatActionDto) {
        if let Some(slot) = entry
            .actions
            .iter_mut()
            .find(|a| a.action_id == action.action_id)
        {
            *slot = action;
            return;
        }
        entry.actions.push(action);
        entry.actions.sort_by(|a, b| {
            a.created_at_ms
                .cmp(&b.created_at_ms)
                .then_with(|| a.action_id.cmp(&b.action_id))
        });
        while entry.actions.len() > Self::MAX_ACTIONS {
            entry.actions.remove(0);
        }
    }

    /// One-line rendering for an action projection (ids/kind/status
    /// only; prompts stay daemon-side).
    pub fn action_line(action: &crate::protocol::core::ChatActionDto) -> String {
        let kind = match action.kind {
            crate::protocol::core::ChatActionKindDto::AgentTask => "task",
            crate::protocol::core::ChatActionKindDto::ReviewRequest => "review",
            crate::protocol::core::ChatActionKindDto::JobSubmit => "job",
            crate::protocol::core::ChatActionKindDto::JobReference => "ref",
        };
        let job = action.job_id.as_deref().unwrap_or("-");
        let title = action
            .title
            .as_deref()
            .map(|t| truncate_display(t, 80))
            .unwrap_or_default();
        if title.is_empty() {
            format!(
                "⚡ {} {} {} [{}]",
                short_id(&action.action_id),
                kind,
                short_id(job),
                action.status
            )
        } else {
            format!(
                "⚡ {} {} {} [{}] {}",
                short_id(&action.action_id),
                kind,
                short_id(job),
                action.status,
                title
            )
        }
    }

    /// Mark a liveness hint (`ChatMessageCommitted` / `ChatComposingUpdated`
    /// for a project without a cached entry). Records the project as
    /// needing data without starting a fetch here (the open path owns the
    /// request). Returns `true` when the caller should issue a refresh.
    pub fn note_hint(&mut self, project_id: &str) -> bool {
        if !valid_locator(project_id) {
            return false;
        }
        if self.capability_supported == Some(false) {
            return false;
        }
        match self.entries.get_mut(project_id) {
            None => {
                let mut entry = ProjectChat::fresh(project_id.to_string());
                entry.needs_resync = true;
                self.entries.insert(project_id.to_string(), entry);
                self.touch_order(project_id);
                self.evict_if_needed(None);
                true
            }
            Some(entry) => {
                if entry.status == ChatStatus::Loading {
                    false
                } else {
                    entry.needs_resync = true;
                    true
                }
            }
        }
    }

    /// Transport reconnect: bump the epoch, drop in-flight request
    /// bindings, and flag every project for resync. Stale presentation is
    /// replaced by the next authoritative page; nothing is fabricated.
    /// Returns the new epoch; the caller resumes from each entry's
    /// `next_cursor` (M001 cursor) or resyncs the bounded window.
    pub fn on_reconnect(&mut self) -> u64 {
        self.reconnect_epoch = self.reconnect_epoch.saturating_add(1);
        for entry in self.entries.values_mut() {
            entry.current_request_id = 0;
            entry.needs_resync = true;
            if entry.status == ChatStatus::Loading {
                entry.status = ChatStatus::Unknown;
            }
        }
        self.reconnect_epoch
    }

    /// Authorization loss or tab close: drop the cached entry so hidden
    /// data is never rendered from a local cache.
    pub fn clear_project(&mut self, project_id: &str) {
        self.entries.remove(project_id);
        self.order.retain(|p| p != project_id);
    }

    /// Drop all entries (daemon restart / logout equivalent).
    pub fn clear_all(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// Header summary for the tab strip: `Some("💬 N[ unread]")` when
    /// authorized data exists, `None` when unavailable/loading/unknown
    /// (hidden — identical for unauthorized and absent).
    pub fn header_summary(&self, project_id: &str) -> Option<String> {
        let entry = self.entries.get(project_id)?;
        match entry.status {
            ChatStatus::Ready => {
                let unread = entry.unread_count();
                if unread > 0 {
                    Some(format!("💬 {} ({} unread)", entry.messages.len(), unread))
                } else if entry.messages.is_empty() {
                    Some("💬 0".to_string())
                } else {
                    Some(format!("💬 {}", entry.messages.len()))
                }
            }
            ChatStatus::Error if !entry.messages.is_empty() => {
                Some(format!("💬 {} (stale)", entry.messages.len()))
            }
            _ => None,
        }
    }

    /// Bounded panel lines for `/chat`. Renders the active-channel window
    /// (newest last), unread separators, composing indicator, and the
    /// standard loading/unavailable/error branches. `now_ms` filters
    /// expired composing leases for display.
    pub fn panel_lines(&self, project_id: &str, now_ms: i64) -> Vec<String> {
        let Some(entry) = self.entries.get(project_id) else {
            if self.capability_supported == Some(false) {
                return unavailable_lines();
            }
            return vec![
                "Project chat — loading…".to_string(),
                String::new(),
                "Fetching chat for this project…".to_string(),
            ];
        };
        match entry.status {
            ChatStatus::Unknown | ChatStatus::Loading => vec![
                "Project chat — loading…".to_string(),
                String::new(),
                "Fetching chat for this project…".to_string(),
            ],
            ChatStatus::Unavailable => unavailable_lines(),
            ChatStatus::Error if entry.messages.is_empty() => {
                let mut lines = vec!["Project chat — refresh failed".to_string(), String::new()];
                lines.push(format!(
                    "Error: {}",
                    entry.last_error.as_deref().unwrap_or("unknown error")
                ));
                lines.push("Retry with /chat-history.".to_string());
                lines
            }
            _ => {
                let channel_label = entry
                    .channels
                    .iter()
                    .find(|c| Some(c.channel_id.as_str()) == entry.active_channel_id.as_deref())
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "general".to_string());
                let mut lines = vec![
                    format!(
                        "Project chat — #{} ({} messages{})",
                        channel_label,
                        entry.messages.len(),
                        if entry.history_truncated {
                            ", truncated"
                        } else {
                            ""
                        }
                    ),
                    String::new(),
                ];
                if entry.messages.is_empty() {
                    lines.push("No messages yet.".to_string());
                    lines.push("Use /chat-send <text> to start the conversation.".to_string());
                } else {
                    let total = entry.messages.len();
                    let start = total.saturating_sub(MAX_CHAT_DISPLAY_MESSAGES);
                    if start > 0 {
                        lines.push(format!(
                            "… {} older messages (bounded window; use /chat-history for paging)",
                            start
                        ));
                    }
                    let unread = entry.unread_count();
                    let unread_from = total.saturating_sub(unread);
                    for (idx, msg) in entry.messages.iter().enumerate().skip(start) {
                        if unread > 0 && idx == unread_from {
                            lines.push(format!("— {unread} unread —"));
                        }
                        lines.push(message_line(msg));
                    }
                    if entry.resync_required {
                        lines.push(String::new());
                        lines.push("Retention moved — resync with /chat-sync.".to_string());
                    } else if entry.needs_resync {
                        lines.push(String::new());
                        lines.push("Stale — refresh with /chat-history to resync.".to_string());
                    }
                }
                let composing = entry.active_composing(now_ms);
                if !composing.is_empty() {
                    lines.push(String::new());
                    if composing.len() == 1 {
                        lines.push(format!(
                            "{} is composing…",
                            short_id(&composing[0].principal_id)
                        ));
                    } else {
                        lines.push(format!("{} people composing…", composing.len()));
                    }
                }
                if let Some(err) = entry.last_send_error.as_deref() {
                    lines.push(String::new());
                    lines.push(format!("Last send failed: {err} — draft retained."));
                }
                if !entry.actions.is_empty() {
                    lines.push(String::new());
                    lines.push(format!("Actions ({}):", entry.actions.len()));
                    let total = entry.actions.len();
                    let start = total.saturating_sub(10);
                    for action in entry.actions.iter().skip(start) {
                        lines.push(Self::action_line(action));
                    }
                }
                if let Some(err) = entry.last_action_error.as_deref() {
                    lines.push(String::new());
                    lines.push(format!("Last action failed: {err}."));
                }
                lines.push(String::new());
                lines.push(
                    "Commands: /chat-send <text> · /chat-reply <id> <text> · /chat-history · /chat-read · /chat-edit · /chat-redact"
                        .to_string(),
                );
                lines.push(
                    "Actions (explicit only): /chat-action-task <msg> <agent> <prompt> · /chat-action-review <msg> <agent> <prompt> · /chat-action-list [msg]"
                        .to_string(),
                );
                lines
            }
        }
    }

    fn touch_order(&mut self, project_id: &str) {
        self.order.retain(|p| p != project_id);
        self.order.push_back(project_id.to_string());
    }

    fn evict_if_needed(&mut self, protect: Option<&str>) {
        while self.entries.len() > MAX_CHAT_PROJECTS {
            let victim = self
                .order
                .iter()
                .find(|p| Some(p.as_str()) != protect)
                .cloned();
            match victim {
                Some(id) => {
                    self.entries.remove(&id);
                    self.order.retain(|p| p != &id);
                }
                None => break,
            }
        }
    }
}

fn unavailable_lines() -> Vec<String> {
    vec![
        "Project chat unavailable".to_string(),
        String::new(),
        "Chat is unavailable for this project.".to_string(),
        "Older daemons hide this panel; unauthorized projects look the same.".to_string(),
    ]
}

/// Bounded one-line rendering for a chat message: sequence, author,
/// truncated body, reply/mention/reference suffixes, revision/redaction
/// markers. References stay opaque locators (`kind:target_id` plus an
/// optional hint); no content is fetched.
pub fn message_line(msg: &ChatMessageDto) -> String {
    let mut line = format!(
        "[#{}] {}: {}",
        msg.seq,
        truncate_display(&msg.author_principal, 48),
        truncate_display(&msg.body, MAX_CHAT_BODY_DISPLAY_LEN)
    );
    if let Some(agent) = msg.author_agent.as_deref() {
        if !agent.is_empty() {
            line.push_str(&format!(" (via {})", truncate_display(agent, 32)));
        }
    }
    if let Some(reply_to) = msg.reply_to.as_deref() {
        line.push_str(&format!(" (reply to {})", short_id(reply_to)));
    }
    for mention in msg.mentions.iter().take(MAX_CHAT_MENTIONS) {
        line.push_str(&format!(" @{}", truncate_display(mention, 64)));
    }
    for reference in msg.references.iter().take(8) {
        let kind = format!("{:?}", reference.kind).to_lowercase();
        match reference.display_hint.as_deref() {
            Some(hint) if !hint.is_empty() => line.push_str(&format!(
                " [{}:{} \"{}\"]",
                kind,
                truncate_display(&reference.target_id, 48),
                truncate_display(hint, 64)
            )),
            _ => line.push_str(&format!(
                " [{}:{}]",
                kind,
                truncate_display(&reference.target_id, 48)
            )),
        }
    }
    if msg.redacted {
        line.push_str(&format!(" [REDACTED r{}]", msg.revision));
    } else if msg.revision > 0 {
        line.push_str(&format!(" (edited r{})", msg.revision));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::core::{ChatReferenceDto, ChatReferenceKindDto};

    fn test_channel(project: &str, id: &str, name: &str) -> ChatChannelDto {
        ChatChannelDto {
            channel_id: id.to_string(),
            project_id: project.to_string(),
            name: name.to_string(),
            created_by: "alice".to_string(),
            created_at_ms: 1,
        }
    }

    fn message(project: &str, channel: &str, seq: u64, id: &str, body: &str) -> ChatMessageDto {
        ChatMessageDto {
            message_id: id.to_string(),
            channel_id: channel.to_string(),
            project_id: project.to_string(),
            seq,
            author_principal: "alice".to_string(),
            author_agent: None,
            body: body.to_string(),
            reply_to: None,
            thread_root: None,
            mentions: Vec::new(),
            references: Vec::new(),
            revision: 0,
            redacted: false,
            created_at_ms: 1,
            edited_at_ms: None,
        }
    }

    fn ready_window(state: &mut ChatState, project: &str, channel: &str, n: u64) {
        let epoch = state.reconnect_epoch;
        let ensure_req = state.begin_ensure(project).unwrap();
        assert!(state.apply_channel_ensured(
            ensure_req,
            project,
            &test_channel(project, channel, "general"),
            epoch
        ));
        let hist_req = state.begin_history(project, channel).unwrap();
        let messages: Vec<ChatMessageDto> = (1..=n)
            .map(|s| message(project, channel, s, &format!("m{s}"), &format!("body {s}")))
            .collect();
        assert!(state.apply_history(hist_req, project, channel, messages, n + 1, false, 0, epoch));
    }

    #[test]
    fn multi_project_routing_does_not_cross_contaminate() {
        let mut state = ChatState::new();
        state.set_capability(true);
        ready_window(&mut state, "proj-a", "ch-a", 2);
        ready_window(&mut state, "proj-b", "ch-b", 1);
        assert_eq!(state.get("proj-a").unwrap().messages.len(), 2);
        assert_eq!(state.get("proj-b").unwrap().messages.len(), 1);
        // Cross-project page is rejected.
        let epoch = state.reconnect_epoch;
        let req = state.begin_history("proj-a", "ch-a").unwrap();
        let foreign = vec![message("proj-b", "ch-b", 9, "mx", "hijack")];
        assert!(!state.apply_history(req, "proj-a", "ch-a", foreign, 10, false, 0, epoch));
        assert_eq!(state.get("proj-a").unwrap().messages.len(), 2);
        // Cross-channel page is rejected.
        let req2 = state.begin_history("proj-a", "ch-a").unwrap();
        let wrong_channel = vec![message("proj-a", "ch-other", 9, "my", "hijack")];
        assert!(!state.apply_history(req2, "proj-a", "ch-a", wrong_channel, 10, false, 0, epoch));
        assert_eq!(state.get("proj-a").unwrap().messages.len(), 2);
    }

    #[test]
    fn drafts_are_per_project_and_bounded() {
        let mut state = ChatState::new();
        state.set_draft("proj-a", "hello a".to_string());
        state.set_draft("proj-b", "hello b".to_string());
        assert_eq!(state.draft_for("proj-a"), "hello a");
        assert_eq!(state.draft_for("proj-b"), "hello b");
        // Oversized drafts truncate to the daemon body bound.
        state.set_draft("proj-a", "x".repeat(MAX_CHAT_DRAFT_LEN + 100));
        assert_eq!(state.draft_for("proj-a").len(), MAX_CHAT_DRAFT_LEN);
        // Successful send clears only that project's draft.
        let msg = message("proj-a", "ch-a", 1, "m1", "hi");
        state.apply_sent("proj-a", "ch-a", &msg, state.reconnect_epoch);
        assert_eq!(state.draft_for("proj-a"), "");
        assert_eq!(state.draft_for("proj-b"), "hello b");
    }

    #[test]
    fn failed_send_retains_draft_with_typed_error_and_stores_nothing() {
        let mut state = ChatState::new();
        state.set_capability(true);
        ready_window(&mut state, "proj-a", "ch-a", 1);
        state.note_failed_send(
            "proj-a",
            "unsent draft".to_string(),
            "project_not_found: denied".to_string(),
        );
        let entry = state.get("proj-a").unwrap();
        assert_eq!(entry.draft, "unsent draft");
        assert!(entry
            .last_send_error
            .as_deref()
            .unwrap()
            .contains("project_not_found"));
        assert_eq!(entry.messages.len(), 1);
    }

    #[test]
    fn history_window_is_bounded_and_paging_replaces() {
        let mut state = ChatState::new();
        state.set_capability(true);
        let epoch = state.reconnect_epoch;
        let ensure_req = state.begin_ensure("p").unwrap();
        assert!(state.apply_channel_ensured(
            ensure_req,
            "p",
            &test_channel("p", "ch", "general"),
            epoch
        ));
        let req = state.begin_history("p", "ch").unwrap();
        let messages: Vec<ChatMessageDto> = (1..=MAX_CHAT_MESSAGES_PER_CHANNEL as u64 + 10)
            .map(|s| message("p", "ch", s, &format!("m{s}"), "b"))
            .collect();
        assert!(state.apply_history(req, "p", "ch", messages, 200, true, 5, epoch));
        let entry = state.get("p").unwrap();
        assert_eq!(entry.messages.len(), MAX_CHAT_MESSAGES_PER_CHANNEL);
        assert!(entry.history_truncated);
        assert_eq!(entry.retention_floor_seq, 5);
        // Oldest retained is the tail of the window.
        assert_eq!(entry.messages.front().unwrap().seq, 11);
    }

    #[test]
    fn sync_merges_without_duplicates_and_resync_replaces() {
        let mut state = ChatState::new();
        state.set_capability(true);
        ready_window(&mut state, "p", "ch", 3);
        let epoch = state.reconnect_epoch;
        // Incremental merge: one duplicate (same id, bumped revision) + one new.
        let req = state.begin_history("p", "ch").unwrap();
        let mut dup = message("p", "ch", 3, "m3", "edited body");
        dup.revision = 1;
        let fresh = message("p", "ch", 4, "m4", "new");
        assert!(state.apply_sync(req, "p", "ch", vec![dup, fresh], 5, false, 0, epoch));
        let entry = state.get("p").unwrap();
        assert_eq!(entry.messages.len(), 4);
        assert_eq!(
            entry
                .messages
                .iter()
                .find(|m| m.message_id == "m3")
                .unwrap()
                .body,
            "edited body"
        );
        // Resync replaces the window.
        let req2 = state.begin_history("p", "ch").unwrap();
        let replacement = vec![message("p", "ch", 50, "m50", "after retention")];
        assert!(state.apply_sync(req2, "p", "ch", replacement, 51, true, 40, epoch));
        let entry = state.get("p").unwrap();
        assert_eq!(entry.messages.len(), 1);
        assert_eq!(entry.retention_floor_seq, 40);
    }

    #[test]
    fn unread_counts_follow_forward_only_read_markers() {
        let mut state = ChatState::new();
        state.set_capability(true);
        ready_window(&mut state, "p", "ch", 3);
        // Unknown marker: no unread badge (fail closed).
        assert_eq!(state.get("p").unwrap().unread_count(), 0);
        state.apply_read_marker("p", "ch", 2);
        assert_eq!(state.get("p").unwrap().unread_count(), 1);
        // Stale writer cannot regress the marker.
        state.apply_read_marker("p", "ch", 1);
        assert_eq!(state.get("p").unwrap().unread_count(), 1);
        state.apply_read_marker("p", "ch", 3);
        assert_eq!(state.get("p").unwrap().unread_count(), 0);
        // Wrong channel is ignored.
        state.apply_read_marker("p", "other", 99);
        assert_eq!(state.get("p").unwrap().unread_count(), 0);
    }

    #[test]
    fn composing_display_filters_expired_leases() {
        let mut state = ChatState::new();
        state.set_capability(true);
        ready_window(&mut state, "p", "ch", 1);
        state.apply_composing(
            "p",
            "ch",
            vec![
                ChatComposingDto {
                    channel_id: "ch".to_string(),
                    principal_id: "alice".to_string(),
                    client_id: "c1".to_string(),
                    expires_at_ms: 100,
                },
                ChatComposingDto {
                    channel_id: "ch".to_string(),
                    principal_id: "bob".to_string(),
                    client_id: "c2".to_string(),
                    expires_at_ms: 10,
                },
            ],
        );
        let entry = state.get("p").unwrap();
        assert_eq!(entry.active_composing(50).len(), 1);
        assert_eq!(entry.active_composing(500).len(), 0);
        let lines = state.panel_lines("p", 50);
        assert!(lines.iter().any(|l| l.contains("composing")));
        let expired_lines = state.panel_lines("p", 500);
        assert!(!expired_lines.iter().any(|l| l.contains("composing")));
    }

    #[test]
    fn unauthorized_and_feature_absent_render_identically_and_clear() {
        let mut state = ChatState::new();
        state.set_capability(true);
        ready_window(&mut state, "secret", "ch", 2);
        let epoch = state.reconnect_epoch;
        let req_a = state.begin_history("secret", "ch").unwrap();
        assert!(state.apply_error(
            req_a,
            "secret",
            "project_not_found: denied".into(),
            true,
            false,
            epoch
        ));
        ready_window(&mut state, "old", "ch", 1);
        let req_b = state.begin_history("old", "ch").unwrap();
        assert!(state.apply_error(req_b, "old", "unsupported".into(), false, true, epoch));
        assert_eq!(state.panel_lines("secret", 0), state.panel_lines("old", 0));
        assert_eq!(state.header_summary("secret"), None);
        assert_eq!(state.header_summary("old"), None);
        // Cached content is cleared on authorization loss.
        assert_eq!(state.get("secret").unwrap().messages.len(), 0);
    }

    #[test]
    fn reconnect_drops_pre_reconnect_completions_and_flags_resync() {
        let mut state = ChatState::new();
        state.set_capability(true);
        ready_window(&mut state, "p", "ch", 1);
        let req = state.begin_history("p", "ch").unwrap();
        let old_epoch = state.reconnect_epoch;
        let new_epoch = state.on_reconnect();
        assert_ne!(old_epoch, new_epoch);
        assert!(!state.apply_history(
            req,
            "p",
            "ch",
            vec![message("p", "ch", 9, "ghost", "x")],
            10,
            false,
            0,
            old_epoch
        ));
        assert!(state.get("p").unwrap().needs_resync);
        let req2 = state.begin_history("p", "ch").unwrap();
        assert!(state.apply_history(
            req2,
            "p",
            "ch",
            vec![message("p", "ch", 2, "m2", "fresh")],
            3,
            false,
            0,
            new_epoch
        ));
        assert!(!state.get("p").unwrap().needs_resync);
    }

    #[test]
    fn live_events_merge_only_into_matching_channel() {
        let mut state = ChatState::new();
        state.set_capability(true);
        ready_window(&mut state, "p", "ch-a", 1);
        // Matching channel merges.
        assert!(state.apply_event_committed(&message("p", "ch-a", 2, "m2", "live")));
        assert_eq!(state.get("p").unwrap().messages.len(), 2);
        // Other channel only flags resync.
        assert!(state.apply_event_committed(&message("p", "ch-b", 1, "mx", "other")));
        assert_eq!(state.get("p").unwrap().messages.len(), 2);
        assert!(state.get("p").unwrap().needs_resync);
        // Redaction applies identity + revision in place.
        assert!(state.apply_event_redacted("p", "ch-a", "m2", 2));
        let redacted = state
            .get("p")
            .unwrap()
            .messages
            .iter()
            .find(|m| m.message_id == "m2")
            .unwrap();
        assert!(redacted.redacted);
        assert_eq!(redacted.body, codegg_core::collaboration::REDACTED_BODY);
    }

    #[test]
    fn stale_request_id_is_dropped() {
        let mut state = ChatState::new();
        state.set_capability(true);
        let epoch = state.reconnect_epoch;
        let ensure_req = state.begin_ensure("p").unwrap();
        assert!(state.apply_channel_ensured(
            ensure_req,
            "p",
            &test_channel("p", "ch", "general"),
            epoch
        ));
        let stale = state.begin_history("p", "ch").unwrap();
        let fresh = state.begin_history("p", "ch").unwrap();
        assert!(!state.apply_history(
            stale,
            "p",
            "ch",
            vec![message("p", "ch", 1, "old", "x")],
            2,
            false,
            0,
            epoch
        ));
        assert!(state.apply_history(
            fresh,
            "p",
            "ch",
            vec![message("p", "ch", 1, "new", "y")],
            2,
            false,
            0,
            epoch
        ));
        assert_eq!(state.get("p").unwrap().messages[0].message_id, "new");
    }

    #[test]
    fn invalid_locators_fail_closed() {
        let mut state = ChatState::new();
        assert!(state.begin_history("", "ch").is_none());
        assert!(state.begin_history("p", "").is_none());
        assert!(state.begin_history("p", "a\0b").is_none());
        assert!(state.begin_ensure("p\nq").is_none());
        assert_eq!(state.project_count(), 0);
        state.set_draft("", "x".to_string());
        assert_eq!(state.project_count(), 0);
    }

    #[test]
    fn message_line_renders_references_as_handles_and_truncates() {
        let mut msg = message("p", "ch", 7, "m7", "hello world");
        msg.author_agent = Some("reviewer".to_string());
        msg.reply_to = Some("m1-long-id".to_string());
        msg.mentions = vec!["bob".to_string()];
        msg.references = vec![ChatReferenceDto {
            kind: ChatReferenceKindDto::Session,
            target_id: "sess-123".to_string(),
            display_hint: Some("review session".to_string()),
        }];
        let line = message_line(&msg);
        assert!(line.contains("[#7]"));
        assert!(line.contains("alice"));
        assert!(line.contains("hello world"));
        assert!(line.contains("(via reviewer)"));
        assert!(line.contains("(reply to m1-long-"));
        assert!(line.contains("@bob"));
        assert!(line.contains("[session:sess-123 \"review session\"]"));
        // Long bodies truncate with char boundary.
        let long = message(
            "p",
            "ch",
            8,
            "m8",
            &"y".repeat(MAX_CHAT_BODY_DISPLAY_LEN + 50),
        );
        let long_line = message_line(&long);
        assert!(
            long_line.ends_with('…')
                || long_line.len() < "y".repeat(MAX_CHAT_BODY_DISPLAY_LEN + 50).len() + 64
        );
    }

    #[test]
    fn mention_extraction_is_bounded_and_never_authorizes() {
        assert_eq!(extract_mentions("hi @bob and @carol"), vec!["bob", "carol"]);
        assert_eq!(
            extract_mentions("email bob@example.com"),
            Vec::<String>::new()
        );
        assert_eq!(extract_mentions("lone @"), Vec::<String>::new());
        // Duplicates collapse.
        assert_eq!(extract_mentions("@bob @bob"), vec!["bob"]);
        // Bounded.
        let many = (0..40)
            .map(|i| format!("@u{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(extract_mentions(&many).len(), MAX_CHAT_MENTIONS);
    }

    #[test]
    fn eviction_keeps_newest_projects_bounded() {
        let mut state = ChatState::new();
        state.set_capability(true);
        for i in 0..(MAX_CHAT_PROJECTS + 4) {
            let project = format!("proj-{i:02}");
            ready_window(&mut state, &project, "ch", 1);
        }
        assert!(state.project_count() <= MAX_CHAT_PROJECTS);
        assert!(state
            .get(&format!("proj-{:02}", MAX_CHAT_PROJECTS + 3))
            .is_some());
    }

    #[test]
    fn hint_creates_resync_flag_without_storing_content() {
        let mut state = ChatState::new();
        state.set_capability(true);
        assert!(state.note_hint("proj-new"));
        let entry = state.get("proj-new").unwrap();
        assert!(entry.needs_resync);
        assert!(entry.messages.is_empty());
        // Loading entries coalesce (no fetch storm).
        let _ = state.begin_history("proj-new", "ch").unwrap();
        assert!(!state.note_hint("proj-new"));
    }
}
