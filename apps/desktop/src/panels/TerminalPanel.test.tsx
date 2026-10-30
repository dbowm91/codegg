import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { bridge } from '../bridge';
import { TerminalPanel } from './TerminalPanel';
import { emptyTerminalPanelProps } from '../terminal/contract';
import type { TerminalPanelProps } from '../terminal/contract';

// The bridge is mocked so that a panel that grew an authority of its own
// would call one of these and the test would say so.
vi.mock('../bridge', () => ({
  bridge: {
    terminalList: vi.fn(),
    terminalAttach: vi.fn(),
    terminalResume: vi.fn(),
    terminalInput: vi.fn(),
    terminalResize: vi.fn(),
    terminalDetach: vi.fn(),
  },
}));

const props = (overrides: Partial<TerminalPanelProps> = {}): TerminalPanelProps => ({
  ...emptyTerminalPanelProps(),
  ...overrides,
});

describe('the terminal panel', () => {
  it('renders the props the session gave it', () => {
    render(
      <TerminalPanel
        terminal={props({
          handle: 'h-1',
          lines: [{ text: 'hello', spans: [{ text: 'hello', className: '' }] }],
          droppedLines: 3,
          notice: 'the terminal output was resynchronized',
          error: 'the daemon refused the read',
        })}
      />,
    );
    expect(screen.getByTestId('terminal-handle')).toHaveTextContent('h-1');
    expect(screen.getByTestId('terminal-line')).toHaveTextContent('hello');
    expect(screen.getByTestId('terminal-dropped')).toHaveTextContent(
      '3 earlier lines were dropped',
    );
    expect(screen.getByTestId('terminal-notice')).toHaveTextContent('resynchronized');
    expect(screen.getByTestId('terminal-error')).toHaveTextContent('the daemon refused the read');
  });

  it('forwards a printable key to the session and calls no host function', () => {
    const onInput = vi.fn();
    const onDetach = vi.fn();
    render(
      <TerminalPanel terminal={props({ handle: 'h-1', onInput, onDetach })} />,
    );
    fireEvent.keyDown(screen.getByTestId('terminal-input'), { key: 'a' });
    fireEvent.click(screen.getByTestId('terminal-detach-button'));
    // The panel calls the callbacks it was given, and nothing else.
    expect(onInput).toHaveBeenCalledWith('a');
    expect(onDetach).toHaveBeenCalledTimes(1);
    expect(bridge.terminalList).not.toHaveBeenCalled();
    expect(bridge.terminalAttach).not.toHaveBeenCalled();
    expect(bridge.terminalResume).not.toHaveBeenCalled();
    expect(bridge.terminalInput).not.toHaveBeenCalled();
    expect(bridge.terminalResize).not.toHaveBeenCalled();
    expect(bridge.terminalDetach).not.toHaveBeenCalled();
  });

  it('sends the newline the terminal scope binds', () => {
    const onInput = vi.fn();
    render(<TerminalPanel terminal={props({ handle: 'h-1', onInput })} />);
    fireEvent.keyDown(screen.getByTestId('terminal-input'), { key: 'Enter' });
    expect(onInput).toHaveBeenCalledWith('\n');
  });
});
