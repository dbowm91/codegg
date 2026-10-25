import type {
  AgentRunView,
  ConvergenceView,
  RunGroupView,
  ToolProgramView,
  WorktreeView,
} from '../bridge-types';

// Background activity: the work that is running beside the turn.
//
// The TUI gives each of these its own sidebar section
// (`src/tui/components/sidebar.rs:19-30`: ToolPrograms, AgentRuns,
// Convergences, plus the Jobs and Worktrees rows), all fed by the same
// projection snapshot. The desktop groups them into one surface
// because they answer one question — what is happening outside the
// transcript — but every group is rendered separately so a reader can
// tell an agent run from a tool program from a convergence.

export interface ActivityPanelProps {
  agentRuns: AgentRunView[];
  toolPrograms: ToolProgramView[];
  worktrees: WorktreeView[];
  runGroups: RunGroupView[];
  convergences: ConvergenceView[];
}

export function ActivityPanel({
  agentRuns,
  toolPrograms,
  worktrees,
  runGroups,
  convergences,
}: ActivityPanelProps) {
  const empty =
    agentRuns.length === 0 &&
    toolPrograms.length === 0 &&
    worktrees.length === 0 &&
    runGroups.length === 0 &&
    convergences.length === 0;

  return (
    <section className="surface" data-testid="activity-surface">
      <h3>Background activity</h3>
      {empty ? (
        <p className="muted" data-testid="activity-empty">
          Nothing running in the background.
        </p>
      ) : (
        <>
          {agentRuns.length > 0 && (
            <>
              <h4>Agent runs</h4>
              <ul data-testid="agent-run-list">
                {agentRuns.map((run) => (
                  <li key={run.runId} data-testid="agent-run-item" data-run-id={run.runId}>
                    <strong data-testid="agent-run-agent">{run.agent}</strong>
                    <span data-testid="agent-run-status">{run.status}</span>
                    {run.branch && <span data-testid="agent-run-branch">{run.branch}</span>}
                    {run.groupId && <span data-testid="agent-run-group">{run.groupId}</span>}
                    {run.progress && <span data-testid="agent-run-progress">{run.progress}</span>}
                    {run.terminalSummary && (
                      <span data-testid="agent-run-summary">{run.terminalSummary}</span>
                    )}
                  </li>
                ))}
              </ul>
            </>
          )}
          {toolPrograms.length > 0 && (
            <>
              <h4>Tool programs</h4>
              <ul data-testid="tool-program-list">
                {toolPrograms.map((program) => (
                  <li
                    key={program.programId}
                    data-testid="tool-program-item"
                    data-program-id={program.programId}
                  >
                    <strong data-testid="tool-program-state">{program.state}</strong>
                    {program.phase && <span data-testid="tool-program-phase">{program.phase}</span>}
                    {program.language && (
                      <span data-testid="tool-program-language">{program.language}</span>
                    )}
                    <span data-testid="tool-program-calls">
                      {program.callsCompleted} call(s)
                    </span>
                    {program.lastProgress && (
                      <span data-testid="tool-program-progress">{program.lastProgress}</span>
                    )}
                  </li>
                ))}
              </ul>
            </>
          )}
          {worktrees.length > 0 && (
            <>
              <h4>Worktrees</h4>
              <ul data-testid="worktree-list">
                {worktrees.map((worktree) => (
                  <li
                    key={worktree.worktreeId}
                    data-testid="worktree-item"
                    data-worktree-id={worktree.worktreeId}
                  >
                    <strong data-testid="worktree-branch">{worktree.branch}</strong>
                    <span data-testid="worktree-state">{worktree.state}</span>
                    {worktree.health && (
                      <span data-testid="worktree-health">{worktree.health}</span>
                    )}
                    {worktree.dirty && <span data-testid="worktree-dirty">dirty</span>}
                    {worktree.conflicted && (
                      <span data-testid="worktree-conflicted">conflicted</span>
                    )}
                  </li>
                ))}
              </ul>
            </>
          )}
          {runGroups.length > 0 && (
            <>
              <h4>Run groups</h4>
              <ul data-testid="run-group-list">
                {runGroups.map((group) => (
                  <li key={group.groupId} data-testid="run-group-item" data-group-id={group.groupId}>
                    <strong data-testid="run-group-status">{group.status}</strong>
                    <span data-testid="run-group-owner-kind">{group.ownerKind}</span>
                    <span data-testid="run-group-members">{group.memberCount} member(s)</span>
                    {group.winnerRunId && (
                      <span data-testid="run-group-winner">{group.winnerRunId}</span>
                    )}
                  </li>
                ))}
              </ul>
            </>
          )}
          {convergences.length > 0 && (
            <>
              <h4>Convergences</h4>
              <ul data-testid="convergence-list">
                {convergences.map((convergence) => (
                  <li
                    key={convergence.convergenceId}
                    data-testid="convergence-item"
                    data-convergence-id={convergence.convergenceId}
                  >
                    <strong data-testid="convergence-status">{convergence.status}</strong>
                    <span data-testid="convergence-owner">{convergence.ownerSummary}</span>
                    <span data-testid="convergence-cycle">
                      cycle {convergence.cycleOrdinal}/{convergence.maxCycles}
                    </span>
                    {convergence.verdictSummary && (
                      <span data-testid="convergence-verdict">{convergence.verdictSummary}</span>
                    )}
                  </li>
                ))}
              </ul>
            </>
          )}
        </>
      )}
    </section>
  );
}
