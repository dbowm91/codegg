import { useCallback, useEffect, useRef, useState } from 'react';
import { bridge } from '../bridge';
import type { EffectiveMap } from '../keymap';
import type {
  EditorCompletionView,
  EditorDiagnosticsView,
  EditorNavigationView,
  EditorPanelProps,
  EditorStateView,
  EditorTreeView,
  EditorViewportView,
} from './contract';

// The editor session: the renderer's half of the editor.
//
// The renderer owns **no** text. It holds the bounded viewport slice the
// host sent, the presentation of the host's state, and the intent callbacks
// the panel renders. Every byte of the document stays in the host's
// controller, and every byte the host sends is a bounded window.
//
// The undo stack is the host's, not the renderer's: the host owns the
// inverse transactions, and the renderer only names "undo" or "redo".

export interface EditorSessionOptions {
  /** The route generation the editor's scope is fenced on, or null. */
  routeGeneration: number | null;
  /** Whether a workspace is selected for the current route. */
  enabled: boolean;
  /**
   * The effective keymap: the chords that are actually bound.
   *
   * The composition root owns it, so the editor resolves its chords
   * against the same map as every other surface.
   */
  effectiveMap: EffectiveMap;
}

/** Read a failure's message, whatever the host answered with. */
function failureMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return 'the editor could not complete the request';
}

/**
 * The editor session.
 *
 * Every host call is fenced on the route generation, and a call that fails
 * leaves the last good view in place: the editor shows a failure, never a
 * blank document.
 */
export function useEditorSession(options: EditorSessionOptions): EditorPanelProps {
  const { routeGeneration, enabled, effectiveMap } = options;
  const [viewport, setViewport] = useState<EditorViewportView | null>(null);
  const [status, setStatus] = useState<EditorStateView | null>(null);
  const [tree, setTree] = useState<EditorTreeView | null>(null);
  const [diagnostics, setDiagnostics] = useState<EditorDiagnosticsView | null>(null);
  const [completion, setCompletion] = useState<EditorCompletionView | null>(null);
  const [navigation, setNavigation] = useState<EditorNavigationView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  /**
   * Run one host call: mark the editor busy, clear the last failure, and
   * publish the result. A failure is reported, and the previous view stays.
   */
  const run = useCallback(async <T,>(call: () => Promise<T>, apply: (value: T) => void) => {
    setBusy(true);
    setError(null);
    try {
      const value = await call();
      if (!mounted.current) return;
      apply(value);
    } catch (caught) {
      if (!mounted.current) return;
      setError(failureMessage(caught));
    } finally {
      if (mounted.current) setBusy(false);
    }
  }, []);

  /** Re-read the visible window. */
  const refreshViewport = useCallback(
    async (firstLine?: number) => {
      await run(
        () => bridge.documentViewport(firstLine ?? null, null),
        (view) => setViewport(view),
      );
    },
    [run],
  );

  /** Re-read the document's presentation state. */
  const refreshStatus = useCallback(async () => {
    await run(() => bridge.documentState(), (view) => setStatus(view));
  }, [run]);

  /** Open one workspace-relative document. */
  const open = useCallback(
    async (relativePath: string) => {
      if (routeGeneration === null) return;
      await run(
        () => bridge.documentOpen(relativePath, routeGeneration),
        () => undefined,
      );
      await refreshViewport();
      await refreshStatus();
    },
    [refreshStatus, refreshViewport, routeGeneration, run],
  );

  /** Move the caret by one named movement. */
  const moveCursor = useCallback(
    async (movement: string, extend: boolean) => {
      await run(
        () => bridge.documentCursor(movement, extend),
        (view) => setViewport(view),
      );
    },
    [run],
  );

  /** Apply one edit over a byte range. */
  const edit = useCallback(
    async (start: number, end: number, insert: string) => {
      await run(() => bridge.documentEdit(start, end, insert), () => undefined);
      await refreshViewport();
    },
    [refreshViewport, run],
  );

  /** Undo the newest edit. */
  const undo = useCallback(async () => {
    await run(() => bridge.documentUndo(), () => undefined);
    await refreshViewport();
    await refreshStatus();
  }, [refreshStatus, refreshViewport, run]);

  /** Redo the newest undone edit. */
  const redo = useCallback(async () => {
    await run(() => bridge.documentRedo(), () => undefined);
    await refreshViewport();
    await refreshStatus();
  }, [refreshStatus, refreshViewport, run]);

  /** Save the open document. */
  const save = useCallback(async () => {
    await run(() => bridge.documentSave(), (view) => setStatus(view));
    await refreshViewport();
  }, [refreshViewport, run]);

  /** Reload the open document from disk. */
  const reload = useCallback(async () => {
    await run(() => bridge.documentReload(), (view) => setStatus(view));
    await refreshViewport();
  }, [refreshViewport, run]);

  /** Resynchronise the open document with the daemon. */
  const resync = useCallback(async () => {
    await run(() => bridge.documentResync(), (view) => setStatus(view));
    await refreshViewport();
  }, [refreshViewport, run]);

  /** Close the open document. */
  const close = useCallback(async () => {
    await run(() => bridge.documentClose(), (view) => setStatus(view));
    setViewport(null);
    setCompletion(null);
    setNavigation(null);
  }, [run]);

  /** Re-read the workspace file tree. */
  const refreshTree = useCallback(async () => {
    if (routeGeneration === null) return;
    await run(
      () => bridge.documentTree('', routeGeneration),
      (view) => setTree(view),
    );
  }, [routeGeneration, run]);

  /** Re-read the project's diagnostics. */
  const refreshDiagnostics = useCallback(async () => {
    if (routeGeneration === null) return;
    await run(
      () => bridge.documentDiagnostics(routeGeneration),
      (view) => setDiagnostics(view),
    );
  }, [routeGeneration, run]);

  /** Ask for completion candidates at the caret. */
  const complete = useCallback(async () => {
    await run(() => bridge.documentCompletion(null), (view) => setCompletion(view));
  }, [run]);

  /** Ask for navigation targets at the caret. */
  const navigate = useCallback(
    async (operation: string) => {
      await run(
        () => bridge.documentNavigate(operation),
        (view) => setNavigation(view),
      );
    },
    [run],
  );

  /** Scroll the viewport to a first visible line. */
  const scroll = useCallback(
    async (firstLine: number) => {
      await refreshViewport(firstLine);
    },
    [refreshViewport],
  );

  // The route's scope changed: the old document belongs to the old route,
  // so the session starts clean rather than showing another route's file.
  useEffect(() => {
    setViewport(null);
    setStatus(null);
    setCompletion(null);
    setNavigation(null);
    setError(null);
  }, [routeGeneration]);

  // The workspace became available: read the file tree and the diagnostics
  // the editor's affordances show.
  useEffect(() => {
    if (!enabled || routeGeneration === null) return;
    void refreshTree();
    void refreshDiagnostics();
  }, [enabled, refreshDiagnostics, refreshTree, routeGeneration]);

  return {
    effectiveMap,
    viewport,
    status,
    tree,
    diagnostics,
    completion,
    navigation,
    busy,
    error,
    onOpen: (relativePath) => void open(relativePath),
    onMoveCursor: (movement, extend) => void moveCursor(movement, extend),
    onEdit: (start, end, insert) => void edit(start, end, insert),
    onUndo: () => void undo(),
    onRedo: () => void redo(),
    onSave: () => void save(),
    onReload: () => void reload(),
    onResync: () => void resync(),
    onClose: () => void close(),
    onComplete: () => void complete(),
    onNavigate: (operation) => void navigate(operation),
    onScroll: (firstLine) => void scroll(firstLine),
    onRefreshTree: () => void refreshTree(),
  };
}
