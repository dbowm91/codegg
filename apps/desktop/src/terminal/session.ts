import { useCallback, useEffect, useRef, useState } from 'react';
import { bridge } from '../bridge';
import type { EffectiveMap } from '../keymap';
import type {
  TerminalChunkView,
  TerminalPanelProps,
  TerminalProcessView,
} from './contract';
import {
  DEFAULT_TERMINAL_COLS,
  DEFAULT_TERMINAL_ROWS,
  TerminalGrid,
  utf8Length,
} from './vt';
import type { TerminalCursor, TerminalLine } from './vt';

// The terminal session.
//
// The terminal's state lives **here**, above the panel: the panel renders
// props and calls nothing, and the session owns every call to the host.
// The panel layer therefore holds no bridge authority — the same rule the
// editor surface follows.
//
// Three properties are load-bearing:
//
//   - The resume cadence is a **single** interval, and it runs only while
//     an attachment is live. It is cleared on detach, on a route change and
//     on unmount, so a detached terminal leaves no timer behind.
//   - Input is **queued and flushed in order**, one bounded write per tick,
//     and a queue past the daemon's own cap is refused locally and said
//     out loud rather than silently truncated.
//   - A resync is **reported**. A chunk with a gap, or an attach that
//     answers with a typed reason, resets the grid and says so: the view is
//     never presented as a continuous stream when it is not one.

/** The resume cadence, in milliseconds. */
export const TERMINAL_RESUME_INTERVAL_MS = 100;

/** The daemon's own cap on one input write, in bytes. */
export const TERMINAL_MAX_INPUT_BYTES = 32 * 1024;

/** The resync reasons the daemon types, in words. */
const RESYNC_MESSAGES: Readonly<Record<string, string>> = {
  HistoryExpired:
    'the terminal output was resynchronized: the requested position is older than the retained history',
  CursorAhead:
    'the terminal output was resynchronized: the requested position was ahead of the process output',
  HandleGone: 'the terminal is gone: the daemon no longer knows that handle',
  VersionMismatch:
    'the terminal output was resynchronized: the daemon negotiated another protocol version',
  default: 'the terminal output was resynchronized',
};

/** The notice a chunk with a gap earns. */
const GAP_NOTICE =
  'the terminal output was resynchronized: the requested position is older than the retained history';

/** The notices the unsupported sequence classes earn, in words. */
const UNSUPPORTED_MESSAGES: Readonly<Record<string, string>> = {
  osc: 'the terminal did not draw an operating-system command',
  'private-mode': 'the terminal did not draw a private mode, such as the alternate buffer',
  'scroll-region': 'the terminal did not draw a scroll region',
  'unknown-csi': 'the terminal did not draw a control sequence',
  'unknown-escape': 'the terminal did not draw an escape sequence',
};

/** The options the composition root gives the session. */
export interface TerminalSessionOptions {
  /** The route generation the host stamped on this route, or null. */
  routeGeneration: number | null;
  /** The connection generation, which changes when the client reconnects. */
  connectionGeneration: number | null;
  /** Whether the terminal may read the workspace's processes at all. */
  enabled: boolean;
  /** The effective keymap the panel resolves its chords against. */
  effectiveMap: EffectiveMap;
}

/** Encode text as base64, the transport's own encoding for terminal bytes. */
function toBase64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

/** Decode base64 back into the bytes a process produced. */
function decodeBase64(data: string): Uint8Array {
  if (!data) return new Uint8Array(0);
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

/** The message a failed call carries. */
function failureMessage(caught: unknown): string {
  if (caught instanceof Error && caught.message) return caught.message;
  if (typeof caught === 'string' && caught) return caught;
  return 'the terminal call failed';
}

/** The terminal session. */
export function useTerminalSession(options: TerminalSessionOptions): TerminalPanelProps {
  const { routeGeneration, connectionGeneration, enabled, effectiveMap } = options;

  const [processes, setProcesses] = useState<TerminalProcessView[]>([]);
  const [handle, setHandle] = useState<string | null>(null);
  const [attachmentId, setAttachmentId] = useState<string | null>(null);
  const [lines, setLines] = useState<TerminalLine[]>([]);
  const [scrollback, setScrollback] = useState<string[]>([]);
  const [droppedLines, setDroppedLines] = useState(0);
  const [cursor, setCursor] = useState<TerminalCursor>({ row: 0, col: 0 });
  const [cols, setCols] = useState(DEFAULT_TERMINAL_COLS);
  const [rows, setRows] = useState(DEFAULT_TERMINAL_ROWS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [resyncReason, setResyncReason] = useState<string | null>(null);
  const [unsupported, setUnsupported] = useState<string | null>(null);

  const gridRef = useRef<TerminalGrid>(new TerminalGrid(DEFAULT_TERMINAL_COLS, DEFAULT_TERMINAL_ROWS));
  const nextSeqRef = useRef(0);
  const queueRef = useRef<string[]>([]);
  const queuedBytesRef = useRef(0);
  const attachmentRef = useRef<string | null>(null);
  const shownHandleRef = useRef<string | null>(null);
  const processesRef = useRef<TerminalProcessView[]>([]);
  const lastSizeRef = useRef<{ cols: number; rows: number } | null>(null);
  const routeGenerationRef = useRef(routeGeneration);
  routeGenerationRef.current = routeGeneration;

  processesRef.current = processes;

  /** Copy the mutable grid into the state the panel renders. */
  const publish = useCallback(() => {
    const grid = gridRef.current;
    setLines(grid.styledLines());
    setScrollback(grid.scrollback());
    setDroppedLines(grid.droppedLines());
    setCursor(grid.cursor());
    const seen = grid.lastUnsupported();
    setUnsupported(seen ? (UNSUPPORTED_MESSAGES[seen] ?? UNSUPPORTED_MESSAGES['unknown-csi']) : null);
  }, []);

  /** Apply one chunk: a gap or a typed reason resets the view and says so. */
  const consumeChunk = useCallback(
    (chunk: TerminalChunkView, reason: string | null) => {
      const grid = gridRef.current;
      if (chunk.gap || reason !== null) {
        grid.reset();
        setNotice(reason !== null ? (RESYNC_MESSAGES[reason] ?? RESYNC_MESSAGES.default) : GAP_NOTICE);
        setResyncReason(reason);
      }
      const bytes = decodeBase64(chunk.dataB64);
      if (bytes.length > 0) grid.write(bytes);
      nextSeqRef.current = chunk.nextSeq;
      publish();
    },
    [publish],
  );

  /** Attach to a named handle. */
  const attach = useCallback(
    async (target: string) => {
      const generation = routeGenerationRef.current;
      if (generation === null) {
        setError('the desktop is not connected to a daemon workspace');
        return;
      }
      setBusy(true);
      setError(null);
      try {
        const view = await bridge.terminalAttach(target, generation);
        if (view.resyncReason === 'HandleGone') {
          // The daemon no longer knows the handle. Report it plainly: an
          // empty terminal must not be presented as a live one.
          shownHandleRef.current = null;
          attachmentRef.current = null;
          setAttachmentId(null);
          setHandle(null);
          setResyncReason(view.resyncReason);
          setNotice(RESYNC_MESSAGES.HandleGone);
          gridRef.current.reset();
          publish();
          return;
        }
        shownHandleRef.current = view.handle;
        attachmentRef.current = view.attachmentId;
        setAttachmentId(view.attachmentId);
        setHandle(view.handle);
        lastSizeRef.current = null;
        consumeChunk(view.chunk, view.resyncReason);
      } catch (caught) {
        setError(failureMessage(caught));
      } finally {
        setBusy(false);
      }
    },
    [consumeChunk, publish],
  );

  /** Release the attachment. The process itself keeps running. */
  const releaseAttachment = useCallback(() => {
    const attachment = attachmentRef.current;
    const generation = routeGenerationRef.current;
    attachmentRef.current = null;
    setAttachmentId(null);
    if (attachment !== null && generation !== null) {
      void bridge.terminalDetach(attachment, generation).catch(() => undefined);
    }
  }, []);

  /** The detach the panel's button asks for. */
  const detach = useCallback(() => {
    releaseAttachment();
    shownHandleRef.current = null;
    setHandle(null);
    setNotice(null);
    setResyncReason(null);
    setError(null);
    queueRef.current = [];
    queuedBytesRef.current = 0;
    nextSeqRef.current = 0;
    lastSizeRef.current = null;
    gridRef.current.reset();
    publish();
  }, [publish, releaseAttachment]);

  /** Show a named handle: the desktop's answer to the terminal action. */
  const show = useCallback(
    (target: string) => {
      const wanted = target.trim();
      if (!wanted) {
        setError('the terminal action needs a handle');
        return;
      }
      const known = processesRef.current.some((process) => process.handle === wanted);
      if (!known) {
        setError(`the daemon published no terminal with the handle ${wanted}`);
        return;
      }
      void attach(wanted);
    },
    [attach],
  );

  /** Queue the user's input. The queue is bounded by the daemon's own cap. */
  const queueInput = useCallback((text: string) => {
    if (!text) return;
    const bytes = utf8Length(text);
    if (queuedBytesRef.current + bytes > TERMINAL_MAX_INPUT_BYTES) {
      setError(
        `the terminal input queue is full: the daemon accepts at most ${TERMINAL_MAX_INPUT_BYTES} bytes at a time`,
      );
      return;
    }
    queueRef.current.push(text);
    queuedBytesRef.current += bytes;
  }, []);

  /** Flush one queued write, in order. */
  const flushInput = useCallback(async (attachment: string, generation: number) => {
    const next = queueRef.current.shift();
    if (next === undefined) return;
    queuedBytesRef.current -= utf8Length(next);
    try {
      await bridge.terminalInput(attachment, toBase64(next), generation);
    } catch (caught) {
      setError(failureMessage(caught));
    }
  }, []);

  /** Report the panel's measured cell size with one request. */
  const resize = useCallback(
    async (nextCols: number, nextRows: number) => {
      const generation = routeGenerationRef.current;
      const attachment = attachmentRef.current;
      if (generation === null || attachment === null) return;
      const measuredCols = Math.max(1, Math.floor(nextCols));
      const measuredRows = Math.max(1, Math.floor(nextRows));
      if (lastSizeRef.current?.cols === measuredCols && lastSizeRef.current?.rows === measuredRows) {
        return;
      }
      lastSizeRef.current = { cols: measuredCols, rows: measuredRows };
      gridRef.current.resize(measuredCols, measuredRows);
      setCols(measuredCols);
      setRows(measuredRows);
      publish();
      try {
        await bridge.terminalResize(attachment, measuredCols, measuredRows, generation);
      } catch (caught) {
        setError(failureMessage(caught));
      }
    },
    [publish],
  );

  /** One tick: flush one queued write, then read the output. */
  const pull = useCallback(async () => {
    const generation = routeGenerationRef.current;
    const attachment = attachmentRef.current;
    if (generation === null || attachment === null) return;
    await flushInput(attachment, generation);
    try {
      const chunk = await bridge.terminalResume(attachment, nextSeqRef.current, generation);
      consumeChunk(chunk, null);
    } catch (caught) {
      setError(failureMessage(caught));
    }
  }, [consumeChunk, flushInput]);

  const pullRef = useRef<() => void>(() => undefined);
  useEffect(() => {
    pullRef.current = () => {
      void pull();
    };
  });

  // The cadence: one interval, and only while an attachment is live. It is
  // cleared when the attachment goes, when the route changes and on
  // unmount, so a detached terminal leaves no timer behind.
  useEffect(() => {
    if (!attachmentId) return undefined;
    const timer = setInterval(() => pullRef.current(), TERMINAL_RESUME_INTERVAL_MS);
    return () => clearInterval(timer);
  }, [attachmentId, routeGeneration]);

  // The workspace's processes, read once per route.
  useEffect(() => {
    if (!enabled) return undefined;
    const generation = routeGenerationRef.current;
    if (generation === null) return undefined;
    let cancelled = false;
    void bridge
      .terminalList(generation)
      .then((view) => {
        if (cancelled) return;
        setProcesses(view.processes);
      })
      .catch((caught: unknown) => {
        if (cancelled) return;
        setError(failureMessage(caught));
      });
    return () => {
      cancelled = true;
    };
  }, [enabled, routeGeneration]);

  // A reconnect orphans the attachment: the daemon owns it by the transport
  // connection's client id, and the host holds **one** client. The desktop
  // re-attaches to the handle it was showing — it never opens a second
  // connection for the terminal — and the daemon's answer decides whether
  // the handle is still there. A handle that is gone is reported as gone,
  // not as an empty terminal.
  useEffect(() => {
    const target = shownHandleRef.current;
    if (target === null) return;
    attachmentRef.current = null;
    setAttachmentId(null);
    void attach(target);
  }, [attach, connectionGeneration]);

  return {
    effectiveMap,
    handle,
    processes,
    lines,
    scrollback,
    droppedLines,
    cursor,
    cols,
    rows,
    busy,
    error,
    notice,
    resyncReason,
    unsupported,
    onShow: show,
    onDetach: detach,
    onInput: queueInput,
    onResize: resize,
  };
}
