import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { StatusRow } from './StatusRow';
import type { SessionPresentationView } from '../bridge-types';

const projection = (overrides: Partial<SessionPresentationView> = {}): SessionPresentationView => ({
  sessionId: 's-1',
  projectId: 'p0',
  workspaceId: 'w0',
  state: 'attached',
  turn: {
    turnId: 'turn-1',
    status: 'active',
    updatedAt: 7,
    stopReason: null,
    error: null,
    messageCount: 1,
    toolCount: 0,
    pendingPermissions: 0,
    pendingQuestions: 0,
    inputTokens: 10,
    outputTokens: 20,
    cachedTokens: 0,
    contextLimit: 200000,
    reasoningTokens: 0,
  },
  messages: [],
  truncatedMessages: 0,
  transcript: [],
  truncatedTranscriptMessages: 0,
  tools: [],
  runs: [],
  jobs: [],
  subagents: [],
  agentTree: [],
  agentRuns: [],
  toolPrograms: [],
  worktrees: [],
  runGroups: [],
  convergences: [],
  recentTurns: [],
  todos: null,
  pendingPermissions: [],
  pendingQuestions: [],
  controller: null,
  artifactHandles: [],
  cursor: { eventSeq: 42, driverCursorSeq: 42, subscriptionKnown: true },
  resyncReason: null,
  ...overrides,
});

describe('the desktop status row', () => {
  it('states the projection, the turn and the cursor', () => {
    render(<StatusRow projection={projection()} controller={null} onRefreshControl={vi.fn()} />);
    expect(screen.getByTestId('projection-state')).toHaveTextContent('attached');
    expect(screen.getByTestId('turn-status')).toHaveTextContent('turn-1 / active');
    expect(screen.getByTestId('projection-cursor')).toHaveTextContent('seq 42');
  });

  it('states who holds the controller lease', () => {
    render(
      <StatusRow
        projection={projection()}
        controller={{ turnId: 'turn-1', controllerPrincipal: 'alice', revision: 4 }}
        onRefreshControl={vi.fn()}
      />,
    );
    expect(screen.getByTestId('controller-state')).toHaveTextContent('alice @ rev 4');
  });

  it('says there is no exclusive controller when nobody holds one', () => {
    render(<StatusRow projection={projection()} controller={null} onRefreshControl={vi.fn()} />);
    expect(screen.getByTestId('controller-state')).toHaveTextContent('no exclusive controller');
  });

  it('raises the daemon’s resync banner', () => {
    render(
      <StatusRow
        projection={projection({ state: 'resyncing', resyncReason: 'gap' })}
        controller={null}
        onRefreshControl={vi.fn()}
      />,
    );
    expect(screen.getByTestId('resync-banner')).toHaveTextContent('Resuming projection');
  });

  it('asks the renderer to re-read the controller lease', () => {
    const onRefreshControl = vi.fn();
    render(<StatusRow projection={projection()} controller={null} onRefreshControl={onRefreshControl} />);
    fireEvent.click(screen.getByTestId('control-refresh-button'));
    expect(onRefreshControl).toHaveBeenCalledTimes(1);
  });

  it('says no session is attached when the renderer holds no projection', () => {
    render(<StatusRow projection={null} controller={null} onRefreshControl={vi.fn()} />);
    expect(screen.getByTestId('status-empty')).toHaveTextContent('No session attached.');
    expect(screen.queryByTestId('projection-state')).not.toBeInTheDocument();
  });
});
