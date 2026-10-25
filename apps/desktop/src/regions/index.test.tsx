import { describe, expect, it } from 'vitest';
import { REGION_COMPONENTS, REGION_IDS } from './index';

describe('the desktop region registry', () => {
  it('names the four regions of the frame', () => {
    expect(REGION_IDS).toEqual(['chat', 'sidePanel', 'composer', 'status']);
  });

  it('gives every region a component of its own', () => {
    const components = REGION_IDS.map((id) => REGION_COMPONENTS[id]);
    // A registry whose entries share a component is a registry that names
    // one surface twice.
    expect(new Set(components).size).toBe(REGION_IDS.length);
  });
});
