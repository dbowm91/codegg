/*
 * The desktop renderer's focus model (M007-C).
 *
 * The desktop is **direct manipulation**, not modal: there is no
 * Insert/Normal mode, every declared chord acts immediately, and a plain
 * letter key is always text (`plans/implementation/desktop-frontend-ide-foundation/
 * 013-desktop-keyboard-first-input.md` §1.1). What makes such a surface
 * keyboard-first rather than keyboard-*capable* is that the focus is
 * always visible and always somewhere determinate — which is what this
 * module owns.
 *
 * The model has four parts:
 *
 *   - a **region registry**: the named regions of the frame, in focus
 *     order. M007-B's region model is the source; this milestone adds the
 *     focus layer over it and does not change the regions themselves.
 *   - **region focus**: `mod+1`…`mod+9` moves the focus to the *n*-th
 *     region directly.
 *   - **in-region traversal**: `Tab` / `Shift+Tab` move within the focused
 *     region and **wrap** at the ends (the TUI's rule,
 *     `architecture/tui.md:4884-4886`). The implementation is a roving
 *     `tabindex`: exactly one control in the focused region carries
 *     `tabindex="0"`, the rest carry `-1`, so the browser's own `Tab`
 *     never escapes the region.
 *   - an **overlay stack**: an overlay traps the focus, `escape` pops
 *     exactly one layer and restores the focus the overlay interrupted,
 *     and the focus never falls back to `document.body`.
 *
 * The visible indicator has two halves: the focused region container
 * carries `data-focus-region` and an outline, and the focused control
 * carries the `focus-ring` class. The control's ring is drawn from the
 * class and not only from `:focus-visible`, because a mouse user must
 * also see where the focus is.
 *
 * This module holds no durable state and reaches no authority: it reads
 * and writes the DOM, and nothing else.
 */

/** A region of the frame, in focus order. */
export interface RegionDefinition {
  /** The region's identity — the key a chord names. */
  id: string;
  /** The region's container `data-testid` (M007-B's). */
  testId: string;
  /** The name the focus indicator and the bindings surface show. */
  label: string;
  /** The region the focus falls back to when this one unmounts. */
  fallbackId: string;
}

/**
 * The frame's regions, in focus order.
 *
 * The order is the reading order of the shell: the catalog rail on the
 * left, then the conversation, then the panel column beside it, then the
 * composer, then the status row. The side panel is *absent* whenever its
 * slot is hidden, and a region that is not in the DOM is not a focus
 * target — `mod+3` then moves nothing, which is why the side panel has
 * its own toggle.
 *
 * The terminal comes last: it is a **panel** inside the side panel rather
 * than a region of the frame, so it does not take a place in the reading
 * order, and it keeps the frame's own chords where they were. It is a
 * region all the same, so the focus can move into it, the focus indicator
 * can mark it, and the elements under it belong to it rather than to the
 * side panel around it.
 */
export const REGIONS: readonly RegionDefinition[] = [
  { id: 'catalog', testId: 'catalog-region', label: 'Catalog', fallbackId: 'composer' },
  { id: 'chat', testId: 'chat-region', label: 'Conversation', fallbackId: 'composer' },
  { id: 'sidePanel', testId: 'side-panel-frame', label: 'Side panel', fallbackId: 'composer' },
  { id: 'composer', testId: 'composer-region', label: 'Composer', fallbackId: 'composer' },
  { id: 'status', testId: 'status-row', label: 'Status', fallbackId: 'composer' },
  { id: 'terminal', testId: 'terminal-panel', label: 'Terminal', fallbackId: 'composer' },
];

/** Every region's identity, in focus order. */
export const REGION_IDS: readonly string[] = REGIONS.map((region) => region.id);

/** The region the focus falls back to when the focused region unmounts. */
export const FALLBACK_REGION_ID = 'composer';

/** The class the focused control carries. */
export const FOCUS_RING_CLASS = 'focus-ring';

/** The attribute the focused region container carries. */
export const FOCUS_REGION_ATTRIBUTE = 'data-focus-region';

/**
 * The controls the traversal moves between.
 *
 * A disabled control is not a control, and the roving `tabindex` marks
 * are excluded so the traversal never lands on the element it just left.
 */
const FOCUSABLE_SELECTOR = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(', ');

/** The region a region id names. */
export function regionById(regionId: string | null | undefined): RegionDefinition | undefined {
  if (!regionId) return undefined;
  return REGIONS.find((region) => region.id === regionId);
}

/** The container element of a region, or null when the region is absent. */
export function regionContainer(root: ParentNode, regionId: string): HTMLElement | null {
  const region = regionById(regionId);
  if (!region) return null;
  return root.querySelector<HTMLElement>(`[data-testid="${region.testId}"]`);
}

/**
 * The region an element belongs to.
 *
 * The regions are disjoint, so the nearest container wins; an element
 * outside every region (the page chrome) belongs to none.
 *
 * The terminal is a panel *inside* the side panel, and it is declared last:
 * the nearest container is the rule, so an element inside the terminal
 * belongs to the terminal rather than to the side panel around it. The
 * side panel's own controls still belong to the side panel.
 */
export function regionOfElement(element: Element | null): RegionDefinition | undefined {
  if (!element) return undefined;
  let nearest: RegionDefinition | undefined;
  let nearestDepth = Number.POSITIVE_INFINITY;
  for (const region of REGIONS) {
    const container = element.closest(`[data-testid="${region.testId}"]`);
    if (!container) continue;
    let depth = 0;
    let node: Element | null = element;
    while (node && node !== container) {
      depth += 1;
      node = node.parentElement;
    }
    if (depth < nearestDepth) {
      nearest = region;
      nearestDepth = depth;
    }
  }
  return nearest;
}

/** The focusable controls of a container, in document order. */
export function focusableControls(container: Element): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
}

/** The focused element inside a container, or null when the focus is elsewhere. */
export function activeElementWithin(container: Element): HTMLElement | null {
  const active = container.ownerDocument?.activeElement;
  if (!active || !(active instanceof HTMLElement)) return null;
  return container.contains(active) ? active : null;
}

/**
 * Mark the region's controls for a roving `tabindex`.
 *
 * Exactly one control — the active one — carries `tabindex="0"`; the
 * rest carry `-1`. That is what keeps the browser's own `Tab` inside the
 * focused region, and what makes the traversal deterministic: the
 * region's controls have an order, and the traversal walks it.
 */
export function applyRovingTabIndex(container: Element, active: Element | null): void {
  for (const control of focusableControls(container)) {
    control.setAttribute('tabindex', control === active ? '0' : '-1');
  }
}

/**
 * Make the region's container focusable.
 *
 * The container is a landmark, not a control, so it is marked
 * `tabindex="-1"`: it can hold the focus programmatically (a click on an
 * empty part of the region, or a fallback) but it never appears in the
 * traversal.
 */
export function prepareRegionContainers(root: ParentNode): void {
  for (const region of REGIONS) {
    const container = regionContainer(root, region.id);
    if (container && !container.hasAttribute('tabindex')) {
      container.setAttribute('tabindex', '-1');
    }
  }
}

/**
 * Draw the focus indicator.
 *
 * The focused region container carries `data-focus-region`; the focused
 * control carries the `focus-ring` class. The indicator is drawn from the
 * class and not from `:focus-visible` alone, so a mouse user sees it
 * too.
 */
export function applyFocusIndicator(root: ParentNode, regionId: string | null): void {
  for (const region of REGIONS) {
    const container = regionContainer(root, region.id);
    if (!container) continue;
    if (region.id === regionId) container.setAttribute(FOCUS_REGION_ATTRIBUTE, region.id);
    else container.removeAttribute(FOCUS_REGION_ATTRIBUTE);
  }

  for (const element of Array.from(root.querySelectorAll(`.${FOCUS_RING_CLASS}`))) {
    element.classList.remove(FOCUS_RING_CLASS);
  }

  if (!regionId) return;
  const container = regionContainer(root, regionId);
  const active = container?.ownerDocument?.activeElement ?? null;
  if (container && active instanceof HTMLElement && container.contains(active)) {
    active.classList.add(FOCUS_RING_CLASS);
  }
}

/**
 * Move the focus to a region.
 *
 * The focus lands on the control the region already had, or on its first
 * control, or on the container itself when the region has none. The
 * region's controls are re-marked for the roving `tabindex` so the
 * traversal starts from the control that holds the focus.
 */
export function focusRegion(root: ParentNode, regionId: string): boolean {
  const container = regionContainer(root, regionId);
  if (!container) return false;
  const controls = focusableControls(container);
  const current = activeElementWithin(container);
  const target = current ?? controls[0] ?? container;
  if (target instanceof HTMLElement) {
    if (target === container && !container.hasAttribute('tabindex')) {
      container.setAttribute('tabindex', '-1');
    }
    target.focus();
  }
  applyRovingTabIndex(container, target);
  return true;
}

/**
 * Move the focus within a region, wrapping at the ends.
 *
 * `delta` is `1` for `Tab` and `-1` for `Shift+Tab`. The traversal wraps:
 * the control after the last one is the first, and the control before the
 * first one is the last (the TUI's rule, `architecture/tui.md:4884-4886`).
 */
export function moveFocusWithinRegion(root: ParentNode, regionId: string, delta: number): boolean {
  const container = regionContainer(root, regionId);
  if (!container) return false;
  const controls = focusableControls(container);
  if (controls.length === 0) return false;
  const current = activeElementWithin(container);
  const index = current ? controls.indexOf(current) : -1;
  const start = index === -1 ? (delta > 0 ? 0 : controls.length - 1) : index;
  const next = (start + delta + controls.length) % controls.length;
  const target = controls[next];
  target.focus();
  applyRovingTabIndex(container, target);
  return true;
}

/**
 * Move the focus to a region's declared fallback.
 *
 * A region that unmounts must not leave the focus on `document.body`:
 * a keyboard user with no focus at all cannot reach anything. The
 * fallback is the region the removed region declares — the composer, for
 * every region of the frame.
 */
export function focusFallback(root: ParentNode, regionId: string): boolean {
  const region = regionById(regionId);
  const fallbackId = region?.fallbackId ?? FALLBACK_REGION_ID;
  return focusRegion(root, fallbackId);
}

/**
 * The first element matching a `data-testid` prefix.
 *
 * The affordances of the catalog's lists are *families* — every project
 * row carries `project-select-<id>` — so an action that opens one of them
 * focuses the first member.
 */
export function firstByTestIdPrefix(root: ParentNode, prefix: string): HTMLElement | null {
  return root.querySelector<HTMLElement>(`[data-testid^="${prefix}"]`);
}
