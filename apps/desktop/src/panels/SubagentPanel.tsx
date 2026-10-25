import type { SubagentSummaryView } from '../bridge-types';
import { isRunningSubagent, subagentLabel, subagentStatusGlyph } from './format';

// Subagents: the session's child agents, from the projection's liveness
// authority (`snapshot.subagents`) plus the active turn's agent tree.
//
// The TUI's rule is that a finished subagent is not work in flight: the
// task surface is a status view, not a history log
// (`src/tui/app/state/tasks.rs:189-192`, and
// `tests/tui_render/tui_main_buffer.inc:499`). The desktop keeps the
// same split — the running agents are the ones the session is waiting
// on, and the finished ones are listed separately rather than dropped,
// so the panel is a truthful view of both.

export interface SubagentPanelProps {
  /** The projection's liveness authority: the session's child agents. */
  subagents: SubagentSummaryView[];
  /** The active turn's per-turn agent tree. */
  agentTree: SubagentSummaryView[];
}

export function SubagentPanel({ subagents, agentTree }: SubagentPanelProps) {
  const running = subagents.filter((subagent) => isRunningSubagent(subagent.status));
  const finished = subagents.filter((subagent) => !isRunningSubagent(subagent.status));
  const treeOnly = agentTree.filter(
    (node) => !subagents.some((subagent) => subagent.taskId === node.taskId),
  );

  const row = (subagent: SubagentSummaryView, source: 'subagent' | 'tree') => (
    <li
      key={`${source}-${subagent.taskId}`}
      data-testid="subagent-item"
      data-task-id={subagent.taskId}
      data-source={source}
    >
      <span data-testid="subagent-glyph">{subagentStatusGlyph(subagent.status)}</span>
      <strong data-testid="subagent-label">
        {subagentLabel(subagent.displayName, subagent.agent)}
      </strong>
      <span data-testid="subagent-agent">{subagent.agent}</span>
      <span data-testid="subagent-status">{subagent.status}</span>
      {subagent.description && (
        <span data-testid="subagent-description">{subagent.description}</span>
      )}
      {subagent.resultSummary && (
        <span data-testid="subagent-result">{subagent.resultSummary}</span>
      )}
    </li>
  );

  return (
    <section className="surface" data-testid="subagent-surface">
      <h3>
        Subagents
        <small data-testid="subagent-count">{running.length} running</small>
      </h3>
      {running.length === 0 && finished.length === 0 && treeOnly.length === 0 ? (
        <p className="muted" data-testid="subagent-empty">
          No subagents.
        </p>
      ) : (
        <>
          {running.length > 0 && (
            <>
              <h4>Running</h4>
              <ul data-testid="subagent-running-list">{running.map((s) => row(s, 'subagent'))}</ul>
            </>
          )}
          {finished.length > 0 && (
            <>
              <h4>Finished</h4>
              <ul data-testid="subagent-finished-list">
                {finished.map((s) => row(s, 'subagent'))}
              </ul>
            </>
          )}
          {treeOnly.length > 0 && (
            <>
              <h4>Turn tree</h4>
              <ul data-testid="subagent-tree-list">{treeOnly.map((s) => row(s, 'tree'))}</ul>
            </>
          )}
        </>
      )}
    </section>
  );
}
