import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { RecentTurns } from './RecentTurns';
import type { TurnSummaryView } from '../bridge-types';

const turn = (overrides: Partial<TurnSummaryView> = {}): TurnSummaryView => ({
  turnId: 'turn-1',
  status: 'active',
  updatedAt: 7,
  stopReason: null,
  error: null,
  messageCount: 2,
  toolCount: 1,
  pendingPermissions: 0,
  pendingQuestions: 0,
  inputTokens: 10,
  outputTokens: 20,
  cachedTokens: 0,
  contextLimit: 200000,
  reasoningTokens: 0,
  ...overrides,
});

describe('the desktop recent-turn history', () => {
  it('shows the turn and the daemon’s own status token', () => {
    render(<RecentTurns state="attached" turns={[turn()]} />);
    expect(screen.getByTestId('recent-turn-id')).toHaveTextContent('turn-1');
    expect(screen.getByTestId('recent-turn-status')).toHaveTextContent('active');
  });

  it('summarises the turn from the counts the daemon reported', () => {
    render(<RecentTurns state="attached" turns={[turn()]} />);
    const summary = screen.getByTestId('recent-turn-summary');
    expect(summary).toHaveTextContent('2 message(s)');
    expect(summary).toHaveTextContent('1 tool(s)');
  });

  it('carries the stop reason the daemon reported', () => {
    render(<RecentTurns state="attached" turns={[turn({ stopReason: 'cancelled' })]} />);
    expect(screen.getByTestId('recent-turn-summary')).toHaveTextContent('cancelled');
  });

  it('says the snapshot is not installed yet instead of drawing an empty list', () => {
    render(<RecentTurns state="subscribing" turns={[]} />);
    expect(screen.getByTestId('recent-turns-no-snapshot')).toHaveTextContent(
      'No snapshot installed yet.',
    );
    expect(screen.queryByTestId('recent-turns-empty')).not.toBeInTheDocument();
  });

  it('says the session has no recent turns once a snapshot is installed', () => {
    render(<RecentTurns state="attached" turns={[]} />);
    expect(screen.getByTestId('recent-turns-empty')).toHaveTextContent('No recent turns.');
  });
});
