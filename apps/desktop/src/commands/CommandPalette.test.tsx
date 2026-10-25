import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { CommandPalette, filterCommands, resolveForm, resolveTypedLine } from './CommandPalette';
import type { CommandCatalogView } from '../bridge-types';

const bridge = vi.hoisted(() => ({
  commandCatalog: vi.fn(),
  commandExecute: vi.fn(),
}));

vi.mock('../bridge', () => ({ bridge }));

const catalog = (): CommandCatalogView => ({
  catalogVersion: 7,
  commands: [
    {
      commandId: 'reload',
      name: '/reload',
      aliases: [],
      description: 'Refresh project runtime assets',
      domain: 'System',
      kind: 'daemon_effect',
      capability: 'command.execute',
      forms: [
        { form: '', kind: 'daemon_effect', argumentHint: '', argumentRequired: false, daemonServed: true },
      ],
    },
    {
      commandId: 'agents',
      name: '/agents',
      aliases: [],
      description: 'Inspect the discovered agents',
      domain: 'Agent',
      kind: 'frontend_effect',
      capability: null,
      forms: [
        { form: '', kind: 'frontend_effect', argumentHint: '', argumentRequired: false, daemonServed: true },
        { form: 'show', kind: 'daemon_effect', argumentHint: '<name>', argumentRequired: true, daemonServed: false },
      ],
    },
  ],
});

describe('the desktop command palette', () => {
  it('resolves a typed line to the catalog entry that owns the spelling', () => {
    const resolved = resolveTypedLine(catalog(), '/reload');
    expect(resolved?.command.commandId).toBe('reload');
    expect(resolved?.argument).toBe('');
  });

  it('resolves the argument the user typed after the command', () => {
    const resolved = resolveTypedLine(catalog(), '/agents show tester');
    expect(resolved?.command.commandId).toBe('agents');
    expect(resolved?.argument).toBe('show tester');
  });

  it('treats free text that names no command as prompt text', () => {
    expect(resolveTypedLine(catalog(), 'please reload the thing')).toBeNull();
    expect(resolveTypedLine(catalog(), '/no-such-command')).toBeNull();
  });

  it('resolves the form a typed argument selects', () => {
    const agents = catalog().commands[1];
    expect(resolveForm(agents, 'show tester')).toBe('show');
    expect(resolveForm(agents, '')).toBe('');
  });

  it('filters the rows by the typed text', () => {
    expect(filterCommands(catalog(), 'rel').map((command) => command.commandId)).toEqual([
      'reload',
    ]);
  });

  it('dispatches a frontend effect locally and never asks the daemon to run it', async () => {
    bridge.commandCatalog.mockResolvedValue(catalog());
    const onFrontendEffect = vi.fn();
    const onDaemonResult = vi.fn();
    render(
      <CommandPalette
        open
        routeGeneration={3}
        onClose={vi.fn()}
        onFrontendEffect={onFrontendEffect}
        onDaemonResult={onDaemonResult}
      />,
    );
    await waitFor(() => expect(bridge.commandCatalog).toHaveBeenCalledWith(3));
    fireEvent.change(screen.getByPlaceholderText('Type a command'), {
      target: { value: '/agents' },
    });
    fireEvent.keyDown(screen.getByPlaceholderText('Type a command'), { key: 'Enter' });
    await waitFor(() => expect(onFrontendEffect).toHaveBeenCalledTimes(1));
    expect(onFrontendEffect.mock.calls[0][0].commandId).toBe('agents');
    expect(bridge.commandExecute).not.toHaveBeenCalled();
    expect(onDaemonResult).not.toHaveBeenCalled();
  });

  it('sends a daemon effect to the daemon with the catalog version it resolved against', async () => {
    bridge.commandCatalog.mockResolvedValue(catalog());
    bridge.commandExecute.mockResolvedValue({
      runId: 'run-1',
      commandId: 'reload',
      status: 'completed',
      summary: 'refreshed',
      lines: [],
      report: null,
      startedAtMs: 1,
      completedAtMs: 2,
    });
    const onDaemonResult = vi.fn();
    render(
      <CommandPalette
        open
        routeGeneration={3}
        onClose={vi.fn()}
        onFrontendEffect={vi.fn()}
        onDaemonResult={onDaemonResult}
      />,
    );
    await waitFor(() => expect(bridge.commandCatalog).toHaveBeenCalledWith(3));
    fireEvent.change(screen.getByPlaceholderText('Type a command'), {
      target: { value: '/reload' },
    });
    fireEvent.keyDown(screen.getByPlaceholderText('Type a command'), { key: 'Enter' });
    await waitFor(() => expect(bridge.commandExecute).toHaveBeenCalledTimes(1));
    expect(bridge.commandExecute).toHaveBeenCalledWith('reload', '', '', 7, 3);
    await waitFor(() => expect(onDaemonResult).toHaveBeenCalledTimes(1));
  });

  it('says a daemon effect the daemon does not serve is unavailable, and does not dispatch it', async () => {
    bridge.commandCatalog.mockResolvedValue(catalog());
    const onDaemonResult = vi.fn();
    render(
      <CommandPalette
        open
        routeGeneration={3}
        onClose={vi.fn()}
        onFrontendEffect={vi.fn()}
        onDaemonResult={onDaemonResult}
      />,
    );
    await waitFor(() => expect(bridge.commandCatalog).toHaveBeenCalledWith(3));
    fireEvent.change(screen.getByPlaceholderText('Type a command'), {
      target: { value: '/agents show tester' },
    });
    fireEvent.keyDown(screen.getByPlaceholderText('Type a command'), { key: 'Enter' });
    await waitFor(() =>
      expect(screen.getByText('/agents is not served by the daemon yet')).toBeTruthy(),
    );
    expect(bridge.commandExecute).not.toHaveBeenCalled();
    expect(onDaemonResult).not.toHaveBeenCalled();
  });

  it('sends free text that names no command nowhere at all', async () => {
    bridge.commandCatalog.mockResolvedValue(catalog());
    const onClose = vi.fn();
    render(
      <CommandPalette
        open
        routeGeneration={3}
        onClose={onClose}
        onFrontendEffect={vi.fn()}
        onDaemonResult={vi.fn()}
      />,
    );
    await waitFor(() => expect(bridge.commandCatalog).toHaveBeenCalledWith(3));
    fireEvent.change(screen.getByPlaceholderText('Type a command'), {
      target: { value: 'please reload the thing' },
    });
    fireEvent.keyDown(screen.getByPlaceholderText('Type a command'), { key: 'Enter' });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(bridge.commandExecute).not.toHaveBeenCalled();
  });
});
