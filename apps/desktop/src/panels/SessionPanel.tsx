import type { ControllerSummaryView, TurnSummaryView } from '../bridge-types';

// The session's own identity: which session is on screen, what state the
// projection is in, which turn is running, and who holds the controller
// lease.
//
// Every field here arrived on the projection channel except the
// controller lease, which the renderer reads through the bridge's own
// control read (`apps/desktop/src/bridge.ts:38-41`). The panel renders
// what it is given and authorizes nothing.

export interface SessionPanelProps {
  sessionId: string;
  projectId: string;
  workspaceId: string;
  /** The projection's driver state. */
  state: string;
  /** The current (or most recent) turn. */
  turn: TurnSummaryView | null;
  /** The controller lease, if one is held. */
  controller: ControllerSummaryView | null;
}

export function SessionPanel({
  sessionId,
  projectId,
  workspaceId,
  state,
  turn,
  controller,
}: SessionPanelProps) {
  return (
    <section className="surface" data-testid="session-panel-surface">
      <h3>Session</h3>
      <dl>
        <dt>Session</dt>
        <dd data-testid="session-panel-session">{sessionId || '—'}</dd>
        <dt>Project</dt>
        <dd data-testid="session-panel-project">{projectId || '—'}</dd>
        <dt>Workspace</dt>
        <dd data-testid="session-panel-workspace">{workspaceId || '—'}</dd>
        <dt>State</dt>
        <dd data-testid="session-panel-state">{state}</dd>
        <dt>Turn</dt>
        <dd data-testid="session-panel-turn">
          {turn ? `${turn.turnId} / ${turn.status}` : 'idle'}
        </dd>
        <dt>Controller</dt>
        <dd data-testid="session-panel-controller">
          {controller ? `${controller.controllerPrincipal} @ rev ${controller.revision}` : 'no exclusive controller'}
        </dd>
      </dl>
    </section>
  );
}
