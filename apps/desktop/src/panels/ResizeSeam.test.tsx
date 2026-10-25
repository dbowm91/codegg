import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { ResizeSeam } from './ResizeSeam';

function frameOf(width: number, height: number): HTMLElement {
  const element = document.createElement('div');
  element.getBoundingClientRect = () =>
    ({
      width,
      height,
      top: 0,
      left: 0,
      right: width,
      bottom: height,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    }) as DOMRect;
  return element;
}

describe('the desktop resize seam', () => {
  it('clamps a drag that would starve the main region', () => {
    const onResize = vi.fn();
    const frameRef = { current: frameOf(1000, 800) };
    render(
      <ResizeSeam
        placement="right"
        width={340}
        ratio={0.3}
        frameRef={frameRef}
        onResize={onResize}
      />,
    );
    const seam = screen.getByTestId('resize-seam');
    fireEvent.pointerDown(seam, { clientX: 500, clientY: 100 });
    // Dragging the shared border left grows the panel; the clamp stops it
    // at what the main region's minimum leaves.
    fireEvent.pointerMove(seam, { clientX: 100, clientY: 100 });
    expect(onResize).toHaveBeenCalledWith('right', { kind: 'column', width: 680 });
  });

  it('grows a right-docked panel only when the border moves towards it', () => {
    const onResize = vi.fn();
    const frameRef = { current: frameOf(2000, 800) };
    render(
      <ResizeSeam
        placement="right"
        width={340}
        ratio={0.3}
        frameRef={frameRef}
        onResize={onResize}
      />,
    );
    const seam = screen.getByTestId('resize-seam');
    fireEvent.pointerDown(seam, { clientX: 500, clientY: 100 });
    fireEvent.pointerMove(seam, { clientX: 460, clientY: 100 });
    expect(onResize).toHaveBeenCalledWith('right', { kind: 'column', width: 380 });
  });

  it('ignores a second drag while one is in flight', () => {
    const onResize = vi.fn();
    const frameRef = { current: frameOf(2000, 800) };
    render(
      <ResizeSeam
        placement="right"
        width={340}
        ratio={0.3}
        frameRef={frameRef}
        onResize={onResize}
      />,
    );
    const seam = screen.getByTestId('resize-seam');
    fireEvent.pointerDown(seam, { clientX: 500, clientY: 100 });
    fireEvent.pointerMove(seam, { clientX: 460, clientY: 100 });
    // A second pointer down mid-drag is ignored rather than restarting the
    // drag from a new origin, so the last move is still measured from the
    // first one.
    fireEvent.pointerDown(seam, { clientX: 200, clientY: 100 });
    fireEvent.pointerMove(seam, { clientX: 100, clientY: 100 });
    expect(onResize).toHaveBeenLastCalledWith('right', { kind: 'column', width: 740 });
  });

  it('gives the bottom row a share of the frame height', () => {
    const onResize = vi.fn();
    const frameRef = { current: frameOf(1000, 1000) };
    render(
      <ResizeSeam
        placement="bottom"
        width={340}
        ratio={0.3}
        frameRef={frameRef}
        onResize={onResize}
      />,
    );
    const seam = screen.getByTestId('resize-seam');
    fireEvent.pointerDown(seam, { clientX: 100, clientY: 500 });
    // Dragging the top border up grows the row.
    fireEvent.pointerMove(seam, { clientX: 100, clientY: 400 });
    const call = onResize.mock.calls[0];
    expect(call[0]).toBe('bottom');
    expect(call[1].kind).toBe('row');
    expect(call[1].ratio).toBeCloseTo(0.4, 5);
  });

  it('does not resize before a drag starts', () => {
    const onResize = vi.fn();
    const frameRef = { current: frameOf(1000, 800) };
    render(
      <ResizeSeam
        placement="right"
        width={340}
        ratio={0.3}
        frameRef={frameRef}
        onResize={onResize}
      />,
    );
    fireEvent.pointerMove(screen.getByTestId('resize-seam'), { clientX: 10, clientY: 10 });
    expect(onResize).not.toHaveBeenCalled();
  });
});
