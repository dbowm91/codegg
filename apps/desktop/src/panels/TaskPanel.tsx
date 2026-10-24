import type { JobSummaryView, TodoListView } from '../bridge-types';
import { isLiveJobState, todoStatusGlyph } from './format';

// The task surface: the session's todo list and its live jobs.
//
// This is the TUI's `Ctrl+T` task surface in DOM form — "todos, live
// jobs, active subagents". The subagents half lives in its own surface
// (`SubagentPanel`), because the projection carries them as the session's
// liveness authority rather than as task state; the todos and the live
// jobs are the two halves that are task state.
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
// list, because "no tasks" and "not known yet" are different claims.

export interface TaskPanelProps {
  todos: TodoListView | null;
  /** The session's jobs; only the live ones are work in flight. */
  jobs?: JobSummaryView[];
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
  busy = false,
  error = null,
  onRefresh,
}: TaskPanelProps) {
  const liveJobs = jobs.filter((job) => isLiveJobState(job.state));
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
    </section>
  );
}
