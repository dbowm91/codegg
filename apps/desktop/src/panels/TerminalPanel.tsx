import { useCallback, useEffect, useRef } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent } from 'react';
import { ALL_ACTION_DECLARATIONS, TERMINAL_SCOPE, normalizeChord, resolveAction } from '../keymap';
import type { TerminalPanelProps } from '../terminal/contract';

// The terminal panel: the panel that docks the terminal.
//
// It renders the props the session gave it and holds **no authority**: it
// names no bridge target, imports no host module, and reaches no
// filesystem or process. Everything it does is either a rendering of a
// prop or a call to a callback it was handed — the session above it owns
// the terminal's state and every call to the host.
//
// The panel measures its own output surface and reports the cell size, and
// it resolves its own chords against the canonical keymap, exactly as the
// editor surface does.

/** The width of one cell, in pixels. The stylesheet sets the font to match. */
const CELL_WIDTH_PX = 8;

/** The height of one cell, in pixels. The stylesheet sets the line to match. */
const CELL_HEIGHT_PX = 17;

/** The terminal panel's own props. */
export interface TerminalPanelOwnProps {
  /** The terminal's state and intents. */
  terminal: TerminalPanelProps;
}

/** The terminal panel. */
export function TerminalPanel({ terminal }: TerminalPanelOwnProps) {
  const outputRef = useRef<HTMLDivElement | null>(null);

  const onKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      // The chord is resolved by the canonical keymap: the terminal
      // declares no chords of its own, and a chord the user remapped
      // reaches the terminal through the same table as every other action.
      const chord = normalizeChord(event);
      const actionId = resolveAction(terminal.effectiveMap, [TERMINAL_SCOPE], chord);
      if (actionId) {
        const declaration = ALL_ACTION_DECLARATIONS.find((entry) => entry.id === actionId);
        if (declaration?.captured) event.preventDefault();
        if (actionId === 'terminal.send') {
          terminal.onInput('\n');
          return;
        }
        if (actionId === 'terminal.backspace') {
          terminal.onInput('\u007f');
        }
        return;
      }
      // The printable insert is not a chord, so it is not an action: it is
      // the input surface's own affordance.
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.key.length !== 1) return;
      event.preventDefault();
      terminal.onInput(event.key);
    },
    [terminal],
  );

  // The measured cell size. The panel owns the measurement because it owns
  // the surface; the session owns the request.
  useEffect(() => {
    const element = outputRef.current;
    if (!element) return undefined;
    const measure = () => {
      const width = element.clientWidth;
      const height = element.clientHeight;
      if (width <= 0 || height <= 0) return;
      terminal.onResize(Math.floor(width / CELL_WIDTH_PX), Math.floor(height / CELL_HEIGHT_PX));
    };
    measure();
    if (typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(() => measure());
    observer.observe(element);
    return () => observer.disconnect();
  }, [terminal.onResize]);

  return (
    <section className="terminal-panel" data-testid="terminal-panel">
      <header className="terminal-toolbar" data-testid="terminal-toolbar">
        <span className="terminal-handle" data-testid="terminal-handle">
          {terminal.handle ?? 'no terminal'}
        </span>
        <select
          className="terminal-picker"
          data-testid="terminal-picker"
          value={terminal.handle ?? ''}
          onChange={(event) => {
            const wanted = event.target.value;
            if (!wanted) {
              terminal.onDetach();
              return;
            }
            terminal.onShow(wanted);
          }}
        >
          <option value="">no terminal</option>
          {terminal.processes.map((process) => (
            <option key={process.handle} value={process.handle}>
              {process.handle}
            </option>
          ))}
        </select>
        <button
          type="button"
          data-testid="terminal-detach-button"
          disabled={!terminal.handle}
          onClick={() => terminal.onDetach()}
        >
          detach
        </button>
        {terminal.busy ? (
          <span className="terminal-busy" data-testid="terminal-busy">
            working
          </span>
        ) : null}
      </header>

      <div className="terminal-output" data-testid="terminal-output" ref={outputRef}>
        {terminal.scrollback.length > 0 ? (
          <div className="terminal-scrollback" data-testid="terminal-scrollback">
            {terminal.scrollback.map((line, index) => (
              <div className="terminal-line" key={`scrollback-${index}`}>
                {line}
              </div>
            ))}
          </div>
        ) : null}
        {terminal.lines.map((line, index) => (
          <div className="terminal-line" data-testid="terminal-line" key={`line-${index}`}>
            {line.spans.map((span, spanIndex) => (
              <span className={span.className || undefined} key={`span-${spanIndex}`}>
                {span.text}
              </span>
            ))}
          </div>
        ))}
      </div>

      <div className="terminal-notices" data-testid="terminal-notices">
        {terminal.notice ? (
          <span className="terminal-notice" data-testid="terminal-notice">
            {terminal.notice}
          </span>
        ) : null}
        {terminal.unsupported ? (
          <span className="terminal-unsupported" data-testid="terminal-unsupported">
            {terminal.unsupported}
          </span>
        ) : null}
        {terminal.droppedLines > 0 ? (
          <span className="terminal-dropped" data-testid="terminal-dropped">
            {terminal.droppedLines} earlier lines were dropped by the scrollback bound
          </span>
        ) : null}
        {terminal.error ? (
          <span className="terminal-error" data-testid="terminal-error">
            {terminal.error}
          </span>
        ) : null}
      </div>

      <div
        className="terminal-input"
        data-testid="terminal-input"
        role="textbox"
        aria-label="Terminal input"
        tabIndex={0}
        onKeyDown={onKeyDown}
      />
    </section>
  );
}
