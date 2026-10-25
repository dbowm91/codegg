import { useEffect, useMemo, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent } from 'react';
import type { ActionDeclaration } from '../keymap';

/*
 * The key bindings surface (M007-C, WP-5).
 *
 * The surface is where the chords are *changed*, not only read: a user
 * rebinds an action, resets the map to the defaults, and exports or
 * imports the keymap document.
 *
 * The panel layer holds no authority of its own (§6.1): it renders the
 * props it is given and calls the callbacks it is given. It never
 * touches a durable store, never reaches the daemon and never imports
 * the keymap module's runtime — the chord a capture produced arrives
 * through the `captureChord` prop, so the panel layer stays free of the
 * renderer's persistence.
 *
 * The surface is operable from the keyboard alone: the list is a
 * listbox with a roving `tabindex`, `Enter` starts a capture, `Escape`
 * cancels it, and the reset/export/import actions have their own
 * chords. The mouse remains a first-class route — every row, the
 * capture field and the three actions are buttons and inputs.
 */

/** The action catalog the surface lists. */
export interface KeybindingsPanelProps {
  /** Every declared action, in declaration order. */
  declarations: readonly ActionDeclaration[];
  /** The effective binding of each action, keyed by action id. */
  bindings: Record<string, string[]>;
  /** The platform-neutral chord a key event carries. */
  captureChord: (event: ReactKeyboardEvent) => string;
  /**
   * Commit a remap.
   *
   * Returns the refusal message when the chord is already bound, and
   * null when the remap was accepted.
   */
  onRemap: (actionId: string, chord: string) => string | null;
  /** Restore every default binding. */
  onReset: () => void;
  /** The export document, as text. */
  exportText: string;
  /**
   * Apply an import document.
   *
   * Returns the failure when the document was rejected, and null when
   * it was applied.
   */
  onImport: (text: string) => string | null;
  /** Close the surface. */
  onClose: () => void;
}

/** The panel's own chords: the three actions the surface owns. */
export const PANEL_RESET_CHORD = 'mod+alt+shift+r';
export const PANEL_EXPORT_CHORD = 'mod+alt+shift+e';
export const PANEL_IMPORT_CHORD = 'mod+alt+shift+i';

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

function isTextEntry(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  if (!element) return false;
  return element.tagName === 'INPUT' || element.tagName === 'TEXTAREA';
}

export function KeybindingsPanel(props: KeybindingsPanelProps) {
  const {
    declarations,
    bindings,
    captureChord,
    onRemap,
    onReset,
    exportText,
    onImport,
    onClose,
  } = props;

  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(declarations[0]?.id ?? null);
  const [capturingId, setCapturingId] = useState<string | null>(null);
  const [conflict, setConflict] = useState<string | null>(null);
  const [showExport, setShowExport] = useState(false);
  const [importText, setImportText] = useState('');
  const panelRef = useRef<HTMLElement | null>(null);
  const listRef = useRef<HTMLUListElement | null>(null);
  const captureRef = useRef<HTMLInputElement | null>(null);

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return declarations;
    return declarations.filter(
      (declaration) =>
        declaration.label.toLowerCase().includes(needle) ||
        declaration.id.toLowerCase().includes(needle) ||
        declaration.description.toLowerCase().includes(needle),
    );
  }, [declarations, query]);

  // A filter that hides the selected row selects the first visible one,
  // so the list always has a roving `tabindex` owner.
  useEffect(() => {
    if (visible.length === 0) return;
    if (!visible.some((declaration) => declaration.id === selectedId)) {
      setSelectedId(visible[0].id);
    }
  }, [visible, selectedId]);

  const focusAction = (actionId: string) => {
    const list = listRef.current;
    const element = list?.querySelector<HTMLElement>(`[data-testid="keybinds-action-${actionId}"]`);
    element?.focus();
  };

  const startCapture = (actionId: string) => {
    setCapturingId(actionId);
    setConflict(null);
    // The capture field takes the focus on the next frame, so the key
    // that started the capture is not the key that is captured.
    window.requestAnimationFrame(() => captureRef.current?.focus());
  };

  const onCaptureKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Escape') {
      setCapturingId(null);
      return;
    }
    const actionId = capturingId;
    if (!actionId) return;
    const chord = captureChord(event);
    setCapturingId(null);
    if (!chord) return;
    setConflict(onRemap(actionId, chord));
    // The capture field unmounts with the capture, so the focus goes back
    // to the row it was remapping rather than to the document.
    focusAction(actionId);
  };

  const onListKeyDown = (event: ReactKeyboardEvent, actionId: string) => {
    if (isTextEntry(event.target)) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      event.stopPropagation();
      const index = visible.findIndex((declaration) => declaration.id === actionId);
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      const next = visible[(index + delta + visible.length) % visible.length];
      if (!next) return;
      setSelectedId(next.id);
      focusAction(next.id);
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      event.stopPropagation();
      setSelectedId(actionId);
      startCapture(actionId);
    }
  };

  const onPanelKeyDown = (event: ReactKeyboardEvent) => {
    const chord = captureChord(event);
    if (chord === PANEL_RESET_CHORD) {
      event.preventDefault();
      event.stopPropagation();
      setConflict(null);
      onReset();
      return;
    }
    if (chord === PANEL_EXPORT_CHORD) {
      event.preventDefault();
      event.stopPropagation();
      setShowExport(true);
      return;
    }
    if (chord === PANEL_IMPORT_CHORD) {
      event.preventDefault();
      event.stopPropagation();
      setConflict(onImport(importText));
      return;
    }
    if (event.key === 'Tab') {
      // The overlay traps the focus: `Tab` walks the surface's own
      // controls and wraps, and never escapes to the surface behind it.
      const panel = panelRef.current;
      if (!panel) return;
      const controls = Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
        (control) => control.getAttribute('data-keymap-capture') === null,
      );
      if (controls.length === 0) return;
      const active = document.activeElement as HTMLElement | null;
      const index = active ? controls.indexOf(active) : -1;
      const next = (index + (event.shiftKey ? -1 : 1) + controls.length) % controls.length;
      event.preventDefault();
      event.stopPropagation();
      controls[next].focus();
    }
  };

  return (
    <section
      className="keybindings-panel"
      data-testid="keybindings-panel"
      role="dialog"
      aria-label="Keyboard bindings"
      ref={panelRef}
      onKeyDown={onPanelKeyDown}
    >
      <header className="keybindings-panel-header">
        <h2>Keyboard bindings</h2>
        <button type="button" data-testid="overlay-close-button" onClick={onClose}>
          Close
        </button>
      </header>

      <label className="keybindings-search">
        <span>Search actions</span>
        <input
          type="search"
          data-testid="keybinds-search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
      </label>

      <ul className="keybindings-list" data-testid="keybinds-list" role="listbox" ref={listRef}>
        {visible.map((declaration) => {
          const chord = (bindings[declaration.id] ?? []).join(', ');
          return (
            <li key={declaration.id}>
              <button
                type="button"
                role="option"
                aria-selected={selectedId === declaration.id}
                tabIndex={selectedId === declaration.id ? 0 : -1}
                data-testid={`keybinds-action-${declaration.id}`}
                onClick={() => setSelectedId(declaration.id)}
                onKeyDown={(event) => onListKeyDown(event, declaration.id)}
              >
                <span className="keybindings-action-label">{declaration.label}</span>
                <span className="keybindings-action-chord">{chord || 'unbound'}</span>
              </button>
            </li>
          );
        })}
      </ul>

      {capturingId ? (
        <div className="keybindings-capture">
          <p data-testid="keybinds-capture-hint">
            Press the key to bind to {capturingId} (Escape cancels)
          </p>
          <input
            type="text"
            readOnly
            autoFocus
            data-testid="keybinds-capture"
            data-keymap-capture="true"
            ref={captureRef}
            onKeyDown={onCaptureKeyDown}
          />
        </div>
      ) : (
        <p className="keybindings-hint">
          Select an action and press Enter to rebind it. {PANEL_RESET_CHORD} resets,{' '}
          {PANEL_EXPORT_CHORD} exports, {PANEL_IMPORT_CHORD} applies an import.
        </p>
      )}

      {conflict ? (
        <p className="keybindings-conflict" role="alert" data-testid="keybinds-conflict">
          {conflict}
        </p>
      ) : null}

      <div className="keybindings-actions">
        <button type="button" data-testid="keybinds-reset-button" onClick={onReset}>
          Reset to defaults
        </button>
        <button type="button" data-testid="keybinds-export-button" onClick={() => setShowExport(true)}>
          Export
        </button>
        <button type="button" data-testid="keybinds-import-button" onClick={() => setConflict(onImport(importText))}>
          Apply import
        </button>
      </div>

      {showExport ? (
        <textarea
          className="keybindings-export"
          data-testid="keybinds-export"
          readOnly
          rows={6}
          value={exportText}
        />
      ) : null}

      <label className="keybindings-import">
        <span>Import a keymap document</span>
        <textarea
          data-testid="keybinds-import"
          rows={4}
          value={importText}
          onChange={(event) => setImportText(event.target.value)}
        />
      </label>
    </section>
  );
}
