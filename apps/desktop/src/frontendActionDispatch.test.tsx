import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { App } from './App';
import { bridge } from './bridge';
import type { CommandCatalogView } from './bridge-types';

// M007-017: the desktop's half of a declared frontend action.
//
// The daemon's catalog publishes the declared action's wire name on the
// form, and the desktop dispatches by that name. The command spellings here
// are fixtures: the daemon's catalog is the only source of real spellings,
// and the renderer never holds one of its own.
vi.mock('./bridge', () => ({
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
    documentOpen: vi.fn(),
    documentViewport: vi.fn(),
    documentCursor: vi.fn(),
    documentEdit: vi.fn(),
    documentUndo: vi.fn(),
    documentRedo: vi.fn(),
    documentSave: vi.fn(),
    documentReload: vi.fn(),
    documentResync: vi.fn(),
    documentClose: vi.fn(),
    documentState: vi.fn(),
    documentTree: vi.fn(),
    documentDiagnostics: vi.fn(),
    documentCompletion: vi.fn(),
    documentNavigate: vi.fn(),
    commandCatalog: vi.fn(),
    commandExecute: vi.fn(),
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
  activeSessions: 1,
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

const presentation = () => ({
  sessionId: 's-1',
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
    toolCount: 0,
    pendingPermissions: 0,
    pendingQuestions: 0,
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
  tools: [],
  runs: [],
  jobs: [],
  subagents: [],
  agentTree: [],
  agentRuns: [],
  toolPrograms: [],
  worktrees: [
    {
      worktreeId: 'wt-1',
      ownerRunId: 'run-1',
      branch: 'main',
      state: 'ready',
      health: 'ok',
      dirty: false,
      conflicted: false,
      retainedForAttention: false,
      updatedAt: 1,
    },
  ],
  runGroups: [],
  convergences: [],
  recentTurns: [],
  todos: null,
  pendingPermissions: [],
  pendingQuestions: [],
  controller: null,
  artifactHandles: [],
  cursor: { eventSeq: 42, driverCursorSeq: 42, subscriptionKnown: true },
  resyncReason: null,
});

/**
 * A catalog whose forms carry the daemon's published action.
 *
 * The four commands stand for the six forms the correction moves to the
 * frontend: one the desktop performs, two it knows it cannot perform, and
 * one whose form published no action at all.
 */
const catalog = (): CommandCatalogView => ({
  catalogVersion: 7,
  commands: [
    {
      commandId: 'fixture-skills',
      name: '/fixture-skills',
      aliases: [],
      description: 'Inspect the effective skills',
      domain: 'Memory',
      kind: 'frontend_effect',
      capability: null,
      forms: [
        {
          form: '',
          kind: 'frontend_effect',
          argumentHint: '[name]',
          argumentRequired: false,
          daemonServed: true,
          action: 'show_skills',
        },
      ],
    },
    {
      commandId: 'fixture-status',
      name: '/fixture-status',
      aliases: [],
      description: 'Report the session status',
      domain: 'System',
      kind: 'frontend_effect',
      capability: null,
      forms: [
        {
          form: '',
          kind: 'frontend_effect',
          argumentHint: '',
          argumentRequired: false,
          daemonServed: true,
          action: 'show_status',
        },
      ],
    },
    {
      commandId: 'fixture-worktrees',
      name: '/fixture-worktrees',
      aliases: [],
      description: 'List the managed worktrees',
      domain: 'System',
      kind: 'frontend_effect',
      capability: null,
      forms: [
        {
          form: '',
          kind: 'frontend_effect',
          argumentHint: '',
          argumentRequired: false,
          daemonServed: true,
          action: 'show_worktrees',
        },
      ],
    },
    {
      commandId: 'fixture-contracts',
      name: '/fixture-contracts',
      aliases: [],
      description: 'Inspect the tool contracts',
      domain: 'Diagnostics',
      kind: 'frontend_effect',
      capability: null,
      forms: [
        {
          form: '',
          kind: 'frontend_effect',
          argumentHint: '',
          argumentRequired: false,
          daemonServed: true,
          action: 'show_tool_contracts',
        },
      ],
    },
    {
      commandId: 'fixture-unpublished',
      name: '/fixture-unpublished',
      aliases: [],
      description: 'A frontend form the daemon published no action for',
      domain: 'System',
      kind: 'frontend_effect',
      capability: null,
      forms: [
        {
          form: '',
          kind: 'frontend_effect',
          argumentHint: '',
          argumentRequired: false,
          daemonServed: true,
          action: null,
        },
      ],
    },
  ],
});

beforeEach(() => {
  vi.mocked(bridge.snapshot).mockResolvedValue(connected(1));
  vi.mocked(bridge.connect).mockResolvedValue(connected(1));
  vi.mocked(bridge.projects).mockResolvedValue([
    { projectId: 'p0', displayName: 'Demo project', lifecycle: 'active' },
  ]);
  vi.mocked(bridge.disconnect).mockResolvedValue(undefined);
  vi.mocked(bridge.unsubscribe).mockResolvedValue(undefined);
  vi.mocked(bridge.projectDetail).mockResolvedValue({
    projectId: 'p0',
    displayName: 'Demo project',
    workspaces: [{ workspaceId: 'w0', displayName: 'Workspace' }],
    sessionCount: 1,
    routeToken: token(1),
  });
  vi.mocked(bridge.workspaceSelect).mockResolvedValue(token(2));
  vi.mocked(bridge.sessionList).mockResolvedValue({
    sessions: [{ sessionId: 's-1', title: 'First', projectId: 'p0', workspaceId: 'w0' }],
    routeToken: token(2, null),
  });
  vi.mocked(bridge.sessionOpen).mockResolvedValue({
    session: { sessionId: 's-1', title: 'First', projectId: 'p0', workspaceId: 'w0' },
    routeToken: token(3, 's-1'),
  });
  vi.mocked(bridge.projectionStart).mockResolvedValue(presentation());
  vi.mocked(bridge.projectionCurrent).mockResolvedValue(presentation());
  vi.mocked(bridge.todoRefresh).mockResolvedValue(presentation());
  vi.mocked(bridge.controlRefresh).mockResolvedValue(null);
  vi.mocked(bridge.permissionRespond).mockResolvedValue(null);
  vi.mocked(bridge.questionRespond).mockResolvedValue(null);
  vi.mocked(bridge.subscribeProjection).mockResolvedValue({ unsubscribe: vi.fn() });
  vi.mocked(bridge.subscribe).mockImplementation(async () => ({
    subscriptionId: 'desktop-sub-1',
    connectionGeneration: 1,
    unsubscribe: vi.fn(),
  }));
  vi.mocked(bridge.gitRead).mockResolvedValue({
    kind: 'status',
    status: {
      branch: 'main',
      isDirty: false,
      commitHash: 'abc123',
      stashCount: 0,
      consistency: 'immutable_generation',
    },
    diff: null,
    branches: null,
  });
  vi.mocked(bridge.skillCatalogue).mockResolvedValue({
    projectId: 'p0',
    skills: [
      {
        name: 'testing-ci',
        description: 'Select focused verification.',
        source: 'project',
        contentDigest: 'sha256:abc',
        resourceCount: 0,
      },
    ],
    total: 1,
    truncated: false,
  });
  vi.mocked(bridge.commandCatalog).mockResolvedValue(catalog());
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

/** Open the palette and run one catalog command by its spelling. */
async function runFromPalette(spelling: string): Promise<void> {
  await act(async () => {
    fireEvent.click(screen.getByTestId('command-palette-open-button'));
  });
  await waitFor(() => expect(bridge.commandCatalog).toHaveBeenCalled());
  await act(async () => {
    fireEvent.change(screen.getByPlaceholderText('Type a command'), {
      target: { value: spelling },
    });
    fireEvent.keyDown(screen.getByPlaceholderText('Type a command'), { key: 'Enter' });
  });
}

/** Select the route the palette's catalog read is fenced on. */
async function selectRoute(): Promise<void> {
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

describe('the desktop frontend-action dispatch', () => {
  it('performs the skills action from the catalogue the daemon published', async () => {
    render(<App />);
    await selectRoute();
    await runFromPalette('/fixture-skills');
    await waitFor(() =>
      expect(screen.getByText('skills: 1 discovered (project 1)')).toBeInTheDocument(),
    );
    expect(bridge.commandExecute).not.toHaveBeenCalled();
  });

  it('performs the status action from the session projection', async () => {
    render(<App />);
    await selectRoute();
    await runFromPalette('/fixture-status');
    await waitFor(() =>
      expect(screen.getByText('status: attached | tokens: 10↑ 20↓')).toBeInTheDocument(),
    );
    expect(bridge.commandExecute).not.toHaveBeenCalled();
  });

  it('performs the worktrees action from the worktrees the projection carries', async () => {
    render(<App />);
    await selectRoute();
    await runFromPalette('/fixture-worktrees');
    await waitFor(() =>
      expect(screen.getByText('worktrees: 1 managed (main: ready)')).toBeInTheDocument(),
    );
    expect(bridge.commandExecute).not.toHaveBeenCalled();
  });

  it('says which data the desktop is missing for an action it cannot carry', async () => {
    render(<App />);
    await selectRoute();
    await runFromPalette('/fixture-contracts');
    await waitFor(() =>
      expect(
        screen.getByText(
          '/fixture-contracts is a frontend effect; the desktop does not implement it yet — the session projection publishes the tools a turn used, not the registry contracts',
        ),
      ).toBeInTheDocument(),
    );
    expect(bridge.commandExecute).not.toHaveBeenCalled();
  });

  it('does not guess an action for a form the daemon published none for', async () => {
    render(<App />);
    await selectRoute();
    await runFromPalette('/fixture-unpublished');
    await waitFor(() =>
      expect(
        screen.getByText(
          '/fixture-unpublished is a frontend effect; the desktop does not implement it yet',
        ),
      ).toBeInTheDocument(),
    );
    expect(bridge.commandExecute).not.toHaveBeenCalled();
  });
});
