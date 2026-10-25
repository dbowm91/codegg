import type { ControllerSummaryView, SessionPresentationView } from '../bridge-types';

// The status row: the context strip beneath the composer.
//
// It carries the four facts the renderer always needs to have in front of
// it — the projection's state, the turn that is running, the cursor it
// has reached, and who holds the controller lease — plus the banner the
// daemon raises while it is resyncing.
//
// Nothing here is a decision. Every value arrived on the projection
// channel or through the bridge's own control read; the row states them
// and authorizes nothing.

export interface StatusRowProps {
  /** The session's presentation view, or null when nothing is attached. */
  projection: SessionPresentationView | null;
  /** The controller lease the renderer last read. */
  controller: ControllerSummaryView | null;
  /** Re-read the controller lease. */
  onRefreshControl: () => void;
}

export function StatusRow({ projection, controller, onRefreshControl }: StatusRowProps) {
  if (!projection) {
    return (
      <section className="status-row" data-testid="status-row">
        <p className="muted" data-testid="status-empty">
          No session attached.
        </p>
      </section>
    );
  }

  return (
    <section className="status-row" data-testid="status-row">
      <dl>
        <dt>Projection</dt>
        <dd data-testid="projection-state">{projection.state}</dd>
        <dt>Turn</dt>
        <dd data-testid="turn-status">
          {projection.turn ? `${projection.turn.turnId} / ${projection.turn.status}` : 'idle'}
        </dd>
        <dt>Cursor</dt>
        <dd data-testid="projection-cursor">seq {projection.cursor.eventSeq}</dd>
        <dt>Controller</dt>
        <dd data-testid="controller-state">
          {controller
            ? `${controller.controllerPrincipal} @ rev ${controller.revision}`
            : 'no exclusive controller'}
        </dd>
      </dl>
      <button data-testid="control-refresh-button" onClick={onRefreshControl}>
        Refresh control
      </button>
      {projection.state === 'resyncing' && (
        <p data-testid="resync-banner" role="status">
          Resuming projection — live updates paused.
        </p>
      )}
    </section>
  );
}
