import { useCallback, useMemo, useRef } from 'react';
import type { KeyboardEvent } from 'react';
import { ALL_ACTION_DECLARATIONS, EDITOR_SCOPE, normalizeChord, resolveAction } from '../keymap';
import { editorIntent } from './actions';
import type {
  EditorPanelProps,
  EditorTreeNodeView,
  EditorViewportView,
} from './contract';

// The editor surface: the viewport, the gutter, the caret and the
// selection, the document's chrome, and the file tree the editor opens
// documents from.
//
// The surface renders what the session gives it and holds **no** text of its
// own — the visible lines it draws are the bounded window the host sent.
//
// Its keyboard route is the canonical keymap: the chords it answers to are
// declared in `apps/desktop/src/keymap.ts` under the editor's scope, and
// every key event is resolved by the keymap's own resolver. The editor
// declares no chord of its own, so a remapped chord reaches the editor the
// way it reaches every other surface. The one route that is not a chord is
// the printable insert: it is the viewport's own editing affordance, and the
// keymap records why it is not an action.

/** The column range one visible line has selected, if any. */
function selectedRange(
  view: EditorViewportView,
  line: number,
): [number, number] | null {
  // The anchor may sit after the caret, so the selection is normalised to
  // the earlier position first.
  const anchorFirst =
    view.anchorLine < view.caretLine ||
    (view.anchorLine === view.caretLine && view.anchorColumn <= view.caretColumn);
  const first = anchorFirst ? view.anchorLine : view.caretLine;
  const last = anchorFirst ? view.caretLine : view.anchorLine;
  const firstColumn = anchorFirst ? view.anchorColumn : view.caretColumn;
  const lastColumn = anchorFirst ? view.caretColumn : view.anchorColumn;
  if (first === last && firstColumn === lastColumn) return null;
  if (line < first || line > last) return null;
  const from = line === first ? firstColumn : 0;
  const to = line === last ? lastColumn : Number.MAX_SAFE_INTEGER;
  if (to <= from) return null;
  return [from, to];
}

/** The caret's row inside the visible window, if it is on screen. */
function caretRow(view: EditorViewportView): number | null {
  const row = view.caretLine - view.firstLine;
  if (row < 0 || row >= view.visibleLines.length) return null;
  return row;
}

/** The one visible line, with its selection marked. */
function VisibleLine({
  view,
  index,
  line,
}: {
  view: EditorViewportView;
  index: number;
  line: string;
}) {
  const absolute = view.firstLine + index;
  const selection = selectedRange(view, absolute);
  const isCaretLine = caretRow(view) === index;
  const body = (() => {
    if (!selection) return <span className="editor-line-body">{line}</span>;
    const [from, to] = selection;
    const head = line.slice(0, from);
    const middle = line.slice(from, Math.min(to, line.length));
    const tail = line.slice(Math.min(to, line.length));
    return (
      <span className="editor-line-body">
        {head}
        <span className="editor-selection" data-testid="editor-selection">
          {middle}
        </span>
        {tail}
      </span>
    );
  })();
  return (
    <div
      className={isCaretLine ? 'editor-line is-caret-line' : 'editor-line'}
      data-testid="editor-line"
      data-line={absolute}
    >
      <span className="editor-gutter" data-testid="editor-gutter">
        {absolute + 1}
      </span>
      {body}
      {isCaretLine ? (
        <span
          className="editor-caret"
          data-testid="editor-caret"
          data-column={view.caretColumn}
        />
      ) : null}
    </div>
  );
}

/** One tree entry, with the affordance that opens it. */
function TreeEntry({
  node,
  onOpen,
}: {
  node: EditorTreeNodeView;
  onOpen: (relativePath: string) => void;
}) {
  const isFile = node.kind === 'file';
  return (
    <li className="editor-tree-entry" data-testid="editor-tree-item" data-kind={node.kind}>
      <span className="editor-tree-name" style={{ paddingLeft: `${node.depth * 12}px` }}>
        {node.name}
      </span>
      {isFile ? (
        <button
          type="button"
          className="editor-tree-open"
          data-testid="editor-open-button"
          onClick={() => onOpen(node.relativePath)}
        >
          open
        </button>
      ) : null}
      {node.children.length > 0 ? (
        <ul className="editor-tree-children">
          {node.children.map((child) => (
            <TreeEntry key={child.relativePath} node={child} onOpen={onOpen} />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

/** The editor surface. */
export function EditorSurface(props: EditorPanelProps) {
  const { viewport, status, tree, diagnostics, completion, navigation } = props;
  const viewportRef = useRef<HTMLDivElement | null>(null);

  const selection = useMemo(() => {
    if (!viewport) return null;
    const start = Math.min(viewport.caret, viewport.anchor);
    const end = Math.max(viewport.caret, viewport.anchor);
    return start === end ? null : { start, end };
  }, [viewport]);

  const onKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      // The chord is resolved by the canonical keymap: the editor declares
      // no chords of its own, and a chord the user remapped reaches the
      // editor through the same table as every other action.
      const chord = normalizeChord(event);
      const actionId = resolveAction(props.effectiveMap, [EDITOR_SCOPE], chord);
      if (actionId) {
        const intent = editorIntent(actionId);
        if (!intent) return;
        // Only a captured chord swallows the event, exactly as the
        // composition root's dispatcher does it.
        const declaration = ALL_ACTION_DECLARATIONS.find((entry) => entry.id === actionId);
        if (declaration?.captured) event.preventDefault();
        if (intent.kind === 'command') {
          if (intent.command === 'undo') props.onUndo();
          else if (intent.command === 'redo') props.onRedo();
          else props.onSave();
          return;
        }
        if (!viewport) return;
        if (intent.kind === 'move') {
          props.onMoveCursor(intent.movement, intent.extend);
          return;
        }
        const caret = viewport.caret;
        if (intent.edit === 'backspace') {
          if (selection) {
            props.onEdit(selection.start, selection.end, '');
            return;
          }
          if (caret === 0) return;
          props.onEdit(Math.max(0, caret - 1), caret, '');
          return;
        }
        if (intent.edit === 'delete') {
          if (selection) {
            props.onEdit(selection.start, selection.end, '');
            return;
          }
          props.onEdit(caret, caret + 1, '');
          return;
        }
        props.onEdit(caret, caret, '\n');
        return;
      }
      // The printable insert is not a chord, so it is not an action: it is
      // the viewport's own editing affordance, and it belongs to the
      // viewport rather than to the surface around it.
      const viewportElement = viewportRef.current;
      if (!viewport || !viewportElement) return;
      if (!(event.target instanceof Node) || !viewportElement.contains(event.target)) return;
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.key.length !== 1) return;
      event.preventDefault();
      props.onEdit(viewport.caret, viewport.caret, event.key);
    },
    [props, selection, viewport],
  );

  return (
    <section
      className="editor-surface"
      data-testid="editor-surface"
      onKeyDown={onKeyDown}
    >
      <header className="editor-toolbar" data-testid="editor-toolbar">
        <span className="editor-path" data-testid="editor-path">
          {status?.relativePath ?? 'no document'}
        </span>
        <span className="editor-state" data-testid="editor-state" data-state={status?.state ?? 'none'}>
          {status?.state ?? 'none'}
        </span>
        {status?.conflicted ? (
          <span className="editor-conflict" data-testid="editor-conflict">
            conflict
          </span>
        ) : null}
        <button
          type="button"
          data-testid="editor-undo-button"
          disabled={!status?.canUndo}
          onClick={() => props.onUndo()}
        >
          undo
        </button>
        <button
          type="button"
          data-testid="editor-redo-button"
          disabled={!status?.canRedo}
          onClick={() => props.onRedo()}
        >
          redo
        </button>
        <button type="button" data-testid="editor-save-button" onClick={() => props.onSave()}>
          save
        </button>
        <button type="button" data-testid="editor-reload-button" onClick={() => props.onReload()}>
          reload
        </button>
        <button type="button" data-testid="editor-resync-button" onClick={() => props.onResync()}>
          resync
        </button>
        <button type="button" data-testid="editor-close-button" onClick={() => props.onClose()}>
          close
        </button>
        <button type="button" data-testid="editor-complete-button" onClick={() => props.onComplete()}>
          complete
        </button>
        <button
          type="button"
          data-testid="editor-navigate-button"
          onClick={() => props.onNavigate('definition')}
        >
          definition
        </button>
        <button
          type="button"
          data-testid="editor-references-button"
          onClick={() => props.onNavigate('references')}
        >
          references
        </button>
        {props.busy ? <span data-testid="editor-busy">working</span> : null}
      </header>

      {props.error ? (
        <p role="alert" className="editor-error" data-testid="editor-error">
          {props.error}
        </p>
      ) : null}

      <div className="editor-body">
        <nav className="editor-tree" data-testid="editor-tree">
          <button
            type="button"
            className="editor-tree-refresh"
            data-testid="editor-tree-refresh-button"
            onClick={() => props.onRefreshTree()}
          >
            refresh
          </button>
          {tree && tree.status !== 'ready' ? (
            <p className="editor-tree-status" data-testid="editor-tree-status">
              {tree.message}
            </p>
          ) : null}
          {tree ? (
            <ul className="editor-tree-list">
              {tree.nodes.map((node) => (
                <TreeEntry key={node.relativePath} node={node} onOpen={props.onOpen} />
              ))}
            </ul>
          ) : (
            <p className="editor-empty" data-testid="editor-tree-empty">
              no file tree
            </p>
          )}
        </nav>

        <div
          className="editor-viewport"
          data-testid="editor-viewport"
          tabIndex={0}
          role="group"
          aria-label="Editor viewport"
          ref={viewportRef}
        >
          {viewport ? (
            viewport.visibleLines.map((line, index) => (
              <VisibleLine key={`${viewport.firstLine + index}`} view={viewport} index={index} line={line} />
            ))
          ) : (
            <p className="editor-empty" data-testid="editor-empty">
              no document is open
            </p>
          )}
        </div>      </div>

      <footer className="editor-footer">
        <section className="editor-diagnostics" data-testid="editor-diagnostics">
          <h3>Diagnostics</h3>
          {diagnostics ? (
            <>
              <p data-testid="editor-diagnostics-status" data-status={diagnostics.status}>
                {diagnostics.message}
              </p>
              <ul>
                {diagnostics.diagnostics.map((diagnostic, index) => (
                  <li key={`${diagnostic.relativePath}:${diagnostic.line}:${index}`} data-testid="editor-diagnostic">
                    <span data-testid="editor-diagnostic-severity">{diagnostic.severity}</span>
                    <span data-testid="editor-diagnostic-path">{diagnostic.relativePath}</span>
                    <span data-testid="editor-diagnostic-line">{diagnostic.line + 1}</span>
                    <span data-testid="editor-diagnostic-message">{diagnostic.message}</span>
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <p data-testid="editor-diagnostics-empty">no diagnostics</p>
          )}
        </section>

        <section className="editor-completion" data-testid="editor-completion">
          <h3>Completion</h3>
          {completion ? (
            <>
              <p data-testid="editor-completion-status" data-status={completion.status}>
                {completion.message}
              </p>
              <ul>
                {completion.candidates.map((candidate, index) => (
                  <li key={`${candidate.label}:${index}`} data-testid="editor-completion-item">
                    <span data-testid="editor-completion-label">{candidate.label}</span>
                    {candidate.applicable ? null : (
                      <span data-testid="editor-completion-reason">{candidate.reason}</span>
                    )}
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <p data-testid="editor-completion-empty">no completion</p>
          )}
        </section>

        <section className="editor-navigation" data-testid="editor-navigation">
          <h3>Navigation</h3>
          {navigation ? (
            <>
              <p data-testid="editor-navigation-status" data-status={navigation.status}>
                {navigation.message}
              </p>
              <ul>
                {navigation.locations.map((location, index) => (
                  <li key={`${location.relativePath}:${location.line}:${index}`} data-testid="editor-navigation-item">
                    <span data-testid="editor-navigation-path">{location.relativePath}</span>
                    <span data-testid="editor-navigation-line">{location.line + 1}</span>
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <p data-testid="editor-navigation-empty">no navigation</p>
          )}
        </section>
      </footer>
    </section>
  );
}
