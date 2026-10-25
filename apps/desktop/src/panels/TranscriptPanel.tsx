import { useMemo, useRef } from 'react';
import { observeElementRect, useVirtualizer } from '@tanstack/react-virtual';
import type { TranscriptMessageView } from '../bridge-types';

// The multi-turn transcript, virtualised.
//
// The TUI's transcript is a scrolling buffer of turn-grouped messages
// (`src/tui/components/messages.rs`); the desktop renders the same
// content, but the DOM must stay bounded when a session carries a long
// history — the host caps the transcript at 256 messages
// (`apps/desktop/src-tauri/src/present.rs:69`), and a 256-row DOM with
// full message bodies is a real cost on the least capable renderer.
//
// The virtualiser is `@tanstack/react-virtual` (a real dependency, not a
// hand-rolled window): it owns the scroll window and the row cache.
//
// Two adapters keep it honest off-screen:
//
// - the scroll container measures 0x0 when it has not been laid out (a
//   collapsed panel, or a test DOM), which would virtualise the
//   transcript down to nothing, so a zero height falls back to a
//   nominal viewport;
// - a row measures 0 for the same reason, which would collapse it, so
//   a zero measurement falls back to the row estimate.

/** The viewport assumed for a scroll container that has not been laid
 * out. Only used to keep the window non-empty; a laid-out container
 * always measures its own size. */
const FALLBACK_VIEWPORT = { width: 720, height: 480 };

/** The height assumed for a transcript row that has not been laid out. */
const ROW_ESTIMATE = 96;

export interface TranscriptPanelProps {
  messages: TranscriptMessageView[];
  /** How many older messages the host dropped from the head. */
  truncatedMessages: number;
}

/** One turn's worth of transcript rows, in the order the host sent them
 * (oldest first). */
export interface TranscriptTurn {
  turnId: string;
  messages: TranscriptMessageView[];
}

/**
 * Group the transcript into consecutive turns.
 *
 * The host sends the transcript oldest-first with every message tagged
 * with its turn (`apps/desktop/src-tauri/src/present.rs:426-438`), so
 * the grouping is a single pass over consecutive equal turn ids. A turn
 * id that reappears later is a separate group: the host's ordering is
 * the only ordering claim, and inventing a merge would hide the fact
 * that the transcript is not contiguous.
 */
export function groupTranscriptByTurn(
  messages: TranscriptMessageView[],
): TranscriptTurn[] {
  const turns: TranscriptTurn[] = [];
  for (const message of messages) {
    const last = turns[turns.length - 1];
    if (last && last.turnId === message.turnId) {
      last.messages.push(message);
    } else {
      turns.push({ turnId: message.turnId, messages: [message] });
    }
  }
  return turns;
}

export function TranscriptPanel({ messages, truncatedMessages }: TranscriptPanelProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const turns = useMemo(() => groupTranscriptByTurn(messages), [messages]);
  // The index of the first message of each turn. The transcript is
  // grouped by consecutive turn ids, so the header rides on the first
  // message of its group rather than being a row of its own.
  const turnStarts = useMemo(() => {
    const starts = new Set<number>();
    let index = 0;
    for (const turn of turns) {
      starts.add(index);
      index += turn.messages.length;
    }
    return starts;
  }, [turns]);

  // One virtual row per *message*, so a long message is one row and a
  // turn header is not a row of its own: the header repeats inside the
  // first message of each turn.
  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_ESTIMATE,
    overscan: 8,
    observeElementRect: (instance, cb) =>
      observeElementRect(instance, (rect) => {
        cb(rect.height > 0 ? rect : FALLBACK_VIEWPORT);
      }),
    measureElement: (element, _entry, instance) => {
      const measured = (element as HTMLElement).offsetHeight;
      if (measured > 0) return measured;
      return instance.options.estimateSize(instance.indexFromElement(element));
    },
  });

  const virtualRows = virtualizer.getVirtualItems();

  return (
    <section className="surface" data-testid="transcript-surface">
      <h3>
        Transcript
        {truncatedMessages > 0 && (
          <small data-testid="transcript-truncated">{truncatedMessages} older message(s) omitted</small>
        )}
      </h3>
      {messages.length === 0 ? (
        <p className="muted" data-testid="transcript-empty">
          No transcript yet.
        </p>
      ) : (
        <div className="transcript-scroll" data-testid="transcript-list" ref={scrollRef}>
          <div className="transcript-canvas" style={{ height: virtualizer.getTotalSize() }}>
            {virtualRows.map((row) => {
              const message = messages[row.index];
              const firstOfTurn = turnStarts.has(row.index);
              return (
                <div
                  key={row.key}
                  className="transcript-row"
                  data-testid="transcript-row"
                  data-turn-id={message.turnId}
                  ref={virtualizer.measureElement}
                  style={{ transform: `translateY(${row.start}px)` }}
                >
                  {firstOfTurn && (
                    <p className="transcript-turn" data-testid="transcript-turn">
                      Turn {message.turnId}
                    </p>
                  )}
                  <article className="transcript-message" data-testid="transcript-message">
                    <strong data-testid="transcript-role">{message.role}</strong>
                    <p data-testid="transcript-text">
                      {message.text}
                      {message.truncated && ' …'}
                    </p>
                    {message.truncated && (
                      <small data-testid="transcript-truncated-message">truncated</small>
                    )}
                  </article>
                </div>
              );
            })}
          </div>
        </div>
      )}
      <small className="muted" data-testid="transcript-window">
        {virtualRows.length} of {messages.length} message(s) rendered
      </small>
    </section>
  );
}
