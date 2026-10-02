import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { App } from './App';
import { bridge } from './bridge';

vi.mock('./bridge', () => ({
  bridge: {
    snapshot: vi.fn(),
    connect: vi.fn(),
    projects: vi.fn(),
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

  it('host rejection from concurrent disconnect leaves no phantom handle', async () => {
    // C002 linearizes subscribe against disconnect: when disconnect wins,
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
