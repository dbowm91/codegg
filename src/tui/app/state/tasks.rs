//! Rows for the unified tasks overlay (`Shift+T`).
//!
//! The overlay answers one question — *what is this session waiting on?* —
//! across three kinds of work that used to need three different places to
//! look: the todo list (`Ctrl+T`), the main buffer's subagent strip, and the
//! job list that previously had no surface at all. Grouping them behind one
//! key is the whole point; splitting them again per group would defeat it.
//!
//! # Two rules shape this module
//!
//! - **Projection-derived, never synthesized.** Subagents come from
//!   [`crate::tui::app::state::subagents::SubagentRow`] and jobs come from
//!   `snapshot.jobs`, both of which are already authoritative. This module
//!   only *selects and formats*; it never invents a row, never mints a name,
//!   and never claims a status the projection did not report.
//! - **Empty groups are omitted.** A heading with nothing under it is noise
//!   that costs a row and teaches the user to ignore headings. A group appears
//!   only when it has at least one row, so the overlay can be three rows or
//!   thirty without a single placeholder.
//!
//! # Why subagent and job groups are filtered but todos are not
//!
//! Todos persist for the life of a session: a completed item is still part of
//! the plan, and hiding it would make the progress count lie. Subagents and
//! jobs are *waited on*, and a finished one is no longer being waited on —
//! listing them would fill the overlay with history the user did not ask for.
//! So those two groups filter to live states only. See [`is_live_state`].

use crate::tui::app::state::subagents::SubagentRow;

/// Rows a *single* group may contribute to the overlay.
///
/// A per-group ceiling, so one busy kind — twenty queued jobs, say — cannot
/// crowd the others out entirely. The overlay's real budget is
/// [`MAX_TASKS_TOTAL_ROWS`], which is what actually keeps the transcript
/// above its floor.
pub const MAX_TASKS_STRIP_ROWS: usize = 6;

/// Rows the whole overlay may contribute, headers included.
///
/// This is the number that matters. The overlay is carved out of the prompt
/// allocation, so an unbounded block does not merely get tall — it starves
/// the carve until the overlay is skipped *entirely* and the key appears to
/// do nothing. That is the failure mode a per-group cap alone does not
/// prevent: three capped groups still add up to more than a short terminal
/// has rows.
///
/// The value is deliberately small. The overlay is a status surface, not a
/// work queue; when the lists outgrow it, the group headers report the true
/// totals and the user knows there is more than fits.
pub const MAX_TASKS_TOTAL_ROWS: usize = 12;

/// One rendered row of the overlay.
///
/// Deliberately flat: `Group` is carried on the row rather than expressed as
/// nesting so the renderer is a straight `for` loop and the height is simply
/// the row count. A nested shape would buy nothing here — there is no
/// per-group collapsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskRow {
    /// A group separator, e.g. `── Subagents ──`.
    Header { group: TaskGroup },
    /// A todo entry.
    Todo {
        /// Completion bubble glyph, from `App::todo_bubble`.
        bubble: &'static str,
        /// The todo's content text.
        content: String,
    },
    /// An in-flight subagent.
    Subagent {
        /// The projection's canonical task identity.
        ///
        /// Carried rather than re-derived so the overlay can address the
        /// subject when a row is activated — a tab, or (from M004) a sessions
        /// panel section — without the caller having to match the rendered
        /// label back to a row. A label is not an identity: two live
        /// subagents can display the same name, and `display_name` is optional
        /// so it can fall back to the agent type, which is *never* unique.
        task_id: String,
        /// Display name when the projection carried one, else the agent type.
        label: String,
        /// The agent type, shown alongside the name so a display name is not
        /// mistaken for the whole identity.
        agent: String,
        /// What the subagent was asked to do.
        description: String,
        /// Raw status from the projection.
        status: String,
    },
    /// An in-flight job.
    Job {
        /// Job kind, e.g. `test` or `build`.
        kind: String,
        /// Bounded human summary of the job.
        summary: String,
        /// Raw state from the projection.
        state: String,
        /// Present when the job failed; drives the error styling.
        error_class: Option<String>,
    },
}

impl TaskRow {
    /// The group this row belongs to.
    pub fn group(&self) -> TaskGroup {
        match self {
            TaskRow::Header { group } => *group,
            TaskRow::Todo { .. } => TaskGroup::Todos,
            TaskRow::Subagent { .. } => TaskGroup::Subagents,
            TaskRow::Job { .. } => TaskGroup::Jobs,
        }
    }
}

/// The three groups, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskGroup {
    Todos,
    Subagents,
    Jobs,
}

impl TaskGroup {
    /// Header text. Each names its own contents so the reader never has to
    /// infer what a block is — this is why a *scheduled* run is never called a
    /// "task" here: `/tasks` owns that word for schedules, and reusing it
    /// beside a live subagent is how the two get confused.
    pub fn title(self) -> &'static str {
        match self {
            TaskGroup::Todos => "Todo list",
            TaskGroup::Subagents => "Subagents",
            TaskGroup::Jobs => "Jobs",
        }
    }
}

/// Build the overlay rows from the three sources.
///
/// Order is fixed — todos, then subagents, then jobs — so the block does not
/// reshuffle between renders as counts change. A stable order matters more
/// than an alphabetical one here: the user's eye returns to a position.
pub fn build_task_rows(
    todos: &[TodoLike],
    subagents: &[SubagentRow],
    jobs: &[JobLike],
) -> Vec<TaskRow> {
    let mut rows = Vec::new();

    if !todos.is_empty() {
        rows.push(TaskRow::Header {
            group: TaskGroup::Todos,
        });
        rows.extend(todos.iter().map(|todo| TaskRow::Todo {
            bubble: todo.bubble,
            content: todo.content.clone(),
        }));
    }

    // Only live subagents: a finished one is no longer something the session
    // is waiting on, and the overlay is a status view, not a history log.
    let live_subagents: Vec<&SubagentRow> =
        subagents.iter().filter(|row| row.is_running()).collect();
    if !live_subagents.is_empty() {
        rows.push(TaskRow::Header {
            group: TaskGroup::Subagents,
        });
        rows.extend(live_subagents.into_iter().map(|row| TaskRow::Subagent {
            task_id: row.task_id.clone(),
            label: row.label().to_string(),
            agent: row.agent.clone(),
            description: row.description.clone(),
            status: row.status.clone(),
        }));
    }

    let live_jobs: Vec<&JobLike> = jobs
        .iter()
        .filter(|job| is_live_state(&job.state))
        .collect();
    if !live_jobs.is_empty() {
        rows.push(TaskRow::Header {
            group: TaskGroup::Jobs,
        });
        rows.extend(live_jobs.into_iter().map(|job| TaskRow::Job {
            kind: job.kind.clone(),
            summary: job.summary.clone(),
            state: job.state.clone(),
            error_class: job.error_class.clone(),
        }));
    }

    rows
}

/// Whether a job state means "still being waited on".
///
/// Matches the same tolerant token set as `SubagentRow::is_running`. The two
/// rules differ deliberately:
///
/// - A **known terminal** state (`succeeded`, `failed`, `cancelled`,
///   `completed`) means finished, and is filtered out.
/// - An **empty** state means the producer never reported one. Treated as
///   live, because an unknown state must not read as "finished" — that would
///   hide work that is genuinely running.
/// - An **unrecognised non-empty** state is a producer that invented a
///   vocabulary this build has not learned. It is treated as live for the same
///   reason: showing a job that turns out to be done costs the user a glance;
///   hiding one that is still running costs them the fact they asked for. The
///   asymmetry is deliberate and is what `SubagentRow::is_running` already
///   does for the subagent group.
pub fn is_live_state(state: &str) -> bool {
    let state = state.trim().to_ascii_lowercase();
    if state.is_empty() {
        return true;
    }
    if TERMINAL_STATES
        .iter()
        .any(|terminal| state.contains(terminal))
    {
        return false;
    }
    true
}

/// Job states that mean the job is no longer being waited on.
///
/// Checked before falling through to "assume live", so adding a new terminal
/// state is a one-line change here rather than a behavioural edit to the
/// matcher above.
const TERMINAL_STATES: &[&str] = &[
    "succeed",
    "success",
    "complete",
    "fail",
    "error",
    "cancel",
    "abort",
    "expired",
    "timed_out",
    "timeout",
];

/// Minimal shape the overlay needs from a todo, so this module does not depend
/// on the todo type's own layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoLike {
    pub bubble: &'static str,
    pub content: String,
}

/// Minimal shape the overlay needs from a job projection.
///
/// Kept separate from `JobProjection` so this module stays testable without
/// constructing a full projection, and so adding a field to `JobProjection`
/// does not silently change what the overlay shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobLike {
    pub kind: String,
    pub summary: String,
    pub state: String,
    pub error_class: Option<String>,
}

/// Cap the overlay to [`MAX_TASKS_TOTAL_ROWS`] rows **including headers**.
///
/// Every header is kept, even when the body budget is spent: a truncated list
/// whose group headings silently vanish leaves the reader with unattributed
/// rows and no way to tell what they are looking at. Headers are what make a
/// partial list honest, so they are reserved before any body row is
/// considered and never compete with it for budget.
///
/// Allocation is **fair-share with redistribution**, not first-come-first-
/// served. Filling groups in display order until the budget runs out means a
/// long todo list consumes the whole allocation and the jobs group - last in
/// order - silently loses rows it actually had room for. Instead every group
/// is granted an equal share up front, and whatever a small group does not
/// need is handed back to the groups that still want it. A group is reduced
/// only when the overlay genuinely has more to show than it has rows.
///
/// [`MAX_TASKS_STRIP_ROWS`] remains a per-group ceiling, so one busy kind
/// cannot dominate even when it is the only group with work left.
pub fn bounded(rows: Vec<TaskRow>) -> Vec<TaskRow> {
    // Partition into (header, body) pairs, preserving display order.
    let mut groups: Vec<(TaskRow, Vec<TaskRow>)> = Vec::new();
    for row in rows {
        match row {
            TaskRow::Header { .. } => groups.push((row, Vec::new())),
            body => {
                // A body row with no preceding header is a builder bug, not a
                // runtime condition. Dropping it keeps the output well-formed
                // instead of rendering an unattributed line.
                if let Some((_, existing)) = groups.last_mut() {
                    existing.push(body);
                }
            }
        }
    }

    let body_budget = MAX_TASKS_TOTAL_ROWS.saturating_sub(groups.len());
    let fair_share = if groups.is_empty() {
        0
    } else {
        body_budget / groups.len()
    };

    let mut allowance: Vec<usize> = groups
        .iter()
        .map(|(_, body)| body.len().min(fair_share).min(MAX_TASKS_STRIP_ROWS))
        .collect();

    // Hand each group's unused share back to the groups that still want it,
    // one row at a time, until the budget is spent or everyone is satisfied.
    let mut spent: usize = allowance.iter().sum();
    let mut progressed = true;
    while spent < body_budget && progressed {
        progressed = false;
        for (index, (_, body)) in groups.iter().enumerate() {
            if spent >= body_budget {
                break;
            }
            if allowance[index] < body.len() && allowance[index] < MAX_TASKS_STRIP_ROWS {
                allowance[index] += 1;
                spent += 1;
                progressed = true;
            }
        }
    }

    let mut out: Vec<TaskRow> = Vec::with_capacity(MAX_TASKS_TOTAL_ROWS);
    for ((header, body), take) in groups.into_iter().zip(allowance) {
        out.push(header);
        out.extend(body.into_iter().take(take));
    }
    out
}

/// The group headers present in `rows`, in order.
///
/// Exposed so a caller can report which groups rendered without walking the
/// rows itself, and so a test can assert "no empty heading appeared" as a
/// single value.
pub fn headers_of(rows: &[TaskRow]) -> Vec<TaskGroup> {
    rows.iter()
        .filter_map(|row| match row {
            TaskRow::Header { group } => Some(*group),
            _ => None,
        })
        .collect()
}

/// The indices of the rows a selection cursor can land on.
///
/// Headers are excluded: `── Subagents ──` is a label, not something to open,
/// and letting the cursor rest on one would make `Enter` a no-op the user has
/// to learn to step over. Indices refer to the *bounded* row list the overlay
/// actually renders, so a selection cannot address a row that truncation
/// removed.
pub fn selectable_indices(rows: &[TaskRow]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| !matches!(row, TaskRow::Header { .. }))
        .map(|(index, _)| index)
        .collect()
}

/// What activating `row` would open, if anything.
///
/// Only a subagent row has a target today: it resolves to a tab in the main
/// buffer. Todos are inline text with no separate view, and a job row's
/// durable detail has no surface yet — so those rows are *selectable* (the
/// cursor may rest on them and show which line is selected) but not
/// activatable, rather than being skipped and making navigation jump.
pub fn activatable_target(row: &TaskRow) -> Option<&str> {
    match row {
        TaskRow::Subagent { task_id, .. } => Some(task_id.as_str()),
        TaskRow::Header { .. } | TaskRow::Todo { .. } | TaskRow::Job { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo(content: &str) -> TodoLike {
        TodoLike {
            bubble: "○",
            content: content.into(),
        }
    }

    fn subagent(agent: &str, status: &str) -> SubagentRow {
        SubagentRow {
            task_id: "1".into(),
            run_id: None,
            display_name: Some("Sally".into()),
            agent: agent.into(),
            description: "work".into(),
            status: status.into(),
        }
    }

    fn job(kind: &str, state: &str) -> JobLike {
        JobLike {
            kind: kind.into(),
            summary: "build the thing".into(),
            state: state.into(),
            error_class: None,
        }
    }

    fn headers(rows: &[TaskRow]) -> Vec<TaskGroup> {
        headers_of(rows)
    }

    #[test]
    fn nothing_running_renders_todos_only_with_no_empty_headings() {
        let rows = build_task_rows(&[todo("write the thing")], &[], &[]);
        assert_eq!(headers(&rows), vec![TaskGroup::Todos]);
        assert_eq!(rows.len(), 2, "one header plus one todo: {rows:?}");
    }

    #[test]
    fn a_running_subagent_and_a_queued_job_both_appear() {
        let rows = build_task_rows(
            &[],
            &[subagent("explore", "running")],
            &[job("test", "queued")],
        );
        assert_eq!(headers(&rows), vec![TaskGroup::Subagents, TaskGroup::Jobs]);
        assert!(rows.iter().any(|r| matches!(
            r,
            TaskRow::Subagent { label, .. } if label == "Sally"
        )));
        assert!(rows.iter().any(|r| matches!(
            r,
            TaskRow::Job { kind, .. } if kind == "test"
        )));
    }

    #[test]
    fn a_subagent_row_carries_the_identity_a_tab_is_keyed_on() {
        let mut row = subagent("explore", "running");
        row.task_id = "42".into();
        let rows = build_task_rows(&[], &[row], &[]);
        let body = rows
            .iter()
            .find(|r| matches!(r, TaskRow::Subagent { .. }))
            .expect("a subagent row");
        assert_eq!(activatable_target(body), Some("42"));
    }

    #[test]
    fn only_subagent_rows_have_something_to_open() {
        let rows = build_task_rows(
            &[todo("write the thing")],
            &[subagent("explore", "running")],
            &[job("test", "queued")],
        );
        // Rows are [header, todo, header, subagent, header, job].
        // Selectable: todos and jobs are still rows the cursor may rest on,
        // so navigation does not jump over them.
        assert_eq!(selectable_indices(&rows), vec![1, 3, 5]);
        // Activatable: only the subagent resolves to something today.
        assert_eq!(activatable_target(&rows[0]), None, "header");
        assert_eq!(activatable_target(&rows[1]), None, "todo");
        assert!(activatable_target(&rows[3]).is_some(), "subagent");
        assert_eq!(activatable_target(&rows[4]), None, "header");
        assert_eq!(activatable_target(&rows[5]), None, "job");
    }

    #[test]
    fn group_order_is_todos_subagents_jobs_regardless_of_source_order() {
        let rows = build_task_rows(
            &[todo("a")],
            &[subagent("explore", "running")],
            &[job("build", "pending")],
        );
        assert_eq!(
            headers(&rows),
            vec![TaskGroup::Todos, TaskGroup::Subagents, TaskGroup::Jobs]
        );
    }

    #[test]
    fn empty_inputs_render_nothing_at_all() {
        assert!(build_task_rows(&[], &[], &[]).is_empty());
    }

    #[test]
    fn finished_subagents_and_jobs_are_omitted() {
        let rows = build_task_rows(
            &[],
            &[
                subagent("explore", "completed"),
                subagent("build", "failed"),
                subagent("review", "running"),
            ],
            &[
                job("test", "succeeded"),
                job("build", "failed"),
                job("lint", "running"),
            ],
        );
        // Only the one live subagent and the one live job.
        let subs = rows
            .iter()
            .filter(|r| matches!(r, TaskRow::Subagent { .. }))
            .count();
        let jobs = rows
            .iter()
            .filter(|r| matches!(r, TaskRow::Job { .. }))
            .count();
        assert_eq!(subs, 1);
        assert_eq!(jobs, 1);
    }

    #[test]
    fn live_state_matching_tolerates_producer_spelling() {
        for state in [
            "running",
            "Running",
            "in_progress",
            "in-progress",
            "PENDING",
            "queued",
            "scheduled",
            "retrying",
        ] {
            assert!(is_live_state(state), "{state} should count as live");
        }
    }

    #[test]
    fn terminal_states_are_filtered_out() {
        for state in [
            "succeeded",
            "Succeeded",
            "success",
            "completed",
            "failed",
            "cancelled",
            "expired",
            "timed_out",
        ] {
            assert!(!is_live_state(state), "{state} should not count as live");
        }
    }

    #[test]
    fn an_unknown_job_state_reads_as_live_rather_than_finished() {
        // Dropping real work because a producer invented a state name would
        // be worse than showing a job that turned out to be done. The
        // asymmetry is deliberate: hiding a running job costs the user the
        // fact they asked for, showing a finished one costs them a glance.
        assert!(is_live_state(""));
        assert!(is_live_state("weird-new-state"));
        assert!(is_live_state("quantum-superposition"));
    }

    #[test]
    fn subagent_label_falls_back_to_agent_type_when_unnamed() {
        let mut row = subagent("explore", "running");
        row.display_name = None;
        let rows = build_task_rows(&[], &[row], &[]);
        assert!(rows.iter().any(|r| matches!(
            r,
            TaskRow::Subagent {
                label, agent, ..
            } if label == "explore" && agent == "explore"
        )));
    }

    #[test]
    fn bounding_respects_the_global_budget_however_many_groups_are_full() {
        // Three saturated groups is the case a per-group cap alone gets
        // wrong: each group stays under its own ceiling and the overlay still
        // outgrows a short terminal.
        let todos: Vec<TodoLike> = (0..40).map(|i| todo(&format!("t{i}"))).collect();
        let subagents: Vec<SubagentRow> = (0..40)
            .map(|i| subagent(&format!("a{i}"), "running"))
            .collect();
        let jobs: Vec<JobLike> = (0..40).map(|i| job(&format!("j{i}"), "running")).collect();

        let rows = bounded(build_task_rows(&todos, &subagents, &jobs));
        assert!(
            rows.len() <= MAX_TASKS_TOTAL_ROWS,
            "overlay grew to {} rows, budget is {MAX_TASKS_TOTAL_ROWS}",
            rows.len()
        );
        // Headers survive the truncation so no row is left unattributed.
        assert_eq!(
            headers(&rows),
            vec![TaskGroup::Todos, TaskGroup::Subagents, TaskGroup::Jobs],
            "every group stays attributed even when its body is cut"
        );
    }

    #[test]
    fn one_saturated_group_cannot_starve_the_others() {
        // Forty todos and two of everything else. Without a per-group ceiling
        // the todo group takes the whole budget and the other two render as
        // bare headings.
        let todos: Vec<TodoLike> = (0..40).map(|i| todo(&format!("t{i}"))).collect();
        let subagents: Vec<SubagentRow> = (0..2)
            .map(|i| subagent(&format!("a{i}"), "running"))
            .collect();
        let jobs: Vec<JobLike> = (0..2).map(|i| job(&format!("j{i}"), "running")).collect();

        let rows = bounded(build_task_rows(&todos, &subagents, &jobs));
        let kept = |pred: fn(&TaskRow) -> bool| rows.iter().filter(|r| pred(r)).count();
        assert!(
            kept(|r| matches!(r, TaskRow::Todo { .. })) <= MAX_TASKS_STRIP_ROWS,
            "the todo group exceeded its own ceiling"
        );
        assert_eq!(
            kept(|r| matches!(r, TaskRow::Subagent { .. })),
            2,
            "a saturated todo group must not starve the subagents"
        );
        assert_eq!(
            kept(|r| matches!(r, TaskRow::Job { .. })),
            2,
            "a saturated todo group must not starve the jobs"
        );
    }

    #[test]
    fn a_small_overlay_is_passed_through_untouched() {
        let rows = build_task_rows(
            &[todo("a"), todo("b")],
            &[subagent("explore", "running")],
            &[job("test", "queued")],
        );
        assert_eq!(
            bounded(rows.clone()),
            rows,
            "a short overlay is not truncated"
        );
    }

    #[test]
    fn bounding_keeps_every_header() {
        let rows = bounded(build_task_rows(
            &[todo("a")],
            &[subagent("explore", "running")],
            &[job("test", "queued")],
        ));
        assert_eq!(
            headers(&rows),
            vec![TaskGroup::Todos, TaskGroup::Subagents, TaskGroup::Jobs]
        );
    }
}
