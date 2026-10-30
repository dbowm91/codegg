import { afterEach, describe, expect, it } from 'vitest';
import {
  FALLBACK_REGION_ID,
  FOCUS_REGION_ATTRIBUTE,
  FOCUS_RING_CLASS,
  REGION_IDS,
  activeElementWithin,
  applyFocusIndicator,
  applyRovingTabIndex,
  focusFallback,
  focusRegion,
  firstByTestIdPrefix,
  focusableControls,
  moveFocusWithinRegion,
  prepareRegionContainers,
  regionById,
  regionContainer,
  regionOfElement,
} from './focus';

/**
 * A frame with the same shape the renderer has: a catalog rail, a
 * conversation, a side panel, a composer and a status row, each in its
 * own container.
 */
function mountFrame(): HTMLElement {
  document.body.innerHTML = `
    <div data-testid="catalog-region">
      <button data-testid="project-select-p0">Project</button>
      <button data-testid="reconnect-button">Reconnect</button>
    </div>
    <div data-testid="chat-region">
      <button data-testid="chat-first">First</button>
      <button data-testid="chat-second">Second</button>
    </div>
    <aside data-testid="side-panel-frame">
      <button data-testid="panel-first">Panel</button>
    </aside>
    <section data-testid="composer-region">
      <textarea data-testid="prompt-input"></textarea>
      <button data-testid="prompt-submit-button">Send</button>
    </section>
    <section data-testid="status-row">
      <button data-testid="status-first">Status</button>
    </section>
  `;
  return document.body;
}

afterEach(() => {
  document.body.innerHTML = '';
});

describe('the region registry', () => {
  it('names_the_frame_regions_in_focus_order', () => {
    expect(REGION_IDS).toEqual([
      'catalog',
      'chat',
      'sidePanel',
      'composer',
      'status',
      'terminal',
    ]);
    expect(regionById('chat')?.testId).toBe('chat-region');
    expect(regionById('terminal')?.testId).toBe('terminal-panel');
    expect(regionById('nope')).toBeUndefined();
    expect(FALLBACK_REGION_ID).toBe('composer');
  });

  it('finds_the_region_an_element_belongs_to', () => {
    mountFrame();
    const project = document.querySelector('[data-testid="project-select-p0"]');
    expect(regionOfElement(project)?.id).toBe('catalog');
    expect(regionOfElement(document.querySelector('[data-testid="prompt-input"]'))?.id).toBe('composer');
    // An element outside every region belongs to none.
    expect(regionOfElement(document.createElement('div'))).toBeUndefined();
    expect(regionOfElement(null)).toBeUndefined();
  });

  it('finds_the_container_of_a_region', () => {
    mountFrame();
    expect(regionContainer(document, 'composer')?.getAttribute('data-testid')).toBe('composer-region');
    expect(regionContainer(document, 'nope')).toBeNull();
  });
});

describe('the region focus', () => {
  it('moves_the_focus_into_the_region_and_its_first_control', () => {
    mountFrame();
    expect(focusRegion(document, 'chat')).toBe(true);
    expect(document.activeElement?.getAttribute('data-testid')).toBe('chat-first');
    // A region that is not in the DOM is not a focus target.
    expect(focusRegion(document, 'status')).toBe(true);
  });

  it('marks_exactly_one_control_for_the_roving_tabindex', () => {
    mountFrame();
    const container = regionContainer(document, 'chat');
    expect(container).not.toBeNull();
    focusRegion(document, 'chat');
    applyRovingTabIndex(container as HTMLElement, document.activeElement);
    const tabbable = focusableControls(container as HTMLElement).filter(
      (control) => control.getAttribute('tabindex') === '0',
    );
    expect(tabbable).toHaveLength(1);
    expect(tabbable[0].getAttribute('data-testid')).toBe('chat-first');
    // The other control is reachable only through the traversal.
    const other = focusableControls(container as HTMLElement).find(
      (control) => control.getAttribute('data-testid') === 'chat-second',
    );
    expect(other?.getAttribute('tabindex')).toBe('-1');
  });

  it('traverses_within_the_region_and_wraps_at_the_ends', () => {
    mountFrame();
    focusRegion(document, 'chat');
    expect(moveFocusWithinRegion(document, 'chat', 1)).toBe(true);
    expect(document.activeElement?.getAttribute('data-testid')).toBe('chat-second');
    // The end wraps to the beginning.
    expect(moveFocusWithinRegion(document, 'chat', 1)).toBe(true);
    expect(document.activeElement?.getAttribute('data-testid')).toBe('chat-first');
    // And backwards it wraps too.
    expect(moveFocusWithinRegion(document, 'chat', -1)).toBe(true);
    expect(document.activeElement?.getAttribute('data-testid')).toBe('chat-second');
  });

  it('falls_back_to_the_declared_region_when_the_focused_one_is_gone', () => {
    mountFrame();
    focusRegion(document, 'sidePanel');
    expect(activeElementWithin(regionContainer(document, 'sidePanel') as HTMLElement)).not.toBeNull();
    // The side panel unmounts with its slot.
    regionContainer(document, 'sidePanel')?.remove();
    expect(focusFallback(document, 'sidePanel')).toBe(true);
    expect(document.activeElement?.getAttribute('data-testid')).toBe('prompt-input');
    expect(document.activeElement).not.toBe(document.body);
  });

  it('makes_the_region_container_focusable_without_adding_it_to_the_traversal', () => {
    mountFrame();
    prepareRegionContainers(document);
    const container = regionContainer(document, 'status');
    expect(container?.getAttribute('tabindex')).toBe('-1');
    // The container is not one of the region's controls.
    expect(
      focusableControls(container as HTMLElement).some(
        (control) => control === container,
      ),
    ).toBe(false);
  });
});

describe('the focus indicator', () => {
  it('marks_the_focused_region_and_the_focused_control', () => {
    mountFrame();
    focusRegion(document, 'chat');
    applyFocusIndicator(document, 'chat');
    expect(regionContainer(document, 'chat')?.getAttribute(FOCUS_REGION_ATTRIBUTE)).toBe('chat');
    expect(regionContainer(document, 'composer')?.hasAttribute(FOCUS_REGION_ATTRIBUTE)).toBe(false);
    expect(document.activeElement?.classList.contains(FOCUS_RING_CLASS)).toBe(true);
  });

  it('clears_the_indicator_when_the_focus_leaves', () => {
    mountFrame();
    focusRegion(document, 'chat');
    applyFocusIndicator(document, 'chat');
    applyFocusIndicator(document, null);
    expect(regionContainer(document, 'chat')?.hasAttribute(FOCUS_REGION_ATTRIBUTE)).toBe(false);
    expect(document.querySelectorAll(`.${FOCUS_RING_CLASS}`)).toHaveLength(0);
  });
});

describe('the affordance families', () => {
  it('finds_the_first_member_of_a_test_id_family', () => {
    mountFrame();
    expect(firstByTestIdPrefix(document, 'project-select-')?.getAttribute('data-testid')).toBe(
      'project-select-p0',
    );
    expect(firstByTestIdPrefix(document, 'nothing-')).toBeNull();
  });
});
