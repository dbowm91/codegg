//! Authorized read-only session observation (Presence M003).
//!
//! Daemon-owned canonical projections are the authority; this module is a
//! bounded frontend projection of the observer session plus the central
//! read-only input policy. The TUI owns no observation truth: every field
//! here is derived from an authorized `ProjectionSubscribe` /
//! `ProjectionResume` round-trip through `CoreClient`, keyed by canonical
//! `session_id`.
//!
//! Invariants:
//!
//! * One observed session at a time. Starting a new observation replaces
//!   (and must unsubscribe) the previous observer-owned subscription;
//!   observer disconnect tears down only the observer-owned subscription.
//! * Stale completions are dropped via per-observation `request_id` plus a
//!   global `reconnect_epoch`. Pre-reconnect completions fail closed.
//! * Unauthorized (`project_not_found`) and unsupported (old daemon)
//!   render identically as unavailable; cached observed state is cleared
//!   on authorization loss so hidden data is never rendered locally.
//! * Observer mode is explicitly read-only. Ordinary observer input
//!   (prompt submit, permission/question answers, steering, cancels,
//!   model/agent/provider changes, file/worktree/Git/job mutations)
//!   is rejected centrally via [`ObserverState::blocks_command`] and
//!   [`ObserverState::blocks_prompt_submit`]. The collaboration chat input
//!   seam ([`ObserverState::collaboration_input_placeholder`]) is a stable
//!   placeholder only; no chat storage lives here.
//! * Memory is bounded: session/project locators are validated
//!   (non-empty, <=128 bytes, no NUL/control); artifact/projection payloads
//!   are never stored here (they live in `ProjectionClientState` bounds).
//! * Raw terminal frames are never observation protocol; observation uses
//!   canonical snapshot/replay/live semantics with existing redaction and
//!   artifact bounds.

use codegg_protocol::projection::replay::{ProjectionCursor, ProjectionSubscriptionId};

/// Maximum locator bytes for an observed session/project id.
pub const MAX_OBSERVE_LOCATOR_LEN: usize = 128;

/// Presentation status for the observed session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveStatus {
    /// No observation active.
    Idle,
    /// Subscribe/resume in flight.
    Loading,
    /// Authorized live subscription (snapshot + replay/live tail).
    Live,
    /// Transport reconnect; resync required before live resumes.
    Reconnecting,
    /// Transient failure with resync flagged (stale view retained).
    Error,
    /// Authorization denied (`project_not_found`); indistinguishable from
    /// absent. No observed content retained.
    Denied,
    /// Daemon without projection support; observation unavailable.
    Unsupported,
}

/// One observed session target.
#[derive(Debug, Clone)]
pub struct ObservedTarget {
    /// Canonical project id (opaque, never interpreted as a path).
    pub project_id: String,
    /// Canonical session id under observation.
    pub session_id: String,
    /// Observer-owned daemon subscription id (cleared on stop/deny).
    pub subscription_id: Option<ProjectionSubscriptionId>,
    /// Last authoritative cursor (for resume/resync).
    pub cursor: Option<ProjectionCursor>,
    /// Last delivered event sequence (for lag display).
    pub last_delivered_seq: u64,
    /// Last acked sequence.
    pub last_acked_seq: u64,
    /// Current in-flight request id (0 when idle).
    pub current_request_id: u64,
    /// Whether a resync/re-fetch is required.
    pub needs_resync: bool,
    /// Bounded last error (secret-free, display-truncated).
    pub last_error: Option<String>,
    /// Presentation status.
    pub status: ObserveStatus,
}

/// Bounded observer-mode projection. Single target only.
#[derive(Debug, Default)]
pub struct ObserverState {
    target: Option<ObservedTarget>,
    /// Monotonic reconnect epoch. Bumped on transport reconnect so
    /// pre-reconnect completions are dropped.
    pub reconnect_epoch: u64,
    request_counter: u64,
}

fn valid_locator(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_OBSERVE_LOCATOR_LEN {
        return false;
    }
    !value.bytes().any(|b| b == 0 || b.is_ascii_control())
}

fn truncate_error(msg: &str) -> String {
    const MAX: usize = 256;
    if msg.len() <= MAX {
        return msg.to_string();
    }
    let mut cut = MAX;
    while cut > 0 && !msg.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + 1);
    out.push_str(&msg[..cut]);
    out.push('…');
    out
}

impl ObserverState {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` when an observation target is active (loading/live/error/
    /// reconnecting). Denied/unsupported/idle count as not observing for
    /// input-policy purposes once cleared; while a denied target is
    /// retained for display, input remains blocked until `stop`.
    pub fn is_observing(&self) -> bool {
        self.target.is_some()
    }

    /// `true` when the observer holds a live subscription.
    pub fn is_live(&self) -> bool {
        matches!(
            self.target.as_ref().map(|t| t.status),
            Some(ObserveStatus::Live)
        )
    }

    pub fn target(&self) -> Option<&ObservedTarget> {
        self.target.as_ref()
    }

    pub fn observed_session_id(&self) -> Option<&str> {
        self.target.as_ref().map(|t| t.session_id.as_str())
    }

    pub fn observed_project_id(&self) -> Option<&str> {
        self.target.as_ref().map(|t| t.project_id.as_str())
    }

    pub fn subscription_id(&self) -> Option<&ProjectionSubscriptionId> {
        self.target
            .as_ref()
            .and_then(|t| t.subscription_id.as_ref())
    }

    pub fn cursor(&self) -> Option<&ProjectionCursor> {
        self.target.as_ref().and_then(|t| t.cursor.as_ref())
    }

    /// Begin observing `session_id` in `project_id`. Returns the request id
    /// the completion must echo, or `None` when the locator is invalid
    /// (fail closed, no state change). Replaces any existing target; the
    /// caller MUST unsubscribe the previous observer-owned subscription
    /// before or immediately after calling this (teardown is owned by the
    /// command layer, not the reducer).
    pub fn begin_observe(&mut self, project_id: &str, session_id: &str) -> Option<u64> {
        if !valid_locator(project_id) || !valid_locator(session_id) {
            return None;
        }
        self.request_counter = self.request_counter.saturating_add(1);
        let request_id = self.request_counter;
        self.target = Some(ObservedTarget {
            project_id: project_id.to_string(),
            session_id: session_id.to_string(),
            subscription_id: None,
            cursor: None,
            last_delivered_seq: 0,
            last_acked_seq: 0,
            current_request_id: request_id,
            needs_resync: false,
            last_error: None,
            status: ObserveStatus::Loading,
        });
        Some(request_id)
    }

    /// Apply an authorized subscribe completion. Drops stale completions
    /// (wrong request id, wrong epoch, or session mismatch).
    pub fn apply_subscribed(
        &mut self,
        request_id: u64,
        project_id: &str,
        session_id: &str,
        subscription_id: ProjectionSubscriptionId,
        cursor: ProjectionCursor,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        let Some(target) = self.target.as_mut() else {
            return false;
        };
        if target.current_request_id != request_id {
            return false;
        }
        if target.project_id != project_id || target.session_id != session_id {
            return false;
        }
        target.subscription_id = Some(subscription_id);
        target.cursor = Some(cursor);
        target.current_request_id = 0;
        target.needs_resync = false;
        target.last_error = None;
        target.status = ObserveStatus::Live;
        true
    }

    /// Apply a resume/replay continuation. Updates the cursor and delivery
    /// watermark; drops stale completions identically to subscribe.
    pub fn apply_resumed(
        &mut self,
        request_id: u64,
        session_id: &str,
        cursor: ProjectionCursor,
        last_delivered_seq: u64,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        let Some(target) = self.target.as_mut() else {
            return false;
        };
        if target.current_request_id != request_id {
            return false;
        }
        if target.session_id != session_id {
            return false;
        }
        target.cursor = Some(cursor);
        target.last_delivered_seq = last_delivered_seq;
        target.current_request_id = 0;
        target.needs_resync = false;
        target.last_error = None;
        target.status = ObserveStatus::Live;
        true
    }

    /// Apply an authorization denial (`project_not_found`). Clears
    /// subscription/cursor so no observed content is retained; the target
    /// shell is retained in `Denied` state so the banner can explain
    /// without leaking existence (identical rendering to absent).
    pub fn apply_denied(
        &mut self,
        request_id: u64,
        session_id: &str,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        let Some(target) = self.target.as_mut() else {
            return false;
        };
        if target.current_request_id != request_id {
            return false;
        }
        if target.session_id != session_id {
            return false;
        }
        target.subscription_id = None;
        target.cursor = None;
        target.current_request_id = 0;
        target.needs_resync = false;
        target.last_error = None;
        target.status = ObserveStatus::Denied;
        true
    }

    /// Apply a transient failure. Retains the last cursor (when present)
    /// and flags resync; drops stale completions.
    pub fn apply_error(
        &mut self,
        request_id: u64,
        session_id: &str,
        error: String,
        unsupported: bool,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        let Some(target) = self.target.as_mut() else {
            return false;
        };
        if target.current_request_id != request_id {
            return false;
        }
        if target.session_id != session_id {
            return false;
        }
        target.current_request_id = 0;
        if unsupported {
            target.subscription_id = None;
            target.cursor = None;
            target.needs_resync = false;
            target.last_error = None;
            target.status = ObserveStatus::Unsupported;
        } else {
            target.status = ObserveStatus::Error;
            target.last_error = Some(truncate_error(&error));
            target.needs_resync = true;
        }
        true
    }

    /// Flag the active observation for resync (reconnect, lag, or hint).
    /// Returns `true` when the caller should issue a resume/re-fetch.
    /// Coalesces while loading (no fetch storm).
    pub fn note_resync(&mut self) -> bool {
        let Some(target) = self.target.as_mut() else {
            return false;
        };
        if target.status == ObserveStatus::Loading {
            return false;
        }
        if matches!(
            target.status,
            ObserveStatus::Denied | ObserveStatus::Unsupported
        ) {
            return false;
        }
        target.needs_resync = true;
        if target.status == ObserveStatus::Live {
            target.status = ObserveStatus::Reconnecting;
        }
        true
    }

    /// Begin a resume/re-fetch for the active target. Returns the request
    /// id the completion must echo, or `None` when no target is active or
    /// a fetch is already in flight.
    pub fn begin_resume(&mut self) -> Option<(u64, String, Option<ProjectionCursor>)> {
        let target = self.target.as_mut()?;
        if target.current_request_id != 0 {
            return None;
        }
        if matches!(
            target.status,
            ObserveStatus::Denied | ObserveStatus::Unsupported
        ) {
            return None;
        }
        self.request_counter = self.request_counter.saturating_add(1);
        let request_id = self.request_counter;
        target.current_request_id = request_id;
        target.status = ObserveStatus::Loading;
        target.needs_resync = false;
        Some((request_id, target.session_id.clone(), target.cursor.clone()))
    }

    /// Transport reconnect: bump the epoch, drop in-flight bindings, and
    /// flag the target for resync. Stale presentation is replaced by the
    /// next authoritative snapshot/replay; nothing is fabricated.
    pub fn on_reconnect(&mut self) -> u64 {
        self.reconnect_epoch = self.reconnect_epoch.saturating_add(1);
        if let Some(target) = self.target.as_mut() {
            target.current_request_id = 0;
            if matches!(
                target.status,
                ObserveStatus::Live | ObserveStatus::Error | ObserveStatus::Reconnecting
            ) {
                target.needs_resync = true;
                target.status = ObserveStatus::Reconnecting;
            }
        }
        self.reconnect_epoch
    }

    /// Stop observing. Clears the target so hidden data is never rendered
    /// from a local cache. Returns the observer-owned subscription id (when
    /// present) so the caller can issue the authoritative unsubscribe.
    pub fn stop(&mut self) -> Option<ProjectionSubscriptionId> {
        self.target.take().and_then(|t| t.subscription_id)
    }

    /// Clear on authorization loss (project switch, sign-out). Identical to
    /// `stop` but discards the subscription id without use (the daemon
    /// already revoked delivery; the caller MUST still attempt unsubscribe
    /// best-effort when an id was present — use `stop` when the id is
    /// needed).
    pub fn clear(&mut self) {
        self.target = None;
    }

    /// Explicit read-only banner for the observed session. `None` when not
    /// observing. Never carries prompts, file content, or secrets — only
    /// the opaque session locator and coarse status.
    pub fn banner_line(&self) -> Option<String> {
        let target = self.target.as_ref()?;
        let state = match target.status {
            ObserveStatus::Idle => "idle",
            ObserveStatus::Loading => "connecting…",
            ObserveStatus::Live => "live",
            ObserveStatus::Reconnecting => "reconnecting…",
            ObserveStatus::Error => "stale — refresh to resync",
            ObserveStatus::Denied => "unavailable",
            ObserveStatus::Unsupported => "unavailable",
        };
        Some(format!(
            "👁 OBSERVING {} (read-only) · {}",
            truncate_session(&target.session_id),
            state
        ))
    }

    /// Collaboration input seam (project-collaboration M002 consumes
    /// this). Bare observer insert-mode input routes to project chat via
    /// `crate::tui::commands::chat::route_observer_insert_to_chat`; this
    /// returns the stable user-facing fallback explaining that observer
    /// input is not sent as a turn, used only when no project is
    /// available for chat routing. The text is bounded and secret-free.
    pub fn collaboration_input_placeholder(&self) -> Option<String> {
        let target = self.target.as_ref()?;
        Some(format!(
            "Observer mode is read-only — input for {} was not sent as a turn. Bare text goes to project chat; use /stop-observing to resume control.",
            truncate_session(&target.session_id)
        ))
    }

    /// Central read-only policy: `true` when `command` (slash name without
    /// args, case-insensitive, leading `/` optional) MUST be rejected while
    /// observing. When not observing, nothing is blocked. The allowlist is
    /// intentionally narrow (fail closed); every control/mutation family is
    /// denied by default.
    pub fn blocks_command(&self, command: &str) -> bool {
        if !self.is_observing() {
            return false;
        }
        // While denied/unsupported the shell is retained for display but
        // input stays blocked until an explicit stop (prevents confused
        // control attempts against an unavailable target).
        !is_observer_allowed_command(command)
    }

    /// `true` when prompt submit (Enter) MUST be rejected while observing.
    /// Ordinary observer input causes no mutation/control; the caller
    /// should surface [`Self::collaboration_input_placeholder`].
    pub fn blocks_prompt_submit(&self) -> bool {
        self.is_observing()
    }

    /// `true` when permission/question answers MUST be rejected while
    /// observing. Observers can view pending counts via the projection
    /// summary but can never answer for the target session.
    pub fn blocks_permission_response(&self) -> bool {
        self.is_observing()
    }
}

fn truncate_session(session_id: &str) -> String {
    const MAX: usize = 24;
    if session_id.len() <= MAX {
        return session_id.to_string();
    }
    let mut cut = MAX;
    while cut > 0 && !session_id.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &session_id[..cut])
}

/// Narrow read-only allowlist for observer mode. Everything not listed is
/// blocked (fail closed). Names are normalized (trimmed, leading `/`
/// stripped, lowercased) before comparison.
pub fn is_observer_allowed_command(command: &str) -> bool {
    let normalized = command
        .trim()
        .trim_start_matches('/')
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    matches!(
        normalized.as_str(),
        // Observer lifecycle itself.
        "observe" | "watch" | "stop-observing" | "unwatch" |
        // Project chat (M002 collaboration seam). Read/write through the
        // daemon-authorized `chat.v1` surface only; never session control.
        "chat" | "chat-send" | "chat-reply" | "chat-history" | "chat-sync" |
        "chat-read" | "chat-edit" | "chat-redact" | "chat-composing" |
        // Help/status/navigation (read-only).
        "help" | "status" | "sessions" | "resume" | "continue" |
        "collaborators" | "presence" | "team" |
        "context" | "cost" | "usage" | "stats" | "state" |
        "search" | "timeline" | "export" |
        "timestamps" | "toggle-timestamps" | "thinking" | "toggle-thinking" |
        "tui" | "fullscreen" | "themes" | "theme" | "keybinds" |
        "tts" | "voice" |
        "doctor" | "tree" | "diff" | "review" |
        "tests" |
        // Read-only LSP inspection (no apply/clear/restart/stop/repair).
        "lsp-status" | "lsp-previews" | "preview-list" |
        "lsp-preview" | "preview-show" |
        "lsp-servers" | "lsp-detail" | "lsp-capabilities" |
        "lsp-errors" | "lsp-root" | "lsp-cache-status" |
        "lsp-doctor" | "lsp-context-diagnostics" |
        // Read-only shell/terminal inspection.
        "shell-list" | "shell-show" | "shell-include" | "shell-expand" |
        "terminal-list" | "terminal-show" |
        // Read-only memory/research/tool inspection.
        "memory" | "memory-search" | "memory-list" |
        "habits" |
        "research-runs" | "research-open" | "research-show" |
        "tool-backends" | "tools" | "backends" |
        "plugins" | "plugin-list" | "plugin-ls" | "plugin-info" | "plugin-doctor" |
        "security-review-show" |
        "tui-stats" |
        "models-refresh" | "refresh-models"
    )
}

/// Human-readable denial for blocked observer commands. Bounded and
/// secret-free; directs to `/stop-observing` and the project chat seam.
pub fn observer_blocked_message(command: &str) -> String {
    format!(
        "Observer mode is read-only — /{0} is disabled while observing. Use /stop-observing to resume control.",
        command
            .trim()
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("command")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub_id(id: &str) -> ProjectionSubscriptionId {
        ProjectionSubscriptionId::new(id)
    }

    fn cursor(stream: &str, seq: u64) -> ProjectionCursor {
        ProjectionCursor {
            stream_id: codegg_protocol::projection::replay::ProjectionStreamId(format!(
                "stream-{stream}"
            )),
            event_seq: seq,
            projection_version: 1,
        }
    }

    #[test]
    fn begin_rejects_invalid_locators() {
        let mut state = ObserverState::new();
        assert!(state.begin_observe("", "s1").is_none());
        assert!(state.begin_observe("p1", "").is_none());
        assert!(state.begin_observe("p1", "a\0b").is_none());
        assert!(state.begin_observe("p1", "a\nb").is_none());
        assert!(!state.is_observing());
    }

    #[test]
    fn subscribe_round_trip_with_stale_guards() {
        let mut state = ObserverState::new();
        let req = state.begin_observe("proj-1", "sess-1").unwrap();
        let epoch = state.reconnect_epoch;
        // Wrong session mismatch drops.
        assert!(!state.apply_subscribed(
            req,
            "proj-1",
            "sess-other",
            sub_id("sub-1"),
            cursor("a", 10),
            epoch
        ));
        assert!(!state.is_live());
        // Wrong epoch drops.
        assert!(!state.apply_subscribed(
            req,
            "proj-1",
            "sess-1",
            sub_id("sub-1"),
            cursor("a", 10),
            epoch + 1
        ));
        // Correct applies.
        assert!(state.apply_subscribed(
            req,
            "proj-1",
            "sess-1",
            sub_id("sub-1"),
            cursor("a", 10),
            epoch
        ));
        assert!(state.is_live());
        assert!(state.banner_line().unwrap().contains("sess-1"));
    }

    #[test]
    fn denied_clears_subscription_and_blocks_input() {
        let mut state = ObserverState::new();
        let req = state.begin_observe("proj-1", "sess-1").unwrap();
        let epoch = state.reconnect_epoch;
        assert!(state.apply_denied(req, "sess-1", epoch));
        assert!(state.is_observing());
        assert!(state.subscription_id().is_none());
        assert!(state.cursor().is_none());
        assert!(state.blocks_prompt_submit());
        assert!(state.blocks_permission_response());
        assert!(state.blocks_command("/turn"));
        assert!(state.collaboration_input_placeholder().is_some());
    }

    #[test]
    fn read_only_policy_blocks_every_control_family() {
        let mut state = ObserverState::new();
        assert!(!state.blocks_command("/help"));
        let _ = state.begin_observe("p1", "s1").unwrap();
        // Allowed read-only sample.
        for allowed in [
            "/help",
            "/status",
            "/sessions",
            "/collaborators",
            "/observe",
            "/stop-observing",
            "/context",
            "/search",
            "/diff",
            "/lsp-status",
            "/shell-list",
            "/memory-search",
        ] {
            assert!(
                !state.blocks_command(allowed),
                "should allow {allowed} while observing"
            );
        }
        // Every control family from the plan must be blocked.
        for blocked in [
            // Turn steering/cancel + prompt submit.
            "/turn",
            "turn_submit",
            "turn_steer",
            "turn_cancel",
            // Permission/question answers.
            "permission_respond",
            "question_respond",
            "/permission",
            // Model/provider/agent settings.
            "/models",
            "/agent",
            "/agents",
            "/connections",
            "/connect",
            // Session mutations.
            "/new",
            "/fork",
            "/rename",
            "/share",
            "/delete",
            "/compact",
            "/undo",
            "/goal",
            "/plan",
            // File/worktree mutations.
            "/revert",
            "/checkpoint",
            "/lsp-preview-apply",
            "/preview-apply",
            // Git-adjacent mutations via agent templates.
            "/pr",
            "/issue",
            // Jobs/schedules.
            "/loop",
            "/task-del",
            "/test",
            // Shell/terminal control.
            "/shell-rerun",
            "/shell-kill",
            "/terminal-send",
            "/terminal-terminate",
            // Plugin/policy mutations.
            "/plugin-enable",
            "/plugin-install",
            // Interactive process control.
            "/terminal-create",
        ] {
            assert!(
                state.blocks_command(blocked),
                "must block {blocked} while observing"
            );
        }
        assert!(state.blocks_prompt_submit());
        assert!(state.blocks_permission_response());
    }

    #[test]
    fn reconnect_bumps_epoch_and_flags_resync() {
        let mut state = ObserverState::new();
        let req = state.begin_observe("p1", "s1").unwrap();
        let epoch = state.reconnect_epoch;
        assert!(state.apply_subscribed(req, "p1", "s1", sub_id("sub-1"), cursor("a", 5), epoch));
        let new_epoch = state.on_reconnect();
        assert_eq!(new_epoch, epoch + 1);
        // Stale completion with old epoch drops.
        let stale_req = 999;
        assert!(!state.apply_subscribed(
            stale_req,
            "p1",
            "s1",
            sub_id("sub-2"),
            cursor("a", 6),
            epoch
        ));
        // Resume path works on the new epoch.
        let resume_req = state.begin_resume().map(|(id, _, _)| id).unwrap();
        assert!(state.apply_resumed(resume_req, "s1", cursor("a", 7), 7, new_epoch));
        assert!(state.is_live());
    }

    #[test]
    fn stop_clears_without_leak() {
        let mut state = ObserverState::new();
        let req = state.begin_observe("p1", "s1").unwrap();
        let epoch = state.reconnect_epoch;
        assert!(state.apply_subscribed(req, "p1", "s1", sub_id("sub-1"), cursor("a", 1), epoch));
        let owned = state.stop();
        assert_eq!(owned.unwrap().as_str(), "sub-1");
        assert!(!state.is_observing());
        assert!(state.banner_line().is_none());
        assert!(!state.blocks_prompt_submit());
    }

    #[test]
    fn banner_never_carries_content() {
        let mut state = ObserverState::new();
        let req = state.begin_observe("p1", "sess-abc").unwrap();
        let epoch = state.reconnect_epoch;
        assert!(state.apply_subscribed(req, "p1", "sess-abc", sub_id("s"), cursor("a", 1), epoch));
        let banner = state.banner_line().unwrap();
        assert!(banner.contains("OBSERVING"));
        assert!(banner.contains("read-only"));
        assert!(!banner.contains("token"));
        assert!(!banner.contains("secret"));
    }
}
