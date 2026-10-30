import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import type { ReactElement } from 'react';
import { PANEL_IDS, PANEL_REGISTRY } from './registry';
import type { PanelId, PanelProps } from './registry';
import type { SessionPresentationView } from '../bridge-types';
import { emptyEditorPanelProps } from '../editor/fixtures';
import { emptyTerminalPanelProps } from '../terminal/contract';

const turn = {
  turnId: 'turn-1',
  status: 'active',
  updatedAt: 7,
  stopReason: null,
  error: null,
  messageCount: 1,
  toolCount: 0,
  pendingPermissions: 0,
  pendingQuestions: 0,
  inputTokens: 10,
  outputTokens: 20,
  cachedTokens: 0,
  contextLimit: 200000,
  reasoningTokens: 0,
};

const projection: SessionPresentationView = {
  sessionId: 's-1',
  projectId: 'p0',
  workspaceId: 'w0',
  state: 'attached',
  selectedModel: null,
  selectedAgent: null,
  turn,
  messages: [],
  truncatedMessages: 0,
  transcript: [],
  truncatedTranscriptMessages: 0,
  tools: [],
  runs: [{ runId: 'run-1', kind: 'test', status: 'running', summary: 'cargo test' }],
  jobs: [],
  subagents: [],
  agentTree: [],
  agentRuns: [],
  toolPrograms: [],
  worktrees: [],
  runGroups: [],
  convergences: [],
  shellRuns: [],
  toolContracts: [],
  recentTurns: [turn],
  todos: null,
  pendingPermissions: [],
  pendingQuestions: [],
  controller: null,
  artifactHandles: [],
  cursor: { eventSeq: 42, driverCursorSeq: 42, subscriptionKnown: true },
  resyncReason: null,
};

const props: PanelProps = {
  projection,
  controller: null,
  todoBusy: false,
  todoError: null,
  editor: emptyEditorPanelProps(),
  terminal: emptyTerminalPanelProps(),
};

const surfaces: Record<PanelId, string> = {
  tasks: 'task-surface',
  subagents: 'subagent-surface',
  activity: 'activity-surface',
  session: 'session-panel-surface',
  editor: 'editor-surface',
  terminal: 'terminal-panel',
};

describe('the desktop panel registry', () => {
  it('registers the six panels the desktop has content for', () => {
    expect(PANEL_IDS).toEqual(['tasks', 'subagents', 'activity', 'session', 'editor', 'terminal']);
  });

  it('gives every panel a label of its own', () => {
    const labels = PANEL_IDS.map((id) => PANEL_REGISTRY[id].label);
    expect(new Set(labels).size).toBe(PANEL_IDS.length);
    expect(labels.every((label) => label.length > 0)).toBe(true);
  });

  for (const id of PANEL_IDS) {
    it(`renders the ${id} panel's own surface`, () => {
      const element = PANEL_REGISTRY[id].render(props);
      // A registry entry that renders nothing is a claim the renderer
      // cannot keep.
      expect(element).not.toBeNull();
      render(element as ReactElement);
      expect(screen.getByTestId(surfaces[id])).toBeInTheDocument();
    });
  }

  it('renders the projection the panel is given, not one of its own', () => {
    const element = PANEL_REGISTRY.tasks.render(props);
    render(element as ReactElement);
    // The run the projection carries reaches the task surface.
    expect(screen.getByTestId('run-history-item')).toHaveTextContent('cargo test');
  });

  it('renders a panel that has no projection without inventing one', () => {
    const element = PANEL_REGISTRY.session.render({ ...props, projection: null });
    render(element as ReactElement);
    expect(screen.getByTestId('session-panel-state')).toHaveTextContent('unknown');
  });
});
