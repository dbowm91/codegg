import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ActivityPanel } from './ActivityPanel';

describe('the desktop background-activity surface', () => {
  it('renders each background group as its own list', () => {
    render(
      <ActivityPanel
        agentRuns={[
          {
            runId: 'run-1',
            taskId: 'task-1',
            agent: 'build',
            status: 'running',
            depth: 0,
            parentRunId: null,
            worktreeId: 'wt-1',
            branch: 'feat/x',
            resultCommit: null,
            groupId: 'group-1',
            controlStatus: 'owned',
            attentionRequired: false,
            progress: 'step 2',
            terminalSummary: null,
            failureClass: null,
            updatedAt: 5,
          },
        ]}
        toolPrograms={[
          {
            programId: 'prog-1',
            jobId: 'job-1',
            state: 'running',
            phase: 'compile',
            language: 'rust',
            parentTurnId: 'turn-1',
            callsCompleted: 3,
            childJobsRunning: 1,
            submittedAt: 1,
            startedAt: 2,
            completedAt: null,
            failureClass: null,
            terminalHandle: null,
            lastProgress: 'linking',
          },
        ]}
        worktrees={[
          {
            worktreeId: 'wt-1',
            ownerRunId: 'run-1',
            branch: 'feat/x',
            state: 'ready',
            health: 'clean',
            dirty: true,
            conflicted: false,
            retainedForAttention: false,
            updatedAt: 5,
          },
        ]}
        runGroups={[
          {
            groupId: 'group-1',
            ownerRunId: 'run-1',
            ownerKind: 'agent',
            status: 'active',
            joinPolicy: 'any',
            memberCount: 2,
            successful: 0,
            failed: 0,
            active: 2,
            winnerRunId: null,
            cancelRemainingOnSatisfaction: false,
            updatedAt: 5,
          },
        ]}
        convergences={[
          {
            convergenceId: 'conv-1',
            ownerSummary: 'repair the build',
            status: 'verifying',
            cycleOrdinal: 1,
            maxCycles: 3,
            remainingCycles: 2,
            producerCompleted: 1,
            producerFailed: 0,
            producerCancelled: 0,
            producerActive: 0,
            verifierRunId: 'run-2',
            verdictKind: 'pass',
            verdictSummary: 'the build is green',
            awaitingDecision: false,
            terminalReasonClass: null,
            selectedRunId: null,
            lastFindingCount: 0,
          },
        ]}
      />,
    );
    expect(screen.getByTestId('agent-run-list')).toHaveTextContent('feat/x');
    expect(screen.getByTestId('tool-program-list')).toHaveTextContent('compile');
    expect(screen.getByTestId('worktree-list')).toHaveTextContent('ready');
    expect(screen.getByTestId('run-group-list')).toHaveTextContent('2 member(s)');
    expect(screen.getByTestId('convergence-list')).toHaveTextContent('the build is green');
  });

  it('says so when nothing is running in the background', () => {
    render(
      <ActivityPanel
        agentRuns={[]}
        toolPrograms={[]}
        worktrees={[]}
        runGroups={[]}
        convergences={[]}
      />,
    );
    expect(screen.getByTestId('activity-empty')).toHaveTextContent(
      'Nothing running in the background.',
    );
  });
});
