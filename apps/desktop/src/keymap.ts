/*
 * The desktop renderer's keymap (M007-C).
 *
 * ── WP-1: the boundary this module sits on ─────────────────────────────
 *
 * The TUI's `ActionKey` (`src/tui/input.rs:259`, 61 variants) is the
 * **source vocabulary** for the catalog below, and
 * `plans/000-long-term-specification.md:564` — "Exact keys remain
 * configurable. The conceptual hierarchy and read-only observation
 * behavior are normative." — is the licence for carrying the TUI's
 * *concepts* rather than its bytes. The TUI's keymap is never read,
 * never written and never mirrored here: the desktop's defaults are the
 * desktop's own, and the TUI's behaviour is unchanged by this milestone
 * (I4).
 *
 * `ActionKey::all()` (`src/tui/input.rs:349-407`, 56 actions) is the
 * *configurable* subset of the TUI's catalog. The five variants it
 * excludes — `OpenPanelManager`, `DirectionLeft`, `DirectionRight`,
 * `DirectionUp` and `DirectionDown` — are router and motion concepts
 * rather than remappable actions; the desktop maps them onto its own
 * regions and its own `Tab` traversal.
 *
 * ── The modality decision ──────────────────────────────────────────────
 *
 * The desktop is **direct manipulation**, not modal (§1.1 of the
 * milestone plan): there is no Insert/Normal mode, every declared chord
 * acts immediately, and a plain letter key is always text. A Helix-style
 * modal layer is a recorded deferral with its own decision, not this
 * milestone's to take.
 *
 * ── What this module owns ──────────────────────────────────────────────
 *
 * The declaration table (the single place an action is named), the chord
 * normaliser, the platform resolver, the effective-map builder, the
 * conflict check, the reset path, the export/import document shape and
 * the `localStorage` persistence adapter.
 *
 * The persistence is renderer-local by design: the keymap is presentation
 * state, not session state, and the renderer holds no durable state and
 * no daemon authority (I1). One user's TUI remap must not change the
 * desktop, and reading the TUI's config file from the renderer would
 * need a filesystem capability the boundary forbids (§6.2).
 */

/** The renderer-local storage namespace of the keymap document. */
export const KEYMAP_STORAGE_KEY = 'codegg.desktop.keymap.v1';

/** The document version this build reads and writes. */
export const KEYMAP_VERSION = 1;

// ── The vocabulary ───────────────────────────────────────────────────────

/**
 * The affordance class of an action.
 *
 *   - `button` — an activatable control; the mouse route is a click. A
 *     checkbox is one of these: the plan-mode toggle is a control the
 *     user clicks.
 *   - `region` — a region container; the mouse route is a click on the
 *     region, which focuses it.
 *   - `editing` — an editable element; the mouse route is to click into
 *     it and use the element's own editing affordance.
 *   - `surface` — a custom editing surface (M007-E): an element that owns
 *     a caret and is focused by a click, but holds **no text of its own**.
 *     The editor's viewport is one. It is not an `editing` element, and it
 *     cannot be one: a native text field would keep a second copy of the
 *     text, which the editor's text authority forbids. The mouse route is
 *     a click into the surface, and the action's chord applies there.
 */
export type AffordanceClass = 'button' | 'region' | 'editing' | 'surface';

/**
 * The mouse route of an action.
 *
 * The affordance names the `data-testid` **prefix** of the elements that
 * carry the route, because a list is a family of elements: every project
 * row carries `project-select-<id>`, and every overlay carries
 * `overlay-close-button`.
 */
export interface Affordance {
  /** The affordance class. */
  class: AffordanceClass;
  /** The `data-testid` prefixes that identify the affordance's elements. */
  testIdPrefixes: string[];
  /**
   * Whether the affordance names a family of elements — at least one
   * member must exist — rather than exactly one.
   */
  family?: boolean;
}

/** The scope that owns a chord. */
export interface Scope {
  /** The kind of scope. */
  kind: 'global' | 'region' | 'overlay';
  /** The region id, when the scope is a region. */
  region?: string;
}

/** A chord and the scope that owns it. */
export interface Binding {
  /** The chord, platform-neutral (`mod+shift+p`). */
  chord: string;
  /** The scope that owns the chord. */
  scope: Scope;
}

/** One declared action: the desktop's catalog entry. */
export interface ActionDeclaration {
  /** The action's identity — the key the keymap and the bindings surface name it by. */
  id: string;
  /** The action's UI label. */
  label: string;
  /** What the action does, in one line. */
  description: string;
  /** The chords the action answers to by default. */
  defaultBindings: Binding[];
  /**
   * The scope a remap of this action lands in.
   *
   * A remap replaces the action's defaults entirely (the TUI's
   * single-binding rule, `src/tui/components/dialogs/keybind.rs:197-207`),
   * so the replacement needs one scope of its own.
   */
  overrideScope: Scope;
  /** The mouse route of the action. */
  affordance: Affordance;
  /** The TUI concept the action carries, when it carries one. */
  tuiConcept?: string;
  /**
   * Whether the chord shadows a WebView or OS accelerator and is
   * therefore captured with an explicit `preventDefault()`.
   *
   * A chord that is *not* captured must not call `preventDefault()`, so
   * ordinary typing and native editing stay untouched (I6).
   */
  captured: boolean;
}

/** The scope every chord belongs to when nothing more specific owns it. */
export const GLOBAL_SCOPE: Scope = { kind: 'global' };

/** The scope the open overlay stack owns. */
export const OVERLAY_SCOPE: Scope = { kind: 'overlay' };

/** The scope of one region. */
export function regionScope(regionId: string): Scope {
  return { kind: 'region', region: regionId };
}

/**
 * The scope the editor's own chords belong to (M007-E).
 *
 * The editor is a frontend-local surface, so its bindings are **actions**
 * like any other, and they live in this table with the rest of the catalog.
 * The scope is the editor's viewport — the editing surface that owns the
 * caret — so an editor chord reaches the editor only while the editor has
 * the focus, and never shadows a global chord.
 */
export const EDITOR_SCOPE: Scope = regionScope('editor');

/** The key a scope is stored under. */
export function scopeKey(scope: Scope): string {
  if (scope.kind === 'region') return `region:${scope.region ?? ''}`;
  if (scope.kind === 'overlay') return 'overlay';
  return 'global';
}

// ── The declaration table ────────────────────────────────────────────────

/**
 * The desktop's action catalog.
 *
 * The order is the declaration order: it decides which action keeps a
 * chord when two of them claim one (§8), and it is the order the bindings
 * surface lists.
 */
export const ACTION_DECLARATIONS: readonly ActionDeclaration[] = [
  {
    id: 'prompt.submit',
    label: 'Submit prompt',
    description: "Send the composer's draft to the attached session.",
    defaultBindings: [
      { chord: 'mod+enter', scope: GLOBAL_SCOPE },
      { chord: 'enter', scope: regionScope('composer') },
    ],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'button', testIdPrefixes: ['prompt-submit-button'] },
    tuiConcept: 'ActionKey::Send (Enter)',
    captured: true,
  },
  {
    id: 'prompt.newline',
    label: 'Insert a newline',
    description: 'Insert a line break in the composer without sending the draft.',
    defaultBindings: [{ chord: 'shift+enter', scope: regionScope('composer') }],
    overrideScope: regionScope('composer'),
    affordance: { class: 'editing', testIdPrefixes: ['prompt-input'] },
    tuiConcept: 'ActionKey::Newline (Shift+Enter)',
    captured: true,
  },
  {
    id: 'overlay.cancel',
    label: 'Cancel the current overlay',
    description: 'Close the topmost overlay and restore the focus it interrupted.',
    defaultBindings: [{ chord: 'escape', scope: OVERLAY_SCOPE }],
    overrideScope: OVERLAY_SCOPE,
    affordance: { class: 'button', testIdPrefixes: ['overlay-close-button'], family: true },
    tuiConcept: 'ActionKey::Cancel (Esc)',
    captured: true,
  },
  {
    id: 'project.switcher.open',
    label: 'Open the project switcher',
    description: 'Move the focus to the project list so a project can be chosen.',
    defaultBindings: [{ chord: 'mod+p', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'button', testIdPrefixes: ['project-switcher-button'] },
    tuiConcept: 'ActionKey::OpenProjectPicker / Space f',
    captured: true,
  },
  {
    id: 'session.switcher.open',
    label: 'Open the session switcher',
    description: 'Move the focus to the session list of the routed project.',
    defaultBindings: [{ chord: 'mod+shift+s', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'button', testIdPrefixes: ['session-switcher-button'] },
    tuiConcept: 'Space s — project session picker',
    captured: true,
  },
  {
    id: 'item.activate',
    label: 'Activate the focused row',
    description: 'Open the project or session the focus is on.',
    defaultBindings: [{ chord: 'enter', scope: regionScope('catalog') }],
    overrideScope: regionScope('catalog'),
    affordance: {
      class: 'button',
      testIdPrefixes: ['project-select-', 'session-open-'],
      family: true,
    },
    tuiConcept: 'Enter — open/focus project tab',
    captured: true,
  },
  {
    id: 'tasks.toggle',
    label: 'Toggle the tasks panel',
    description: 'Show the tasks panel, or hide it when it is already shown.',
    defaultBindings: [{ chord: 'mod+t', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'button', testIdPrefixes: ['tasks-toggle-button'] },
    tuiConcept: 'ActionKey::ToggleTasksList / Space j / Space a',
    captured: false,
  },
  {
    id: 'sidepanel.toggle',
    label: 'Toggle the side panel',
    description: 'Show or hide the side panel on the selected edge.',
    defaultBindings: [{ chord: 'mod+b', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'button', testIdPrefixes: ['sidepanel-toggle-button'] },
    tuiConcept: 'ActionKey::ToggleSidebar (Ctrl+B)',
    captured: false,
  },
  {
    id: 'sidepanel.focus',
    label: 'Focus the side panel',
    description: 'Move the focus into the docked side panel.',
    defaultBindings: [{ chord: 'mod+alt+s', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'button', testIdPrefixes: ['sidepanel-focus-button'] },
    tuiConcept: 'ActionKey::FocusSidebar (Ctrl+Alt+S)',
    captured: false,
  },
  {
    id: 'prompt.planMode.toggle',
    label: 'Toggle plan mode',
    description: 'Turn the plan mode of the next prompt on or off.',
    defaultBindings: [{ chord: 'mod+alt+p', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'button', testIdPrefixes: ['plan-mode-checkbox'] },
    tuiConcept: 'ActionKey::TogglePermissionMode (Shift+Tab)',
    captured: false,
  },
  {
    id: 'help.open',
    label: 'Open the keyboard help',
    description: 'Show the actions and the chords that reach them.',
    defaultBindings: [{ chord: 'mod+/', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'button', testIdPrefixes: ['help-button'] },
    tuiConcept: 'ActionKey::Help (Space / ?)',
    captured: false,
  },
  {
    id: 'session.create',
    label: 'Create a session',
    description: 'Create a session in the routed workspace.',
    defaultBindings: [{ chord: 'mod+n', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'button', testIdPrefixes: ['session-create-button'] },
    tuiConcept: 'ActionKey::NewSession',
    captured: true,
  },
  {
    id: 'keybinds.open',
    label: 'Open the key bindings',
    description: 'Show the key bindings surface, where the chords are changed.',
    defaultBindings: [{ chord: 'mod+k', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'button', testIdPrefixes: ['keybinds-open-button'] },
    captured: true,
  },
  {
    id: 'connection.reconnect',
    label: 'Reconnect',
    description: 'Reconnect to the daemon.',
    defaultBindings: [{ chord: 'mod+alt+r', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'button', testIdPrefixes: ['reconnect-button'] },
    captured: false,
  },
  // The editor's own actions (M007-E).
  //
  // The editor is a frontend-local surface, so its bindings are actions like
  // any other: they live in this table, under the editor's scope, and they
  // are remappable and conflict-checked with everything else. The editor
  // holds no chord table of its own.
  //
  // A movement and its selection-extending variant are two actions, not one
  // action with two chords: the chords are alternatives for the *same*
  // action, and moving the caret and extending the selection are different
  // actions with different outcomes.
  {
    id: 'editor.cursor.left',
    label: 'Move the caret left',
    description: 'Move the caret one character to the left.',
    defaultBindings: [{ chord: 'arrowleft', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.cursor.right',
    label: 'Move the caret right',
    description: 'Move the caret one character to the right.',
    defaultBindings: [{ chord: 'arrowright', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.cursor.up',
    label: 'Move the caret up',
    description: 'Move the caret one line up.',
    defaultBindings: [{ chord: 'arrowup', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.cursor.down',
    label: 'Move the caret down',
    description: 'Move the caret one line down.',
    defaultBindings: [{ chord: 'arrowdown', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.cursor.lineStart',
    label: 'Move the caret to the line start',
    description: 'Move the caret to the start of the line it is on.',
    defaultBindings: [{ chord: 'home', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.cursor.lineEnd',
    label: 'Move the caret to the line end',
    description: 'Move the caret to the end of the line it is on.',
    defaultBindings: [{ chord: 'end', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.cursor.pageUp',
    label: 'Move the caret up one page',
    description: 'Move the caret up by one viewport of lines.',
    defaultBindings: [{ chord: 'pageup', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.cursor.pageDown',
    label: 'Move the caret down one page',
    description: 'Move the caret down by one viewport of lines.',
    defaultBindings: [{ chord: 'pagedown', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.select.left',
    label: 'Extend the selection left',
    description: 'Extend the selection one character to the left.',
    defaultBindings: [{ chord: 'shift+arrowleft', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.select.right',
    label: 'Extend the selection right',
    description: 'Extend the selection one character to the right.',
    defaultBindings: [{ chord: 'shift+arrowright', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.select.up',
    label: 'Extend the selection up',
    description: 'Extend the selection one line up.',
    defaultBindings: [{ chord: 'shift+arrowup', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.select.down',
    label: 'Extend the selection down',
    description: 'Extend the selection one line down.',
    defaultBindings: [{ chord: 'shift+arrowdown', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.select.lineStart',
    label: 'Extend the selection to the line start',
    description: 'Extend the selection to the start of the caret’s line.',
    defaultBindings: [{ chord: 'shift+home', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.select.lineEnd',
    label: 'Extend the selection to the line end',
    description: 'Extend the selection to the end of the caret’s line.',
    defaultBindings: [{ chord: 'shift+end', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.select.pageUp',
    label: 'Extend the selection up one page',
    description: 'Extend the selection up by one viewport of lines.',
    defaultBindings: [{ chord: 'shift+pageup', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.select.pageDown',
    label: 'Extend the selection down one page',
    description: 'Extend the selection down by one viewport of lines.',
    defaultBindings: [{ chord: 'shift+pagedown', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.deleteBackward',
    label: 'Delete backwards',
    description: 'Delete the character before the caret, or the selection.',
    defaultBindings: [{ chord: 'backspace', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.deleteForward',
    label: 'Delete forwards',
    description: 'Delete the character after the caret, or the selection.',
    defaultBindings: [{ chord: 'delete', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.insertNewline',
    label: 'Insert a newline',
    description: 'Insert a line break at the caret.',
    defaultBindings: [{ chord: 'enter', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.undo',
    label: 'Undo the last edit',
    description: 'Undo the newest edit the editor made.',
    defaultBindings: [{ chord: 'mod+z', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.redo',
    label: 'Redo the last undone edit',
    description: 'Redo the newest edit the editor undid.',
    defaultBindings: [
      { chord: 'mod+shift+z', scope: EDITOR_SCOPE },
      { chord: 'mod+y', scope: EDITOR_SCOPE },
    ],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
  {
    id: 'editor.save',
    label: 'Save the document',
    description: 'Save the open document through the daemon.',
    defaultBindings: [{ chord: 'mod+s', scope: EDITOR_SCOPE }],
    overrideScope: EDITOR_SCOPE,
    affordance: { class: 'surface', testIdPrefixes: ['editor-viewport'] },
    captured: true,
  },
];

/**
 * The editor's printable insert is **not** a chord, and so it is not an
 * action (M007-E).
 *
 * An action is a frontend-local **binding**, and a binding is a chord. A
 * printable character is not a chord — it is the whole class of characters a
 * keyboard can produce, and there is no single token to declare. The insert
 * is therefore the editor viewport's own *editing affordance*, the same
 * class of affordance the composer's draft carries: the mouse route is to
 * click into the viewport and type.
 *
 * It is recorded here rather than left implicit, because the distinction is
 * load-bearing: the canonical vocabulary separates an **action** (a
 * frontend-local binding) from a **command** (an entry in the daemon-owned
 * catalog), and an insert that is not a chord is neither.
 */
export const EDITOR_INSERT_AFFORDANCE: Affordance = {
  class: 'editing',
  testIdPrefixes: ['editor-viewport'],
};

/**
 * The region-focus actions.
 *
 * The family is one action per **declared** region: the desktop declares
 * five, so there are five actions and five chords. The action's affordance
 * is the region's own container, which is also the mouse route: a click
 * on the region focuses it.
 */
const REGION_FOCUS_ACTIONS: readonly ActionDeclaration[] = [
  {
    id: 'region.focus.1',
    label: 'Focus the catalog',
    description: 'Move the focus to the catalog rail.',
    defaultBindings: [{ chord: 'mod+1', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'region', testIdPrefixes: ['catalog-region'] },
    captured: true,
  },
  {
    id: 'region.focus.2',
    label: 'Focus the conversation',
    description: 'Move the focus to the conversation.',
    defaultBindings: [{ chord: 'mod+2', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'region', testIdPrefixes: ['chat-region'] },
    captured: true,
  },
  {
    id: 'region.focus.3',
    label: 'Focus the side panel',
    description: 'Move the focus to the docked side panel.',
    defaultBindings: [{ chord: 'mod+3', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'region', testIdPrefixes: ['side-panel-frame'] },
    captured: true,
  },
  {
    id: 'region.focus.4',
    label: 'Focus the composer',
    description: 'Move the focus to the composer.',
    defaultBindings: [{ chord: 'mod+4', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'region', testIdPrefixes: ['composer-region'] },
    captured: true,
  },
  {
    id: 'region.focus.5',
    label: 'Focus the status row',
    description: 'Move the focus to the status row.',
    defaultBindings: [{ chord: 'mod+5', scope: GLOBAL_SCOPE }],
    overrideScope: GLOBAL_SCOPE,
    affordance: { class: 'region', testIdPrefixes: ['status-row'] },
    captured: true,
  },
];

/** Every declared action, in declaration order. */
export const ALL_ACTION_DECLARATIONS: readonly ActionDeclaration[] = [
  ...ACTION_DECLARATIONS,
  ...REGION_FOCUS_ACTIONS,
];

/** Every declared action id, in declaration order. */
export const ACTION_IDS: readonly string[] = ALL_ACTION_DECLARATIONS.map(
  (declaration) => declaration.id,
);

/** The declaration of an action id. */
export function declarationById(actionId: string): ActionDeclaration | undefined {
  return ALL_ACTION_DECLARATIONS.find((declaration) => declaration.id === actionId);
}

// ── The concept-parity table ─────────────────────────────────────────────

/**
 * One canonical concept and the desktop action that carries it.
 *
 * The canonical hierarchy is `plans/000-long-term-specification.md:550-562`.
 * A concept with no desktop surface is recorded as *not carried*, with
 * the reason: it is not silently dropped.
 */
export interface ConceptParityRow {
  /** The canonical concept, as the long-term specification names it. */
  concept: string;
  /** The desktop action that carries it, or null when it is not carried. */
  actionId: string | null;
  /** Why the concept is not carried, when it is not. */
  notCarriedReason?: string;
}

/** The canonical concepts and the desktop actions that carry them. */
export const CONCEPT_PARITY: readonly ConceptParityRow[] = [
  {
    concept: 'Space f — project picker',
    actionId: 'project.switcher.open',
  },
  {
    concept: 'Enter — open/focus project tab',
    actionId: 'item.activate',
  },
  {
    concept: 'gt / gT — next/previous project tab',
    actionId: null,
    notCarriedReason:
      "the desktop has no project-tab set; M007-B's side panel is one region, so there is no tab to move between.",
  },
  {
    concept: 'Space s — project session picker',
    actionId: 'session.switcher.open',
  },
  {
    concept: 'Space p — collaborators/presence',
    actionId: null,
    notCarriedReason:
      "the desktop's projection DTO carries no presence data, so there is nothing to show.",
  },
  {
    concept: 'Space o — observe the selected session',
    actionId: null,
    notCarriedReason:
      "the desktop has no observe workflow; the observer mode is the TUI's, and this milestone does not port it.",
  },
  {
    concept: 'Space c — focus the project chat',
    actionId: null,
    notCarriedReason: 'the desktop has no project-chat surface.',
  },
  {
    concept: 'Space j — project jobs',
    actionId: 'tasks.toggle',
  },
  {
    concept: 'Space a — agent tree',
    actionId: 'tasks.toggle',
  },
];

/** The TUI actions the desktop deliberately does not carry, with the reason. */
export const NOT_CARRIED_TUI_ACTIONS: readonly { action: string; reason: string }[] = [
  { action: 'CopyMessage', reason: 'no clipboard surface; the clipboard permission is forbidden by the boundary guard.' },
  { action: 'Search / SearchNext / SearchPrev / ClearSearch', reason: 'no in-app find surface in M007-C.' },
  { action: 'Quit / ForceQuit', reason: 'the native window close owns the exit; a chord would shadow the OS quit accelerator.' },
  { action: 'StashPrompt / RestorePrompt / ExternalEditor', reason: 'no desktop counterpart for a stashed prompt or an external editor.' },
  { action: 'ExpandAllDisclosures / ToggleSection', reason: 'the desktop has no disclosure sections to expand.' },
  { action: 'GoToTop / GoToBottom', reason: 'scrolling commands belong to the editor surface (M007-E).' },
  { action: 'Left / Right / Home / End / PageUp / PageDown / Backspace / Delete', reason: 'editor-surface concerns (M007-E).' },
  { action: 'ToggleComposerMode', reason: 'a modal concept with no analogue under direct manipulation.' },
  { action: 'Direction* / OpenPanelManager', reason: 'router and motion concepts the desktop maps onto its own regions and traversal.' },
  { action: 'NextProjectTab / PreviousProjectTab / CloseProjectTab', reason: 'the desktop has no project-tab set.' },
  { action: 'NextSubagentTab / PreviousSubagentTab / CloseSubagentTab', reason: 'the desktop has no subagent-tab set.' },
  { action: 'SwitchAgent / SelectModel', reason: 'no desktop surface yet.' },
  { action: 'CloseSession / ClearSession', reason: 'no desktop surface; NewSession is carried as session.create.' },
  { action: 'ToggleTts / StopTts / ToggleFullscreen / ToggleReasoning', reason: 'no desktop surface.' },
  { action: 'ToggleAgentRun', reason: 'the desktop has no agent-run control, and the bridge carries no command to stop or resume one.' },
  { action: 'CycleModelForward / CycleModelBackward', reason: 'the bridge DTO surface carries no model catalogue, so there is no model to cycle.' },
];

// ── The reserved chords ──────────────────────────────────────────────────

/** One reserved chord: a chord that is deliberately **not** bound. */
export interface ReservedChord {
  /** The chord, platform-neutral. */
  chord: string;
  /** What the chord is reserved for. */
  reservedFor: string;
  /** Why it is not bound in this milestone. */
  reason: string;
}

/**
 * The reserved chords.
 *
 * A chord that shadows an OS or browser accelerator is either captured
 * with an explicit purpose or explicitly reserved and left unbound
 * (I6). These are the reserved ones: they are recorded so a later
 * milestone can bind them without a conflict, and so they are not
 * shadowed by accident.
 */
export const RESERVED_CHORDS: readonly ReservedChord[] = [
  {
    chord: 'mod+shift+p',
    reservedFor: 'M007-D (command palette)',
    reason: "the palette's command contents are M007-D's, and a chord bound to an empty surface would be a dead chord.",
  },
  {
    chord: 'mod+f',
    reservedFor: 'a future in-app find',
    reason: 'no find surface in M007-C.',
  },
  {
    chord: 'mod+q',
    reservedFor: 'never — the OS',
    reason: 'quitting is OS-owned.',
  },
  {
    chord: 'mod+w',
    reservedFor: 'never — the OS',
    reason: 'closing the window is OS-owned.',
  },
  {
    chord: 'mod+c',
    reservedFor: 'never — the OS',
    reason: 'copy is OS-owned, including for the keymap export surface.',
  },
  {
    chord: 'mod+x',
    reservedFor: 'never — the OS',
    reason: 'cut is OS-owned.',
  },
  {
    chord: 'mod+v',
    reservedFor: 'never — the OS',
    reason: 'paste is OS-owned.',
  },
  {
    chord: 'mod+a',
    reservedFor: 'never — the OS',
    reason: 'select-all is OS-owned.',
  },
  {
    chord: 'mod+tab',
    reservedFor: 'never — the OS',
    reason: 'OS application switching.',
  },
  {
    chord: 'mod+shift+tab',
    reservedFor: 'never — the OS',
    reason: 'OS application switching.',
  },
];

// ── The chord machinery ──────────────────────────────────────────────────

/** The modifier tokens, in the fixed order a chord is written in. */
const MODIFIER_ORDER = ['mod', 'meta', 'ctrl', 'alt', 'shift'] as const;

type ModifierToken = (typeof MODIFIER_ORDER)[number];

/** The key tokens that are not their own lowercased name. */
const KEY_TOKENS: Record<string, string> = {
  ' ': 'space',
  Escape: 'escape',
  Enter: 'enter',
  Tab: 'tab',
  Backspace: 'backspace',
  Delete: 'delete',
  ArrowLeft: 'arrowleft',
  ArrowRight: 'arrowright',
  ArrowUp: 'arrowup',
  ArrowDown: 'arrowdown',
  Home: 'home',
  End: 'end',
  PageUp: 'pageup',
  PageDown: 'pagedown',
  Insert: 'insert',
};

/** The key part of a key event. */
export interface ChordEvent {
  /** The event's `key`. */
  key: string;
  /** Whether the platform's accelerator key is held. */
  metaKey?: boolean;
  /** Whether the control key is held. */
  ctrlKey?: boolean;
  /** Whether the alt key is held. */
  altKey?: boolean;
  /** Whether the shift key is held. */
  shiftKey?: boolean;
}

/** Whether a platform string names macOS. */
export function isMacPlatform(platform: string): boolean {
  return /mac|iphone|ipad|ipod/i.test(platform);
}

/**
 * The name of the platform's accelerator key.
 *
 * `mod` is the platform-neutral token: it is the command key on macOS
 * and the control key everywhere else, so one declaration serves both
 * platforms.
 */
export function modKeyName(platform: string): 'meta' | 'ctrl' {
  return isMacPlatform(platform) ? 'meta' : 'ctrl';
}

/** The platform this renderer runs on. */
export function currentPlatform(): string {
  if (typeof navigator === 'undefined') return '';
  return navigator.platform || navigator.userAgent || '';
}

/** The key token of a key event's key. */
export function keyToken(key: string): string {
  const mapped = KEY_TOKENS[key];
  if (mapped) return mapped;
  if (key.length === 1) return key.toLowerCase();
  return key.toLowerCase().replace(/[^a-z0-9]/g, '');
}

function isModifierToken(part: string): part is ModifierToken {
  return (MODIFIER_ORDER as readonly string[]).includes(part);
}

function orderModifiers(modifiers: string[]): string[] {
  const unique = [...new Set(modifiers)];
  unique.sort((left, right) => {
    const leftIndex = MODIFIER_ORDER.indexOf(left as ModifierToken);
    const rightIndex = MODIFIER_ORDER.indexOf(right as ModifierToken);
    return leftIndex - rightIndex;
  });
  return unique;
}

/**
 * Normalise a chord *string*.
 *
 * The modifier tokens are emitted in a fixed order (`mod`, `ctrl`, `alt`,
 * `shift`) and the key token is lowercased, so `mod+shift+p` and
 * `shift+mod+p` are the same chord. A string with an unknown modifier
 * is not a chord at all, and returns null.
 */
export function parseChord(text: string): string | null {
  const raw = text.trim().toLowerCase();
  if (!raw) return null;
  const parts = raw.split('+');
  const key = parts.pop();
  if (!key) return null;
  const modifiers: string[] = [];
  for (const part of parts) {
    if (!isModifierToken(part)) return null;
    modifiers.push(part);
  }
  return [...orderModifiers(modifiers), keyToken(key)].join('+');
}

/**
 * Resolve the `mod` token of a chord for a platform.
 *
 * `mod+p` is `meta+p` on macOS and `ctrl+p` everywhere else, so one
 * declaration serves both platforms.
 */
export function resolveChordToken(token: string, platform: string = currentPlatform()): string | null {
  const parsed = parseChord(token);
  if (!parsed) return null;
  const modName = modKeyName(platform);
  return parsed
    .split('+')
    .map((part) => (part === 'mod' ? modName : part))
    .join('+');
}

/**
 * The platform-neutral chord of a key event.
 *
 * The platform's accelerator key is written `mod`, so the chord a user
 * captures is the same chord on every platform. This is the form the
 * keymap document stores.
 */
export function neutralChord(event: ChordEvent, platform: string = currentPlatform()): string {
  const modName = modKeyName(platform);
  const modifiers: string[] = [];
  if (modName === 'meta' && event.metaKey) modifiers.push('mod');
  else if (modName === 'ctrl' && event.ctrlKey) modifiers.push('mod');
  else {
    if (event.metaKey) modifiers.push('meta');
    if (event.ctrlKey) modifiers.push('ctrl');
  }
  if (event.altKey) modifiers.push('alt');
  if (event.shiftKey) modifiers.push('shift');
  return [...orderModifiers(modifiers), keyToken(event.key)].join('+');
}

/**
 * The resolved chord of a key event.
 *
 * The modifiers are the ones the event actually carries, and the key
 * token is normalised. The resolution is total: an event whose key is
 * unknown still produces a chord, and that chord matches no action.
 */
export function normalizeChord(event: ChordEvent): string {
  const modifiers: string[] = [];
  if (event.metaKey) modifiers.push('meta');
  if (event.ctrlKey) modifiers.push('ctrl');
  if (event.altKey) modifiers.push('alt');
  if (event.shiftKey) modifiers.push('shift');
  return [...orderModifiers(modifiers), keyToken(event.key)].join('+');
}

// ── The effective map ────────────────────────────────────────────────────

/** A conflict the effective-map builder resolved. */
export interface ConflictNotice {
  /** The chord two actions claimed. */
  chord: string;
  /** The action that keeps the chord — the later-declared one. */
  holder: string;
  /** The action that fell back to its default binding. */
  loser: string;
}

/** The effective map: the chords that are actually bound. */
export interface EffectiveMap {
  /** The claims, keyed by scope and then by the resolved chord. */
  byScope: Map<string, Map<string, string>>;
  /** The effective binding of every action. */
  bindings: Map<string, Binding[]>;
  /** The conflicts the builder resolved. */
  conflicts: ConflictNotice[];
}

/**
 * Build the effective map: the defaults, merged with the user's
 * overrides.
 *
 * An override **replaces** an action's defaults entirely (the TUI's
 * single-binding rule). A conflict — two actions claiming one chord in
 * one scope — is resolved deterministically: the later-declared action
 * keeps the chord and the earlier one falls back to its default binding
 * (§8). The conflict is reported, never silent.
 */
export function buildEffectiveMap(
  declarations: readonly ActionDeclaration[],
  overrides: Record<string, string[]> = {},
  platform: string = currentPlatform(),
): EffectiveMap {
  const desired = new Map<string, Binding[]>();
  for (const declaration of declarations) {
    const override = overrides[declaration.id];
    const bindings: Binding[] = [];
    if (override && override.length > 0) {
      for (const chord of override) {
        bindings.push({ chord, scope: declaration.overrideScope });
      }
    } else {
      for (const binding of declaration.defaultBindings) {
        bindings.push({ ...binding });
      }
    }
    desired.set(declaration.id, bindings);
  }

  // Who claims what. The claim key is the scope and the resolved chord.
  const claims = new Map<string, { scope: Scope; chord: string; resolved: string; actionIds: string[] }>();
  for (const declaration of declarations) {
    for (const binding of desired.get(declaration.id) ?? []) {
      const resolved = resolveChordToken(binding.chord, platform);
      if (!resolved) continue;
      const key = `${scopeKey(binding.scope)}\u0000${resolved}`;
      const claim = claims.get(key);
      if (claim) claim.actionIds.push(declaration.id);
      else claims.set(key, { scope: binding.scope, chord: binding.chord, resolved, actionIds: [declaration.id] });
    }
  }

  const byScope = new Map<string, Map<string, string>>();
  const bindings = new Map<string, Binding[]>();
  const conflicts: ConflictNotice[] = [];
  const winners = new Map<string, string>();

  for (const claim of claims.values()) {
    const key = scopeKey(claim.scope);
    const winner = claim.actionIds[claim.actionIds.length - 1];
    winners.set(`${key}\u0000${claim.resolved}`, winner);
    const scopeMap = byScope.get(key) ?? new Map<string, string>();
    scopeMap.set(claim.resolved, winner);
    byScope.set(key, scopeMap);
    if (claim.actionIds.length > 1) {
      for (const loser of claim.actionIds.slice(0, -1)) {
        conflicts.push({ chord: claim.chord, holder: winner, loser });
      }
    }
  }

  // The losers fall back to their default binding, and the fallback is
  // granted only to a chord no winner holds.
  for (const declaration of declarations) {
    const own = desired.get(declaration.id) ?? [];
    const lost = own.filter((binding) => {
      const resolved = resolveChordToken(binding.chord, platform);
      if (!resolved) return false;
      const winner = winners.get(`${scopeKey(binding.scope)}\u0000${resolved}`);
      return winner !== undefined && winner !== declaration.id;
    });
    const granted: Binding[] = own.filter((binding) => {
      const resolved = resolveChordToken(binding.chord, platform);
      if (!resolved) return false;
      return winners.get(`${scopeKey(binding.scope)}\u0000${resolved}`) === declaration.id;
    });
    if (lost.length > 0) {
      for (const fallback of declaration.defaultBindings) {
        const resolved = resolveChordToken(fallback.chord, platform);
        if (!resolved) continue;
        const key = scopeKey(fallback.scope);
        const scopeMap = byScope.get(key) ?? new Map<string, string>();
        if (scopeMap.has(resolved)) continue;
        scopeMap.set(resolved, declaration.id);
        byScope.set(key, scopeMap);
        granted.push({ ...fallback });
      }
    }
    bindings.set(declaration.id, granted);
  }

  return { byScope, bindings, conflicts };
}

/**
 * The action a chord reaches.
 *
 * The scopes are consulted in order — the overlay stack, then the focused
 * region, then the global map — so a chord has exactly one owner at a
 * time (§7 WP-3).
 */
export function resolveAction(
  effective: EffectiveMap,
  scopes: readonly Scope[],
  resolvedChord: string,
): string | null {
  for (const scope of scopes) {
    const hit = effective.byScope.get(scopeKey(scope))?.get(resolvedChord);
    if (hit) return hit;
  }
  return null;
}

/** The action that holds a chord in a scope, or null. */
export function holderOf(
  effective: EffectiveMap,
  scope: Scope,
  resolvedChord: string,
): string | null {
  return effective.byScope.get(scopeKey(scope))?.get(resolvedChord) ?? null;
}

// ── The conflict check ───────────────────────────────────────────────────

/** The result of a remap. */
export interface RemapResult {
  /** The overrides after the remap — unchanged when the remap was refused. */
  overrides: Record<string, string[]>;
  /** The refusal message, or null when the remap was accepted. */
  refusal: string | null;
}

/**
 * Remap an action to a chord.
 *
 * The conflict check is evaluated against the **current** effective map,
 * not a stale snapshot, so two remaps that race cannot both win (§8). A
 * refused remap leaves the keymap byte-identical and says why, in the
 * TUI's shape of message: "<chord> is already bound to <action>".
 */
export function remapAction(
  overrides: Record<string, string[]>,
  declarations: readonly ActionDeclaration[],
  actionId: string,
  chord: string,
  platform: string = currentPlatform(),
): RemapResult {
  const declaration = declarations.find((entry) => entry.id === actionId);
  if (!declaration) {
    return { overrides, refusal: `unknown action ${actionId}` };
  }
  const parsed = parseChord(chord);
  if (!parsed) {
    return { overrides, refusal: `${chord} is not a chord` };
  }
  const resolved = resolveChordToken(parsed, platform);
  if (!resolved) {
    return { overrides, refusal: `${chord} is not a chord` };
  }
  const effective = buildEffectiveMap(declarations, overrides, platform);
  const holder = holderOf(effective, declaration.overrideScope, resolved);
  if (holder && holder !== actionId) {
    const held = declarations.find((entry) => entry.id === holder);
    return { overrides, refusal: `${parsed} is already bound to ${held?.label ?? holder}` };
  }
  return { overrides: { ...overrides, [actionId]: [parsed] }, refusal: null };
}

// ── The document shape ───────────────────────────────────────────────────

/** The keymap document: the shape the renderer stores and exports. */
export interface KeymapDocument {
  /** The document version. */
  version: number;
  /** The overrides, keyed by action id. */
  bindings: Record<string, string[]>;
}

/** Something the renderer tells the user about its own keymap. */
export interface KeymapNotice {
  /** The kind of notice. */
  kind: 'malformed' | 'version' | 'unknown-action' | 'conflict' | 'invalid-chord';
  /** The message the user sees. */
  message: string;
}

/** The export document, as text. */
export function exportDocument(overrides: Record<string, string[]>): string {
  const document: KeymapDocument = { version: KEYMAP_VERSION, bindings: overrides };
  return JSON.stringify(document, null, 2);
}

/** The result of an import. */
export interface ImportResult {
  /** The overrides the document carries — empty when the import was rejected. */
  overrides: Record<string, string[]>;
  /** The failure, or null when the document was accepted. */
  error: string | null;
  /** The notices the import produced. */
  notices: KeymapNotice[];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Parse a keymap document.
 *
 * The import is **atomic**: a malformed or conflicting document is
 * rejected whole, so a partially applied import is impossible (§6.4).
 * An unknown action id is dropped on its own — the other overrides still
 * apply — and an unknown chord is kept but inert.
 */
export function importDocument(
  text: string,
  declarations: readonly ActionDeclaration[] = ALL_ACTION_DECLARATIONS,
  platform: string = currentPlatform(),
): ImportResult {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    return { overrides: {}, error: `the document is not valid JSON: ${String(error)}`, notices: [] };
  }
  if (!isRecord(parsed)) {
    return { overrides: {}, error: 'the document is not a keymap document', notices: [] };
  }
  if (parsed.version !== KEYMAP_VERSION) {
    return {
      overrides: {},
      error: `unsupported keymap version ${String(parsed.version)}`,
      notices: [],
    };
  }
  const bindings = parsed.bindings;
  if (!isRecord(bindings)) {
    return { overrides: {}, error: 'the document carries no bindings', notices: [] };
  }

  const overrides: Record<string, string[]> = {};
  const notices: KeymapNotice[] = [];
  for (const [actionId, value] of Object.entries(bindings)) {
    if (!declarations.some((declaration) => declaration.id === actionId)) {
      notices.push({
        kind: 'unknown-action',
        message: `the stored keymap names an action this build does not have (${actionId}); it was dropped`,
      });
      continue;
    }
    if (!Array.isArray(value) || value.some((entry) => typeof entry !== 'string')) {
      return {
        overrides: {},
        error: `the bindings of ${actionId} are not a list of chords`,
        notices: [],
      };
    }
    const chords = value.map((entry) => parseChord(entry)).filter((entry): entry is string => entry !== null);
    if (chords.length === 0) {
      notices.push({
        kind: 'invalid-chord',
        message: `the stored binding of ${actionId} is not a chord; it was dropped`,
      });
      continue;
    }
    overrides[actionId] = chords;
  }

  const effective = buildEffectiveMap(declarations, overrides, platform);
  if (effective.conflicts.length > 0) {
    const conflict = effective.conflicts[0];
    const holder = declarations.find((declaration) => declaration.id === conflict.holder);
    const loser = declarations.find((declaration) => declaration.id === conflict.loser);
    return {
      overrides: {},
      error: `${conflict.chord} is claimed by both ${holder?.label ?? conflict.holder} and ${loser?.label ?? conflict.loser}`,
      notices: [],
    };
  }

  return { overrides, error: null, notices };
}

// ── The persistence adapter ──────────────────────────────────────────────

/** The part of the Web Storage API the keymap uses. */
export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

/** The renderer's own storage, when the runtime has one. */
export function browserStorage(): StorageLike | null {
  try {
    if (typeof localStorage === 'undefined') return null;
    return localStorage;
  } catch {
    return null;
  }
}

/** The result of loading the keymap. */
export interface LoadedKeymap {
  /** The overrides the stored document carries. */
  overrides: Record<string, string[]>;
  /** The notices the load produced. */
  notices: KeymapNotice[];
}

/**
 * Load the keymap.
 *
 * The posture is the TUI's (`src/tui/app/mod.rs:868-876`): a stale or
 * hand-edited entry drops one slot and never blocks startup. A missing
 * document yields the defaults and no notice; a malformed or
 * wrong-version document yields the defaults and a visible notice.
 */
export function loadKeymap(
  storage: StorageLike | null,
  declarations: readonly ActionDeclaration[] = ALL_ACTION_DECLARATIONS,
  platform: string = currentPlatform(),
): LoadedKeymap {
  if (!storage) return { overrides: {}, notices: [] };

  let raw: string | null = null;
  try {
    raw = storage.getItem(KEYMAP_STORAGE_KEY);
  } catch (error) {
    return {
      overrides: {},
      notices: [
        {
          kind: 'malformed',
          message: `the stored keymap could not be read (${String(error)}); the defaults are in effect`,
        },
      ],
    };
  }
  if (raw === null) return { overrides: {}, notices: [] };

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {
      overrides: {},
      notices: [
        {
          kind: 'malformed',
          message: 'the stored keymap is not valid JSON; the defaults are in effect',
        },
      ],
    };
  }
  if (!isRecord(parsed) || parsed.version !== KEYMAP_VERSION) {
    return {
      overrides: {},
      notices: [
        {
          kind: 'version',
          message: `the stored keymap has an unsupported version; the defaults are in effect`,
        },
      ],
    };
  }
  const bindings = parsed.bindings;
  if (!isRecord(bindings)) {
    return {
      overrides: {},
      notices: [
        {
          kind: 'malformed',
          message: 'the stored keymap carries no bindings; the defaults are in effect',
        },
      ],
    };
  }

  const overrides: Record<string, string[]> = {};
  const notices: KeymapNotice[] = [];
  for (const [actionId, value] of Object.entries(bindings)) {
    if (!declarations.some((declaration) => declaration.id === actionId)) {
      notices.push({
        kind: 'unknown-action',
        message: `the stored keymap names an action this build does not have (${actionId}); it was dropped`,
      });
      continue;
    }
    if (!Array.isArray(value) || value.some((entry) => typeof entry !== 'string')) {
      notices.push({
        kind: 'malformed',
        message: `the stored binding of ${actionId} is malformed; it was dropped`,
      });
      continue;
    }
    const chords = value.filter((entry) => entry.length > 0);
    if (chords.length === 0) {
      notices.push({
        kind: 'invalid-chord',
        message: `the stored binding of ${actionId} is not a chord; it was dropped`,
      });
      continue;
    }
    overrides[actionId] = chords;
  }

  const effective = buildEffectiveMap(declarations, overrides, platform);
  for (const conflict of effective.conflicts) {
    const holder = declarations.find((declaration) => declaration.id === conflict.holder);
    const loser = declarations.find((declaration) => declaration.id === conflict.loser);
    notices.push({
      kind: 'conflict',
      message: `${conflict.chord} is claimed by both ${holder?.label ?? conflict.holder} and ${loser?.label ?? conflict.loser}; ${holder?.label ?? conflict.holder} keeps it`,
    });
  }

  return { overrides, notices };
}

/**
 * Store the keymap.
 *
 * A commit is **one** `setItem`, so a reload can never observe a
 * half-applied remap, and a crash mid-remap leaves the previous
 * committed state (§8).
 */
export function saveKeymap(storage: StorageLike | null, overrides: Record<string, string[]>): void {
  if (!storage) return;
  try {
    storage.setItem(KEYMAP_STORAGE_KEY, exportDocument(overrides));
  } catch {
    // A renderer that cannot store the keymap still runs: the remap
    // holds for this session and the defaults return on the next one.
  }
}

/** Drop the override document entirely and restore the defaults. */
export function clearKeymap(storage: StorageLike | null): void {
  if (!storage) return;
  try {
    storage.removeItem(KEYMAP_STORAGE_KEY);
  } catch {
    // Nothing to do: the in-memory map is reset by the caller anyway.
  }
}
