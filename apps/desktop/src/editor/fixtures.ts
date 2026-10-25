import { ALL_ACTION_DECLARATIONS, buildEffectiveMap } from '../keymap';
import type { EditorPanelProps } from './contract';

// The editor's empty props.
//
// The editor panel renders these when nothing is open: no document, no
// tree, no diagnostics, and every intent a no-op. Tests use them so a
// fixture does not have to spell the whole contract out.

/** The editor props a test renders with: no document, nothing in flight. */
export function emptyEditorPanelProps(): EditorPanelProps {
  return {
    effectiveMap: buildEffectiveMap(ALL_ACTION_DECLARATIONS),
    viewport: null,
    status: null,
    tree: null,
    diagnostics: null,
    completion: null,
    navigation: null,
    busy: false,
    error: null,
    onOpen: () => undefined,
    onMoveCursor: () => undefined,
    onEdit: () => undefined,
    onUndo: () => undefined,
    onRedo: () => undefined,
    onSave: () => undefined,
    onReload: () => undefined,
    onResync: () => undefined,
    onClose: () => undefined,
    onComplete: () => undefined,
    onNavigate: () => undefined,
    onScroll: () => undefined,
    onRefreshTree: () => undefined,
  };
}
