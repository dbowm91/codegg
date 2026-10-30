// The bounded VT renderer.
//
// The daemon publishes **raw PTY bytes** for an interactive process, so the
// desktop has to interpret escape sequences itself. The tree has no ANSI or
// grid library, and this milestone adds none: a first-party renderer keeps
// the renderer's Content Security Policy local-only and adds no worker, no
// wasm and no new supply-chain surface.
//
// The subset is deliberately bounded, and it is the whole of what this
// renderer implements:
//
//   - printable bytes, written at the cursor;
//   - `CR` (carriage return), `LF` (line feed), `BS` (backspace);
//   - `CSI` erase — `ED` (`J`) and `EL` (`K`), with their parameter
//     variants;
//   - `CSI` cursor movement — `CUU` (`A`), `CUD` (`B`), `CUF` (`C`),
//     `CUB` (`D`), `CUP` (`H`);
//   - `CSI` `SGR` (`m`) for the eight standard colours and the bold
//     attribute, mapped to a small class set.
//
// Everything else is **consumed and ignored** — `OSC` sequences, `CSI`
// scroll regions, and the `CSI` private modes that select the alternate
// buffer. A consumed sequence never leaks into the visible text, and the
// last unsupported class is recorded so the panel can say honestly that the
// renderer did not draw what the process sent.
//
// The parser is **pure**: bytes in, grid out. It never touches the bridge,
// the DOM, a filesystem or a process.

/** The maximum number of lines the scrollback retains. */
export const MAX_SCROLLBACK_LINES = 2000;

/** The maximum number of bytes of text the scrollback retains. */
export const MAX_SCROLLBACK_BYTES = 256 * 1024;

/** The grid's default width, in cells. */
export const DEFAULT_TERMINAL_COLS = 80;

/** The grid's default height, in cells. */
export const DEFAULT_TERMINAL_ROWS = 24;

/**
 * The class of a sequence the bounded subset does not implement.
 *
 * The class is a name, never a spelling of a command or a permission: the
 * panel renders it as prose.
 */
export type UnsupportedClass =
  | 'osc'
  | 'private-mode'
  | 'scroll-region'
  | 'unknown-csi'
  | 'unknown-escape';

/** One run of cells that share a class. */
export interface TerminalSpan {
  /** The run's text. */
  text: string;
  /** The class the run carries, or the empty string for the default. */
  className: string;
}

/** One visible row: its plain text and its styled runs. */
export interface TerminalLine {
  /** The row's text, with the trailing padding trimmed. */
  text: string;
  /** The row's runs, in order. */
  spans: TerminalSpan[];
}

/** The cursor's position inside the visible rows. */
export interface TerminalCursor {
  /** The cursor's row. */
  row: number;
  /** The cursor's column. */
  col: number;
}

/** One cell of the grid. */
interface Cell {
  /** The cell's character. */
  ch: string;
  /** The class the cell carries, or the empty string for the default. */
  className: string;
}

/** The parser's states. */
type ParserState =
  | 'ground'
  | 'escape'
  | 'csi'
  | 'osc'
  | 'osc-escape'
  | 'string'
  | 'string-escape';

/** The eight standard colours, in the order the `SGR` parameters name them. */
const STANDARD_COLOURS = [
  'black',
  'red',
  'green',
  'yellow',
  'blue',
  'magenta',
  'cyan',
  'white',
] as const;

const encoder = new TextEncoder();

/** The UTF-8 byte length of a string. */
export function utf8Length(text: string): number {
  return encoder.encode(text).length;
}

/** The bounded VT grid. */
export class TerminalGrid {
  private cols: number;
  private rows: number;
  private cells: Cell[][];
  private cursorRow = 0;
  private cursorCol = 0;
  private scrollbackLines: string[] = [];
  private scrollbackBytes = 0;
  private dropped = 0;
  private unsupported: UnsupportedClass | null = null;
  private state: ParserState = 'ground';
  private params = '';
  private intermediates = '';
  private readonly decoder = new TextDecoder('utf-8');
  private bold = false;
  private foreground: string | null = null;
  private background: string | null = null;

  constructor(cols: number = DEFAULT_TERMINAL_COLS, rows: number = DEFAULT_TERMINAL_ROWS) {
    this.cols = Math.max(1, Math.floor(cols));
    this.rows = Math.max(1, Math.floor(rows));
    this.cells = this.blankGrid();
  }

  // ── The grid's shape ──────────────────────────────────────────────────

  /** The grid's width, in cells. */
  width(): number {
    return this.cols;
  }

  /** The grid's height, in cells. */
  height(): number {
    return this.rows;
  }

  /**
   * Reshape the grid.
   *
   * The content is kept where it still fits and the rest is padded, and the
   * cursor is clamped into the new shape.
   */
  resize(cols: number, rows: number): void {
    const nextCols = Math.max(1, Math.floor(cols));
    const nextRows = Math.max(1, Math.floor(rows));
    const reshaped: Cell[][] = [];
    for (let row = 0; row < nextRows; row += 1) {
      const line: Cell[] = [];
      for (let col = 0; col < nextCols; col += 1) {
        line.push(this.cells[row]?.[col] ?? this.blankCell());
      }
      reshaped.push(line);
    }
    this.cells = reshaped;
    this.cols = nextCols;
    this.rows = nextRows;
    this.cursorRow = Math.min(this.cursorRow, nextRows - 1);
    this.cursorCol = Math.min(this.cursorCol, nextCols - 1);
  }

  /** Clear the grid, the scrollback and the parser's state. */
  reset(): void {
    this.cells = this.blankGrid();
    this.cursorRow = 0;
    this.cursorCol = 0;
    this.scrollbackLines = [];
    this.scrollbackBytes = 0;
    this.dropped = 0;
    this.unsupported = null;
    this.state = 'ground';
    this.params = '';
    this.intermediates = '';
    this.bold = false;
    this.foreground = null;
    this.background = null;
  }

  // ── What the panel renders ────────────────────────────────────────────

  /** The visible rows' plain text, one entry per row. */
  lines(): string[] {
    return this.cells.map((line) => this.rowText(line));
  }

  /** The visible rows with their styled runs. */
  styledLines(): TerminalLine[] {
    return this.cells.map((line) => ({ text: this.rowText(line), spans: this.rowSpans(line) }));
  }

  /** The retained history above the visible rows. */
  scrollback(): string[] {
    return [...this.scrollbackLines];
  }

  /** How many lines the scrollback bound dropped. */
  droppedLines(): number {
    return this.dropped;
  }

  /** The cursor's position inside the visible rows. */
  cursor(): TerminalCursor {
    return { row: this.cursorRow, col: this.cursorCol };
  }

  /** The class of the last unsupported sequence, or null when there was none. */
  lastUnsupported(): string | null {
    return this.unsupported;
  }

  // ── The parser ────────────────────────────────────────────────────────

  /** Feed the grid the bytes a process produced. */
  write(bytes: Uint8Array): void {
    for (const byte of bytes) this.step(byte);
  }

  private step(byte: number): void {
    switch (this.state) {
      case 'ground':
        this.ground(byte);
        return;
      case 'escape':
        this.escape(byte);
        return;
      case 'csi':
        this.csi(byte);
        return;
      case 'osc':
        this.osc(byte);
        return;
      case 'osc-escape':
        this.oscEscape(byte);
        return;
      case 'string':
        this.string(byte);
        return;
      case 'string-escape':
        this.stringEscape(byte);
        return;
      default:
        this.state = 'ground';
    }
  }

  private ground(byte: number): void {
    if (byte === 0x1b) {
      this.state = 'escape';
      this.params = '';
      this.intermediates = '';
      return;
    }
    if (byte === 0x0d) {
      this.cursorCol = 0;
      return;
    }
    if (byte === 0x0a) {
      this.lineFeed();
      return;
    }
    if (byte === 0x08) {
      this.cursorCol = Math.max(0, this.cursorCol - 1);
      return;
    }
    // A tab, a bell and a delete are control bytes the bounded subset does
    // not implement. They are consumed, and they are not sequences, so they
    // are not recorded as an unsupported class.
    if (byte === 0x09 || byte === 0x07 || byte === 0x7f) return;
    if (byte >= 0x20 && byte <= 0x7e) {
      this.print(String.fromCharCode(byte));
      return;
    }
    if (byte >= 0x80) {
      // The rest of a multi-byte UTF-8 character. The decoder keeps the
      // partial sequence until the rest arrives.
      const text = this.decoder.decode(new Uint8Array([byte]), { stream: true });
      for (const character of text) this.print(character);
    }
  }

  private escape(byte: number): void {
    if (this.intermediates === '') {
      if (byte === 0x5b) {
        this.state = 'csi';
        this.params = '';
        return;
      }
      if (byte === 0x5d) {
        this.state = 'osc';
        return;
      }
      if (byte === 0x50 || byte === 0x58 || byte === 0x5e || byte === 0x5f) {
        this.state = 'string';
        return;
      }
    }
    if (byte >= 0x20 && byte <= 0x2f) {
      this.intermediates += String.fromCharCode(byte);
      return;
    }
    if (byte >= 0x30 && byte <= 0x7e) {
      // The final byte of an escape sequence the bounded subset does not
      // implement. It is consumed: it never reaches the visible text.
      this.record('unknown-escape');
      this.state = 'ground';
      return;
    }
    // A control byte inside an escape sequence is consumed and ignored.
  }

  private csi(byte: number): void {
    if (byte >= 0x30 && byte <= 0x3f) {
      this.params += String.fromCharCode(byte);
      return;
    }
    if (byte >= 0x20 && byte <= 0x2f) {
      this.intermediates += String.fromCharCode(byte);
      return;
    }
    if (byte >= 0x40 && byte <= 0x7e) {
      this.dispatchCsi(String.fromCharCode(byte));
      this.state = 'ground';
      return;
    }
    // A control byte inside a control sequence is consumed and ignored.
  }

  private osc(byte: number): void {
    if (byte === 0x07) {
      this.record('osc');
      this.state = 'ground';
      return;
    }
    if (byte === 0x1b) {
      this.state = 'osc-escape';
    }
  }

  private oscEscape(byte: number): void {
    if (byte === 0x5c) {
      this.record('osc');
      this.state = 'ground';
      return;
    }
    this.state = 'osc';
  }

  private string(byte: number): void {
    if (byte === 0x07) {
      this.record('unknown-escape');
      this.state = 'ground';
      return;
    }
    if (byte === 0x1b) {
      this.state = 'string-escape';
    }
  }

  private stringEscape(byte: number): void {
    if (byte === 0x5c) {
      this.record('unknown-escape');
      this.state = 'ground';
      return;
    }
    this.state = 'string';
  }

  /** Apply one complete control sequence. */
  private dispatchCsi(final: string): void {
    if (this.params.startsWith('?')) {
      // The private modes — the alternate buffer among them — are consumed
      // and recorded: the bounded renderer does not implement them.
      this.record('private-mode');
      return;
    }
    if (this.intermediates !== '') {
      this.record('unknown-csi');
      return;
    }
    const params = this.parseParams(this.params);
    const first = params[0];
    const second = params[1];
    switch (final) {
      case 'A':
        this.moveCursor(-this.count(first), 0);
        return;
      case 'B':
        this.moveCursor(this.count(first), 0);
        return;
      case 'C':
        this.moveCursor(0, this.count(first));
        return;
      case 'D':
        this.moveCursor(0, -this.count(first));
        return;
      case 'H':
        this.cursorRow = this.clamp((first ?? 1) - 1, this.rows);
        this.cursorCol = this.clamp((second ?? 1) - 1, this.cols);
        return;
      case 'J':
        this.eraseDisplay(first ?? 0);
        return;
      case 'K':
        this.eraseLine(first ?? 0);
        return;
      case 'm':
        this.applySgr(params);
        return;
      case 'r':
        // The scroll region is not implemented; it is consumed.
        this.record('scroll-region');
        return;
      default:
        this.record('unknown-csi');
    }
  }

  /** The parameter list of a control sequence; an omitted parameter is undefined. */
  private parseParams(raw: string): (number | undefined)[] {
    if (raw === '') return [];
    return raw.split(';').map((part) => {
      if (part === '') return undefined;
      const value = Number.parseInt(part, 10);
      return Number.isFinite(value) ? value : undefined;
    });
  }

  /** A movement's count: one when the parameter is absent or zero. */
  private count(value: number | undefined): number {
    if (value === undefined || value < 1) return 1;
    return value;
  }

  private clamp(value: number, limit: number): number {
    if (value < 0) return 0;
    if (value > limit - 1) return limit - 1;
    return value;
  }

  // ── The grid's drawing ────────────────────────────────────────────────

  private print(character: string): void {
    if (this.cursorCol >= this.cols) {
      this.cursorCol = 0;
      this.lineFeed();
    }
    this.cells[this.cursorRow][this.cursorCol] = {
      ch: character,
      className: this.currentClass(),
    };
    this.cursorCol += 1;
  }

  private lineFeed(): void {
    this.cursorRow += 1;
    if (this.cursorRow >= this.rows) {
      this.scrollUp();
      this.cursorRow = this.rows - 1;
    }
  }

  private scrollUp(): void {
    const evicted = this.cells.shift();
    this.cells.push(this.blankRow());
    if (evicted) this.pushScrollback(this.rowText(evicted));
  }

  private pushScrollback(text: string): void {
    this.scrollbackLines.push(text);
    this.scrollbackBytes += utf8Length(text);
    while (
      this.scrollbackLines.length > MAX_SCROLLBACK_LINES ||
      (this.scrollbackBytes > MAX_SCROLLBACK_BYTES && this.scrollbackLines.length > 0)
    ) {
      const removed = this.scrollbackLines.shift();
      if (removed === undefined) break;
      this.scrollbackBytes -= utf8Length(removed);
      this.dropped += 1;
    }
  }

  private moveCursor(dRow: number, dCol: number): void {
    this.cursorRow = this.clamp(this.cursorRow + dRow, this.rows);
    this.cursorCol = this.clamp(this.cursorCol + dCol, this.cols);
  }

  private eraseDisplay(mode: number): void {
    switch (mode) {
      case 0:
        this.eraseRange(this.cursorRow, this.cursorCol, this.cols);
        for (let row = this.cursorRow + 1; row < this.rows; row += 1) this.eraseRow(row);
        return;
      case 1:
        for (let row = 0; row < this.cursorRow; row += 1) this.eraseRow(row);
        this.eraseRange(this.cursorRow, 0, this.cursorCol + 1);
        return;
      case 2:
        for (let row = 0; row < this.rows; row += 1) this.eraseRow(row);
        return;
      case 3:
        for (let row = 0; row < this.rows; row += 1) this.eraseRow(row);
        this.scrollbackLines = [];
        this.scrollbackBytes = 0;
        return;
      default:
        this.record('unknown-csi');
    }
  }

  private eraseLine(mode: number): void {
    switch (mode) {
      case 0:
        this.eraseRange(this.cursorRow, this.cursorCol, this.cols);
        return;
      case 1:
        this.eraseRange(this.cursorRow, 0, this.cursorCol + 1);
        return;
      case 2:
        this.eraseRow(this.cursorRow);
        return;
      default:
        this.record('unknown-csi');
    }
  }

  private eraseRow(row: number): void {
    this.eraseRange(row, 0, this.cols);
  }

  /** Erase a half-open range of one row, with the current attributes. */
  private eraseRange(row: number, from: number, to: number): void {
    const className = this.currentClass();
    for (let col = from; col < to && col < this.cols; col += 1) {
      this.cells[row][col] = { ch: ' ', className };
    }
  }

  private applySgr(params: (number | undefined)[]): void {
    const values = params.length > 0 ? params : [0];
    for (const value of values) {
      const code = value ?? 0;
      if (code === 0) {
        this.bold = false;
        this.foreground = null;
        this.background = null;
        continue;
      }
      if (code === 1) {
        this.bold = true;
        continue;
      }
      if (code === 22) {
        this.bold = false;
        continue;
      }
      if (code >= 30 && code <= 37) {
        this.foreground = STANDARD_COLOURS[code - 30];
        continue;
      }
      if (code === 39) {
        this.foreground = null;
        continue;
      }
      if (code >= 40 && code <= 47) {
        this.background = STANDARD_COLOURS[code - 40];
        continue;
      }
      if (code === 49) {
        this.background = null;
        continue;
      }
      // Every other attribute is consumed and ignored: the bounded subset
      // carries the eight standard colours and the bold attribute only.
    }
  }

  private currentClass(): string {
    const parts: string[] = [];
    if (this.bold) parts.push('term-bold');
    if (this.foreground) parts.push(`term-fg-${this.foreground}`);
    if (this.background) parts.push(`term-bg-${this.background}`);
    return parts.join(' ');
  }

  private record(value: UnsupportedClass): void {
    this.unsupported = value;
  }

  // ── The grid's cells ──────────────────────────────────────────────────

  private blankGrid(): Cell[][] {
    const grid: Cell[][] = [];
    for (let row = 0; row < this.rows; row += 1) grid.push(this.blankRow());
    return grid;
  }

  private blankRow(): Cell[] {
    const line: Cell[] = [];
    for (let col = 0; col < this.cols; col += 1) line.push(this.blankCell());
    return line;
  }

  private blankCell(): Cell {
    return { ch: ' ', className: '' };
  }

  private rowText(line: Cell[]): string {
    let text = '';
    for (const cell of line) text += cell.ch;
    return text.replace(/[ \t]+$/, '');
  }

  private rowSpans(line: Cell[]): TerminalSpan[] {
    const spans: TerminalSpan[] = [];
    let current: TerminalSpan | null = null;
    for (const cell of line) {
      if (!current || current.className !== cell.className) {
        current = { text: '', className: cell.className };
        spans.push(current);
      }
      current.text += cell.ch;
    }
    const last = spans[spans.length - 1];
    if (last && last.className === '') last.text = last.text.replace(/[ \t]+$/, '');
    return spans.filter((span) => span.text.length > 0);
  }
}
