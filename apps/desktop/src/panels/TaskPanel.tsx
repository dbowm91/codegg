import type {
  JobSummaryView,
  RunSummaryView,
  SubagentSummaryView,
  TodoListView,
  TurnSummaryView,
} from '../bridge-types';
import { isLiveJobState, isRunningSubagent, subagentLabel, todoStatusGlyph } from './format';
import { RecentTurns } from './RecentTurns';
import { RunHistory } from './RunHistory';
import { snapshotAvailability, snapshotMessage } from './snapshotState';

// The task surface: the session's todo list, its live jobs, its in-flight
// subagents, its runs and its recent turns.
//
// This is the TUI's `Ctrl+T` task surface in DOM form. The TUI's overlay
// is built from three sources — todos, in-flight subagents, in-flight jobs
// (`src/tui/app/state/tasks.rs:173-176`) — and this panel carries the
// same three, plus the runs and the recent turns the daemon reports.
//
// The subagents are the projection's liveness authority rather than task
// state, and they have a surface of their own (`SubagentPanel`) that also
// lists the finished ones and the turn tree. The task surface lists only
// the ones still working, because that is what the overlay lists: a
// finished subagent is not work in flight.
//
// The runs and the recent turns are the projection's own history of the
// session's work (`apps/desktop/src/bridge-types.ts:298`, `:310`). The
// renderer used to drop both, so the panel carries them now: a session's
// past work is part of its task surface, and dropping it was the defect
// this milestone closes.
//
// The TUI's own rule for a todo row is the glyph plus the entry's
// content (`src/tui/components/sidebar.rs:1372-1387`), and the daemon's
// rule is that a malformed entry is skipped rather than fatal
// (`tests/tui_render/tui_presentation_regressions.inc:302`); the host
// applies both before the list reaches this panel
// (`apps/desktop/src-tauri/src/todos.rs`).
//
// A `null` list means the host has not read one yet — not that the
// session has no todos. The panel says so instead of drawing an empty
// list, because "no tasks" and "not known yet" are different claims. The
// same distinction holds for the runs and the recent turns: under a driver
// state that publishes a skeleton, an empty list is "not known yet"
// (`apps/desktop/src-tauri/src/present.rs:742-774`).

export interface TaskPanelProps {
  todos: TodoListView | null;
  /** The session's jobs; only the live ones are work in flight. */
  jobs?: JobSummaryView[];
  /** The session's child agents; only the running ones are work in flight. */
  subagents?: SubagentSummaryView[];
  /** The session's runs. */
  runs?: RunSummaryView[];
  /** The session's recent turns. */
  recentTurns?: TurnSummaryView[];
  /**
   * The projection's driver state. It separates "the list is empty" from
   * "the snapshot is not installed yet".
   */
  state?: string | null;
  /** Whether a read is in flight. */
  busy?: boolean;
  /** The last read's failure, if it failed. */
  error?: string | null;
  /** Re-read the list from the daemon. */
  onRefresh?: () => void;
}

export function TaskPanel({
  todos,
  jobs = [],
  subagents = [],
  runs = [],
  recentTurns = [],
  state,
  busy = false,
  error = null,
  onRefresh,
}: TaskPanelProps) {
  const liveJobs = jobs.filter((job) => isLiveJobState(job.state));
  const liveSubagents = subagents.filter((subagent) => isRunningSubagent(subagent.status));
  const availability = snapshotAvailability(state);
  const snapshotNote = snapshotMessage(availability);
  return (
    <section className="surface" data-testid="task-surface">
      <h3>
        Tasks
        {todos && (
          <small data-testid="task-count">{todos.items.length}</small>
        )}
      </h3>
      {onRefresh && (
        <button data-testid="task-refresh-button" disabled={busy} onClick={onRefresh}>
          {busy ? 'Refreshing…' : 'Refresh tasks'}
        </button>
      )}
      {error && (
        // A failed read is reported here rather than as a route error: it
        // is a task-surface condition, and the last list the host gave
        // us is still the truth.
        <p role="status" data-testid="task-error">
          Task read failed: {error}
        </p>
      )}
      {snapshotNote !== null && (
        <p
          className="muted"
          data-testid={
            availability === 'no-snapshot' ? 'task-no-snapshot' : 'task-no-session'
          }
        >
          {snapshotNote}
        </p>
      )}
      {todos === null ? (
        <p className="muted" data-testid="task-unknown">
          Task list not read yet.
        </p>
      ) : todos.items.length === 0 ? (
        <p className="muted" data-testid="task-empty">
          No tasks.
        </p>
      ) : (
        <ul data-testid="task-list">
          {todos.items.map((todo, index) => (
            <li
              key={todo.id || `${todo.content}-${index}`}
              data-testid="task-item"
              data-status={todo.status}
            >
              <span data-testid="task-glyph">{todoStatusGlyph(todo.status)}</span>
              <span data-testid="task-content">{todo.content}</span>
              <span data-testid="task-priority">{todo.priority}</span>
            </li>
          ))}
        </ul>
      )}
      {liveSubagents.length > 0 && (
        <>
          <h4>Active subagents</h4>
          <ul data-testid="task-subagent-list">
            {liveSubagents.map((subagent) => (
              <li key={subagent.taskId} data-testid="task-subagent-item" data-task-id={subagent.taskId}>
                <strong data-testid="task-subagent-label">
                  {subagentLabel(subagent.displayName, subagent.agent)}
                </strong>
                <span data-testid="task-subagent-status">{subagent.status}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      {liveJobs.length > 0 && (
        <>
          <h4>Live jobs</h4>
          <ul data-testid="task-job-list">
            {liveJobs.map((job) => (
              <li key={job.jobId} data-testid="task-job-item" data-job-id={job.jobId}>
                <strong data-testid="task-job-kind">{job.kind}</strong>
                <span data-testid="task-job-state">{job.state}</span>
                <span data-testid="task-job-summary">{job.summary}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      <RunHistory runs={runs} state={state} />
      <RecentTurns turns={recentTurns} state={state} />
    </section>
  );
}
