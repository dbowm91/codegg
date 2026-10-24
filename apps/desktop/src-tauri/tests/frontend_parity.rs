//! The desktop host's half of the frontend parity proof.
//!
//! The desktop GUI (`apps/desktop/`) and the terminal UI (`src/tui/`) are
//! two clients of the same daemon over the same frontend-neutral protocol.
//! The claim this test proves is narrow and mechanical: **given the same
//! canonical `SessionProjectionSnapshot`, the desktop host reconstructs the
//! same session state the terminal UI reconstructs.**
//!
//! ## Why this test lives in the desktop crate
//!
//! The desktop crate is excluded from the root workspace
//! (`scripts/check-desktop-boundary.sh` fails if `codegg-desktop` appears in
//! the root package's cargo metadata), so the two frontends cannot be driven
//! from one test binary. The proof is therefore split: this file reads the
//! **same golden** the terminal UI's family reads
//! (`tests/frontend_parity/golden/session_projection_parity.json`) and
//! compares the same `expected` block, so the two halves meet at one
//! fixture rather than at two hand-written expectations.
//!
//! ## The canonical shape
//!
//! `CanonicalState` below is this crate's copy of the shared shape. The
//! desktop crate cannot import the root package's test code — the root
//! package cannot import the desktop crate either — so the contract is the
//! golden file, and each side declares its own reader for it. The two
//! declarations must stay field-for-field identical; the golden's
//! `expected` block is what proves they are.

use codegg_client::DriverState;
use codegg_desktop::bridge::{SessionPresentationView, SubagentSummaryView};
use codegg_desktop::present::present_snapshot;
use codegg_protocol::projection::snapshot::SessionProjectionSnapshot;
use serde::{Deserialize, Serialize};

/// The one golden both frontends read.
///
/// The desktop crate reaches the repo tree through its path dependencies on
/// `../../../crates/...`, so the root package's `tests/` tree is reachable
/// from here by a relative path.
const GOLDEN_JSON: &str =
    include_str!("../../../../tests/frontend_parity/golden/session_projection_parity.json");

/// The golden as parsed: the canonical snapshot plus the state both frontends
/// must reconstruct from it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Golden {
    snapshot: SessionProjectionSnapshot,
    expected: CanonicalState,
    desktop_expected: Option<DesktopExpected>,
}

/// The canonical, frontend-neutral session state both frontends reconstruct.
///
/// This is this crate's copy of the shared shape; the terminal UI's family
/// declares the same one. The golden's `expected` block is the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalState {
    session_id: String,
    turn_status: String,
    pending_permission_count: u32,
    pending_question_count: u32,
    subagents: Vec<NamedState>,
    agent_tree: Vec<NamedState>,
    agent_runs: Vec<IdentifiedState>,
    tool_programs: Vec<IdentifiedState>,
    jobs: Vec<IdentifiedState>,
    run_groups: Vec<IdentifiedState>,
    convergences: Vec<IdentifiedState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct NamedState {
    name: String,
    state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct IdentifiedState {
    id: String,
    state: String,
}

/// The transcript-shaped facts only the desktop host reconstructs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopExpected {
    messages: Vec<RoleText>,
    truncated_messages: usize,
    tools: Vec<ToolState>,
    transcript: Vec<TranscriptEntry>,
    truncated_transcript_messages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RoleText {
    role: String,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptEntry {
    turn_id: String,
    role: String,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolState {
    tool_name: String,
    status: String,
}

fn load_golden() -> Golden {
    serde_json::from_str(GOLDEN_JSON).unwrap_or_else(|error| {
        panic!(
            "the shared parity golden must parse as JSON: {error} ({})",
            file!()
        )
    })
}

/// The human-readable name of a subagent node: the display name when the
/// projection carried one, otherwise the agent type.
///
/// This is the same fallback the terminal UI applies
/// (`src/tui/app/state/subagents.rs:81-86`, `SubagentRow::label`).
fn node_name(node: &SubagentSummaryView) -> String {
    node.display_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(node.agent.as_str())
        .to_string()
}

/// The canonical state the desktop host reconstructs from the view it
/// presents.
fn desktop_canonical_state(view: &SessionPresentationView) -> CanonicalState {
    CanonicalState {
        session_id: view.session_id.clone(),
        turn_status: view
            .turn
            .as_ref()
            .map(|turn| turn.status.clone())
            .unwrap_or_default(),
        pending_permission_count: view.pending_permissions.len() as u32,
        pending_question_count: view.pending_questions.len() as u32,
        // The session liveness authority (`present.rs:639-646`).
        subagents: view
            .subagents
            .iter()
            .map(|node| NamedState {
                name: node_name(node),
                state: node.status.clone(),
            })
            .collect(),
        // The per-turn agent tree, kept alongside it because it still holds
        // legacy nodes with no durable child identity (`present.rs:647-659`).
        agent_tree: view
            .agent_tree
            .iter()
            .map(|node| NamedState {
                name: node_name(node),
                state: node.status.clone(),
            })
            .collect(),
        agent_runs: view
            .agent_runs
            .iter()
            .map(|run| IdentifiedState {
                id: run.run_id.clone(),
                state: run.status.clone(),
            })
            .collect(),
        tool_programs: view
            .tool_programs
            .iter()
            .map(|program| IdentifiedState {
                id: program.program_id.clone(),
                state: program.state.clone(),
            })
            .collect(),
        jobs: view
            .jobs
            .iter()
            .map(|job| IdentifiedState {
                id: job.job_id.clone(),
                state: job.state.clone(),
            })
            .collect(),
        run_groups: view
            .run_groups
            .iter()
            .map(|group| IdentifiedState {
                id: group.group_id.clone(),
                state: group.status.clone(),
            })
            .collect(),
        convergences: view
            .convergences
            .iter()
            .map(|convergence| IdentifiedState {
                id: convergence.convergence_id.clone(),
                state: convergence.status.clone(),
            })
            .collect(),
    }
}

/// Present the golden snapshot the way the desktop host does when its
/// driver is attached.
fn present_golden(golden: &Golden) -> SessionPresentationView {
    present_snapshot(
        &golden.snapshot,
        DriverState::Attached,
        None,
        Some(golden.snapshot.event_seq),
        true,
    )
}

#[test]
fn desktop_reconstructs_the_shared_canonical_state_from_the_golden_snapshot() {
    let golden = load_golden();

    let view = present_golden(&golden);
    let reconstructed = desktop_canonical_state(&view);

    assert_eq!(
        reconstructed, golden.expected,
        "the desktop host must reconstruct the shared canonical state from the \
         golden snapshot"
    );
}

#[test]
fn desktop_hides_the_messages_and_tools_that_may_not_cross() {
    let golden = load_golden();
    let expected = golden
        .desktop_expected
        .clone()
        .expect("the golden carries the desktop-only expectations");

    let view = present_golden(&golden);

    let messages: Vec<RoleText> = view
        .messages
        .iter()
        .map(|message| RoleText {
            role: message.role.clone(),
            text: message.text.clone(),
        })
        .collect();
    assert_eq!(
        messages, expected.messages,
        "only public user/assistant/tool messages may cross to the renderer"
    );
    assert!(
        !messages
            .iter()
            .any(|message| message.text.contains("INTERNAL-ONLY")),
        "a non-public message must never reach the renderer"
    );
    assert!(
        !messages
            .iter()
            .any(|message| message.text.contains("SYSTEM-ONLY")),
        "a system message must never reach the renderer"
    );

    let tools: Vec<ToolState> = view
        .tools
        .iter()
        .map(|tool| ToolState {
            tool_name: tool.tool_name.clone(),
            status: tool.status.clone(),
        })
        .collect();
    assert_eq!(
        tools, expected.tools,
        "only public tools may cross to the renderer"
    );
    assert!(
        !tools.iter().any(|tool| tool.tool_name == "internal_probe"),
        "a non-public tool must never reach the renderer"
    );
}

#[test]
fn desktop_lists_only_the_permissions_and_questions_that_are_still_unresolved() {
    let golden = load_golden();

    let view = present_golden(&golden);

    let permissions: Vec<&str> = view
        .pending_permissions
        .iter()
        .map(|permission| permission.tool.as_str())
        .collect();
    assert_eq!(
        permissions,
        vec!["bash"],
        "only an unresolved permission is actionable"
    );
    let questions: Vec<&str> = view
        .pending_questions
        .iter()
        .map(|question| question.question_id.as_str())
        .collect();
    assert_eq!(
        questions,
        vec!["question-pending-1"],
        "only an unresolved question is actionable"
    );
}

#[test]
fn desktop_orders_the_multi_turn_transcript_oldest_turn_first() {
    let golden = load_golden();
    let expected = golden
        .desktop_expected
        .clone()
        .expect("the golden carries the desktop-only expectations");

    let view = present_golden(&golden);

    let transcript: Vec<TranscriptEntry> = view
        .transcript
        .iter()
        .map(|entry| TranscriptEntry {
            turn_id: entry.turn_id.clone(),
            role: entry.role.clone(),
            text: entry.text.clone(),
        })
        .collect();
    assert_eq!(
        transcript, expected.transcript,
        "the transcript must run oldest turn first, with the active turn last"
    );
    assert_eq!(
        transcript.last().map(|entry| entry.turn_id.as_str()),
        Some("turn-active"),
        "the active turn is the newest turn and must come last"
    );
}

#[test]
fn desktop_caps_the_transcript_at_the_newest_five_recent_turns() {
    let golden = load_golden();

    let view = present_golden(&golden);

    // The golden carries six recent turns, newest first. The transcript
    // walks the newest `MAX_TRANSCRIPT_TURNS` of them and renders them
    // oldest first, so the oldest recent turn never reaches the renderer.
    let transcript_turns: Vec<&str> = view
        .transcript
        .iter()
        .map(|entry| entry.turn_id.as_str())
        .collect();
    assert!(
        !transcript_turns.contains(&"turn-recent-6"),
        "the oldest recent turn must be dropped by the transcript turn cap"
    );
    assert_eq!(
        view.recent_turns.len(),
        5,
        "the view keeps the newest five recent turns"
    );
    assert_eq!(
        view.truncated_transcript_messages, 0,
        "the turn cap drops a whole turn; only the message cap is counted"
    );
}
