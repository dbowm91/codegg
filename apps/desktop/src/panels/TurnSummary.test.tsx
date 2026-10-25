import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { TurnSummary } from './TurnSummary';
import type { TurnSummaryView } from '../bridge-types';

const turn = (overrides: Partial<TurnSummaryView> = {}): TurnSummaryView => ({
  turnId: 'turn-1',
  status: 'active',
  updatedAt: 7,
  stopReason: null,
  error: null,
  messageCount: 1,
  toolCount: 0,
  pendingPermissions: 0,
  pendingQuestions: 0,
  inputTokens: 100,
  outputTokens: 20,
  cachedTokens: 5,
  contextLimit: 200000,
  reasoningTokens: 42,
  ...overrides,
});

describe('the desktop turn-summary surface', () => {
  it('shows every token counter the host carries', () => {
    render(<TurnSummary turn={turn()} />);
    expect(screen.getByTestId('turn-input-tokens')).toHaveTextContent('100');
    expect(screen.getByTestId('turn-output-tokens')).toHaveTextContent('20');
    expect(screen.getByTestId('turn-cached-tokens')).toHaveTextContent('5');
    expect(screen.getByTestId('turn-reasoning-tokens')).toHaveTextContent('42');
    expect(screen.getByTestId('turn-context-limit')).toHaveTextContent('200000');
  });

  it('shows an unresolved counter as unknown rather than as zero', () => {
    render(<TurnSummary turn={turn({ reasoningTokens: null, contextLimit: null })} />);
    expect(screen.getByTestId('turn-reasoning-tokens')).toHaveTextContent('—');
    expect(screen.getByTestId('turn-context-limit')).toHaveTextContent('—');
  });

  it('says so when there is no turn yet', () => {
    render(<TurnSummary turn={null} />);
    expect(screen.getByTestId('turn-summary-empty')).toHaveTextContent('No turn yet.');
  });
});
