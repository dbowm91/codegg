import type { EffectiveMap } from '../keymap';

// The editor's bounded bridge contract.
//
// Every type here mirrors one DTO the desktop host serves
// (`apps/desktop/src-tauri/src/document.rs`). The host owns the document;
// the renderer owns nothing but the bounded viewport slice the host sent, so
// every shape here is bounded by construction and none of them carries the
// document.
//
// The mirror is asserted by `scripts/check_desktop_editor_authority.py`,
// which compares these fields with the host's Rust structs field for field.

/** One bounded slice of the open document. */
export interface EditorViewportView {
  /** The workspace-relative path of the open document. */
  relativePath: string;
  /** The document's current revision. */
  revision: number;
  /** The first visible line, zero-based. */
  firstLine: number;
  /** How many lines this slice carries. */
  lineCount: number;
  /** How many lines the document has. */
  totalLines: number;
  /** The document's byte length. */
  byteLength: number;
  /**
   * The visible lines.
   *
   * The one bounded slice of document text that crosses the bridge. The
   * whole document is never materialised, in the host or here.
   */
  visibleLines: string[];
  /** The caret's byte offset. */
  caret: number;
  /** The selection anchor's byte offset. */
  anchor: number;
  /** The caret's zero-based line. */
  caretLine: number;
  /** The caret's zero-based column, in UTF-16 code units. */
  caretColumn: number;
  /** The anchor's zero-based line. */
  anchorLine: number;
  /** The anchor's zero-based column, in UTF-16 code units. */
  anchorColumn: number;
}

/** The result of opening a document. */
export interface EditorOpenView {
  relativePath: string;
  revision: number;
  state: string;
  message: string;
  writable: boolean;
  totalLines: number;
  byteLength: number;
}

/** The presentation of the host's document state, without any text. */
export interface EditorStateView {
  state: string;
  message: string;
  relativePath: string | null;
  revision: number | null;
  writable: boolean;
  dirty: boolean;
  conflicted: boolean;
  pendingChanges: number;
  canUndo: boolean;
  canRedo: boolean;
}

/** The result of one edit, undo or redo. */
export interface EditorEditView {
  caret: number;
  anchor: number;
  revision: number;
  undoDepth: number;
  redoDepth: number;
}

/** One entry of the workspace file tree. */
export interface EditorTreeNodeView {
  name: string;
  relativePath: string;
  depth: number;
  kind: string;
  children: EditorTreeNodeView[];
}

/** The workspace file tree the open affordance lists. */
export interface EditorTreeView {
  workspaceId: string;
  prefix: string;
  status: string;
  message: string;
  nodes: EditorTreeNodeView[];
  truncated: boolean;
}

/** One diagnostic entry. */
export interface EditorDiagnosticView {
  relativePath: string;
  line: number;
  startCharacter: number;
  endCharacter: number;
  severity: number;
  message: string;
  server: string | null;
}

/**
 * The project's diagnostics.
 *
 * `status` is `no_warm_server` when the daemon has no warm language server
 * to answer with. That is **not** an empty diagnostic set, and the renderer
 * must not present it as one.
 */
export interface EditorDiagnosticsView {
  status: string;
  message: string;
  diagnostics: EditorDiagnosticView[];
  truncated: boolean;
  stale?: boolean;
}

/** One inert completion candidate. */
export interface EditorCompletionCandidateView {
  label: string;
  detail: string | null;
  kind: string | null;
  applicable: boolean;
  reason: string | null;
}

/** The completion candidates for the caret's position. */
export interface EditorCompletionView {
  status: string;
  message: string;
  candidates: EditorCompletionCandidateView[];
  truncated: boolean;
}

/** One navigation target. */
export interface EditorLocationView {
  relativePath: string;
  line: number;
  character: number;
}

/** The result of one navigation read. */
export interface EditorNavigationView {
  status: string;
  message: string;
  locations: EditorLocationView[];
  truncated: boolean;
}

/**
 * What the editor panel renders.
 *
 * The panel renders these props and calls nothing: the session above it owns
 * the host calls, and the panel layer stays free of bridge authority.
 */
export interface EditorPanelProps {
  /**
   * The effective keymap: the chords that are actually bound.
   *
   * The composition root owns it — it is where the keymap is loaded and
   * where a remap lands — so the editor resolves its chords against the
   * same map as every other surface.
   */
  effectiveMap: EffectiveMap;
  /** The open document's bounded viewport slice, or null. */
  viewport: EditorViewportView | null;
  /** The document's presentation state, or null when nothing is open. */
  status: EditorStateView | null;
  /** The workspace file tree, or null before it has been read. */
  tree: EditorTreeView | null;
  /** The project's diagnostics, or null before they have been read. */
  diagnostics: EditorDiagnosticsView | null;
  /** The completion candidates, or null before they have been asked for. */
  completion: EditorCompletionView | null;
  /** The navigation targets, or null before they have been asked for. */
  navigation: EditorNavigationView | null;
  /** Whether a host call is in flight. */
  busy: boolean;
  /** The last failure, if any. */
  error: string | null;
  /** Open a document by its workspace-relative path. */
  onOpen: (relativePath: string) => void;
  /** Move the caret. `extend` keeps the selection anchor where it is. */
  onMoveCursor: (movement: string, extend: boolean) => void;
  /** Apply one edit over a byte range. */
  onEdit: (start: number, end: number, insert: string) => void;
  /** Undo the newest edit. */
  onUndo: () => void;
  /** Redo the newest undone edit. */
  onRedo: () => void;
  /** Save the open document. */
  onSave: () => void;
  /** Reload the open document from disk. */
  onReload: () => void;
  /** Resynchronise the open document with the daemon. */
  onResync: () => void;
  /** Close the open document. */
  onClose: () => void;
  /** Ask for completion candidates at the caret. */
  onComplete: () => void;
  /** Ask for navigation targets at the caret. */
  onNavigate: (operation: string) => void;
  /** Scroll the viewport to a first visible line. */
  onScroll: (firstLine: number) => void;
  /** Re-read the workspace file tree. */
  onRefreshTree: () => void;
}
