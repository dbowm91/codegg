import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { App } from '../App';
import { bridge } from '../bridge';
import type { SessionPresentationView } from '../bridge-types';

// The composed tree: the frame's regions and the projection fields that
// reach it. This is the renderer half of M007-B's contract — the frame is
// split, and the split drops nothing.

vi.mock('../bridge', () => ({
  bridge: {
    snapshot: vi.fn(),
    connect: vi.fn(),
    projects: vi.fn(),
    projectDetail: vi.fn(),
    workspaceSelect: vi.fn(),
    sessionList: vi.fn(),
    sessionOpen: vi.fn(),
    sessionCreate: vi.fn(),
    promptSubmit: vi.fn(),
    controlRefresh: vi.fn(),
    permissionRespond: vi.fn(),
    questionRespond: vi.fn(),
    artifactRead: vi.fn(),
    projectionStart: vi.fn(),
    projectionStop: vi.fn(),
    projectionCurrent: vi.fn(),
    todoRefresh: vi.fn(),
    gitRead: vi.fn(),
    skillCatalogue: vi.fn(),
    subscribeProjection: vi.fn(),
    subscribe: vi.fn(),
    disconnect: vi.fn(),
    unsubscribe: vi.fn(),
  },
}));

const connected = (generation: number) => ({
  state: 'connected' as const,
  daemonId: 'daemon-test',
  protocolVersion: 1,
  uptimeSeconds: 7,
  activeSessions: 2,
  error: null,
  connectionGeneration: generation,
});

const token = (routeGeneration: number, sessionId: string | null = null) => ({
  connectionGeneration: 1,
  routeGeneration,
  projectId: 'p0',
  workspaceId: sessionId ? 'w0' : '',
  sessionId,
});

const presentation = (sessionId: string): SessionPresentationView => ({
  sessionId,
  projectId: 'p0',
  workspaceId: 'w0',
  state: 'attached',
  turn: {
    turnId: 'turn-1',
    status: 'active',
    updatedAt: 7,
    stopReason: null,
    error: null,
    messageCount: 1,
    toolCount: 1,
    pendingPermissions: 1,
    pendingQuestions: 1,
    inputTokens: 10,
    outputTokens: 20,
    cachedTokens: 0,
    contextLimit: 200000,
    reasoningTokens: 0,
  },
  messages: [{ messageId: 'm-1', role: 'assistant', text: 'on it', truncated: false }],
  truncatedMessages: 0,
  transcript: [],
  truncatedTranscriptMessages: 0,
  tools: [
    {
      toolId: 't-1',
      toolName: 'bash',
      status: 'ok',
      summary: 'cargo test',
      hasArtifact: false,
      arguments: '',
      output: '',
      startedAt: null,
      completedAt: null,
      durationMs: null,
    },
  ],
  runs: [{ runId: 'run-1', kind: 'test', status: 'running', summary: 'cargo test' }],
  jobs: [{ jobId: 'job-1', kind: 'shell', state: 'running', summary: 'cargo fmt' }],
  subagents: [
    {
      taskId: 1,
      agent: 'build',
      description: 'compile',
      status: 'running',
      resultSummary: null,
      childInstanceId: 'child-1',
      parentTaskId: null,
      originatingTurnId: 'turn-1',
      displayName: null,
      runId: null,
    },
  ],
  agentTree: [],
  agentRuns: [],
  toolPrograms: [],
  worktrees: [],
  runGroups: [],
  convergences: [],
  recentTurns: [
    {
      turnId: 'turn-0',
      status: 'completed',
      updatedAt: 3,
      stopReason: 'finished',
      error: null,
      messageCount: 4,
      toolCount: 2,
      pendingPermissions: 0,
      pendingQuestions: 0,
      inputTokens: 10,
      outputTokens: 20,
      cachedTokens: 0,
      contextLimit: 200000,
      reasoningTokens: 0,
    },
  ],
  todos: null,
  pendingPermissions: [
    { permissionId: 'perm-1', tool: 'write', scopeSummary: null, status: 'pending' },
  ],
  pendingQuestions: [
    { questionId: 'q-1', header: 'Choice', prompt: 'Pick one', status: 'pending' },
  ],
  controller: null,
  artifactHandles: [{ handle: 'handle-1', byteLength: 6 }],
  cursor: { eventSeq: 42, driverCursorSeq: 42, subscriptionKnown: true },
  resyncReason: null,
});

async function attachSession() {
  render(<App />);
  await waitFor(() => expect(screen.getByText('Demo project')).toBeInTheDocument());
  await act(async () => {
    fireEvent.click(screen.getByTestId('project-select-p0'));
  });
  await waitFor(() => expect(screen.getByTestId('workspace-list')).toBeInTheDocument());
  await act(async () => {
    fireEvent.click(screen.getByTestId('workspace-select-w0'));
  });
  await waitFor(() => expect(screen.getByTestId('session-list')).toBeInTheDocument());
  await act(async () => {
    fireEvent.click(screen.getByTestId('session-open-s-1'));
  });
  await waitFor(() => expect(screen.getByTestId('projection-state')).toBeInTheDocument());
}

beforeEach(() => {
  vi.mocked(bridge.snapshot).mockResolvedValue(connected(1));
  vi.mocked(bridge.connect).mockResolvedValue(connected(1));
  vi.mocked(bridge.projects).mockResolvedValue([
    { projectId: 'p0', displayName: 'Demo project', lifecycle: 'active' },
  ]);
  vi.mocked(bridge.disconnect).mockResolvedValue(undefined);
  vi.mocked(bridge.unsubscribe).mockResolvedValue(undefined);
  vi.mocked(bridge.subscribe).mockImplementation(async () => ({
    subscriptionId: 'desktop-sub-1',
    connectionGeneration: 1,
    unsubscribe: vi.fn(),
  }));
  vi.mocked(bridge.projectDetail).mockImplementation(async () => ({
    projectId: 'p0',
    displayName: 'Demo project',
    workspaces: [{ workspaceId: 'w0', displayName: 'Workspace' }],
    sessionCount: 1,
    routeToken: token(1),
  }));
  vi.mocked(bridge.workspaceSelect).mockImplementation(async (_workspaceId: string, routeGeneration: number) =>
    token(routeGeneration + 1),
  );
  vi.mocked(bridge.sessionList).mockImplementation(async (routeGeneration: number) => ({
    sessions: [{ sessionId: 's-1', title: 'First', projectId: 'p0', workspaceId: 'w0' }],
    routeToken: token(routeGeneration, null),
  }));
  vi.mocked(bridge.sessionOpen).mockImplementation(async (sessionId: string, routeGeneration: number) => ({
    session: { sessionId, title: 'First', projectId: 'p0', workspaceId: 'w0' },
    routeToken: token(routeGeneration, sessionId),
  }));
  vi.mocked(bridge.projectionStart).mockImplementation(async (sessionId: string) => presentation(sessionId));
  vi.mocked(bridge.projectionCurrent).mockImplementation(async () => presentation('s-1'));
  vi.mocked(bridge.todoRefresh).mockImplementation(async () => presentation('s-1'));
  vi.mocked(bridge.controlRefresh).mockResolvedValue(null);
  vi.mocked(bridge.gitRead).mockResolvedValue({
    kind: 'status',
    status: {
      branch: 'main',
      isDirty: false,
      commitHash: null,
      stashCount: 0,
      consistency: 'immutable_generation',
    },
    diff: null,
    branches: null,
  });
  vi.mocked(bridge.skillCatalogue).mockResolvedValue({
    projectId: 'p0',
    skills: [],
    total: 0,
    truncated: false,
  });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('the composed desktop frame', () => {
  it('draws the chat region and the side panel as distinct elements', async () => {
    await attachSession();
    const chat = screen.getByTestId('chat-region');
    const panel = screen.getByTestId('side-panel-frame');
    expect(chat).not.toBe(panel);
    expect(panel.contains(chat)).toBe(false);
    expect(chat.contains(panel)).toBe(false);
  });

  it('keeps every region of the frame in the composed tree', async () => {
    await attachSession();
    expect(screen.getByTestId('chat-region')).toBeInTheDocument();
    expect(screen.getByTestId('side-panel-frame')).toBeInTheDocument();
    expect(screen.getByTestId('composer-region')).toBeInTheDocument();
    expect(screen.getByTestId('status-row')).toBeInTheDocument();
  });

  it('keeps the projection’s messages, tools, artifacts and pending items', async () => {
    await attachSession();
    expect(screen.getByTestId('message-item')).toHaveTextContent('on it');
    expect(screen.getByTestId('tool-item')).toHaveTextContent('cargo test');
    expect(screen.getByTestId('artifact-item')).toHaveTextContent('handle-1');
    expect(screen.getByTestId('permission-item')).toHaveTextContent('write');
    expect(screen.getByTestId('question-item')).toHaveTextContent('Pick one');
  });

  it('keeps the projection’s runs, live jobs and recent turns', async () => {
    await attachSession();
    expect(screen.getByTestId('run-history-item')).toHaveTextContent('cargo test');
    expect(screen.getByTestId('task-job-item')).toHaveTextContent('cargo fmt');
    expect(screen.getByTestId('recent-turn-item')).toHaveTextContent('turn-0');
  });

  it('keeps the projection’s subagents in the task surface', async () => {
    await attachSession();
    // The task surface mirrors the TUI's Ctrl+T overlay: todos, in-flight
    // subagents, in-flight jobs.
    expect(screen.getByTestId('task-subagent-item')).toHaveTextContent('build');
  });

  it('keeps the projection’s subagents behind the subagents panel too', async () => {
    await attachSession();
    await act(async () => {
      fireEvent.click(screen.getByTestId('panel-tab-subagents'));
    });
    expect(screen.getByTestId('subagent-item')).toHaveTextContent('build');
  });

  it('docks the panel on the edge the user picks', async () => {
    await attachSession();
    expect(screen.getByTestId('side-panel-frame')).toHaveAttribute('data-placement', 'right');
    await act(async () => {
      fireEvent.click(screen.getByTestId('panel-edge-bottom'));
    });
    expect(screen.getByTestId('side-panel-frame')).toHaveAttribute('data-placement', 'bottom');
    // The panel is moved, not copied: it is on one edge only.
    expect(screen.getAllByTestId('side-panel-frame')).toHaveLength(1);
  });
});
