import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { SubagentPanel } from './SubagentPanel';
import type { SubagentSummaryView } from '../bridge-types';

const subagent = (
  taskId: number,
  status: string,
  overrides: Partial<SubagentSummaryView> = {},
): SubagentSummaryView => ({
  taskId,
  agent: 'explore',
  description: 'Map the auth flow',
  status,
  resultSummary: null,
  childInstanceId: '',
  parentTaskId: null,
  originatingTurnId: 'turn-1',
  displayName: null,
  runId: null,
  ...overrides,
});

describe('the desktop subagent surface', () => {
  it('lists a running subagent and keeps a finished one out of the running group', () => {
    render(
      <SubagentPanel
        subagents={[
          subagent(1, 'running'),
          subagent(2, 'completed', { agent: 'build', description: 'Warm the cargo cache' }),
        ]}
        agentTree={[]}
      />,
    );
    const running = screen.getByTestId('subagent-running-list');
    expect(running).toHaveTextContent('Map the auth flow');
    // A finished subagent is not work in flight.
    expect(running).not.toHaveTextContent('Warm the cargo cache');
    expect(screen.getByTestId('subagent-finished-list')).toHaveTextContent('Warm the cargo cache');
    expect(screen.getByTestId('subagent-count')).toHaveTextContent('1 running');
  });

  it('names a subagent by its curated display name', () => {
    render(
      <SubagentPanel
        subagents={[subagent(1, 'running', { displayName: 'Sally' })]}
        agentTree={[]}
      />,
    );
    expect(screen.getByTestId('subagent-label')).toHaveTextContent('Sally');
    expect(screen.getByTestId('subagent-agent')).toHaveTextContent('explore');
  });

  it('shows the turn tree nodes the liveness authority does not carry', () => {
    render(
      <SubagentPanel
        subagents={[subagent(1, 'running')]}
        agentTree={[subagent(9, 'running', { description: 'A legacy tree node' })]}
      />,
    );
    expect(screen.getByTestId('subagent-tree-list')).toHaveTextContent('A legacy tree node');
  });

  it('says so when the session has no subagents', () => {
    render(<SubagentPanel subagents={[]} agentTree={[]} />);
    expect(screen.getByTestId('subagent-empty')).toHaveTextContent('No subagents.');
  });
});
