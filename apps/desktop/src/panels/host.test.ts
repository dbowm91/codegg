import { describe, expect, it } from 'vitest';
import {
  DEFAULT_PLACEMENT,
  MIN_BOTTOM_RATIO,
  MIN_COLUMN_WIDTH,
  MIN_MAIN_HEIGHT,
  MIN_MAIN_WIDTH,
  activePanel,
  dock,
  initialPanelHostState,
  placementOf,
  resolveBottomRatio,
  resolveColumnWidth,
  setPanelScroll,
  toggleSlot,
  visiblePanels,
} from './host';

describe('the desktop side-panel host', () => {
  it('docks the tasks panel on the right edge before the user chooses another', () => {
    const state = initialPanelHostState();
    expect(DEFAULT_PLACEMENT).toBe('right');
    expect(state.slots.right.panelId).toBe('tasks');
    expect(state.slots.right.visible).toBe(true);
    expect(state.slots.left.panelId).toBeNull();
    expect(state.slots.bottom.panelId).toBeNull();
  });

  it('moves a panel to another edge instead of loading it on two edges', () => {
    const state = dock(initialPanelHostState(), 'tasks', 'left');
    expect(state.slots.left.panelId).toBe('tasks');
    expect(state.slots.right.panelId).toBeNull();
    expect(visiblePanels(state)).toEqual([{ placement: 'left', panelId: 'tasks' }]);
    expect(placementOf(state, 'tasks')).toBe('left');
  });

  it('treats re-docking a panel to the edge it is already on as a no-op', () => {
    const state = initialPanelHostState();
    expect(dock(state, 'tasks', 'right')).toBe(state);
  });

  it('keeps the visibility flag on the slot rather than on the host', () => {
    const hidden = toggleSlot(initialPanelHostState(), 'right');
    expect(hidden.slots.right.visible).toBe(false);
    // The other slots keep their own flags; there is no global one.
    expect(hidden.slots.left.visible).toBe(false);
    expect(hidden.slots.bottom.visible).toBe(false);
    expect(visiblePanels(hidden)).toEqual([]);
  });

  it('carries the panel state with the panel id when the panel moves', () => {
    const scrolled = setPanelScroll(initialPanelHostState(), 'tasks', 120);
    const moved = dock(scrolled, 'tasks', 'bottom');
    expect(moved.panelState.tasks.scrollTop).toBe(120);
    expect(moved.panelState.subagents.scrollTop).toBe(0);
  });

  it('acts on the panel of the edge its controls are pointed at', () => {
    const state = dock(initialPanelHostState(), 'subagents', 'bottom');
    expect(state.selected).toBe('bottom');
    expect(activePanel(state)).toBe('subagents');
  });

  it('collapses a column the frame is too narrow to host', () => {
    const narrow = MIN_MAIN_WIDTH + MIN_COLUMN_WIDTH - 1;
    expect(resolveColumnWidth(340, narrow)).toBe(0);
  });

  it('clamps a column to what the main region leaves it', () => {
    expect(resolveColumnWidth(900, 1000)).toBe(1000 - MIN_MAIN_WIDTH);
    // A column below its own minimum grows back to it.
    expect(resolveColumnWidth(10, 1000)).toBe(MIN_COLUMN_WIDTH);
  });

  it('gives the bottom row a share of the frame height', () => {
    expect(resolveBottomRatio(0.5, 1000)).toBe(0.5);
    expect(resolveBottomRatio(0.01, 1000)).toBe(MIN_BOTTOM_RATIO);
    // A frame too short to split collapses the row instead.
    expect(resolveBottomRatio(0.3, MIN_MAIN_HEIGHT)).toBe(0);
  });
});
