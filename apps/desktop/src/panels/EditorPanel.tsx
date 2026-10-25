import { EditorSurface } from '../editor/EditorSurface';
import type { EditorPanelProps } from '../editor/contract';

// The editor panel: the panel that docks the editor.
//
// It renders the props the session gave it and calls nothing. The panel
// layer holds no bridge authority, and the editor's own state lives in the
// session above it (`apps/desktop/src/editor/session.ts`).

export interface EditorPanelOwnProps {
  /** The editor's state and intents. */
  editor: EditorPanelProps;
}

/** The editor panel. */
export function EditorPanel({ editor }: EditorPanelOwnProps) {
  return (
    <div className="editor-panel" data-testid="editor-panel">
      <EditorSurface {...editor} />
    </div>
  );
}
