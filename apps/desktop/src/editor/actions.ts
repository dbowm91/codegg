import type { EffectiveMap } from '../keymap';

// What each editor action does.
//
// The chords live in the canonical keymap table
// (`apps/desktop/src/keymap.ts`), under the editor's scope, and they are
// resolved by the keymap's own resolver. This module is the editor's half of
// that split: the keymap says which chord reaches which **action**, and this
// module says what that action *does* — the same way the composition root
// says what each of its own actions does.
//
// It is keyed by **action id**, never by chord. There is no second chord
// table in the editor: a chord that the editor answers to is a chord the
// canonical table declares.

/** The caret movement an action performs. */
export type EditorMovement =
  | 'left'
  | 'right'
  | 'up'
  | 'down'
  | 'home'
  | 'end'
  | 'page_up'
  | 'page_down';

/** The edit an action performs. */
export type EditorEditKind = 'backspace' | 'delete' | 'newline';

/** The command an action performs. */
export type EditorCommand = 'undo' | 'redo' | 'save';

/** What the editor does for one action. */
export type EditorIntent =
  | { kind: 'move'; movement: EditorMovement; extend: boolean }
  | { kind: 'edit'; edit: EditorEditKind }
  | { kind: 'command'; command: EditorCommand };

/**
 * The intent an action id stands for, or null when the action is not the
 * editor's.
 *
 * The movement an action names is the caret movement the host serves, so
 * the editor never computes a byte offset of its own.
 */
export function editorIntent(actionId: string): EditorIntent | null {
  switch (actionId) {
    case 'editor.cursor.left':
      return { kind: 'move', movement: 'left', extend: false };
    case 'editor.cursor.right':
      return { kind: 'move', movement: 'right', extend: false };
    case 'editor.cursor.up':
      return { kind: 'move', movement: 'up', extend: false };
    case 'editor.cursor.down':
      return { kind: 'move', movement: 'down', extend: false };
    case 'editor.cursor.lineStart':
      return { kind: 'move', movement: 'home', extend: false };
    case 'editor.cursor.lineEnd':
      return { kind: 'move', movement: 'end', extend: false };
    case 'editor.cursor.pageUp':
      return { kind: 'move', movement: 'page_up', extend: false };
    case 'editor.cursor.pageDown':
      return { kind: 'move', movement: 'page_down', extend: false };
    case 'editor.select.left':
      return { kind: 'move', movement: 'left', extend: true };
    case 'editor.select.right':
      return { kind: 'move', movement: 'right', extend: true };
    case 'editor.select.up':
      return { kind: 'move', movement: 'up', extend: true };
    case 'editor.select.down':
      return { kind: 'move', movement: 'down', extend: true };
    case 'editor.select.lineStart':
      return { kind: 'move', movement: 'home', extend: true };
    case 'editor.select.lineEnd':
      return { kind: 'move', movement: 'end', extend: true };
    case 'editor.select.pageUp':
      return { kind: 'move', movement: 'page_up', extend: true };
    case 'editor.select.pageDown':
      return { kind: 'move', movement: 'page_down', extend: true };
    case 'editor.deleteBackward':
      return { kind: 'edit', edit: 'backspace' };
    case 'editor.deleteForward':
      return { kind: 'edit', edit: 'delete' };
    case 'editor.insertNewline':
      return { kind: 'edit', edit: 'newline' };
    case 'editor.undo':
      return { kind: 'command', command: 'undo' };
    case 'editor.redo':
      return { kind: 'command', command: 'redo' };
    case 'editor.save':
      return { kind: 'command', command: 'save' };
    default:
      return null;
  }
}

/**
 * The effective map the editor resolves its chords against.
 *
 * The composition root owns the map — it is where the keymap is loaded and
 * where a remap lands — so the editor is handed the map rather than loading
 * a second copy of the keymap.
 */
export type EditorKeymap = EffectiveMap;
