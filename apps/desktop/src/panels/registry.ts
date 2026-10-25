import { createElement } from 'react';
import type { ReactElement } from 'react';
import type { ControllerSummaryView, SessionPresentationView } from '../bridge-types';
import type { EditorPanelProps } from '../editor/contract';
import { ActivityPanel } from './ActivityPanel';
import { EditorPanel } from './EditorPanel';
import { SessionPanel } from './SessionPanel';
import { SubagentPanel } from './SubagentPanel';
import { TaskPanel } from './TaskPanel';

// The side-panel registry: the desktop's panel catalogue.
//
// It is **data**, not a switch statement: the frame renders whatever the
// registry names, so a second list that could drift from the render path
// never exists. The TUI's manager follows the same rule — it renders its
// own id list directly (`src/tui/panels/mod.rs:500-506`).
//
// The desktop mirrors the TUI's *model* — an id, a label, a placement —
// and not its catalogue. The TUI has 32 panel kinds
// (`src/tui/panels/mod.rs:513-546`); the desktop ships the four it has
// content for. A panel that renders nothing would be a claim the renderer
// cannot keep.
//
// The registry holds no authority: every panel below renders props the
// renderer was given, and none of them calls the bridge.

export type PanelPlacement = 'right' | 'left' | 'bottom';

export type PanelId = 'tasks' | 'subagents' | 'activity' | 'session' | 'editor';

export interface PanelProps {
  /** The session's presentation view, or null when nothing is attached. */
  projection: SessionPresentationView | null;
  /** The controller lease the renderer last read. */
  controller: ControllerSummaryView | null;
  /** Whether the todo read is in flight. */
  todoBusy: boolean;
  /** The last todo read's failure, if it failed. */
  todoError: string | null;
  /** Re-read the session's todo list from the host. */
  onRefreshTodos?: () => void;
  /**
   * The editor's state and intents.
   *
   * The editor is a panel like any other: it renders what the session gave
   * it and fetches nothing itself.
   */
  editor: EditorPanelProps;
}

export type PanelRenderer = (props: PanelProps) => ReactElement | null;

export interface PanelDefinition {
  id: PanelId;
  /** The name the frame's own controls show. */
  label: string;
  /** The panel's renderer. It renders props; it fetches nothing. */
  render: PanelRenderer;
}

export const PANEL_REGISTRY: Record<PanelId, PanelDefinition> = {
  tasks: {
    id: 'tasks',
    label: 'Tasks',
    render: (props) =>
      createElement(TaskPanel, {
        todos: props.projection?.todos ?? null,
        jobs: props.projection?.jobs ?? [],
        subagents: props.projection?.subagents ?? [],
        runs: props.projection?.runs ?? [],
        recentTurns: props.projection?.recentTurns ?? [],
        state: props.projection?.state ?? null,
        busy: props.todoBusy,
        error: props.todoError,
        onRefresh: props.onRefreshTodos,
      }),
  },
  subagents: {
    id: 'subagents',
    label: 'Subagents',
    render: (props) =>
      createElement(SubagentPanel, {
        subagents: props.projection?.subagents ?? [],
        agentTree: props.projection?.agentTree ?? [],
      }),
  },
  activity: {
    id: 'activity',
    label: 'Activity',
    render: (props) =>
      createElement(ActivityPanel, {
        agentRuns: props.projection?.agentRuns ?? [],
        toolPrograms: props.projection?.toolPrograms ?? [],
        worktrees: props.projection?.worktrees ?? [],
        runGroups: props.projection?.runGroups ?? [],
        convergences: props.projection?.convergences ?? [],
      }),
  },
  session: {
    id: 'session',
    label: 'Session',
    render: (props) =>
      createElement(SessionPanel, {
        sessionId: props.projection?.sessionId ?? '',
        projectId: props.projection?.projectId ?? '',
        workspaceId: props.projection?.workspaceId ?? '',
        state: props.projection?.state ?? 'unknown',
        turn: props.projection?.turn ?? null,
        controller: props.controller,
      }),
  },
  editor: {
    id: 'editor',
    label: 'Editor',
    render: (props) => createElement(EditorPanel, { editor: props.editor }),
  },
};

/** Every registered panel id, in registry order. */
export const PANEL_IDS = Object.keys(PANEL_REGISTRY) as PanelId[];
