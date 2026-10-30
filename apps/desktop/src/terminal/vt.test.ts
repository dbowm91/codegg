import { describe, expect, it } from 'vitest';
import {
  MAX_SCROLLBACK_BYTES,
  MAX_SCROLLBACK_LINES,
  TerminalGrid,
} from './vt';

// The escape sequences are spelled by their names, never by a literal that
// reads like a permission string: `ESC` is the escape byte and `CSI` is the
// control sequence introducer.
const ESC = '\u001b';
const CSI = `${ESC}[`;
const BEL = '\u0007';

const encoder = new TextEncoder();
const bytes = (text: string): Uint8Array => encoder.encode(text);

const grid = (cols = 20, rows = 4): TerminalGrid => new TerminalGrid(cols, rows);

describe('the bounded VT renderer', () => {
  it('writes printable bytes at the cursor', () => {
    const terminal = grid();
    terminal.write(bytes('hi'));
    expect(terminal.lines()[0]).toBe('hi');
    expect(terminal.cursor()).toEqual({ row: 0, col: 2 });
  });

  it('returns to the start of the line on a carriage return', () => {
    const terminal = grid();
    terminal.write(bytes('ab\r'));
    terminal.write(bytes('c'));
    expect(terminal.lines()[0]).toBe('cb');
    expect(terminal.cursor()).toEqual({ row: 0, col: 1 });
  });

  it('moves down and keeps the column on a line feed', () => {
    const terminal = grid();
    terminal.write(bytes('a\nb'));
    expect(terminal.lines()[0]).toBe('a');
    // The line feed moves down without returning to the column: a new line
    // is a carriage return and a line feed.
    expect(terminal.lines()[1]).toBe(' b');
    expect(terminal.cursor()).toEqual({ row: 1, col: 2 });
  });

  it('moves back one column on a backspace', () => {
    const terminal = grid();
    terminal.write(bytes('abc'));
    expect(terminal.lines()[0]).toBe('ac');
    expect(terminal.cursor()).toEqual({ row: 0, col: 2 });
  });

  it('erases from the cursor to the end of the display', () => {
    const terminal = grid();
    terminal.write(bytes('abc\r\ndef'));
    // The cursor is at the start of the second row, so the erase takes
    // everything from there to the end of the display.
    terminal.write(bytes(`${CSI}2;1H`));
    terminal.write(bytes(`${CSI}0J`));
    expect(terminal.lines()[0]).toBe('abc');
    expect(terminal.lines()[1]).toBe('');
  });

  it('erases from the start of the display to the cursor', () => {
    const terminal = grid();
    terminal.write(bytes('abc\r\ndef'));
    terminal.write(bytes(`${CSI}2;2H`));
    terminal.write(bytes(`${CSI}1J`));
    expect(terminal.lines()[1]).toBe('  f');
  });

  it('erases the whole display', () => {
    const terminal = grid();
    terminal.write(bytes('abc\r\ndef'));
    terminal.write(bytes(`${CSI}2J`));
    expect(terminal.lines()[0]).toBe('');
    expect(terminal.lines()[1]).toBe('');
  });

  it('erases from the cursor to the end of the line', () => {
    const terminal = grid();
    terminal.write(bytes('abcdef'));
    terminal.write(bytes(`${CSI}1;4H`));
    terminal.write(bytes(`${CSI}0K`));
    expect(terminal.lines()[0]).toBe('abc');
  });

  it('erases from the start of the line to the cursor', () => {
    const terminal = grid();
    terminal.write(bytes('abcdef'));
    terminal.write(bytes(`${CSI}1;4H`));
    terminal.write(bytes(`${CSI}1K`));
    // The erase reaches the cursor itself, so the four cells before it go.
    expect(terminal.lines()[0]).toBe('    ef');
  });

  it('erases the whole line', () => {
    const terminal = grid();
    terminal.write(bytes('abcdef'));
    terminal.write(bytes(`${CSI}2K`));
    expect(terminal.lines()[0]).toBe('');
  });

  it('moves the cursor up, down, right and left', () => {
    const terminal = grid();
    terminal.write(bytes(`${CSI}3;5H`));
    expect(terminal.cursor()).toEqual({ row: 2, col: 4 });
    terminal.write(bytes(`${CSI}A`));
    expect(terminal.cursor()).toEqual({ row: 1, col: 4 });
    terminal.write(bytes(`${CSI}2B`));
    expect(terminal.cursor()).toEqual({ row: 3, col: 4 });
    terminal.write(bytes(`${CSI}3C`));
    expect(terminal.cursor()).toEqual({ row: 3, col: 7 });
    terminal.write(bytes(`${CSI}2D`));
    expect(terminal.cursor()).toEqual({ row: 3, col: 5 });
  });

  it('moves the cursor to an absolute position', () => {
    const terminal = grid();
    terminal.write(bytes(`${CSI}2;3H`));
    terminal.write(bytes('x'));
    expect(terminal.lines()[1]).toBe('  x');
    expect(terminal.cursor()).toEqual({ row: 1, col: 3 });
  });

  it('maps the bold attribute and the eight standard colours to classes', () => {
    const terminal = grid();
    terminal.write(bytes(`${CSI}1m`));
    terminal.write(bytes('b'));
    terminal.write(bytes(`${CSI}0m`));
    terminal.write(bytes(`${CSI}31m`));
    terminal.write(bytes('r'));
    terminal.write(bytes(`${CSI}42m`));
    terminal.write(bytes('g'));
    const spans = terminal.styledLines()[0].spans;
    expect(spans.map((span) => [span.text, span.className])).toEqual([
      ['b', 'term-bold'],
      ['r', 'term-fg-red'],
      ['g', 'term-fg-red term-bg-green'],
    ]);
  });

  it('ignores the SGR parameters outside the bounded subset', () => {
    const terminal = grid();
    terminal.write(bytes(`${CSI}5;25m`));
    terminal.write(bytes('x'));
    // Neither the blink nor the "not blinking" parameter is in the subset,
    // so neither reaches the visible text.
    expect(terminal.lines()[0]).toBe('x');
    expect(terminal.styledLines()[0].spans[0].className).toBe('');
  });

  it('draws a sequence of supported sequences in order', () => {
    const terminal = grid();
    terminal.write(bytes(`${CSI}1;1H`));
    terminal.write(bytes('one'));
    terminal.write(bytes('\r\n'));
    terminal.write(bytes(`${CSI}31m`));
    terminal.write(bytes('two'));
    terminal.write(bytes(`${CSI}0K`));
    expect(terminal.lines()[0]).toBe('one');
    expect(terminal.lines()[1]).toBe('two');
    expect(terminal.styledLines()[1].spans[0].className).toBe('term-fg-red');
  });

  it('consumes an operating-system command without leaking it into the text', () => {
    const terminal = grid();
    terminal.write(bytes(`${ESC}]0;a title${BEL}`));
    terminal.write(bytes('after'));
    expect(terminal.lines()[0]).toBe('after');
    expect(terminal.lastUnsupported()).toBe('osc');
  });

  it('consumes an operating-system command terminated by a string terminator', () => {
    const terminal = grid();
    terminal.write(bytes(`${ESC}]0;a title${ESC}\\`));
    terminal.write(bytes('after'));
    expect(terminal.lines()[0]).toBe('after');
    expect(terminal.lastUnsupported()).toBe('osc');
  });

  it('consumes a private mode without leaking it into the text', () => {
    const terminal = grid();
    terminal.write(bytes(`${CSI}?1049h`));
    terminal.write(bytes('after'));
    expect(terminal.lines()[0]).toBe('after');
    expect(terminal.lastUnsupported()).toBe('private-mode');
  });

  it('consumes a scroll region without leaking it into the text', () => {
    const terminal = grid();
    terminal.write(bytes(`${CSI}2;10r`));
    terminal.write(bytes('after'));
    expect(terminal.lines()[0]).toBe('after');
    expect(terminal.lastUnsupported()).toBe('scroll-region');
  });

  it('consumes an unknown control sequence without leaking it into the text', () => {
    const terminal = grid();
    terminal.write(bytes(`${CSI}6n`));
    terminal.write(bytes('after'));
    expect(terminal.lines()[0]).toBe('after');
    expect(terminal.lastUnsupported()).toBe('unknown-csi');
  });

  it('consumes an unknown escape sequence without leaking it into the text', () => {
    const terminal = grid();
    // A character-set designator: the escape byte, an intermediate byte and
    // a final byte. The final byte must not reach the visible text.
    terminal.write(bytes(`${ESC}(B`));
    terminal.write(bytes('after'));
    expect(terminal.lines()[0]).toBe('after');
    expect(terminal.lastUnsupported()).toBe('unknown-escape');
  });

  it('drops the oldest scrollback lines at the line bound and says so', () => {
    const terminal = new TerminalGrid(10, 1);
    const overflow = MAX_SCROLLBACK_LINES + 100;
    for (let index = 0; index < overflow; index += 1) terminal.write(bytes('x\r\n'));
    expect(terminal.scrollback()).toHaveLength(MAX_SCROLLBACK_LINES);
    expect(terminal.droppedLines()).toBe(100);
  });

  it('drops the oldest scrollback lines at the byte bound and says so', () => {
    const terminal = new TerminalGrid(200, 1);
    const line = 'y'.repeat(200);
    for (let index = 0; index < MAX_SCROLLBACK_LINES; index += 1) {
      terminal.write(bytes(`${line}\r\n`));
    }
    // The byte bound binds before the line bound here: the retained text
    // stays inside the bound and the line count falls short of it.
    const retained = terminal.scrollback().reduce((sum, entry) => sum + entry.length, 0);
    expect(retained).toBeLessThanOrEqual(MAX_SCROLLBACK_BYTES);
    expect(terminal.scrollback().length).toBeLessThan(MAX_SCROLLBACK_LINES);
    expect(terminal.droppedLines()).toBeGreaterThan(0);
  });

  it('reshapes the grid without losing the content that still fits', () => {
    const terminal = grid(20, 4);
    terminal.write(bytes('abc\r\ndef'));
    terminal.resize(10, 2);
    expect(terminal.lines()[0]).toBe('abc');
    expect(terminal.lines()[1]).toBe('def');
    expect(terminal.cursor()).toEqual({ row: 1, col: 3 });
  });
});
