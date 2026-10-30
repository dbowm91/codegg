import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import { bridge } from '../bridge';
import { ALL_ACTION_DECLARATIONS, buildEffectiveMap } from '../keymap';
import {
  TERMINAL_MAX_INPUT_BYTES,
  TERMINAL_RESUME_INTERVAL_MS,
  useTerminalSession,
} from './session';
import type { TerminalAttachView, TerminalChunkView, TerminalProcessView } from './contract';

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

const b64 = (text: string): string => btoa(text);

const process = (overrides: Partial<TerminalProcessView> = {}): TerminalProcessView => ({
  handle: 'h-1',
  workspaceId: 'w-0',
  command: 'bash',
  state: 'running',
  cols: 80,
  rows: 24,
  nextSeq: 0,
  retainedBytes: 0,
  totalBytes: 0,
  truncated: false,
  exitCode: null,
  exitSignal: null,
  ...overrides,
});

const chunk = (overrides: Partial<TerminalChunkView> = {}): TerminalChunkView => ({
  handle: 'h-1',
  fromSeq: 0,
  nextSeq: 2,
  gap: false,
  dataB64: b64('hi'),
  ...overrides,
});

const attach = (overrides: Partial<TerminalAttachView> = {}): TerminalAttachView => ({
  attachmentId: 'a-1',
  handle: 'h-1',
  chunk: chunk(),
  resyncReason: null,
  ...overrides,
});

const options = (overrides: Record<string, unknown> = {}) => ({
  routeGeneration: 7 as number | null,
  connectionGeneration: 1 as number | null,
  enabled: true,
  effectiveMap: buildEffectiveMap(ALL_ACTION_DECLARATIONS),
  ...overrides,
});

beforeEach(() => {
  vi.mocked(bridge.terminalList).mockResolvedValue({ processes: [process()], truncated: false });
  vi.mocked(bridge.terminalAttach).mockResolvedValue(attach());
  vi.mocked(bridge.terminalResume).mockResolvedValue(chunk({ dataB64: '', nextSeq: 2 }));
  vi.mocked(bridge.terminalInput).mockResolvedValue({ attachmentId: 'a-1', bytesAccepted: 1 });
  vi.mocked(bridge.terminalResize).mockResolvedValue({ attachmentId: 'a-1', cols: 80, rows: 24 });
  vi.mocked(bridge.terminalDetach).mockResolvedValue({ attachmentId: 'a-1' });
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('the terminal session', () => {
  it('holds one resume cadence while an attachment is live, and clears it on unmount', async () => {
    const setSpy = vi.spyOn(globalThis, 'setInterval');
    const clearSpy = vi.spyOn(globalThis, 'clearInterval');
    const { result, unmount } = renderHook(() => useTerminalSession(options()));
    await waitFor(() => expect(result.current.processes).toHaveLength(1));
    act(() => result.current.onShow('h-1'));
    await waitFor(() => expect(result.current.handle).toBe('h-1'));

    const cadenceIndex = setSpy.mock.calls.findIndex(
      (call) => call[1] === TERMINAL_RESUME_INTERVAL_MS,
    );
    // Exactly one cadence, at the TUI's cadence.
    expect(cadenceIndex).toBeGreaterThanOrEqual(0);
    expect(
      setSpy.mock.calls.filter((call) => call[1] === TERMINAL_RESUME_INTERVAL_MS),
    ).toHaveLength(1);
    expect(TERMINAL_RESUME_INTERVAL_MS).toBe(100);

    const timer = setSpy.mock.results[cadenceIndex].value;
    unmount();
    // The cadence is cleared on unmount, so it does not outlive the panel.
    expect(clearSpy).toHaveBeenCalledWith(timer);
  });

  it('draws the chunk the daemon returned and advances the cursor', async () => {
    const { result } = renderHook(() => useTerminalSession(options()));
    await waitFor(() => expect(result.current.processes).toHaveLength(1));
    act(() => result.current.onShow('h-1'));
    await waitFor(() => expect(result.current.handle).toBe('h-1'));
    expect(result.current.lines[0].text).toBe('hi');
    expect(result.current.cursor).toEqual({ row: 0, col: 2 });
  });

  it('resets the view and says so when a chunk reports a gap', async () => {
    vi.mocked(bridge.terminalResume).mockResolvedValue(
      chunk({ fromSeq: 0, nextSeq: 5, gap: true, dataB64: b64('new') }),
    );
    const { result } = renderHook(() => useTerminalSession(options()));
    await waitFor(() => expect(result.current.processes).toHaveLength(1));
    act(() => result.current.onShow('h-1'));
    await waitFor(() => expect(result.current.handle).toBe('h-1'));
    await waitFor(() => expect(result.current.notice).toContain('resynchronized'));
    // The view was reset rather than appended to: the gap chunk's bytes are
    // the whole of it.
    expect(result.current.lines[0].text).toBe('new');
  });

  it('queues the input and flushes it in order, one write per tick', async () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useTerminalSession(options()));
    await act(async () => {
      await Promise.resolve();
    });
    await act(async () => {
      await Promise.resolve();
    });
    act(() => result.current.onShow('h-1'));
    await act(async () => {
      await Promise.resolve();
    });
    act(() => {
      result.current.onInput('a');
      result.current.onInput('b');
    });
    // Nothing is written before the cadence's first tick.
    expect(bridge.terminalInput).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(TERMINAL_RESUME_INTERVAL_MS);
    });
    expect(bridge.terminalInput).toHaveBeenCalledTimes(1);
    expect(bridge.terminalInput).toHaveBeenLastCalledWith('a-1', b64('a'), 7);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(TERMINAL_RESUME_INTERVAL_MS);
    });
    expect(bridge.terminalInput).toHaveBeenCalledTimes(2);
    expect(bridge.terminalInput).toHaveBeenLastCalledWith('a-1', b64('b'), 7);
  });

  it('issues one resize request with the measured cell size', async () => {
    const { result } = renderHook(() => useTerminalSession(options()));
    await waitFor(() => expect(result.current.processes).toHaveLength(1));
    act(() => result.current.onShow('h-1'));
    await waitFor(() => expect(result.current.handle).toBe('h-1'));

    await act(async () => {
      await result.current.onResize(80, 24);
    });
    expect(bridge.terminalResize).toHaveBeenCalledTimes(1);
    expect(bridge.terminalResize).toHaveBeenCalledWith('a-1', 80, 24, 7);

    // The same size again is a no-op: the daemon would refuse it anyway.
    await act(async () => {
      await result.current.onResize(80, 24);
    });
    expect(bridge.terminalResize).toHaveBeenCalledTimes(1);
  });

  it('refuses a handle the daemon did not publish, and says which handle', async () => {
    const { result } = renderHook(() => useTerminalSession(options()));
    await waitFor(() => expect(result.current.processes).toHaveLength(1));
    act(() => result.current.onShow('h-missing'));
    await waitFor(() => expect(result.current.error).toContain('h-missing'));
    // An unknown handle is never attached: the desktop does not focus an
    // arbitrary terminal.
    expect(bridge.terminalAttach).not.toHaveBeenCalled();
  });

  it('clears the cadence when the terminal is detached', async () => {
    const setSpy = vi.spyOn(globalThis, 'setInterval');
    const clearSpy = vi.spyOn(globalThis, 'clearInterval');
    const { result } = renderHook(() => useTerminalSession(options()));
    await waitFor(() => expect(result.current.processes).toHaveLength(1));
    act(() => result.current.onShow('h-1'));
    await waitFor(() => expect(result.current.handle).toBe('h-1'));

    const cadenceIndex = setSpy.mock.calls.findIndex(
      (call) => call[1] === TERMINAL_RESUME_INTERVAL_MS,
    );
    const timer = setSpy.mock.results[cadenceIndex].value;
    act(() => result.current.onDetach());
    expect(clearSpy).toHaveBeenCalledWith(timer);
  });

  it('clears the cadence when the route changes', async () => {
    const setSpy = vi.spyOn(globalThis, 'setInterval');
    const clearSpy = vi.spyOn(globalThis, 'clearInterval');
    const { result, rerender } = renderHook(
      ({ routeGeneration }: { routeGeneration: number }) =>
        useTerminalSession(options({ routeGeneration })),
      { initialProps: { routeGeneration: 7 } },
    );
    await waitFor(() => expect(result.current.processes).toHaveLength(1));
    act(() => result.current.onShow('h-1'));
    await waitFor(() => expect(result.current.handle).toBe('h-1'));

    const cadenceIndex = setSpy.mock.calls.findIndex(
      (call) => call[1] === TERMINAL_RESUME_INTERVAL_MS,
    );
    const timer = setSpy.mock.results[cadenceIndex].value;
    rerender({ routeGeneration: 8 });
    await waitFor(() => expect(clearSpy).toHaveBeenCalledWith(timer));
  });

  it('refuses an input queue past the daemon cap, and says what the cap is', async () => {
    const { result } = renderHook(() => useTerminalSession(options()));
    await waitFor(() => expect(result.current.processes).toHaveLength(1));
    act(() => result.current.onShow('h-1'));
    await waitFor(() => expect(result.current.handle).toBe('h-1'));

    act(() => result.current.onInput('x'.repeat(TERMINAL_MAX_INPUT_BYTES + 1)));
    await waitFor(() => expect(result.current.error).toContain(String(TERMINAL_MAX_INPUT_BYTES)));
    expect(bridge.terminalInput).not.toHaveBeenCalled();
  });

  it('says when the bounded renderer did not draw a sequence', async () => {
    vi.mocked(bridge.terminalResume).mockResolvedValue(
      chunk({ nextSeq: 4, dataB64: btoa('\u001b]0;a title\u0007') }),
    );
    const { result } = renderHook(() => useTerminalSession(options()));
    await waitFor(() => expect(result.current.processes).toHaveLength(1));
    act(() => result.current.onShow('h-1'));
    await waitFor(() => expect(result.current.handle).toBe('h-1'));
    await waitFor(() => expect(result.current.unsupported).toContain('operating-system command'));
  });

  it('re-attaches to the handle it was showing when the connection is replaced', async () => {
    const { result, rerender } = renderHook(
      ({ connectionGeneration }: { connectionGeneration: number }) =>
        useTerminalSession(options({ connectionGeneration })),
      { initialProps: { connectionGeneration: 1 } },
    );
    await waitFor(() => expect(result.current.processes).toHaveLength(1));
    act(() => result.current.onShow('h-1'));
    await waitFor(() => expect(result.current.handle).toBe('h-1'));
    expect(bridge.terminalAttach).toHaveBeenCalledTimes(1);

    rerender({ connectionGeneration: 2 });
    await waitFor(() => expect(bridge.terminalAttach).toHaveBeenCalledTimes(2));
    expect(bridge.terminalAttach).toHaveBeenLastCalledWith('h-1', 7);
  });

  it('reports a handle that is gone rather than showing an empty terminal', async () => {
    vi.mocked(bridge.terminalAttach).mockResolvedValue(
      attach({ resyncReason: 'HandleGone', chunk: chunk({ dataB64: '' }) }),
    );
    const { result } = renderHook(() => useTerminalSession(options()));
    await waitFor(() => expect(result.current.processes).toHaveLength(1));
    act(() => result.current.onShow('h-1'));
    await waitFor(() => expect(result.current.notice).toContain('gone'));
    expect(result.current.handle).toBeNull();
    expect(result.current.resyncReason).toBe('HandleGone');
  });
});
