import type { TurnSummaryView } from '../bridge-types';
import { snapshotAvailability, snapshotMessage } from './snapshotState';

// The session's recent turns: the projection's `recentTurns` list.
//
// The daemon bounds it (`MAX_RECENT_TURNS = 5`,
// `apps/desktop/src-tauri/src/present.rs:54-63`) and reports no truncation
// count for it, so this panel renders exactly what arrives and marks
// nothing of its own.
//
// The row's status is the daemon's own token. The summary is built from
// the same fields the presenter derived — the turn's message and tool
// counts, its stop reason and its error — so the renderer states facts it
// was given rather than a verdict of its own.

export interface RecentTurnsProps {
  /** The projection's recent turns, oldest first. */
  turns: TurnSummaryView[];
  /**
   * The projection's driver state. It decides between "no recent turns"
   * and "the snapshot is not installed yet", which are different claims.
   */
  state?: string | null;
}

function turnSummary(turn: TurnSummaryView): string {
  const parts = [`${turn.messageCount} message(s)`, `${turn.toolCount} tool(s)`];
  if (turn.stopReason) parts.push(turn.stopReason);
  if (turn.error) parts.push(turn.error);
  return parts.join(' · ');
}

export function RecentTurns({ turns, state }: RecentTurnsProps) {
  const availability = snapshotAvailability(state);
  const message = snapshotMessage(availability);

  return (
    <div className="panel-section" data-testid="recent-turns">
      <h4>
        Recent turns
        {availability === 'present' && <small data-testid="recent-turn-count">{turns.length}</small>}
      </h4>
      {message !== null ? (
        <p
          className="muted"
          data-testid={
            availability === 'no-snapshot' ? 'recent-turns-no-snapshot' : 'recent-turns-no-session'
          }
        >
          {message}
        </p>
      ) : turns.length === 0 ? (
        <p className="muted" data-testid="recent-turns-empty">
          No recent turns.
        </p>
      ) : (
        <ul data-testid="recent-turn-list">
          {turns.map((turn) => (
            <li key={turn.turnId} data-testid="recent-turn-item" data-turn-id={turn.turnId}>
              <strong data-testid="recent-turn-id">{turn.turnId}</strong>
              <span data-testid="recent-turn-status">{turn.status}</span>
              <span data-testid="recent-turn-summary">{turnSummary(turn)}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
