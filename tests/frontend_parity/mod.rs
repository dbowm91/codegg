//! Frontend parity: one canonical projection snapshot, two frontends.
//!
//! The desktop GUI (`apps/desktop/`) and the terminal UI (`src/tui/`) are two
//! clients of the same daemon over the same frontend-neutral protocol. The
//! claim this family proves is mechanical: **given the same canonical
//! `SessionProjectionSnapshot`, both frontends reconstruct the same state.**
//!
//! ## The constraint that shapes the proof
//!
//! The desktop crate is excluded from the root workspace
//! (`scripts/check-desktop-boundary.sh`), so the two frontends cannot be
//! driven from one test binary. The proof is split across two crates that
//! read **one** golden file:
//!
//! * this family (`tests/frontend_parity/`) reconstructs the state with the
//!   TUI and compares it to the golden's `expected` block;
//! * `apps/desktop/src-tauri/tests/frontend_parity.rs` does the same with
//!   the desktop host.
//!
//! The golden lives at `tests/frontend_parity/golden/session_projection_parity.json`
//! because the root package's `tests/` tree is the canonical home for
//! root-package test data, and the desktop crate already reaches the repo
//! tree through its path dependencies on `../../../crates/...`, so a relative
//! `include_str!` reaches the same file from `apps/desktop/src-tauri/tests/`.
//!
//! ## What the golden exercises
//!
//! The golden is not "some JSON both sides can parse". It carries the facts
//! that force each frontend to *choose*:
//!
//! * a message whose visibility class is not `Public`, and a `system` message
//!   — neither may reach a frontend's view;
//! * a resolved permission and a resolved question beside the pending ones —
//!   only the pending ones are actionable;
//! * a legacy turn-local agent-tree node that is **not** in the session's
//!   `subagents` — the liveness authority is `snapshot.subagents`, not
//!   `active_turn.agent_tree`;
//! * six recent turns and an active turn — a transcript built in snapshot
//!   order is wrong, and the transcript keeps the newest five recent turns;
//! * a tool program, a job, durable agent runs, a run group, a convergence,
//!   and a worktree — the background activity surfaces.
//!
//! ## The honest gap
//!
//! The TUI's transcript is **not** projection-derived: it is fed by the raw
//! core event stream, not by the snapshot, so the TUI cannot be compared on
//! messages, tools, or a pending-permission *list*. Those facts live in a
//! separate `desktopExpected` block, which the desktop test asserts and the
//! TUI test does not pretend to cover; `tui_reconstruction.inc` holds the
//! test that pins the gap mechanically.

use serde::{Deserialize, Serialize};

use codegg::protocol::projection::snapshot::SessionProjectionSnapshot;

/// The one golden both frontends read.
///
/// Path note: this file lives under the root package's `tests/` tree because
/// that is the canonical home for root-package test data, and the desktop
/// crate reaches the repo tree already (it has path dependencies on
/// `../../../crates/...`), so `apps/desktop/src-tauri/tests/` reaches the
/// same file through a relative `include_str!`.
pub const GOLDEN_JSON: &str = include_str!("golden/session_projection_parity.json");

/// The golden as parsed: the canonical snapshot plus the state both frontends
/// must reconstruct from it.
#[derive(Debug, Clone, Deserialize)]
pub struct Golden {
    /// The canonical `SessionProjectionSnapshot` both frontends consume.
    pub snapshot: SessionProjectionSnapshot,
    /// The shared canonical state. Both frontends map their own view into
    /// this shape and must produce exactly this value.
    pub expected: CanonicalState,
    /// The facts only the desktop host reconstructs from the snapshot. The
    /// TUI's transcript is event-fed, so it cannot be compared here; see the
    /// family docs.
    #[serde(default)]
    pub desktop_expected: Option<DesktopExpected>,
}

impl Golden {
    /// Parse the golden. A parse failure is a fixture defect, not a product
    /// defect, so it panics with the file path.
    pub fn load() -> Self {
        serde_json::from_str(GOLDEN_JSON).unwrap_or_else(|error| {
            panic!(
                "the shared parity golden must parse as JSON: {error} ({})",
                file!()
            )
        })
    }
}

/// The canonical, frontend-neutral session state both frontends reconstruct.
///
/// This shape is deliberately small: it holds the facts that survive both
/// frontends' presentation choices — the turn's status, how many items are
/// waiting on a human, which subagents are live, and which background work
/// exists. It is the contract, and both crates declare their own copy of it
/// because neither may depend on the other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalState {
    /// The primary session the snapshot describes.
    pub session_id: String,
    /// The active turn's status, spelled the daemon's way
    /// (`awaiting_permission`, not `awaitingpermission`).
    pub turn_status: String,
    /// How many permissions are still waiting on a human.
    pub pending_permission_count: u32,
    /// How many questions are still waiting on a human.
    pub pending_question_count: u32,
    /// The session-owned subagents — the projection's liveness authority.
    pub subagents: Vec<NamedState>,
    /// The per-turn agent tree's legacy children: turn-local nodes with no
    /// durable child identity.
    pub agent_tree: Vec<NamedState>,
    /// Durable delegated agent runs.
    pub agent_runs: Vec<IdentifiedState>,
    /// Background tool programs.
    pub tool_programs: Vec<IdentifiedState>,
    /// Durable scheduler jobs.
    pub jobs: Vec<IdentifiedState>,
    /// Durable run groups.
    pub run_groups: Vec<IdentifiedState>,
    /// Durable convergence summaries.
    pub convergences: Vec<IdentifiedState>,
}

/// A named thing and the state it is in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedState {
    /// The human-readable name: the display name when the projection carried
    /// one, otherwise the agent type.
    pub name: String,
    /// The lifecycle state, spelled the daemon's way.
    pub state: String,
}

/// An identified thing and the state it is in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentifiedState {
    /// The thing's canonical identity.
    pub id: String,
    /// The lifecycle state, spelled the daemon's way.
    pub state: String,
}

/// The transcript-shaped facts only the desktop host reconstructs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopExpected {
    /// The current turn's visible messages, in order.
    pub messages: Vec<RoleText>,
    /// How many visible messages the tail cap dropped.
    pub truncated_messages: usize,
    /// The current turn's visible tools, in order.
    pub tools: Vec<ToolState>,
    /// The multi-turn transcript, oldest first.
    pub transcript: Vec<TranscriptEntry>,
    /// How many transcript messages the tail cap dropped.
    pub truncated_transcript_messages: usize,
}

/// A message's role and text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleText {
    pub role: String,
    pub text: String,
}

/// A transcript entry: the owning turn, the role, and the text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntry {
    pub turn_id: String,
    pub role: String,
    pub text: String,
}

/// A tool's name and status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolState {
    pub tool_name: String,
    pub status: String,
}

#[path = "tui_reconstruction.inc"]
pub mod tui_reconstruction;
