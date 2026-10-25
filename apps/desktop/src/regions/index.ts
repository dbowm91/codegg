import { ChatRegion } from './ChatRegion';
import { ComposerRegion } from './ComposerRegion';
import { PanelControls, SidePanelFrame } from './SidePanelFrame';
import { StatusRow } from './StatusRow';

// The frame's four regions.
//
// The map is the registry the frame is built from, and the components
// themselves are the render path — so the two cannot drift apart. The
// desktop mirrors the TUI's *shape* (a main region beside a side region,
// a composer beneath both, and a status row at the bottom) and not the
// TUI's catalogue of panes.

export const REGION_COMPONENTS = {
  chat: ChatRegion,
  sidePanel: SidePanelFrame,
  composer: ComposerRegion,
  status: StatusRow,
} as const;

export type RegionId = keyof typeof REGION_COMPONENTS;

/** Every region the frame is built from, in registry order. */
export const REGION_IDS = Object.keys(REGION_COMPONENTS) as RegionId[];

export { ChatRegion, ComposerRegion, PanelControls, SidePanelFrame, StatusRow };
export type { ChatRegionProps } from './ChatRegion';
export type { ComposerRegionProps } from './ComposerRegion';
export type { PanelControlsProps, SidePanelFrameProps } from './SidePanelFrame';
export type { StatusRowProps } from './StatusRow';
