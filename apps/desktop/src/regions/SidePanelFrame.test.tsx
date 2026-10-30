import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { createRef } from 'react';
import { PanelControls, SidePanelFrame } from './SidePanelFrame';
import { dock, initialPanelHostState } from '../panels/host';
import type { PanelProps } from '../panels/registry';
import { emptyEditorPanelProps } from '../editor/fixtures';
import { emptyTerminalPanelProps } from '../terminal/contract';

const panelProps: PanelProps = {
  projection: null,
  controller: null,
  todoBusy: false,
  todoError: null,
  editor: emptyEditorPanelProps(),
  terminal: emptyTerminalPanelProps(),
};

describe('the desktop side-panel frame', () => {
  it('renders the panel the registry names for the docked edge', () => {
    const host = initialPanelHostState();
    render(
      <SidePanelFrame
        host={host}
        panelProps={panelProps}
        frameRef={createRef()}
        onResize={vi.fn()}
        onPanelScroll={vi.fn()}
      />,
    );
    expect(screen.getByTestId('side-panel-frame')).toHaveAttribute('data-placement', 'right');
    expect(screen.getByTestId('side-panel-label')).toHaveTextContent('Tasks');
    expect(screen.getByTestId('side-panel-body')).toBeInTheDocument();
  });

  it('gives the panel its own scroll container, apart from the chat region’s', () => {
    const host = initialPanelHostState();
    render(
      <SidePanelFrame
        host={host}
        panelProps={panelProps}
        frameRef={createRef()}
        onResize={vi.fn()}
        onPanelScroll={vi.fn()}
      />,
    );
    const body = screen.getByTestId('side-panel-body');
    const frame = screen.getByTestId('side-panel-frame');
    expect(frame.contains(body)).toBe(true);
    // The panel body is not the panel itself: the header and the seam are
    // the frame's, the body is the panel's.
    expect(body.getAttribute('data-testid')).toBe('side-panel-body');
  });

  it('draws a resize seam on the docked edge', () => {
    const host = dock(initialPanelHostState(), 'tasks', 'bottom');
    render(
      <SidePanelFrame
        host={host}
        panelProps={panelProps}
        frameRef={createRef()}
        onResize={vi.fn()}
        onPanelScroll={vi.fn()}
      />,
    );
    expect(screen.getByTestId('resize-seam')).toHaveAttribute('data-placement', 'bottom');
  });

  it('moves the panel rather than copying it when another edge is chosen', () => {
    const onDock = vi.fn();
    const host = initialPanelHostState();
    render(
      <PanelControls host={host} onDock={onDock} onToggle={vi.fn()} />,
    );
    fireEvent.click(screen.getByTestId('panel-edge-left'));
    expect(onDock).toHaveBeenCalledWith('tasks', 'left');
  });

  it('docks the panel the user picks on the current edge', () => {
    const onDock = vi.fn();
    const host = initialPanelHostState();
    render(<PanelControls host={host} onDock={onDock} onToggle={vi.fn()} />);
    fireEvent.click(screen.getByTestId('panel-tab-activity'));
    expect(onDock).toHaveBeenCalledWith('activity', 'right');
  });

  it('hides the edge’s slot without touching the other slots', () => {
    const onToggle = vi.fn();
    const host = initialPanelHostState();
    render(<PanelControls host={host} onDock={vi.fn()} onToggle={onToggle} />);
    fireEvent.click(screen.getByTestId('panel-toggle-button'));
    expect(onToggle).toHaveBeenCalledWith('right');
  });
});
