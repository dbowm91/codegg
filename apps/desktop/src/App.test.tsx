import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { App } from './App';
import { bridge } from './bridge';

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
    projectionStart: vi.fn(),
    projectionStop: vi.fn(),
    projectionCurrent: vi.fn(),
    subscribeProjection: vi.fn(),
    subscribe: vi.fn(),
    disconnect: vi.fn(),
    unsubscribe: vi.fn(),
  },
}));

const connected = (generation: number, daemonId = 'daemon-test') => ({
  state: 'connected' as const,
  daemonId,
  protocolVersion: 1,
  uptimeSeconds: 7,
  activeSessions: 2,
  error: null,
  connectionGeneration: generation,
});

const projectList = () =>
  Array.from({ length: 55 }, (_, i) => ({
    projectId: `p${i}`,
    displayName: i === 0 ? 'Demo project' : `Project ${i}`,
    lifecycle: 'active',
  }));

beforeEach(() => {
  vi.mocked(bridge.snapshot).mockResolvedValue(connected(1));
  vi.mocked(bridge.connect).mockResolvedValue(connected(1));
  vi.mocked(bridge.projects).mockResolvedValue(projectList());
  vi.mocked(bridge.disconnect).mockResolvedValue(undefined);
  vi.mocked(bridge.unsubscribe).mockResolvedValue(undefined);
  vi.mocked(bridge.subscribe).mockImplementation(async () => ({
    subscriptionId: `desktop-sub-${Math.floor(Math.random() * 100000)}`,
    connectionGeneration: 1,
    unsubscribe: vi.fn(),
  }));
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('desktop shell', () => {
  it('renders daemon identity and bounded project results', async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByText('daemon-test')).toBeInTheDocument());
    expect(await screen.findByText('Demo project')).toBeInTheDocument();
    expect(screen.getByText('2')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText('50 / 50')).toBeInTheDocument());
    expect(screen.queryByText('Project 50')).not.toBeInTheDocument();
  });

  it('shows a disconnected error when daemon connection fails', async () => {
    vi.mocked(bridge.connect).mockRejectedValueOnce(new Error('daemon unavailable'));
    render(<App />);
    expect(await screen.findByRole('alert')).toHaveTextContent('daemon unavailable');
    expect(screen.getByText('disconnected')).toBeInTheDocument();
  });

  it('re-subscribes when generation changes with state remaining connected', async () => {
    vi.mocked(bridge.connect).mockResolvedValueOnce(connected(1));
    const { unmount } = render(<App />);
    await waitFor(() => expect(bridge.subscribe).toHaveBeenCalledTimes(1));
    const firstUnsubs = vi
      .mocked(bridge.subscribe)
      .mock.results[0]?.value as unknown as Promise<{
      unsubscribe: ReturnType<typeof vi.fn>;
    }>;
    // Simulate reconnect returning the same state but a new generation.
    vi.mocked(bridge.connect).mockResolvedValueOnce(connected(2));
    vi.mocked(bridge.subscribe).mockImplementationOnce(async () => ({
      subscriptionId: 'desktop-sub-2',
      connectionGeneration: 2,
      unsubscribe: vi.fn(),
    }));
    const button = screen.getByRole('button', { name: 'Reconnect' });
    await act(async () => {
      fireEvent.click(button);
    });
    await waitFor(() => expect(bridge.subscribe).toHaveBeenCalledTimes(2));
    // First subscription was torn down via Rust unsubscribe path.
    await waitFor(async () => {
      const first = await firstUnsubs;
      expect(first.unsubscribe).toHaveBeenCalled();
    });
    unmount();
  });

  it('cleanup invokes Rust unsubscribe on unmount', async () => {
    const unsubscribe = vi.fn();
    vi.mocked(bridge.subscribe).mockResolvedValueOnce({
      subscriptionId: 'desktop-sub-1',
      connectionGeneration: 1,
      unsubscribe,
    });
    const { unmount } = render(<App />);
    await waitFor(() => expect(bridge.subscribe).toHaveBeenCalled());
    // Allow the subscription promise to resolve and install the handle.
    await waitFor(() => expect(screen.getByText('daemon-test')).toBeInTheDocument());
    unmount();
    expect(unsubscribe).toHaveBeenCalledTimes(1);
  });

  it('unmount before subscribe resolves cleans the late subscription', async () => {
    const unsubscribe = vi.fn();
    let resolveSubscribe!: (value: {
      subscriptionId: string;
      connectionGeneration: number;
      unsubscribe: () => void;
    }) => void;
    vi.mocked(bridge.subscribe).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSubscribe = resolve;
        }),
    );
    const { unmount } = render(<App />);
    await waitFor(() => expect(bridge.subscribe).toHaveBeenCalled());
    unmount();
    await act(async () => {
      resolveSubscribe({
        subscriptionId: 'desktop-sub-late',
        connectionGeneration: 1,
        unsubscribe,
      });
    });
    expect(unsubscribe).toHaveBeenCalledTimes(1);
  });

  it('ignores stale generation events after reconnect', async () => {
    let firstOnEvent!: (event: { version: 1; eventSeq: number; kind: 'project_catalog_changed' }) => void;
    vi.mocked(bridge.subscribe).mockImplementationOnce(async (onEvent) => {
      firstOnEvent = onEvent as typeof firstOnEvent;
      return {
        subscriptionId: 'desktop-sub-1',
        connectionGeneration: 1,
        unsubscribe: vi.fn(),
      };
    });
    vi.mocked(bridge.connect).mockResolvedValueOnce(connected(1));
    render(<App />);
    await waitFor(() => expect(bridge.subscribe).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(bridge.projects).toHaveBeenCalled());
    const callsAfterMount = vi.mocked(bridge.projects).mock.calls.length;
    // Reconnect to generation 2; the effect re-subscribes.
    vi.mocked(bridge.connect).mockResolvedValueOnce(connected(2));
    vi.mocked(bridge.subscribe).mockImplementationOnce(async () => ({
      subscriptionId: 'desktop-sub-2',
      connectionGeneration: 2,
      unsubscribe: vi.fn(),
    }));
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));
    });
    await waitFor(() => expect(bridge.subscribe).toHaveBeenCalledTimes(2));
    const callsAfterReconnect = vi.mocked(bridge.projects).mock.calls.length;
    // Fire the stale generation-1 callback; it must not refresh projects.
    await act(async () => {
      firstOnEvent({ version: 1, eventSeq: 42, kind: 'project_catalog_changed' });
    });
    // Allow any mistaken refresh to flush.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(vi.mocked(bridge.projects).mock.calls.length).toBe(callsAfterReconnect);
    expect(callsAfterReconnect).toBeGreaterThanOrEqual(callsAfterMount);
  });

  it('repeated reconnects do not accumulate subscriptions', async () => {    const unsubscribes: ReturnType<typeof vi.fn>[] = [];
    vi.mocked(bridge.subscribe).mockImplementation(async () => {
      const unsubscribe = vi.fn();
      unsubscribes.push(unsubscribe);
      return {
        subscriptionId: `desktop-sub-${unsubscribes.length}`,
        connectionGeneration: unsubscribes.length,
        unsubscribe,
      };
    });
    let generation = 1;
    vi.mocked(bridge.connect).mockImplementation(async () => connected(generation));
    render(<App />);
    await waitFor(() => expect(bridge.subscribe).toHaveBeenCalledTimes(1));
    for (let i = 0; i < 3; i += 1) {
      generation += 1;
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));
      });
      await waitFor(() =>
        expect(bridge.subscribe).toHaveBeenCalledTimes(2 + i),
      );
    }
    // Every superseded subscription was unsubscribed exactly once.
    expect(unsubscribes.length).toBe(4);
    for (const unsubscribe of unsubscribes.slice(0, 3)) {
      expect(unsubscribe).toHaveBeenCalledTimes(1);
    }
    expect(unsubscribes[3]).not.toHaveBeenCalled();
  });

  it('host rejection from concurrent disconnect leaves no phantom handle', async () => {    // C002 linearizes subscribe against disconnect: when disconnect wins,
    // the host rejects subscribe and the renderer must not retain a phantom
    // handle. A later reconnect must resubscribe cleanly.
    vi.mocked(bridge.connect).mockResolvedValueOnce(connected(1));
    vi.mocked(bridge.subscribe).mockRejectedValueOnce(
      new Error('desktop is not connected'),
    );
    const unsubscribe = vi.fn();
    vi.mocked(bridge.subscribe).mockImplementationOnce(async () => ({
      subscriptionId: 'desktop-sub-2',
      connectionGeneration: 2,
      unsubscribe,
    }));
    const { unmount } = render(<App />);
    await waitFor(() => expect(bridge.subscribe).toHaveBeenCalledTimes(1));
    // Reconnect to generation 2; the effect resubscribes after the rejection.
    vi.mocked(bridge.connect).mockResolvedValueOnce(connected(2));
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));
    });
    await waitFor(() => expect(bridge.subscribe).toHaveBeenCalledTimes(2));
    // No phantom handle exists for the rejected attempt, so unmount only
    // releases the single live subscription.
    unmount();
    expect(unsubscribe).toHaveBeenCalledTimes(1);
  });
});

describe('session route flow', () => {
  const token = (routeGeneration: number, sessionId: string | null = null) => ({
    connectionGeneration: 1,
    routeGeneration,
    projectId: 'p0',
    workspaceId: sessionId ? 'w0' : '',
    sessionId,
  });

  const detailGen = (routeGeneration: number) => ({
    projectId: 'p0',
    displayName: 'Demo project',
    workspaces: [{ workspaceId: 'w0', displayName: 'Workspace' }],
    sessionCount: 1,
    routeToken: token(routeGeneration),
  });

  const presentation = (sessionId: string, state = 'attached') => ({
    sessionId,
    projectId: 'p0',
    workspaceId: 'w0',
    state,
    turn: { turnId: 'turn-1', status: 'active', updatedAt: 7, stopReason: null, error: null, messageCount: 1, toolCount: 0, pendingPermissions: 0, pendingQuestions: 0, inputTokens: 10, outputTokens: 20 },
    messages: [{ messageId: 'm-1', role: 'assistant', text: 'on it', truncated: false }],
    truncatedMessages: 0,
    tools: [],
    runs: [],
    jobs: [],
    subagents: [],
    recentTurns: [],
    pendingPermissions: [],
    pendingQuestions: [],
    controller: null,
    artifactHandles: [],
    cursor: { eventSeq: 42, driverCursorSeq: 42, subscriptionKnown: true },
    resyncReason: null,
  });

  beforeEach(() => {
    vi.mocked(bridge.projectDetail).mockImplementation(async (_projectId: string) => detailGen(1));
    vi.mocked(bridge.workspaceSelect).mockImplementation(async (_workspaceId: string, routeGeneration: number) => token(routeGeneration + 1));
    vi.mocked(bridge.sessionList).mockImplementation(async (routeGeneration: number) => ({
      sessions: [{ sessionId: 's-1', title: 'First', projectId: 'p0', workspaceId: 'w0' }],
      routeToken: token(routeGeneration, null),
    }));
    vi.mocked(bridge.sessionOpen).mockImplementation(async (sessionId: string, routeGeneration: number) => ({
      session: { sessionId, title: 'First', projectId: 'p0', workspaceId: 'w0' },
      routeToken: token(routeGeneration, sessionId),
    }));
    vi.mocked(bridge.projectionStart).mockImplementation(async (sessionId: string) => presentation(sessionId));
    vi.mocked(bridge.promptSubmit).mockImplementation(async (_text: string, _planMode: boolean, routeGeneration: number) => ({
      intentId: 'intent-1',
      routeToken: token(routeGeneration, 's-1'),
    }));
    vi.mocked(bridge.projectionCurrent).mockImplementation(async () => presentation('s-1'));
    vi.mocked(bridge.subscribeProjection).mockImplementation(async () => ({ unsubscribe: vi.fn() }));
  });

  it('drives projectdetail to workspaceselect to session open and projection', async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByText('Demo project')).toBeInTheDocument());
    await act(async () => {
      fireEvent.click(screen.getByTestId('project-select-p0'));
    });
    expect(await screen.findByTestId('workspace-list')).toBeInTheDocument();
    expect(bridge.projectDetail).toHaveBeenCalledWith('p0');
    await act(async () => {
      fireEvent.click(screen.getByTestId('workspace-select-w0'));
    });
    expect(await screen.findByTestId('session-list')).toBeInTheDocument();
    expect(bridge.workspaceSelect).toHaveBeenCalledWith('w0', 1);
    await act(async () => {
      fireEvent.click(screen.getByTestId('session-open-s-1'));
    });
    expect(await screen.findByTestId('projection-state')).toHaveTextContent('attached');
    expect(bridge.projectionStart).toHaveBeenCalledWith('s-1', 2);
    expect(await screen.findByText('on it')).toBeInTheDocument();
    expect(screen.getByTestId('turn-status')).toHaveTextContent('turn-1 / active');
  });

  it('drops a late session list after a newer route wins', async () => {
    let resolveList!: (value: { sessions: never[]; routeToken: ReturnType<typeof token> }) => void;
    vi.mocked(bridge.sessionList).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveList = resolve as typeof resolveList;
        }),
    );
    render(<App />);
    await waitFor(() => expect(screen.getByText('Demo project')).toBeInTheDocument());
    await act(async () => {
      fireEvent.click(screen.getByTestId('project-select-p0'));
    });
    await waitFor(() => expect(screen.getByTestId('workspace-list')).toBeInTheDocument());
    // First workspace select hangs on sessionList; supersede the route by
    // re-selecting the project (new route generation).
    const firstSelect = (async () => {
      await act(async () => {
        fireEvent.click(screen.getByTestId('workspace-select-w0'));
      });
    })();
    await waitFor(() => expect(bridge.sessionList).toHaveBeenCalled());
    vi.mocked(bridge.projectDetail).mockImplementationOnce(async () => detailGen(7));
    await act(async () => {
      fireEvent.click(screen.getByTestId('project-select-p0'));
    });
    await waitFor(() => expect(screen.getByTestId('route-generation')).toHaveTextContent('7'));
    await act(async () => {
      resolveList({ sessions: [], routeToken: token(2, null) });
    });
    await firstSelect;
    // The stale list (generation 2) never renders into the generation-7 route.
    expect(screen.queryByTestId('session-list')).not.toBeInTheDocument();
  });

  it('renders resync state and pending items read-only', async () => {
    vi.mocked(bridge.projectionStart).mockImplementationOnce(async (sessionId: string) => ({
      ...presentation(sessionId, 'resyncing'),
      resyncReason: 'gap',
      pendingPermissions: [{ permissionId: 'perm-1', tool: 'write', scopeSummary: null, status: 'pending' }],
      pendingQuestions: [{ questionId: 'q-1', header: 'Choice', prompt: 'Pick one', status: 'pending' }],
    }));
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
    expect(await screen.findByTestId('resync-banner')).toBeInTheDocument();
    expect(screen.getByTestId('permission-item')).toHaveTextContent('write');
    expect(screen.getByTestId('question-item')).toHaveTextContent('Pick one');
    // WP E wires responses; the buttons stay disabled with no authority.
    expect(screen.getByTitle('Permission responses land in WP E')).toBeDisabled();
  });

  it('submits a prompt once and clears the draft on accept', async () => {
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
    await act(async () => {
      fireEvent.change(screen.getByTestId('prompt-input'), { target: { value: 'hello desktop' } });
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId('prompt-submit-button'));
    });
    await waitFor(() => expect(bridge.promptSubmit).toHaveBeenCalledTimes(1));
    expect(bridge.promptSubmit).toHaveBeenCalledWith('hello desktop', false, 2);
    // Accepted: draft cleared, projection refreshed through current.
    await waitFor(() => expect(bridge.projectionCurrent).toHaveBeenCalled());
    expect(screen.getByTestId('prompt-input')).toHaveValue('');
  });

  it('keeps the draft and surfaces the error on prompt failure', async () => {
    vi.mocked(bridge.promptSubmit).mockRejectedValueOnce(new Error('model_unselected: no model'));
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
    await act(async () => {
      fireEvent.change(screen.getByTestId('prompt-input'), { target: { value: 'doomed draft' } });
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId('prompt-submit-button'));
    });
    await waitFor(() => expect(screen.getByTestId('route-error')).toHaveTextContent('model_unselected'));
    // Failure restores the editable draft; nothing durable fabricated.
    expect(screen.getByTestId('prompt-input')).toHaveValue('doomed draft');
  });

  it('disables submit while a prompt is in flight', async () => {
    let resolveSubmit!: (value: { intentId: string; routeToken: ReturnType<typeof token> }) => void;
    vi.mocked(bridge.promptSubmit).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSubmit = resolve as typeof resolveSubmit;
        }),
    );
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
    await act(async () => {
      fireEvent.change(screen.getByTestId('prompt-input'), { target: { value: 'once' } });
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId('prompt-submit-button'));
    });
    await waitFor(() => expect(screen.getByTestId('prompt-submit-button')).toBeDisabled());
    // A second click while busy cannot issue a second submit.
    await act(async () => {
      fireEvent.click(screen.getByTestId('prompt-submit-button'));
    });
    expect(bridge.promptSubmit).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveSubmit({ intentId: 'intent-9', routeToken: token(2, 's-1') });
    });
    // Accepted: the draft clears (the empty draft keeps submit disabled).
    await waitFor(() => expect(screen.getByTestId('prompt-input')).toHaveValue(''));
  });
});
