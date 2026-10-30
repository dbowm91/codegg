import type { EffectiveMap } from '../keymap';
import { ALL_ACTION_DECLARATIONS, buildEffectiveMap } from '../keymap';
import type { TerminalCursor, TerminalLine } from './vt';

// The terminal's bridge contract.
//
// These are the TypeScript mirrors of the host's terminal views
// (`apps/desktop/src-tauri/src/terminal.rs`), exactly as
// `apps/desktop/src/editor/contract.ts` mirrors `document.rs`: the Rust
// structs carry snake_case fields and serialize them in camelCase, so the
// renderer reads the names the host actually writes. `check-bindings.mjs`
// holds the two in one map, and it compares the Rust field name against
// this file's camelCase name.
//
// The renderer names no other contract: the desktop's bridge types live in
// `bridge-types.ts`, and the terminal's views live here because they belong
// to the terminal, not to the document surface.

/** One interactive process the daemon published. */
export interface TerminalProcessView {
  handle: string;
  workspaceId: string;
  command: string;
  state: string;
  cols: number;
  rows: number;
  nextSeq: number;
  retainedBytes: number;
  totalBytes: number;
  truncated: boolean;
  exitCode: number | null;
  exitSignal: number | null;
}

/** The interactive processes of one workspace. */
export interface TerminalListView {
  processes: TerminalProcessView[];
  truncated: boolean;
}

/** One bounded output read plus its resumption cursor. */
export interface TerminalChunkView {
  handle: string;
  fromSeq: number;
  nextSeq: number;
  gap: boolean;
  dataB64: string;
}

/** The answer to an attach: the attachment, its first chunk and why. */
export interface TerminalAttachView {
  attachmentId: string;
  handle: string;
  chunk: TerminalChunkView;
  resyncReason: string | null;
}

/** The answer to an input write. */
export interface TerminalInputView {
  attachmentId: string;
  bytesAccepted: number;
}

/** The answer to a resize. */
export interface TerminalResizeView {
  attachmentId: string;
  cols: number;
  rows: number;
}

/** The answer to a detach. */
export interface TerminalDetachView {
  attachmentId: string;
}

/** The terminal panel's props. */
export interface TerminalPanelProps {
  /** The effective keymap: the chords that are actually bound. */
  effectiveMap: EffectiveMap;
  /** The handle the panel is showing, or null when nothing is attached. */
  handle: string | null;
  /** The interactive processes the daemon published for the workspace. */
  processes: TerminalProcessView[];
  /** The visible rows of the grid. */
  lines: TerminalLine[];
  /** The retained history above the visible rows. */
  scrollback: string[];
  /** How many lines the scrollback bound dropped. */
  droppedLines: number;
  /** The cursor's position inside the visible rows. */
  cursor: TerminalCursor;
  /** The grid's width, in cells. */
  cols: number;
  /** The grid's height, in cells. */
  rows: number;
  /** Whether a host call is in flight. */
  busy: boolean;
  /** The last failure, when the last call failed. */
  error: string | null;
  /** The honest notice: a resync, or an unsupported sequence. */
  notice: string | null;
  /** The resync reason the daemon typed, when it typed one. */
  resyncReason: string | null;
  /** The notice for the last sequence the bounded renderer did not draw. */
  unsupported: string | null;
  /** Attach to a named handle. */
  onShow: (handle: string) => void;
  /** Release the attachment, without stopping the process. */
  onDetach: () => void;
  /** Queue the user's input for the attached terminal. */
  onInput: (text: string) => void;
  /** Report the panel's measured cell size. */
  onResize: (cols: number, rows: number) => void;
}

/**
 * The terminal props a test renders with: nothing attached, nothing in
 * flight, every intent a no-op.
 */
export function emptyTerminalPanelProps(): TerminalPanelProps {
  return {
    effectiveMap: buildEffectiveMap(ALL_ACTION_DECLARATIONS),
    handle: null,
    processes: [],
    lines: [],
    scrollback: [],
    droppedLines: 0,
    cursor: { row: 0, col: 0 },
    cols: 80,
    rows: 24,
    busy: false,
    error: null,
    notice: null,
    resyncReason: null,
    unsupported: null,
    onShow: () => undefined,
    onDetach: () => undefined,
    onInput: () => undefined,
    onResize: () => undefined,
  };
}
