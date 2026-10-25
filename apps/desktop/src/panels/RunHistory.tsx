import type { RunSummaryView } from '../bridge-types';
import { snapshotAvailability, snapshotMessage } from './snapshotState';

// The session's runs: the projection's `runs` list.
//
// The daemon derives it with an explicit bound (`MAX_VISIBLE_RUNS = 10`,
// `apps/desktop/src-tauri/src/present.rs:54-63`), so this panel renders
// exactly what arrives and adds no truncation marker of its own.
//
// Every row shows the daemon's own status token and summary — the two
// fields the presenter derived from the run — so the renderer never
// interprets a run's state.

export interface RunHistoryProps {
  /** The projection's runs. */
  runs: RunSummaryView[];
  /**
   * The projection's driver state. It decides between "no runs" and "the
   * snapshot is not installed yet", which are different claims.
   */
  state?: string | null;
}

export function RunHistory({ runs, state }: RunHistoryProps) {
  const availability = snapshotAvailability(state);
  const message = snapshotMessage(availability);

  return (
    <div className="panel-section" data-testid="run-history">
      <h4>
        Runs
        {availability === 'present' && <small data-testid="run-history-count">{runs.length}</small>}
      </h4>
      {message !== null ? (
        <p
          className="muted"
          data-testid={
            availability === 'no-snapshot' ? 'run-history-no-snapshot' : 'run-history-no-session'
          }
        >
          {message}
        </p>
      ) : runs.length === 0 ? (
        <p className="muted" data-testid="run-history-empty">
          No runs.
        </p>
      ) : (
        <ul data-testid="run-history-list">
          {runs.map((run) => (
            <li key={run.runId} data-testid="run-history-item" data-run-id={run.runId}>
              <strong data-testid="run-history-kind">{run.kind}</strong>
              <span data-testid="run-history-status">{run.status}</span>
              <span data-testid="run-history-summary">{run.summary}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
