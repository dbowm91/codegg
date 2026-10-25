import { describe, expect, it } from 'vitest';
import {
  ACTION_DECLARATIONS,
  ALL_ACTION_DECLARATIONS,
  GLOBAL_SCOPE,
  KEYMAP_STORAGE_KEY,
  KEYMAP_VERSION,
  RESERVED_CHORDS,
  buildEffectiveMap,
  clearKeymap,
  declarationById,
  exportDocument,
  importDocument,
  loadKeymap,
  modKeyName,
  neutralChord,
  normalizeChord,
  parseChord,
  remapAction,
  resolveChordToken,
  saveKeymap,
  scopeKey,
} from './keymap';
import type { StorageLike } from './keymap';

const MAC = 'MacIntel';
const LINUX = 'Linux x86_64';

function memoryStorage(seed: Record<string, string> = {}) {
  const entries: Record<string, string> = { ...seed };
  const storage: StorageLike & { entries: Record<string, string> } = {
    entries,
    getItem: (key: string) => (key in entries ? entries[key] : null),
    setItem: (key: string, value: string) => {
      entries[key] = value;
    },
    removeItem: (key: string) => {
      delete entries[key];
    },
  };
  return storage;
}

const effective = (overrides: Record<string, string[]> = {}, platform = LINUX) =>
  buildEffectiveMap(ALL_ACTION_DECLARATIONS, overrides, platform);

describe('the chord normaliser', () => {
  it('chord_normalisation_is_stable_and_ordered', () => {
    expect(parseChord('mod+shift+p')).toBe('mod+shift+p');
    expect(parseChord('shift+mod+p')).toBe('mod+shift+p');
    expect(parseChord('ctrl+alt+shift+mod+p')).toBe('mod+ctrl+alt+shift+p');
    expect(parseChord('MOD+SHIFT+P')).toBe('mod+shift+p');
    // An unknown modifier is not a chord at all.
    expect(parseChord('hyper+p')).toBeNull();
    expect(parseChord('')).toBeNull();
  });

  it('mod_resolves_to_meta_on_macos_and_ctrl_elsewhere', () => {
    expect(modKeyName(MAC)).toBe('meta');
    expect(modKeyName(LINUX)).toBe('ctrl');
    expect(resolveChordToken('mod+p', MAC)).toBe('meta+p');
    expect(resolveChordToken('mod+p', LINUX)).toBe('ctrl+p');
    // The same declaration therefore serves both platforms.
    const mac = buildEffectiveMap(ALL_ACTION_DECLARATIONS, {}, MAC);
    const linux = buildEffectiveMap(ALL_ACTION_DECLARATIONS, {}, LINUX);
    expect(mac.byScope.get(scopeKey(GLOBAL_SCOPE))?.get('meta+p')).toBe('project.switcher.open');
    expect(linux.byScope.get(scopeKey(GLOBAL_SCOPE))?.get('ctrl+p')).toBe('project.switcher.open');
  });

  it('chord_normalisation_distinguishes_shift_enter_from_enter', () => {
    expect(normalizeChord({ key: 'Enter' })).toBe('enter');
    expect(normalizeChord({ key: 'Enter', shiftKey: true })).toBe('shift+enter');
    expect(normalizeChord({ key: 'Escape' })).toBe('escape');
    expect(normalizeChord({ key: 'Enter' })).not.toBe(normalizeChord({ key: 'Escape' }));
  });

  it('unknown_key_event_normalises_to_an_unmatched_chord', () => {
    const chord = normalizeChord({ key: 'F13' });
    expect(chord).toBe('f13');
    // The chord normalises, and no action claims it.
    const map = effective();
    const claimed = [...(map.byScope.values() ?? [])].some((scope) => scope.has(chord));
    expect(claimed).toBe(false);
  });
});

describe('the remap path', () => {
  it('remap_onto_an_occupied_chord_is_refused', () => {
    const before = effective();
    const result = remapAction({}, ALL_ACTION_DECLARATIONS, 'help.open', 'mod+p', LINUX);
    expect(result.refusal).toBe('mod+p is already bound to Open the project switcher');
    // A refused remap leaves the keymap byte-identical.
    expect(result.overrides).toEqual({});
    const after = effective();
    expect(after.byScope.get(scopeKey(GLOBAL_SCOPE))?.get('ctrl+p')).toBe(
      before.byScope.get(scopeKey(GLOBAL_SCOPE))?.get('ctrl+p'),
    );
  });

  it('remap_replaces_the_actions_previous_binding', () => {
    const result = remapAction({}, ALL_ACTION_DECLARATIONS, 'prompt.submit', 'mod+j', LINUX);
    expect(result.refusal).toBeNull();
    const map = effective(result.overrides);
    const bindings = map.bindings.get('prompt.submit') ?? [];
    // Exactly one binding: the remap replaces the action's defaults
    // entirely, the TUI's single-binding rule.
    expect(bindings).toHaveLength(1);
    expect(bindings[0].chord).toBe('mod+j');
    // The composer's own `enter` is gone with the defaults it replaced.
    expect(map.bindings.get('prompt.submit')?.some((binding) => binding.chord === 'enter')).toBe(false);
  });
});

describe('the reset path', () => {
  it('reset_to_defaults_restores_the_declaration_table', () => {
    const remapped = remapAction({}, ALL_ACTION_DECLARATIONS, 'prompt.submit', 'mod+j', LINUX);
    const storage = memoryStorage();
    saveKeymap(storage, remapped.overrides);
    expect(loadKeymap(storage, ALL_ACTION_DECLARATIONS, LINUX).overrides).toEqual(remapped.overrides);

    clearKeymap(storage);
    expect(storage.getItem(KEYMAP_STORAGE_KEY)).toBeNull();

    const defaults = effective();
    const bindings = defaults.bindings.get('prompt.submit') ?? [];
    expect(bindings.map((binding) => binding.chord)).toEqual(['mod+enter', 'enter']);
  });
});

describe('the persistence adapter', () => {
  it('overrides_survive_a_renderer_reload', () => {
    const storage = memoryStorage();
    const remapped = remapAction({}, ALL_ACTION_DECLARATIONS, 'tasks.toggle', 'mod+g', LINUX);
    saveKeymap(storage, remapped.overrides);

    // A reload: a fresh load of the same storage.
    const loaded = loadKeymap(storage, ALL_ACTION_DECLARATIONS, LINUX);
    expect(loaded.overrides).toEqual({ 'tasks.toggle': ['mod+g'] });
    expect(loaded.notices).toEqual([]);
    const map = buildEffectiveMap(ALL_ACTION_DECLARATIONS, loaded.overrides, LINUX);
    expect(map.bindings.get('tasks.toggle')?.[0].chord).toBe('mod+g');
  });

  it('malformed_stored_keymap_falls_back_to_defaults_with_a_notice', () => {
    const storage = memoryStorage({ [KEYMAP_STORAGE_KEY]: '{not json' });
    const loaded = loadKeymap(storage, ALL_ACTION_DECLARATIONS, LINUX);
    expect(loaded.overrides).toEqual({});
    expect(loaded.notices).toHaveLength(1);
    expect(loaded.notices[0].kind).toBe('malformed');
    // The defaults are in effect.
    const map = buildEffectiveMap(ALL_ACTION_DECLARATIONS, loaded.overrides, LINUX);
    expect(map.bindings.get('prompt.submit')?.[0].chord).toBe('mod+enter');
  });

  it('unknown_action_id_in_a_stored_keymap_is_dropped_individually', () => {
    const storage = memoryStorage({
      [KEYMAP_STORAGE_KEY]: JSON.stringify({
        version: KEYMAP_VERSION,
        bindings: { 'tasks.toggle': ['mod+g'], 'not.an.action': ['mod+q'] },
      }),
    });
    const loaded = loadKeymap(storage, ALL_ACTION_DECLARATIONS, LINUX);
    // The other stored override still applies.
    expect(loaded.overrides).toEqual({ 'tasks.toggle': ['mod+g'] });
    expect(loaded.notices.some((notice) => notice.kind === 'unknown-action')).toBe(true);
  });

  it('stored_keymap_with_a_wrong_version_is_ignored_wholesale', () => {
    const storage = memoryStorage({
      [KEYMAP_STORAGE_KEY]: JSON.stringify({
        version: KEYMAP_VERSION + 1,
        bindings: { 'tasks.toggle': ['mod+g'] },
      }),
    });
    const loaded = loadKeymap(storage, ALL_ACTION_DECLARATIONS, LINUX);
    expect(loaded.overrides).toEqual({});
    expect(loaded.notices).toHaveLength(1);
    expect(loaded.notices[0].kind).toBe('version');
  });

  it('conflicting_stored_keymap_resolves_deterministically', () => {
    const storage = memoryStorage({
      [KEYMAP_STORAGE_KEY]: JSON.stringify({
        version: KEYMAP_VERSION,
        bindings: { 'session.switcher.open': ['mod+j'], 'help.open': ['mod+j'] },
      }),
    });
    const loaded = loadKeymap(storage, ALL_ACTION_DECLARATIONS, LINUX);
    const map = buildEffectiveMap(ALL_ACTION_DECLARATIONS, loaded.overrides, LINUX);
    const global = map.byScope.get(scopeKey(GLOBAL_SCOPE));
    // The later-declared action keeps the chord.
    expect(global?.get('ctrl+j')).toBe('help.open');
    // The earlier falls back to its default binding.
    expect(map.bindings.get('session.switcher.open')?.[0].chord).toBe('mod+shift+s');
    // A notice names the conflict and both actions.
    const conflict = loaded.notices.find((notice) => notice.kind === 'conflict');
    expect(conflict?.message).toContain('mod+j');
    expect(conflict?.message).toContain('Open the keyboard help');
    expect(conflict?.message).toContain('Open the session switcher');
  });
});

describe('the document shape', () => {
  it('import_of_a_malformed_document_is_rejected_atomically', () => {
    const result = importDocument('{not json', ALL_ACTION_DECLARATIONS, LINUX);
    expect(result.error).toContain('not valid JSON');
    // Nothing is applied: the import is atomic.
    expect(result.overrides).toEqual({});

    const wrongShape = importDocument(JSON.stringify({ version: KEYMAP_VERSION }), ALL_ACTION_DECLARATIONS, LINUX);
    expect(wrongShape.error).toBe('the document carries no bindings');
    expect(wrongShape.overrides).toEqual({});

    const conflicting = importDocument(
      JSON.stringify({
        version: KEYMAP_VERSION,
        bindings: { 'session.switcher.open': ['mod+j'], 'help.open': ['mod+j'] },
      }),
      ALL_ACTION_DECLARATIONS,
      LINUX,
    );
    expect(conflicting.error).toContain('mod+j');
    expect(conflicting.overrides).toEqual({});
  });

  it('export_document_shape_is_the_documented_shape', () => {
    const text = exportDocument({ 'tasks.toggle': ['mod+g'] });
    const parsed = JSON.parse(text);
    expect(parsed).toEqual({ version: 1, bindings: { 'tasks.toggle': ['mod+g'] } });
    expect(Object.keys(parsed)).toEqual(['version', 'bindings']);

    // The document round-trips through the import path.
    const imported = importDocument(text, ALL_ACTION_DECLARATIONS, LINUX);
    expect(imported.error).toBeNull();
    expect(imported.overrides).toEqual({ 'tasks.toggle': ['mod+g'] });
  });
});

describe('the reserved chords', () => {
  it('no_bound_chord_appears_in_the_reserved_list', () => {
    const reserved = new Set(RESERVED_CHORDS.map((entry) => entry.chord));
    const bound = ALL_ACTION_DECLARATIONS.flatMap((declaration) =>
      declaration.defaultBindings.map((binding) => binding.chord),
    );
    for (const chord of bound) {
      expect(reserved.has(chord)).toBe(false);
    }
    // The catalog's own actions are the ones the plan names.
    expect(declarationById('help.open')?.defaultBindings[0].chord).toBe('mod+/');
    expect(ACTION_DECLARATIONS.every((declaration) => declaration.id !== '')).toBe(true);
  });
});

describe('the platform-neutral chord', () => {
  it('neutral_chord_writes_the_platform_accelerator_as_mod', () => {
    expect(neutralChord({ key: 'p', metaKey: true }, MAC)).toBe('mod+p');
    expect(neutralChord({ key: 'p', ctrlKey: true }, LINUX)).toBe('mod+p');
    // A chord that is not the platform's accelerator keeps its own name.
    expect(neutralChord({ key: 'p', ctrlKey: true }, MAC)).toBe('ctrl+p');
  });
});
