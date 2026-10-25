import { useEffect, useRef } from 'react';
import type { RefObject } from 'react';
import { PANEL_IDS, PANEL_REGISTRY } from '../panels/registry';
import type { PanelId, PanelPlacement, PanelProps } from '../panels/registry';
import { PANEL_PLACEMENTS } from '../panels/host';
import type { PanelHostState, PanelSize } from '../panels/host';
import { ResizeSeam } from '../panels/ResizeSeam';

// The side-panel frame: the docked panel, its header, and its resize seam.
//
// The frame renders whatever the registry names for the edge, so there is
// no second list of panels that could drift from the render path. The
// panel itself is mounted once and *moved* between edges, which is why
// its own state — the scroll position, the expanded sections — travels
// with it (`architecture/tui.md:3520-3524`).
//
// The panel body is the panel's own scroll container, apart from the chat
// region's, so the two scroll independently.

export interface SidePanelFrameProps {
  /** The host's state: the slots, the layout and the panel state. */
  host: PanelHostState;
  /** What the panels render. */
  panelProps: PanelProps;
  /** The frame the seam measures its clamp against. */
  frameRef: RefObject<HTMLElement | null>;
  /** Report a panel's new size. */
  onResize: (placement: PanelPlacement, size: PanelSize) => void;
  /** Record a panel's scroll position. */
  onPanelScroll: (panelId: PanelId, scrollTop: number) => void;
}

interface DockedPanelProps {
  placement: PanelPlacement;
  panelId: PanelId;
  host: PanelHostState;
  panelProps: PanelProps;
  frameRef: RefObject<HTMLElement | null>;
  onResize: (placement: PanelPlacement, size: PanelSize) => void;
  onPanelScroll: (panelId: PanelId, scrollTop: number) => void;
}

function DockedPanel({
  placement,
  panelId,
  host,
  panelProps,
  frameRef,
  onResize,
  onPanelScroll,
}: DockedPanelProps) {
  const bodyRef = useRef<HTMLDivElement>(null);
  const scrollTop = host.panelState[panelId]?.scrollTop ?? 0;

  useEffect(() => {
    // The scroll position is keyed by the panel id, not by the edge, so a
    // panel that moves between edges keeps its place.
    const element = bodyRef.current;
    if (element && element.scrollTop !== scrollTop) element.scrollTop = scrollTop;
  }, [scrollTop]);

  const width = placement === 'left' ? host.layout.leftWidth : host.layout.rightWidth;
  const otherWidth =
    placement === 'left' ? host.layout.rightWidth : placement === 'right' ? host.layout.leftWidth : 0;

  return (
    <aside
      className="side-panel"
      data-testid="side-panel-frame"
      data-placement={placement}
      data-panel={panelId}
    >
      <div className="side-panel-head">
        <strong data-testid="side-panel-label">{PANEL_REGISTRY[panelId].label}</strong>
        <span data-testid="side-panel-placement">{placement}</span>
      </div>
      <div
        className="side-panel-body"
        data-testid="side-panel-body"
        ref={bodyRef}
        onScroll={(event) => onPanelScroll(panelId, event.currentTarget.scrollTop)}
      >
        {PANEL_REGISTRY[panelId].render(panelProps)}
      </div>
      <ResizeSeam
        placement={placement}
        width={width}
        ratio={host.layout.bottomRatio}
        frameRef={frameRef}
        otherWidth={otherWidth}
        onResize={onResize}
      />
    </aside>
  );
}

export function SidePanelFrame({
  host,
  panelProps,
  frameRef,
  onResize,
  onPanelScroll,
}: SidePanelFrameProps) {
  const panels = PANEL_PLACEMENTS.flatMap((placement) => {
    const slot = host.slots[placement];
    return slot.visible && slot.panelId !== null ? [{ placement, panelId: slot.panelId }] : [];
  });

  return (
    <>
      {panels.map(({ placement, panelId }) => (
        <DockedPanel
          // The key is the panel id, not the edge: a panel that moves
          // between edges is the same panel, and the DOM node moves with
          // it.
          key={panelId}
          placement={placement}
          panelId={panelId}
          host={host}
          panelProps={panelProps}
          frameRef={frameRef}
          onResize={onResize}
          onPanelScroll={onPanelScroll}
        />
      ))}
    </>
  );
}

export interface PanelControlsProps {
  /** The host's state. */
  host: PanelHostState;
  /** Move a panel to an edge. */
  onDock: (panelId: PanelId, placement: PanelPlacement) => void;
  /** Show or hide an edge's slot. */
  onToggle: (placement: PanelPlacement) => void;
}

/**
 * The frame's panel controls.
 *
 * They live beside the other catalog surfaces rather than inside the
 * panel, because a frame too narrow to host a column collapses the
 * column — and a control that is only reachable from the panel it just
 * collapsed is not a control at all.
 */
export function PanelControls({ host, onDock, onToggle }: PanelControlsProps) {
  const selected = host.selected;
  const slot = host.slots[selected];
  const active = slot.panelId ?? PANEL_IDS[0];

  return (
    <section className="panel" data-testid="panel-controls">
      <h2>Panels</h2>
      <div className="panel-tabs" role="group" aria-label="Side panel">
        {PANEL_IDS.map((id) => (
          <button
            key={id}
            type="button"
            data-testid={`panel-tab-${id}`}
            aria-pressed={active === id}
            onClick={() => onDock(id, selected)}
          >
            {PANEL_REGISTRY[id].label}
          </button>
        ))}
      </div>
      <div className="panel-edges" role="group" aria-label="Panel placement">
        {PANEL_PLACEMENTS.map((placement) => (
          <button
            key={placement}
            type="button"
            data-testid={`panel-edge-${placement}`}
            aria-pressed={selected === placement}
            onClick={() => onDock(active, placement)}
          >
            {placement}
          </button>
        ))}
      </div>
      <button type="button" data-testid="panel-toggle-button" onClick={() => onToggle(selected)}>
        {slot.visible ? 'Hide panel' : 'Show panel'}
      </button>
    </section>
  );
}
