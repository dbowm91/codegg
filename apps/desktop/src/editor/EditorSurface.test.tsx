import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { EditorSurface } from './EditorSurface';
import type { EditorPanelProps, EditorViewportView } from './contract';
import { emptyEditorPanelProps } from './fixtures';
import { currentPlatform, isMacPlatform } from '../keymap';

/** The accelerator the platform's chord map is built with. */
const accelerator = () =>
  isMacPlatform(currentPlatform()) ? { metaKey: true } : { ctrlKey: true };

const viewport = (overrides: Partial<EditorViewportView> = {}): EditorViewportView => ({
  relativePath: 'src/lib.rs',
  revision: 4,
  firstLine: 0,
  lineCount: 3,
  totalLines: 3,
  byteLength: 24,
  visibleLines: ['fn main() {', '    println!("hi");', '}'],
  caret: 3,
  anchor: 3,
  caretLine: 0,
  caretColumn: 3,
  anchorLine: 0,
  anchorColumn: 3,
  ...overrides,
});

const props = (overrides: Partial<EditorPanelProps> = {}): EditorPanelProps => ({
  ...emptyEditorPanelProps(),
  viewport: viewport(),
  ...overrides,
});

describe('the editor surface', () => {
  it('renders the window the host sent, and nothing else', () => {
    render(<EditorSurface {...props()} />);
    const lines = screen.getAllByTestId('editor-line');
    expect(lines).toHaveLength(3);
    expect(lines[0]).toHaveTextContent('fn main() {');
    expect(lines[2]).toHaveTextContent('}');
  });

  it('marks the caret on the caret line', () => {
    render(<EditorSurface {...props()} />);
    expect(screen.getByTestId('editor-caret')).toHaveAttribute('data-column', '3');
    expect(screen.getByTestId('editor-caret').closest('[data-testid="editor-line"]')).toHaveAttribute(
      'data-line',
      '0',
    );
  });

  it('marks the selection between the anchor and the caret', () => {
    render(
      <EditorSurface
        {...props({
          viewport: viewport({
            caret: 8,
            anchor: 3,
            caretLine: 0,
            caretColumn: 8,
            anchorLine: 0,
            anchorColumn: 3,
          }),
        })}
      />,
    );
    expect(screen.getByTestId('editor-selection')).toHaveTextContent('main');
  });

  it('raises an edit intent for a printable key', () => {
    const onEdit = vi.fn();
    render(<EditorSurface {...props({ onEdit })} />);
    fireEvent.keyDown(screen.getByTestId('editor-viewport'), { key: 'x' });
    expect(onEdit).toHaveBeenCalledWith(3, 3, 'x');
  });

  it('raises an edit intent that deletes backwards', () => {
    const onEdit = vi.fn();
    render(<EditorSurface {...props({ onEdit })} />);
    fireEvent.keyDown(screen.getByTestId('editor-viewport'), { key: 'Backspace' });
    expect(onEdit).toHaveBeenCalledWith(2, 3, '');
  });

  it('deletes the selection rather than one character', () => {
    const onEdit = vi.fn();
    render(
      <EditorSurface
        {...props({
          viewport: viewport({
            caret: 8,
            anchor: 3,
            caretLine: 0,
            caretColumn: 8,
            anchorLine: 0,
            anchorColumn: 3,
          }),
          onEdit,
        })}
      />,
    );
    fireEvent.keyDown(screen.getByTestId('editor-viewport'), { key: 'Backspace' });
    expect(onEdit).toHaveBeenCalledWith(3, 8, '');
  });

  it('raises a movement intent for an arrow key', () => {
    const onMoveCursor = vi.fn();
    render(<EditorSurface {...props({ onMoveCursor })} />);
    fireEvent.keyDown(screen.getByTestId('editor-viewport'), { key: 'ArrowDown' });
    expect(onMoveCursor).toHaveBeenCalledWith('down', false);
  });

  it('raises the undo intent for the accelerator chord the keymap declares', () => {
    const onUndo = vi.fn();
    const onEdit = vi.fn();
    render(<EditorSurface {...props({ onUndo, onEdit })} />);
    fireEvent.keyDown(screen.getByTestId('editor-viewport'), { key: 'z', ...accelerator() });
    expect(onUndo).toHaveBeenCalled();
    expect(onEdit).not.toHaveBeenCalled();
  });

  it('raises the save intent for the editor save chord', () => {
    const onSave = vi.fn();
    render(<EditorSurface {...props({ onSave })} />);
    fireEvent.keyDown(screen.getByTestId('editor-viewport'), { key: 's', ...accelerator() });
    expect(onSave).toHaveBeenCalled();
  });

  it('exposes every navigation operation served by the desktop host', () => {
    const onNavigate = vi.fn();
    render(<EditorSurface {...props({ onNavigate })} />);
    for (const [testId, operation] of [
      ['editor-navigate-button', 'definition'],
      ['editor-declaration-button', 'declaration'],
      ['editor-implementation-button', 'implementation'],
      ['editor-references-button', 'references'],
    ] as const) {
      fireEvent.click(screen.getByTestId(testId));
      expect(onNavigate).toHaveBeenLastCalledWith(operation);
    }
    expect(onNavigate).toHaveBeenCalledTimes(4);
  });

  it('raises the selection intent for a shifted movement chord', () => {
    const onMoveCursor = vi.fn();
    render(<EditorSurface {...props({ onMoveCursor })} />);
    fireEvent.keyDown(screen.getByTestId('editor-viewport'), {
      key: 'ArrowRight',
      shiftKey: true,
    });
    expect(onMoveCursor).toHaveBeenCalledWith('right', true);
  });

  it('shows that no warm language server is not an empty diagnostic set', () => {
    render(
      <EditorSurface
        {...props({
          diagnostics: {
            status: 'no_warm_server',
            message: 'no language server is warm for this project yet',
            diagnostics: [],
            truncated: false,
          },
        })}
      />,
    );
    const status = screen.getByTestId('editor-diagnostics-status');
    expect(status).toHaveAttribute('data-status', 'no_warm_server');
    expect(status).not.toHaveTextContent('no diagnostics');
  });

  it('shows a ready diagnostic set with its entries', () => {
    render(
      <EditorSurface
        {...props({
          diagnostics: {
            status: 'ready',
            message: "the project's diagnostics are current",
            diagnostics: [
              {
                relativePath: 'src/lib.rs',
                line: 2,
                startCharacter: 4,
                endCharacter: 9,
                severity: 1,
                message: 'unused variable',
                server: 'rust-analyzer',
              },
            ],
            truncated: false,
          },
        })}
      />,
    );
    expect(screen.getByTestId('editor-diagnostics-status')).toHaveAttribute('data-status', 'ready');
    expect(screen.getByTestId('editor-diagnostic-message')).toHaveTextContent('unused variable');
    expect(screen.getByTestId('editor-diagnostic-line')).toHaveTextContent('3');
  });

  it('opens a document from the file tree', () => {
    const onOpen = vi.fn();
    render(
      <EditorSurface
        {...props({
          tree: {
            workspaceId: 'workspace-1',
            prefix: '',
            status: 'ready',
            message: 'the workspace listing is current',
            truncated: false,
            nodes: [
              {
                name: 'lib.rs',
                relativePath: 'src/lib.rs',
                depth: 1,
                kind: 'file',
                children: [],
              },
            ],
          },
          onOpen,
        })}
      />,
    );
    fireEvent.click(screen.getByTestId('editor-open-button'));
    expect(onOpen).toHaveBeenCalledWith('src/lib.rs');
  });

  it('shows the conflict and the document state', () => {
    render(
      <EditorSurface
        {...props({
          status: {
            state: 'conflict',
            message: 'the daemon reports a conflict on this document',
            relativePath: 'src/lib.rs',
            revision: 4,
            writable: true,
            dirty: true,
            conflicted: true,
            pendingChanges: 1,
            canUndo: true,
            canRedo: false,
          },
        })}
      />,
    );
    expect(screen.getByTestId('editor-state')).toHaveAttribute('data-state', 'conflict');
    expect(screen.getByTestId('editor-conflict')).toBeInTheDocument();
    expect(screen.getByTestId('editor-undo-button')).not.toBeDisabled();
    expect(screen.getByTestId('editor-redo-button')).toBeDisabled();
  });

  it('shows the failure rather than an empty document', () => {
    render(<EditorSurface {...props({ error: 'the daemon connection was lost' })} />);
    expect(screen.getByTestId('editor-error')).toHaveTextContent('the daemon connection was lost');
  });

  it('keeps preview generation inert and requires an explicit apply action', () => {
    const onPreview = vi.fn();
    const onApplyPreview = vi.fn();
    render(
      <EditorSurface
        {...props({
          onPreview,
          onApplyPreview,
          status: {
            state: 'ready',
            message: 'document is saved',
            relativePath: 'src/lib.rs',
            revision: 4,
            writable: true,
            dirty: false,
            conflicted: false,
            pendingChanges: 0,
            canUndo: false,
            canRedo: false,
          },
          preview: {
            status: 'ready',
            message: 'format preview ready for review',
            stale: false,
            candidate: {
              fileCount: 1,
              diff: '--- a/src/lib.rs\n+++ b/src/lib.rs',
              candidate: {
                preview_id: 'preview-1',
                preview_revision: 1,
                preview_digest: 'a'.repeat(64),
                kind: 'formatting',
                title: 'Format src/lib.rs',
                provenance: 'editor:format',
                patches: [{ path: 'src/lib.rs', patch: 'patch', original_hash: 'b'.repeat(64) }],
              },
            },
          },
        })}
      />,
    );
    fireEvent.click(screen.getByTestId('editor-format-preview'));
    expect(onPreview).toHaveBeenCalledWith('format');
    expect(screen.getByTestId('editor-preview-diff')).toHaveTextContent('src/lib.rs');
    fireEvent.click(screen.getByTestId('editor-preview-apply'));
    expect(onApplyPreview).toHaveBeenCalledTimes(1);
  });

  it('does not offer checked apply while the document has unsaved changes', () => {
    render(
      <EditorSurface
        {...props({
          status: {
            state: 'dirty',
            message: 'unsaved changes',
            relativePath: 'src/lib.rs',
            revision: 4,
            writable: true,
            dirty: true,
            conflicted: false,
            pendingChanges: 1,
            canUndo: true,
            canRedo: false,
          },
          preview: {
            status: 'ready',
            message: 'rename preview ready for review',
            stale: false,
            candidate: {
              fileCount: 1,
              diff: 'rename diff',
              candidate: {
                preview_id: 'preview-2',
                preview_revision: 2,
                preview_digest: 'c'.repeat(64),
                kind: 'rename',
                title: 'Rename thing',
                provenance: 'editor:rename',
                patches: [{ path: 'src/lib.rs', patch: 'patch', original_hash: 'd'.repeat(64) }],
              },
            },
          },
        })}
      />,
    );
    expect(screen.getByTestId('editor-preview-apply')).toBeDisabled();
  });
});
