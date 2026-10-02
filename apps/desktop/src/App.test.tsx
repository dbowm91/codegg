import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { App } from './App';
import { bridge } from './bridge';

vi.mock('./bridge', () => ({ bridge: {
  snapshot: vi.fn().mockResolvedValue({ state: 'connected', daemonId: 'daemon-test', protocolVersion: 1, uptimeSeconds: 7, activeSessions: 2, error: null }),
  connect: vi.fn().mockResolvedValue({ state: 'connected', daemonId: 'daemon-test', protocolVersion: 1, uptimeSeconds: 7, activeSessions: 2, error: null }),
  projects: vi.fn().mockResolvedValue(Array.from({ length: 55 }, (_, i) => ({ projectId: `p${i}`, displayName: i === 0 ? 'Demo project' : `Project ${i}`, lifecycle: 'active' }))),
  subscribe: vi.fn(() => () => undefined), disconnect: vi.fn(),
} }));

afterEach(cleanup);
describe('desktop shell', () => {
  it('renders daemon identity and bounded project results', async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByText('daemon-test')).toBeInTheDocument());
    expect(screen.getByText('Demo project')).toBeInTheDocument();
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
});
