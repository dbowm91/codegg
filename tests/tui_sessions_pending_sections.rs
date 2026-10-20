//! C002 — `/sessions` lists the session's live subagents and jobs.
//!
//! The user-facing gap this closes: `/sessions` is the surface someone opens
//! to find their sessions, and it showed none of the work those sessions were
//! actually waiting on. The subagents existed on the sidebar, the working
//! strip, and the `Ctrl+T` overlay; the one list nobody thought to check was
//! the one you reach for first.
//!
//! Three things are load-bearing here, and each has bitten this codebase
//! before:
//!
//! 1. **Two instances.** `open_dialog` pushes a *clone* onto the focus
//!    manager and `render_dialog` paints that clone. A fill that touches only
//!    `dialog_state.session_dialog` produces a dialog that passes every
//!    App-owned assertion and shows nothing on screen. Every assertion below
//!    that cares about what the user sees reads the painted instance.
//! 2. **One derivation.** The sections are fed from `App::task_rows()`, so the
//!    overlay, the strip, and this dialog cannot disagree about which
//!    subagents or jobs are live.
//! 3. **Identity, not index.** A subagent that finishes mid-scroll shifts
//!    every row below it up by one; a cursor held as an index lands on a
//!    different entity and `Enter` opens the wrong tab.

use codegg::session::Session;
use codegg::tui::app::App;
use codegg::tui::components::component::DialogType;
use codegg::tui::components::dialogs::session::{SessionDialog, SessionDialogRow};
use codegg::tui::Dialog;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn test_app() -> App {
    App::new_for_testing("/tmp".into())
}

fn plain(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn enter() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
}

/// Replace the single session subscription with one carrying `subagents` and
/// `jobs`, then render once.
///
/// The subscription is torn down first on purpose: `install_subscription`
/// **rejects** a duplicate `stream_id` (returning `StreamMismatch` rather than
/// panicking), so re-seeding without an unsubscribe is silently ignored — a
/// test that re-seeds then asserts on new rows fails with "0 rows" and no
/// hint about why.
fn seed_projection(app: &mut App, subagents: &[(&str, &str, &str)], jobs: &[(&str, &str, &str)]) {
    let described: Vec<(&str, &str, &str, Option<&str>)> = subagents
        .iter()
        .map(|(task_id, name, status)| (*task_id, *name, *status, None))
        .collect();
    seed_projection_described(app, &described, jobs)
}

/// C005: the same seed, but each subagent may carry the pool descriptor its
/// node received.
///
/// Split out rather than folded into [`seed_projection`] so the C002 cases
/// keep seeding the pre-descriptor shape — they are pinning the fallback, and
/// quietly giving them a descriptor would move every one of them off the path
/// it was written for.
fn seed_projection_described(
    app: &mut App,
    subagents: &[(&str, &str, &str, Option<&str>)],
    jobs: &[(&str, &str, &str)],
) {
    use codegg::protocol::projection::dto::{
        AgentTreeNodeProjection, AgentTreeStatus, JobProjection, TurnStatus,
    };
    use codegg::protocol::projection::fixtures::active_turn_snapshot;
    use codegg::protocol::projection::replay::{
        ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
        ProjectionSubscriptionId,
    };

    // The fixture already carries a well-formed active turn; only the agent
    // tree and the jobs have to be added.
    let mut snapshot = active_turn_snapshot();
    if !subagents.is_empty() {
        let mut turn = snapshot
            .active_turn
            .expect("the fixture has an active turn");
        turn.status = TurnStatus::Active;
        for (task_id, name, status, profile) in subagents {
            let tree_status = match *status {
                "completed" => AgentTreeStatus::Completed,
                "failed" => AgentTreeStatus::Failed,
                _ => AgentTreeStatus::Running,
            };
            turn.agent_tree.push_back(AgentTreeNodeProjection {
                child_instance_id: String::new(),
                originating_turn_id: None,
                task_id: task_id.parse().expect("a numeric task id"),
                agent: "explore".into(),
                description: "map the subsystem".into(),
                status: tree_status,
                parent_task_id: None,
                created_at: 1,
                completed_at: None,
                result_summary: None,
                display_name: Some((*name).to_string()),
                profile: profile.map(str::to_string),
                usefulness: None,
                run_id: None,
            });
        }
        snapshot.active_turn = Some(turn);
    }
    for (job_id, kind, state) in jobs {
        snapshot.jobs.push_back(JobProjection {
            job_id: (*job_id).into(),
            workspace_id: snapshot.workspace_id.clone(),
            kind: (*kind).into(),
            state: (*state).into(),
            summary: "running the suite".into(),
            session_id: Some(snapshot.primary_session_id.clone()),
            turn_id: None,
            active_attempt_id: None,
            error_class: None,
            updated_at: 0,
        });
    }
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
    // One render pass. `ui_state.subagents.rows` is derived by
    // `sync_subagent_rows_from_projection` during rendering, and
    // `task_rows()` reads those rows — so a test that installs a snapshot and
    // immediately opens the dialog has no subagents to show. The real TUI
    // renders every frame, so this is the fixture matching the runtime rather
    // than a workaround for it.
    render(app);
}

fn render(app: &mut App) {
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

/// Read the dialog the user actually sees.
fn painted(app: &App) -> SessionDialog {
    app.focus_manager
        .with_dialog::<SessionDialog, _>(DialogType::Session, |dialog| dialog.clone())
        .expect("the session dialog is on the focus stack")
}

/// Press `j` until the cursor rests on `task_id`, asserting it gets there.
fn focus_subagent(app: &mut App, task_id: &str) {
    for _ in 0..12 {
        if matches!(
            painted(app).selected_row(),
            Some(SessionDialogRow::Subagent(ref r)) if r.task_id == task_id
        ) {
            return;
        }
        app.on_key(plain('j'));
    }
    panic!(
        "never reached subagent {task_id}; parked on {:?}",
        painted(app).selected_row()
    );
}

/// Open `/sessions` with live work already running.
fn open_with_live_work() -> App {
    let mut app = test_app();
    seed_projection(
        &mut app,
        &[("42", "Sally", "running")],
        &[("job-1", "test", "running")],
    );
    app.open_dialog(Dialog::Session);
    app
}

#[test]
fn the_painted_dialog_lists_the_live_subagents_and_jobs() {
    let app = open_with_live_work();
    let dialog = painted(&app);

    assert_eq!(
        dialog.subagent_rows().len(),
        1,
        "the painted instance shows no subagents; the user sees a bare list"
    );
    assert_eq!(dialog.subagent_rows()[0].label, "Sally");
    assert_eq!(dialog.subagent_rows()[0].task_id, "42");
    assert_eq!(
        dialog.job_rows().len(),
        1,
        "the painted instance shows no jobs"
    );
    assert_eq!(dialog.job_rows()[0].job_id, "job-1");
    assert_eq!(dialog.job_rows()[0].kind, "test");
}

#[test]
fn selecting_another_session_clears_rows_and_marks_its_work_unavailable() {
    let mut app = open_with_live_work();
    let active_session_id = codegg::protocol::projection::fixtures::FIXTURE_SESSION_ID.to_string();
    let sessions = vec![
        Session {
            id: active_session_id.clone(),
            title: "active".into(),
            ..Session::default()
        },
        Session {
            id: "other-session".into(),
            title: "other".into(),
            ..Session::default()
        },
    ];
    app.dialog_state
        .session_dialog
        .load_sessions(sessions.clone());
    assert!(app
        .dialog_state
        .session_dialog
        .select_session_id(&active_session_id));
    app.focus_manager
        .with_dialog_mut::<SessionDialog, _>(DialogType::Session, |dialog| {
            dialog.load_sessions(sessions.clone());
            dialog.select_session_id(&active_session_id);
        });
    app.refresh_sessions_dialog_pending_rows();
    assert_eq!(painted(&app).subagent_rows().len(), 1);

    assert!(app
        .dialog_state
        .session_dialog
        .select_session_id("other-session"));
    app.focus_manager
        .with_dialog_mut::<SessionDialog, _>(DialogType::Session, |dialog| {
            dialog.select_session_id("other-session");
        });
    app.refresh_sessions_dialog_pending_rows();
    let painted_other = painted(&app);
    assert!(painted_other.subagent_rows().is_empty());
    assert!(painted_other.job_rows().is_empty());
    let text = drawn_text(&mut app);
    assert!(text.contains("live work: unavailable"), "{text}");
}

/// The regression for the historical empty-dialog bug, restated for the new
/// sections. Asserting on `dialog_state.session_dialog` alone would pass with
/// the painted clone empty — which is exactly what used to happen.
#[test]
fn both_instances_receive_the_sections() {
    let mut app = test_app();
    seed_projection(
        &mut app,
        &[("42", "Sally", "running")],
        &[("job-1", "test", "running")],
    );
    app.open_dialog(Dialog::Session);

    assert_eq!(
        app.dialog_state.session_dialog.subagent_rows().len(),
        1,
        "App-owned dialog was not filled"
    );
    assert_eq!(
        painted(&app).subagent_rows().len(),
        1,
        "the painted focus-manager clone is empty; the user sees no subagents"
    );
    assert_eq!(
        painted(&app).job_rows().len(),
        1,
        "the painted focus-manager clone is empty; the user sees no jobs"
    );
}

#[test]
fn a_finished_subagent_and_a_finished_job_are_both_left_out() {
    let mut app = test_app();
    seed_projection(
        &mut app,
        &[("42", "Sally", "running"), ("43", "Frank", "completed")],
        &[
            ("job-1", "test", "running"),
            ("job-2", "build", "succeeded"),
        ],
    );
    app.open_dialog(Dialog::Session);

    let dialog = painted(&app);
    assert_eq!(
        dialog
            .subagent_rows()
            .iter()
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Sally"],
        "a finished subagent is no longer something the session waits on"
    );
    assert_eq!(
        dialog.job_rows().len(),
        1,
        "a succeeded job is no longer something the session waits on"
    );
}

#[test]
fn a_session_with_no_live_work_shows_neither_section() {
    let mut app = test_app();
    seed_projection(
        &mut app,
        &[("42", "Sally", "completed")],
        &[("job-1", "test", "succeeded")],
    );
    app.open_dialog(Dialog::Session);

    let dialog = painted(&app);
    assert!(dialog.subagent_rows().is_empty());
    assert!(dialog.job_rows().is_empty());
    assert!(
        !dialog
            .rows()
            .iter()
            .any(|row| matches!(row, SessionDialogRow::Header(_))),
        "an empty section must not leave a heading stranded over nothing"
    );
}

/// `Enter` on a subagent row opens the same transcript tab the sidebar and the
/// working strip open for the same `task_id` — asserted by comparing the two
/// routes rather than by checking one of them.
#[test]
fn enter_on_a_subagent_row_opens_the_same_tab_the_sidebar_opens() {
    let mut from_dialog = open_with_live_work();
    focus_subagent(&mut from_dialog, "42");
    from_dialog.on_key(enter());

    let mut from_sidebar = open_with_live_work();
    // The sidebar route for the same `task_id`, via the shared message.
    from_sidebar.process_msg(codegg::tui::app::TuiMsg::OpenSubagentTab {
        task_id: "42".to_string(),
    });

    let dialog_tab = from_dialog
        .ui_state
        .subagents
        .active_tab()
        .map(|t| t.tab_id.as_str().to_string());
    assert_eq!(
        dialog_tab,
        Some("42".to_string()),
        "Enter on a subagent row must open the tab it addresses"
    );
    assert_eq!(
        dialog_tab,
        from_sidebar
            .ui_state
            .subagents
            .active_tab()
            .map(|t| t.tab_id.as_str().to_string()),
        "the dialog and the sidebar opened different tabs for the same subagent"
    );
}

/// `Enter` on a job row does nothing at all: no destination exists to invent,
/// and the dialog must not close as if something had been selected.
#[test]
fn enter_on_a_job_row_changes_nothing() {
    let mut app = open_with_live_work();
    focus_subagent(&mut app, "42");
    app.on_key(plain('j')); // past the subagent and the Jobs heading

    let dialog = painted(&app);
    assert!(
        matches!(dialog.selected_row(), Some(SessionDialogRow::Job(_))),
        "precondition: the cursor is on the job row, got {:?}",
        dialog.selected_row()
    );

    app.on_key(enter());

    assert!(
        app.ui_state.subagents.active_tab().is_none(),
        "a job row opened a tab: there is no job view to open"
    );
    assert!(
        app.focus_manager
            .with_dialog::<SessionDialog, _>(DialogType::Session, |_| true)
            .is_some(),
        "Enter on a job row closed the dialog as if something had been selected"
    );
}

/// WP-4: a subagent that starts while `/sessions` is open appears without the
/// user closing and reopening.
#[test]
fn a_subagent_that_starts_while_the_dialog_is_open_appears() {
    let mut app = test_app();
    seed_projection(&mut app, &[], &[("job-1", "test", "running")]);
    app.open_dialog(Dialog::Session);

    assert!(
        painted(&app).subagent_rows().is_empty(),
        "precondition: no subagent exists yet"
    );

    seed_projection(
        &mut app,
        &[("42", "Sally", "running")],
        &[("job-1", "test", "running")],
    );
    // The render pass inside `seed_projection` is the WP-4 path: it syncs the
    // rows and then refreshes the mounted dialog.
    render(&mut app);

    assert_eq!(
        painted(&app).subagent_rows().len(),
        1,
        "a subagent that started after the dialog opened never reached the \
         painted instance"
    );
}

/// The refresh is a no-op when the dialog is not mounted, so an ordinary turn
/// does no work for a surface nobody is looking at.
#[test]
fn the_refresh_is_a_no_op_while_the_dialog_is_closed() {
    let mut app = test_app();
    seed_projection(&mut app, &[("42", "Sally", "running")], &[]);
    app.refresh_sessions_dialog_pending_rows();
    assert!(
        app.dialog_state.session_dialog.subagent_rows().is_empty(),
        "a closed dialog was filled; the refresh is not gated on the dialog"
    );
}

/// Both sections must actually be drawn, not merely held in the struct — the
/// historical class of defect here is data that arrives correctly and is never
/// painted.
#[test]
fn the_dialog_renders_both_sections() {
    let mut app = open_with_live_work();
    render(&mut app);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(text.contains("Subagents"), "no subagent heading was drawn");
    assert!(text.contains("Sally"), "no subagent name was drawn");
    assert!(text.contains("Jobs"), "no jobs heading was drawn");
    assert!(text.contains("test"), "no job row was drawn");
}

/// Render a frame and return every cell's symbol as one string.
fn drawn_text(app: &mut App) -> String {
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}

/// C005: `/sessions` shows the pool descriptor.
///
/// This is the surface the milestone is most likely to be forgotten on. C002
/// built the subagent section by deriving rows from `TaskRow::Subagent`; the
/// descriptor was added to that enum afterwards, and a section that renders
/// `description` unconditionally would keep passing every C002 test while the
/// user saw a task and never a personality.
#[test]
fn the_dialog_shows_the_pool_descriptor() {
    let mut app = test_app();
    seed_projection_described(
        &mut app,
        &[("42", "Sally", "running", Some("Reconnaissance generalist."))],
        &[],
    );
    app.open_dialog(Dialog::Session);
    render(&mut app);

    let dialog = painted(&app);
    let rows = dialog.subagent_rows();
    assert_eq!(rows.len(), 1, "the descriptor row never arrived");
    assert_eq!(
        rows[0].profile.as_deref(),
        Some("Reconnaissance generalist."),
        "the descriptor dropped between the snapshot and the dialog row"
    );

    let text = drawn_text(&mut app);
    assert!(
        text.contains("Reconnaissance generalist."),
        "the row holds the descriptor but the dialog never painted it"
    );
}

/// The exact fallback C002 relies on: a node recorded before descriptors
/// existed still shows its task, so this surface keeps working on old sessions.
#[test]
fn the_dialog_falls_back_to_the_task_without_a_descriptor() {
    let mut app = test_app();
    seed_projection(&mut app, &[("42", "Sally", "running")], &[]);
    app.open_dialog(Dialog::Session);
    render(&mut app);

    let dialog = painted(&app);
    let rows = dialog.subagent_rows();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].profile.is_none());

    let text = drawn_text(&mut app);
    assert!(
        text.contains("map the subsystem"),
        "a pre-descriptor node must still show its task: {text}"
    );
}
