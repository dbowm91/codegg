import { describe, expect, it } from 'vitest';
import {
  ACTION_DECLARATIONS,
  ALL_ACTION_DECLARATIONS,
  EDITOR_SCOPE,
  RESERVED_CHORDS,
  buildEffectiveMap,
  currentPlatform,
  isMacPlatform,
  normalizeChord,
  resolveAction,
  scopeKey,
} from '../keymap';
import { editorIntent } from './actions';

const MAC = 'MacIntel';
const LINUX = 'Linux';

/** The accelerator the platform's chord map is built with. */
const accelerator = () => (isMacPlatform(currentPlatform()) ? { metaKey: true } : { ctrlKey: true });

describe("the editor's actions in the canonical keymap", () => {
  it('declares every editor action in the one declaration table', () => {
    const editorActions = ALL_ACTION_DECLARATIONS.filter((declaration) =>
      declaration.id.startsWith('editor.'),
    );
    expect(editorActions.length).toBeGreaterThan(0);
    // Every editor action is in ACTION_DECLARATIONS itself, so the bindings
    // surface and the help overlay list them with everything else.
    for (const declaration of editorActions) {
      expect(ACTION_DECLARATIONS.some((entry) => entry.id === declaration.id)).toBe(true);
    }
  });

  it('binds every editor action in the editor scope', () => {
    for (const declaration of ALL_ACTION_DECLARATIONS.filter((entry) =>
      entry.id.startsWith('editor.'),
    )) {
      expect(declaration.defaultBindings.length).toBeGreaterThan(0);
      for (const binding of declaration.defaultBindings) {
        expect(scopeKey(binding.scope)).toBe(scopeKey(EDITOR_SCOPE));
      }
      expect(scopeKey(declaration.overrideScope)).toBe(scopeKey(EDITOR_SCOPE));
    }
  });

  it('claims no chord that the reserved list holds', () => {
    const reserved = new Set(RESERVED_CHORDS.map((entry) => entry.chord));
    for (const declaration of ALL_ACTION_DECLARATIONS) {
      for (const binding of declaration.defaultBindings) {
        expect(reserved.has(binding.chord)).toBe(false);
      }
    }
  });

  it('resolves an editor chord in the editor scope and nowhere else', () => {
    const map = buildEffectiveMap(ALL_ACTION_DECLARATIONS, {}, LINUX);
    expect(resolveAction(map, [EDITOR_SCOPE], 'arrowleft')).toBe('editor.cursor.left');
    expect(resolveAction(map, [EDITOR_SCOPE], 'shift+arrowleft')).toBe('editor.select.left');
    expect(resolveAction(map, [EDITOR_SCOPE], 'backspace')).toBe('editor.deleteBackward');
    expect(resolveAction(map, [EDITOR_SCOPE], 'ctrl+z')).toBe('editor.undo');
    // The same chord in the global scope reaches nothing: the editor's
    // actions never shadow a global one.
    expect(resolveAction(map, [{ kind: 'global' }], 'arrowleft')).toBeNull();
  });

  it('resolves the platform accelerator of the undo chord', () => {
    const mac = buildEffectiveMap(ALL_ACTION_DECLARATIONS, {}, MAC);
    const linux = buildEffectiveMap(ALL_ACTION_DECLARATIONS, {}, LINUX);
    expect(resolveAction(mac, [EDITOR_SCOPE], 'meta+z')).toBe('editor.undo');
    expect(resolveAction(linux, [EDITOR_SCOPE], 'ctrl+z')).toBe('editor.undo');
  });

  it('normalises a key event into the chord the table declares', () => {
    expect(normalizeChord({ key: 'ArrowLeft' })).toBe('arrowleft');
    expect(normalizeChord({ key: 'z', ...accelerator() })).toMatch(/^(meta|ctrl)\+z$/);
  });
});

describe("the editor's actions", () => {
  it('names the caret movement each movement action performs', () => {
    expect(editorIntent('editor.cursor.left')).toEqual({
      kind: 'move',
      movement: 'left',
      extend: false,
    });
    expect(editorIntent('editor.cursor.pageDown')).toEqual({
      kind: 'move',
      movement: 'page_down',
      extend: false,
    });
  });

  it('extends the selection for the selection actions', () => {
    expect(editorIntent('editor.select.down')).toEqual({
      kind: 'move',
      movement: 'down',
      extend: true,
    });
  });

  it('names the edit and the command actions', () => {
    expect(editorIntent('editor.deleteBackward')).toEqual({ kind: 'edit', edit: 'backspace' });
    expect(editorIntent('editor.insertNewline')).toEqual({ kind: 'edit', edit: 'newline' });
    expect(editorIntent('editor.undo')).toEqual({ kind: 'command', command: 'undo' });
    expect(editorIntent('editor.save')).toEqual({ kind: 'command', command: 'save' });
  });

  it('claims no action that is not the editor’s', () => {
    expect(editorIntent('prompt.submit')).toBeNull();
    expect(editorIntent('editor.insert')).toBeNull();
  });

  it('answers every declared editor action', () => {
    for (const declaration of ALL_ACTION_DECLARATIONS.filter((entry) =>
      entry.id.startsWith('editor.'),
    )) {
      expect(editorIntent(declaration.id)).not.toBeNull();
    }
  });
});
