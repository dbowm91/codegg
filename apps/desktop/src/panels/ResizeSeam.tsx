import { useRef } from 'react';
import type { PointerEvent as ReactPointerEvent, RefObject } from 'react';
import { resolveBottomRatio, resolveColumnWidth } from './host';
import type { PanelPlacement, PanelSize } from './host';

// The pointer resize seam: the draggable inner border between a docked
// panel and the main region.
//
// It is pointer-only. The TUI resizes its panels with chords
// (`architecture/tui.md:3516-3518`); the desktop's keyboard surface is
// M007-C's business, so this milestone ships the pointer half and reports
// the keyboard half as a finding rather than inventing a keymap.
//
// Growth is relative: the panel grows into the main region, and the main
// region keeps its own minimum. A frame too narrow to host the column
// collapses the column instead of squeezing the transcript out of
// existence (`src/tui/layout.rs:292-298`).

export interface ResizeSeamProps {
  /** The edge the seam resizes. */
  placement: PanelPlacement;
  /** The panel's current width in pixels — a column edge. */
  width: number;
  /** The panel's current share of the frame height — the bottom edge. */
  ratio: number;
  /** The frame the seam resizes. Its size is what the clamp is measured against. */
  frameRef: RefObject<HTMLElement | null>;
  /** The other column's width, when a second column is showing. */
  otherWidth?: number;
  /** Report the panel's new size. */
  onResize: (placement: PanelPlacement, size: PanelSize) => void;
}

export function ResizeSeam({
  placement,
  width,
  ratio,
  frameRef,
  otherWidth = 0,
  onResize,
}: ResizeSeamProps) {
  // One drag at a time: a second pointer down while a drag is in flight is
  // ignored rather than interleaved with it.
  const dragging = useRef(false);
  const origin = useRef({ x: 0, y: 0, width, ratio });

  const handlePointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (dragging.current) return;
    dragging.current = true;
    origin.current = { x: event.clientX, y: event.clientY, width, ratio };
    event.currentTarget.setPointerCapture?.(event.pointerId);
  };

  const handlePointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return;
    const frame = frameRef.current;
    const rect = frame?.getBoundingClientRect();
    const frameWidth = rect?.width ?? 0;
    const frameHeight = rect?.height ?? 0;
    const deltaX = event.clientX - origin.current.x;
    const deltaY = event.clientY - origin.current.y;

    if (placement === 'bottom') {
      // The bottom panel's shared border is its top edge: dragging up
      // grows it, and its height is a share of the frame height.
      const desired =
        frameHeight > 0 ? origin.current.ratio - deltaY / frameHeight : origin.current.ratio;
      onResize(placement, { kind: 'row', ratio: resolveBottomRatio(desired, frameHeight) });
      return;
    }

    // Only the shared border moves. A right-docked panel's shared border
    // is its left edge, a left-docked panel's is its right edge.
    const delta = placement === 'right' ? -deltaX : deltaX;
    const desired = origin.current.width + delta;
    onResize(placement, {
      kind: 'column',
      width: resolveColumnWidth(desired, frameWidth, otherWidth),
    });
  };

  const endDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return;
    dragging.current = false;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
  };

  return (
    <div
      className="resize-seam"
      data-testid="resize-seam"
      data-placement={placement}
      role="separator"
      aria-label={`Resize the ${placement} panel`}
      aria-orientation={placement === 'bottom' ? 'horizontal' : 'vertical'}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
    />
  );
}
