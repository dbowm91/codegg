import { PANEL_IDS } from './registry';
import type { PanelId, PanelPlacement } from './registry';

export type { PanelId, PanelPlacement };

// The side-panel host: one slot per edge, and the operation that moves a
// panel between them.
//
// The model is the TUI's. `SidePanelHost` holds one `PanelSlot` per
// placement, each slot carries a visibility flag, and `dock(id, placement)`
// **moves** a panel rather than copying it — so a panel is never loaded on
// two edges at once, which is what stops one widget state rendering twice
// (`src/tui/panels/mod.rs:1191`, `:1093`, `:1336`).
//
// The desktop keeps the slot, the visibility flag and the move. It does not
// keep the TUI's per-edge history: that is a convenience memory, not an
// undo stack, and a displaced panel returns to the registry, where the user
// can select it again.
//
// Panel *state* — the scroll position, the expanded sections — is keyed by
// the panel id and not by the edge, so it travels with the id when the
// panel moves (`architecture/tui.md:3520-3524`).

/** Every edge a panel can be docked on. */
export const PANEL_PLACEMENTS: readonly PanelPlacement[] = ['right', 'left', 'bottom'];

/** The edge the frame docks on before the user chooses another. */
export const DEFAULT_PLACEMENT: PanelPlacement = 'right';

export const DEFAULT_COLUMN_WIDTH = 340;
export const MIN_COLUMN_WIDTH = 200;
/** The main region never shrinks below this, and a column that cannot fit beside it collapses. */
export const MIN_MAIN_WIDTH = 320;
export const DEFAULT_BOTTOM_RATIO = 0.3;
export const MIN_BOTTOM_RATIO = 0.15;
export const MAX_BOTTOM_RATIO = 0.6;
export const MIN_MAIN_HEIGHT = 200;

export interface PanelSlot {
  /** The panel this edge shows, or null when the edge is empty. */
  panelId: PanelId | null;
  /** Whether the edge is shown. Per-slot, not a global flag. */
  visible: boolean;
}

export interface PanelViewState {
  /** The panel's own scroll position, in pixels. */
  scrollTop: number;
}

export interface PanelLayoutState {
  /** The left column's width, in pixels. */
  leftWidth: number;
  /** The right column's width, in pixels. */
  rightWidth: number;
  /** The bottom row's share of the frame height, between 0 and 1. */
  bottomRatio: number;
}

export interface PanelHostState {
  /** One slot per edge. */
  slots: Record<PanelPlacement, PanelSlot>;
  /** The edge the host's own controls act on. */
  selected: PanelPlacement;
  /** Per-panel view state, keyed by the panel id so it travels with the id. */
  panelState: Record<PanelId, PanelViewState>;
  layout: PanelLayoutState;
}

/** A panel's new size: a column's width in pixels, or the bottom row's share of the frame height. */
export type PanelSize = { kind: 'column'; width: number } | { kind: 'row'; ratio: number };

export function initialPanelHostState(): PanelHostState {
  const panelState = {} as Record<PanelId, PanelViewState>;
  for (const id of PANEL_IDS) panelState[id] = { scrollTop: 0 };

  return {
    slots: {
      left: { panelId: null, visible: false },
      right: { panelId: 'tasks', visible: true },
      bottom: { panelId: null, visible: false },
    },
    selected: DEFAULT_PLACEMENT,
    panelState,
    layout: {
      leftWidth: DEFAULT_COLUMN_WIDTH,
      rightWidth: DEFAULT_COLUMN_WIDTH,
      bottomRatio: DEFAULT_BOTTOM_RATIO,
    },
  };
}

/** The edge that holds the panel, or null when the panel is not docked. */
export function placementOf(state: PanelHostState, panelId: PanelId): PanelPlacement | null {
  for (const placement of PANEL_PLACEMENTS) {
    if (state.slots[placement].panelId === panelId) return placement;
  }
  return null;
}

/** The panel the host's controls act on. */
export function activePanel(state: PanelHostState): PanelId | null {
  return state.slots[state.selected].panelId;
}

/** Every panel the frame shows, with the edge it is shown on. */
export function visiblePanels(
  state: PanelHostState,
): Array<{ placement: PanelPlacement; panelId: PanelId }> {
  return PANEL_PLACEMENTS.flatMap((placement) => {
    const slot = state.slots[placement];
    return slot.visible && slot.panelId !== null ? [{ placement, panelId: slot.panelId }] : [];
  });
}

/**
 * Move a panel to an edge.
 *
 * The panel is *moved*, never copied: the edge it came from is emptied,
 * and the edge it lands on takes it. Re-docking a panel to the edge it is
 * already on is a no-op rather than a self-push
 * (`architecture/tui.md:3548-3550`).
 */
export function dock(
  state: PanelHostState,
  panelId: PanelId,
  placement: PanelPlacement,
): PanelHostState {
  const origin = placementOf(state, panelId);

  if (origin === placement) {
    const slot = state.slots[placement];
    if (slot.visible) return state;
    return {
      ...state,
      slots: { ...state.slots, [placement]: { ...slot, visible: true } },
      selected: placement,
    };
  }

  const slots: Record<PanelPlacement, PanelSlot> = { ...state.slots };
  if (origin !== null) slots[origin] = { panelId: null, visible: false };
  slots[placement] = { panelId, visible: true };
  return { ...state, slots, selected: placement };
}

/** Show or hide one edge's slot. The flag belongs to the slot, not to the host. */
export function toggleSlot(state: PanelHostState, placement: PanelPlacement): PanelHostState {
  const slot = state.slots[placement];
  return {
    ...state,
    slots: { ...state.slots, [placement]: { ...slot, visible: !slot.visible } },
  };
}

/** The edge the host's controls act on. */
export function selectPlacement(state: PanelHostState, placement: PanelPlacement): PanelHostState {
  if (state.selected === placement) return state;
  return { ...state, selected: placement };
}

/** Record a panel's own scroll position, keyed by the panel id. */
export function setPanelScroll(
  state: PanelHostState,
  panelId: PanelId,
  scrollTop: number,
): PanelHostState {
  const current = state.panelState[panelId];
  if (current && current.scrollTop === scrollTop) return state;
  return {
    ...state,
    panelState: { ...state.panelState, [panelId]: { scrollTop } },
  };
}

/** Record a panel's new size. */
export function resize(
  state: PanelHostState,
  placement: PanelPlacement,
  size: PanelSize,
): PanelHostState {
  if (size.kind === 'row') {
    if (state.layout.bottomRatio === size.ratio) return state;
    return { ...state, layout: { ...state.layout, bottomRatio: size.ratio } };
  }
  const key = placement === 'left' ? 'leftWidth' : 'rightWidth';
  if (state.layout[key] === size.width) return state;
  return { ...state, layout: { ...state.layout, [key]: size.width } };
}

/**
 * The width a column may take.
 *
 * Growth is relative: the panel grows into the main region, which keeps
 * its own minimum. When the frame is too narrow to host the column
 * *beside* that minimum, the column collapses rather than squeezing the
 * transcript out of existence (`src/tui/layout.rs:292-298`).
 */
export function resolveColumnWidth(desired: number, frameWidth: number, otherWidth = 0): number {
  const available = frameWidth - MIN_MAIN_WIDTH - Math.max(0, otherWidth);
  if (available < MIN_COLUMN_WIDTH) return 0;
  return Math.min(Math.max(desired, MIN_COLUMN_WIDTH), available);
}

/**
 * The share of the frame height the bottom row may take.
 *
 * The bottom panel is a ratio of the frame height, not a fixed row count
 * (`src/tui/layout.rs:130-135`), and the main region keeps its minimum
 * height above it.
 */
export function resolveBottomRatio(desired: number, frameHeight: number): number {
  if (frameHeight <= MIN_MAIN_HEIGHT) return 0;
  const available = (frameHeight - MIN_MAIN_HEIGHT) / frameHeight;
  if (available <= MIN_BOTTOM_RATIO) return 0;
  const upper = Math.min(MAX_BOTTOM_RATIO, available);
  return Math.min(Math.max(desired, MIN_BOTTOM_RATIO), upper);
}
