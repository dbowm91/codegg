import { beforeEach, describe, expect, it, vi } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { bridge } from '../bridge';
import { ALL_ACTION_DECLARATIONS, buildEffectiveMap } from '../keymap';
import { useEditorSession } from './session';
import type { EditorViewportView } from './contract';

vi.mock('../bridge', () => ({
  bridge: {
    documentOpen: vi.fn(),
    documentViewport: vi.fn(),
    documentCursor: vi.fn(),
    documentEdit: vi.fn(),
    documentUndo: vi.fn(),
    documentRedo: vi.fn(),
    documentSave: vi.fn(),
    documentReload: vi.fn(),
    documentResync: vi.fn(),
    documentClose: vi.fn(),
    documentState: vi.fn(),
    documentTree: vi.fn(),
    documentDiagnostics: vi.fn(),
    documentDiagnosticsSubscribe: vi.fn(),
    documentDiagnosticsUnsubscribe: vi.fn(),
    documentCompletion: vi.fn(),
    documentNavigate: vi.fn(),
  },
}));

const viewport = (): EditorViewportView => ({
  relativePath: 'src/lib.rs',
  revision: 4,
  firstLine: 0,
  lineCount: 1,
  totalLines: 1,
  byteLength: 12,
  visibleLines: ['fn main() {}'],
  caret: 0,
  anchor: 0,
  caretLine: 0,
  caretColumn: 0,
  anchorLine: 0,
  anchorColumn: 0,
});

beforeEach(() => {
  vi.mocked(bridge.documentViewport).mockResolvedValue(viewport());
  vi.mocked(bridge.documentState).mockResolvedValue({
    state: 'synced',
    message: 'the document is in sync with the daemon',
    relativePath: 'src/lib.rs',
    revision: 4,
    writable: true,
    dirty: false,
    conflicted: false,
    pendingChanges: 0,
    canUndo: false,
    canRedo: false,
  });
  vi.mocked(bridge.documentTree).mockResolvedValue({
    workspaceId: 'workspace-1',
    prefix: '',
    status: 'ready',
    message: 'the workspace listing is current',
    nodes: [],
    truncated: false,
  });
  vi.mocked(bridge.documentDiagnosticsSubscribe).mockImplementation(async (_route, onMessage) => {
    onMessage({
      status: 'no_warm_server',
      message: 'no language server is warm for this project yet',
      diagnostics: [],
      truncated: false,
    });
    return 'diagnostics-1';
  });
  vi.mocked(bridge.documentOpen).mockResolvedValue({
    relativePath: 'src/lib.rs',
    revision: 4,
    state: 'synced',
    message: 'the document is in sync with the daemon',
    writable: true,
    totalLines: 1,
    byteLength: 12,
  });
});

describe('the editor session', () => {
  it('reads the file tree and the diagnostics when a workspace is selected', async () => {
    const { result, rerender } = renderHook(({ sessionId }: { sessionId: string | null }) =>
      useEditorSession({
        routeGeneration: 7,
        connectionGeneration: 1,
        sessionId,
        enabled: true,
        effectiveMap: buildEffectiveMap(ALL_ACTION_DECLARATIONS),
      }), { initialProps: { sessionId: null as string | null } },
    );
    await waitFor(() => expect(result.current.tree).not.toBeNull());
    expect(bridge.documentTree).toHaveBeenCalledWith('', 7);
    expect(bridge.documentDiagnosticsSubscribe).not.toHaveBeenCalled();
    rerender({ sessionId: 's-1' });
    await waitFor(() => expect(result.current.diagnostics?.status).toBe('no_warm_server'));
    expect(bridge.documentDiagnosticsSubscribe).toHaveBeenCalledWith(7, expect.any(Function));
  });

  it('re-subscribes diagnostics when the daemon connection generation changes', async () => {
    const { rerender } = renderHook(
      ({ connectionGeneration }: { connectionGeneration: number }) =>
        useEditorSession({
          routeGeneration: 7,
          connectionGeneration,
          sessionId: 's-1',
          enabled: true,
          effectiveMap: buildEffectiveMap(ALL_ACTION_DECLARATIONS),
        }),
      { initialProps: { connectionGeneration: 1 } },
    );
    await waitFor(() => expect(bridge.documentDiagnosticsSubscribe).toHaveBeenCalledTimes(1));
    rerender({ connectionGeneration: 2 });
    await waitFor(() => expect(bridge.documentDiagnosticsSubscribe).toHaveBeenCalledTimes(2));
    expect(bridge.documentDiagnosticsUnsubscribe).toHaveBeenCalledWith('diagnostics-1');
  });

  it('silently coalesces duplicate completion gestures at the same revision and caret', async () => {
    let resolveCompletion!: (value: { status: string; message: string; candidates: []; truncated: boolean }) => void;
    vi.mocked(bridge.documentCompletion).mockImplementationOnce(
      () => new Promise((resolve) => { resolveCompletion = resolve; }),
    );
    const { result } = renderHook(() =>
      useEditorSession({
        routeGeneration: 7,
        connectionGeneration: 1,
        sessionId: 's-1',
        enabled: true,
        effectiveMap: buildEffectiveMap(ALL_ACTION_DECLARATIONS),
      }),
    );
    await waitFor(() => expect(result.current.tree).not.toBeNull());
    await result.current.onOpen('src/lib.rs');
    await waitFor(() => expect(result.current.viewport).not.toBeNull());
    void result.current.onComplete();
    void result.current.onComplete();
    expect(bridge.documentCompletion).toHaveBeenCalledTimes(1);
    resolveCompletion({ status: 'ready', message: 'ready', candidates: [], truncated: false });
    await waitFor(() => expect(result.current.completion?.status).toBe('ready'));
    expect(result.current.error).toBeNull();
  });

  it('reads nothing while no workspace is selected', async () => {
    const { result } = renderHook(() =>
      useEditorSession({
        routeGeneration: null,
        connectionGeneration: null,
        sessionId: null,
        enabled: false,
        effectiveMap: buildEffectiveMap(ALL_ACTION_DECLARATIONS),
      }),
    );
    await waitFor(() => expect(result.current.busy).toBe(false));
    expect(bridge.documentTree).not.toHaveBeenCalled();
    expect(bridge.documentDiagnostics).not.toHaveBeenCalled();
  });

  it('opens a document and publishes the window the host sent', async () => {
    const { result } = renderHook(() =>
      useEditorSession({
        routeGeneration: 7,
        connectionGeneration: 1,
        sessionId: 's-1',
        enabled: true,
        effectiveMap: buildEffectiveMap(ALL_ACTION_DECLARATIONS),
      }),
    );
    await waitFor(() => expect(result.current.tree).not.toBeNull());
    await result.current.onOpen('src/lib.rs');
    await waitFor(() => expect(result.current.viewport).not.toBeNull());
    expect(bridge.documentOpen).toHaveBeenCalledWith('src/lib.rs', 7);
    expect(result.current.viewport?.visibleLines).toEqual(['fn main() {}']);
    expect(result.current.status?.state).toBe('synced');
  });

  it('reports a failure and keeps the last good view', async () => {
    const { result } = renderHook(() =>
      useEditorSession({
        routeGeneration: 7,
        connectionGeneration: 1,
        sessionId: 's-1',
        enabled: true,
        effectiveMap: buildEffectiveMap(ALL_ACTION_DECLARATIONS),
      }),
    );
    await waitFor(() => expect(result.current.tree).not.toBeNull());
    vi.mocked(bridge.documentViewport).mockRejectedValueOnce('the daemon connection was lost');
    await result.current.onScroll(4);
    await waitFor(() => expect(result.current.error).toBe('the daemon connection was lost'));
    expect(result.current.tree?.status).toBe('ready');
  });

  it('asks the host to move the caret and publishes the returned window', async () => {
    vi.mocked(bridge.documentCursor).mockResolvedValue({ ...viewport(), caret: 3, caretColumn: 3 });
    const { result } = renderHook(() =>
      useEditorSession({
        routeGeneration: 7,
        connectionGeneration: 1,
        sessionId: 's-1',
        enabled: true,
        effectiveMap: buildEffectiveMap(ALL_ACTION_DECLARATIONS),
      }),
    );
    await waitFor(() => expect(result.current.tree).not.toBeNull());
    await result.current.onMoveCursor('right', false);
    await waitFor(() => expect(result.current.viewport?.caret).toBe(3));
    expect(bridge.documentCursor).toHaveBeenCalledWith('right', false);
  });
});
