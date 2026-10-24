//! The TUI side of the parity proof.
//!
//! The terminal UI reconstructs its view from the canonical projection
//! snapshot through its own surfaces:
//!
//! * the **per-tab projection summary**
//!   (`src/tui/app/state/projection_client.rs:133`,
//!   `ProjectionTabSummary::from_snapshot`) — the turn status and how many
//!   permissions and questions are still waiting on a human;
//! * the **canonical session-work selector**
//!   (`src/tui/app/state/session_work.rs:45`, `select_session_work`) — the
//!   session-owned children, the per-turn legacy children, and the durable
//!   runs, jobs, and tool programs, folded so one execution is never counted
//!   twice;
//! * the **sidebar** (`src/tui/app/render.rs:2535-2604`) — the background
//!   activity sections the TUI renders from the same snapshot.
//!
//! Every fact asserted here is read out of the TUI's own state after the
//! golden snapshot was installed in the TUI's projection client, so the test
//! proves the TUI *reconstructs* the state rather than that it can parse the
//! fixture.
//!
//! ## What this side cannot prove
//!
//! The TUI's transcript is fed by the raw core event stream, not by the
//! projection snapshot: the `CoreEvent::Message` arm of the app's event
//! handler is what fills `messages_state`
//! (`src/tui/app/mod.rs:2288` and the `AppEvent` bridge in
//! `src/tui/runtime/app_events.rs`). No projection message ever reaches it.
//! The TUI therefore has no message list, no tool list, and no
//! pending-permission *list* to compare — only counts, which the daemon
//! authors. The last test in this member pins that boundary mechanically so
//! the gap cannot quietly close unnoticed: if a future change wires the
//! transcript to the projection, this test fails and the parity claim has to
//! be re-derived.

use super::{CanonicalState, Golden, IdentifiedState, NamedState};

use codegg::protocol::projection::caps::PROJECTION_PROTOCOL_VERSION;
use codegg::protocol::projection::controller::ControllerSubscribeOutcome;
use codegg::protocol::projection::dto::VisibilityClass;
use codegg::protocol::projection::replay::{
    ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind, ProjectionSubscriptionId,
};
use codegg::protocol::projection::snapshot::SessionProjectionSnapshot;
use codegg::tui::app::state::session_work::{select_session_work, SessionWorkKind};
use codegg::tui::app::App;

/// The tab the golden is installed under.
///
/// The projection client pairs a stream with a tab by suffix
/// (`refresh_tab_summary_from_controller`,
/// `src/tui/app/state/projection_client.rs:397-403`), so the stream id ends
/// with the tab id.
const TAB_ID: &str = "parity";
const STREAM_ID: &str = "stream-parity";

/// Install the golden snapshot in a fresh TUI and render one frame.
///
/// The frame is what populates the sidebar's projection-derived sections
/// (`populate_session_panel`, `src/tui/app/render.rs:2446`), so a sidebar
/// read after this call is the TUI's *rendered* view of the snapshot.
fn rendered_app(snapshot: &SessionProjectionSnapshot) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("root");
    std::fs::create_dir_all(&root).expect("mkdir");
    let mut app = App::new_for_testing(root.display().to_string());

    let descriptor = ProjectionStreamDescriptor {
        stream_id: ProjectionStreamId(STREAM_ID.to_string()),
        kind: ProjectionStreamKind::Session,
        project_id: snapshot.project_id.clone(),
        workspace_id: Some(snapshot.workspace_id.clone()),
        session_id: Some(snapshot.primary_session_id.clone()),
        projection_version: PROJECTION_PROTOCOL_VERSION,
        retention_floor_seq: 0,
        high_water_seq: snapshot.event_seq,
        latest_checkpoint_seq: None,
    };
    let outcome = app.projection_client.controller_mut().install_subscription(
        ProjectionSubscriptionId("subscription-parity".to_string()),
        descriptor,
        snapshot.clone(),
    );
    assert!(
        matches!(outcome, ControllerSubscribeOutcome::Subscribed { .. }),
        "the TUI's projection client must accept the golden snapshot"
    );
    app.projection_client
        .set_active_tab(Some(TAB_ID.to_string()));
    app.projection_client
        .refresh_tab_summary_from_controller(TAB_ID);

    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render(frame))
        .expect("the TUI must render a frame with the golden installed");
    (dir, app)
}

/// The canonical state the TUI reconstructs from the snapshot it holds.
///
/// Every value is read from the TUI's own state: the summary and the work
/// selector from the projection client, the background sections from the
/// sidebar the TUI rendered.
fn tui_canonical_state(app: &App) -> CanonicalState {
    let summary = app
        .projection_client
        .tab_summary(TAB_ID)
        .expect("the TUI must hold a per-tab summary for the golden's tab");
    let (_, held) = app
        .projection_client
        .active_snapshot()
        .expect("the TUI must hold the golden snapshot");
    let rows = select_session_work(held);

    let named = |row: &codegg::tui::app::state::session_work::SessionWorkRow| NamedState {
        name: row
            .subagent
            .as_ref()
            .map(|subagent| subagent.label().to_string())
            .unwrap_or_default(),
        state: row.status.clone(),
    };

    CanonicalState {
        session_id: held.primary_session_id.clone(),
        turn_status: summary.active_turn_status.clone().unwrap_or_default(),
        pending_permission_count: summary.pending_permission_count,
        pending_question_count: summary.pending_question_count,
        // The session liveness authority: `snapshot.subagents`, reached
        // through the work selector's session-owned child rows.
        subagents: rows
            .iter()
            .filter(|row| row.kind == SessionWorkKind::Child && row.child_instance_id.is_some())
            .map(named)
            .collect(),
        // The per-turn legacy children: turn-local nodes with no durable
        // child identity.
        agent_tree: rows
            .iter()
            .filter(|row| row.kind == SessionWorkKind::Child && row.child_instance_id.is_none())
            .map(named)
            .collect(),
        agent_runs: app
            .sidebar
            .agent_runs
            .iter()
            .map(|run| IdentifiedState {
                id: run.run_id.clone(),
                state: run.status.clone(),
            })
            .collect(),
        tool_programs: app
            .sidebar
            .tool_programs
            .iter()
            .map(|program| IdentifiedState {
                id: program.program_id.clone(),
                state: program.state.clone(),
            })
            .collect(),
        jobs: app
            .sidebar
            .jobs
            .iter()
            .map(|job| IdentifiedState {
                id: job.job_id.clone(),
                state: job.state.clone(),
            })
            .collect(),
        run_groups: app
            .sidebar
            .run_groups
            .iter()
            .map(|group| IdentifiedState {
                id: group.group_id.clone(),
                state: group.status.clone(),
            })
            .collect(),
        convergences: app
            .sidebar
            .convergences
            .iter()
            .map(|convergence| IdentifiedState {
                id: convergence.convergence_id.clone(),
                state: convergence.status.clone(),
            })
            .collect(),
    }
}

#[test]
fn tui_reconstructs_the_shared_canonical_state_from_the_golden_snapshot() {
    let golden = Golden::load();
    let (_dir, app) = rendered_app(&golden.snapshot);

    let reconstructed = tui_canonical_state(&app);

    assert_eq!(
        reconstructed, golden.expected,
        "the TUI must reconstruct the shared canonical state from the golden snapshot"
    );
}

#[test]
fn tui_turn_status_uses_the_daemon_turn_status_spelling() {
    let golden = Golden::load();
    let (_dir, app) = rendered_app(&golden.snapshot);
    let summary = app
        .projection_client
        .tab_summary(TAB_ID)
        .expect("the TUI must hold a per-tab summary for the golden's tab");

    // The TUI spells the turn status by hand
    // (`src/tui/app/state/projection_client.rs:139-152`); the desktop host
    // spells it through serde. The golden's `awaiting_permission` is the
    // daemon's spelling, so a drift on either side shows up here.
    assert_eq!(
        summary.active_turn_status.as_deref(),
        Some("awaiting_permission"),
        "the TUI's hand-written turn-status spelling must match the daemon's"
    );
}

#[test]
fn tui_counts_only_the_permissions_and_questions_that_are_still_unresolved() {
    let golden = Golden::load();
    let (_dir, app) = rendered_app(&golden.snapshot);
    let summary = app
        .projection_client
        .tab_summary(TAB_ID)
        .expect("the TUI must hold a per-tab summary for the golden's tab");
    let turn = golden
        .snapshot
        .active_turn
        .as_ref()
        .expect("the golden's snapshot carries an active turn");

    assert_eq!(
        turn.pending_permissions.len(),
        2,
        "the golden carries one pending and one already-resolved permission"
    );
    assert_eq!(
        turn.pending_questions.len(),
        2,
        "the golden carries one pending and one already-resolved question"
    );
    assert_eq!(
        summary.pending_permission_count, 1,
        "only the unresolved permission may be counted as waiting on a human"
    );
    assert_eq!(
        summary.pending_question_count, 1,
        "only the unresolved question may be counted as waiting on a human"
    );
}

#[test]
fn tui_subagent_liveness_comes_from_the_session_authority_not_the_turn_tree() {
    let golden = Golden::load();
    let (_dir, app) = rendered_app(&golden.snapshot);

    let reconstructed = tui_canonical_state(&app);
    let live_names: Vec<&str> = reconstructed
        .subagents
        .iter()
        .map(|subagent| subagent.name.as_str())
        .collect();

    assert_eq!(
        live_names,
        vec!["Sally", "Bob"],
        "the session's own subagents are the liveness authority"
    );
    assert!(
        !live_names.contains(&"reviewer"),
        "a turn-local legacy node must never be reported as a live subagent"
    );
    assert_eq!(
        reconstructed
            .agent_tree
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>(),
        vec!["reviewer"],
        "the legacy turn-local child belongs to the per-turn tree, not to the live subagents"
    );
}

#[test]
fn tui_work_model_carries_each_durable_run_exactly_once() {
    let golden = Golden::load();
    let (_dir, app) = rendered_app(&golden.snapshot);
    let (_, held) = app
        .projection_client
        .active_snapshot()
        .expect("the TUI must hold the golden snapshot");

    let rows = select_session_work(held);
    let child_bound = rows
        .iter()
        .filter(|row| row.kind == SessionWorkKind::Child && row.run_id.is_some())
        .count();
    let detached = rows
        .iter()
        .filter(|row| row.kind == SessionWorkKind::AgentRun)
        .count();

    assert_eq!(
        child_bound + detached,
        2,
        "both durable runs must be carried by the work model exactly once"
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.run_id.as_deref() == Some("run-child-1"))
            .count(),
        1,
        "a run bound to a child is folded into that child's row, not counted twice"
    );
}

#[test]
fn tui_transcript_is_not_reconstructed_from_the_projection_snapshot() {
    let golden = Golden::load();
    let (_dir, app) = rendered_app(&golden.snapshot);
    let (_, held) = app
        .projection_client
        .active_snapshot()
        .expect("the TUI must hold the golden snapshot");
    let turn = held
        .active_turn
        .as_ref()
        .expect("the golden's snapshot carries an active turn");
    let non_public = turn
        .messages
        .iter()
        .find(|message| message.visibility != VisibilityClass::Public)
        .expect("the golden carries a message that is not public");

    // The TUI's projection client stores the snapshot verbatim: it is not a
    // visibility boundary. That is safe only because the TUI never renders
    // snapshot messages — and that is the gap this family cannot close.
    assert!(
        turn.messages
            .iter()
            .any(|message| message.message_id == non_public.message_id),
        "the TUI's projection client stores the snapshot verbatim, so it holds \
         content a visibility boundary would have dropped"
    );

    let rendered: Vec<String> = app
        .messages_state
        .messages
        .messages
        .iter()
        .map(|message| message.text_content())
        .collect();
    assert!(
        rendered.is_empty(),
        "the TUI's transcript is fed by the raw event stream, not by the \
         projection snapshot: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|text| !text.contains(&non_public.text)),
        "no snapshot message text may appear in the TUI's transcript"
    );
}
