import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { TranscriptPanel, groupTranscriptByTurn } from './TranscriptPanel';
import type { TranscriptMessageView } from '../bridge-types';

const message = (
  turnId: string,
  index: number,
  text: string,
  truncated = false,
): TranscriptMessageView => ({
  messageId: `${turnId}-m${index}`,
  turnId,
  role: index % 2 === 0 ? 'user' : 'assistant',
  text,
  truncated,
  createdAt: index,
});

describe('the desktop transcript surface', () => {
  it('groups consecutive messages into the turns the host sent', () => {
    const turns = groupTranscriptByTurn([
      message('t1', 0, 'first'),
      message('t1', 1, 'second'),
      message('t2', 0, 'third'),
    ]);
    expect(turns.map((turn) => turn.turnId)).toEqual(['t1', 't2']);
    expect(turns[0].messages.map((entry) => entry.text)).toEqual(['first', 'second']);
    expect(turns[1].messages.map((entry) => entry.text)).toEqual(['third']);
  });

  it('renders the transcript oldest first with one header per turn', () => {
    render(
      <TranscriptPanel
        messages={[
          message('t1', 0, 'ask the thing'),
          message('t1', 1, 'the answer'),
          message('t2', 0, 'a later turn'),
        ]}
        truncatedMessages={0}
      />,
    );
    const rendered = screen.getAllByTestId('transcript-text').map((node) => node.textContent);
    expect(rendered).toEqual(['ask the thing', 'the answer', 'a later turn']);
    const headers = screen.getAllByTestId('transcript-turn').map((node) => node.textContent);
    expect(headers).toEqual(['Turn t1', 'Turn t2']);
  });

  it('marks a message the host truncated', () => {
    render(
      <TranscriptPanel
        messages={[message('t1', 0, 'a clipped answer', true)]}
        truncatedMessages={0}
      />,
    );
    expect(screen.getByTestId('transcript-truncated-message')).toHaveTextContent('truncated');
    expect(screen.getByTestId('transcript-text')).toHaveTextContent('…');
  });

  it('reports how many older messages the host omitted', () => {
    render(<TranscriptPanel messages={[]} truncatedMessages={7} />);
    expect(screen.getByTestId('transcript-truncated')).toHaveTextContent('7 older message(s) omitted');
    expect(screen.getByTestId('transcript-empty')).toBeInTheDocument();
  });

  it('virtualises the transcript instead of mounting every message', () => {
    const many = Array.from({ length: 400 }, (_, index) =>
      message('t1', index, `message ${index}`),
    );
    render(<TranscriptPanel messages={many} truncatedMessages={0} />);
    const rows = screen.getAllByTestId('transcript-row');
    // The window is bounded by the viewport, not by the message count:
    // a 400-message session must not mount 400 rows.
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(many.length);
    expect(screen.getByTestId('transcript-window')).toHaveTextContent(
      `${rows.length} of 400 message(s) rendered`,
    );
  });
});
