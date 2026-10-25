import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import {
  CommandPalette,
  filterCommands,
  isDispatchable,
  resolveForm,
  resolveTypedLine,
} from './CommandPalette';
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
      commandId: 'fixture-reload',
      name: '/fixture-reload',
      aliases: [],
      description: 'Refresh project runtime assets',
      domain: 'System',
      kind: 'daemon_effect',
      capability: 'command.execute',
      forms: [
        { form: '', kind: 'daemon_effect', argumentHint: '', argumentRequired: false, daemonServed: true, action: null },
      ],
    },
    {
      commandId: 'fixture-agents',
      name: '/fixture-agents',
      aliases: [],
      description: 'Inspect the discovered agents',
      domain: 'Agent',
      kind: 'frontend_effect',
      capability: null,
      forms: [
        {
          form: '',
          kind: 'frontend_effect',
          argumentHint: '',
          argumentRequired: false,
          daemonServed: true,
          action: 'open_agents_dialog',
        },
        {
          form: 'show',
          kind: 'daemon_effect',
          argumentHint: '<name>',
          argumentRequired: true,
          daemonServed: false,
          action: null,
        },
      ],
    },
    {
      commandId: 'legacy_report',
      name: '/legacy-report',
      aliases: [],
      description: 'A command the daemon does not serve yet',
      domain: 'Diagnostics',
      kind: 'daemon_effect',
      capability: 'command.execute',
      forms: [
        {
          form: '',
          kind: 'daemon_effect',
          argumentHint: '',
          argumentRequired: false,
          daemonServed: false,
          action: null,
        },
      ],
    },
  ],
});

describe('the desktop command palette', () => {
  it('resolves a typed line to the catalog entry that owns the spelling', () => {
    const resolved = resolveTypedLine(catalog(), '/fixture-reload');
    expect(resolved?.command.commandId).toBe('fixture-reload');
    expect(resolved?.argument).toBe('');
  });

  it('resolves the argument the user typed after the command', () => {
    const resolved = resolveTypedLine(catalog(), '/fixture-agents show tester');
    expect(resolved?.command.commandId).toBe('fixture-agents');
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
    expect(filterCommands(catalog(), 'fixture-rel').map((command) => command.commandId)).toEqual([
      'fixture-reload',
    ]);
  });

  it('marks a command the daemon does not serve as unavailable, and does not dispatch it', async () => {
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
    // The catalog holds a frontend-effect command whose bare form is one and
    // whose `show` form the daemon does not serve; and `/legacy-report`,
    // whose default form the daemon does not serve at all.
    expect(isDispatchable(catalog().commands[0])).toBe(true);
    expect(isDispatchable(catalog().commands[1])).toBe(true);
    expect(isDispatchable(catalog().commands[2])).toBe(false);
    await waitFor(() =>
      expect(screen.getByText('not served by the daemon yet')).toBeTruthy(),
    );
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
      target: { value: '/fixture-agents' },
    });
    fireEvent.keyDown(screen.getByPlaceholderText('Type a command'), { key: 'Enter' });
    await waitFor(() => expect(onFrontendEffect).toHaveBeenCalledTimes(1));
    expect(onFrontendEffect.mock.calls[0][0].commandId).toBe('fixture-agents');
    expect(bridge.commandExecute).not.toHaveBeenCalled();
    expect(onDaemonResult).not.toHaveBeenCalled();
  });

  it('hands the daemon the action the catalog published on the form', async () => {
    bridge.commandCatalog.mockResolvedValue(catalog());
    const onFrontendEffect = vi.fn();
    render(
      <CommandPalette
        open
        routeGeneration={3}
        onClose={vi.fn()}
        onFrontendEffect={onFrontendEffect}
        onDaemonResult={vi.fn()}
      />,
    );
    await waitFor(() => expect(bridge.commandCatalog).toHaveBeenCalledWith(3));
    fireEvent.change(screen.getByPlaceholderText('Type a command'), {
      target: { value: '/fixture-agents' },
    });
    fireEvent.keyDown(screen.getByPlaceholderText('Type a command'), { key: 'Enter' });
    await waitFor(() => expect(onFrontendEffect).toHaveBeenCalledTimes(1));
    // M007-017: the renderer dispatches by the daemon's own answer, so the
    // form's published action travels with the command.
    expect(onFrontendEffect.mock.calls[0][1].action).toBe('open_agents_dialog');
  });

  it('runs a command from a mouse click on its row', async () => {
    bridge.commandCatalog.mockResolvedValue(catalog());
    bridge.commandExecute.mockResolvedValue({
      runId: 'run-1',
      commandId: 'fixture-reload',
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
    fireEvent.click(screen.getByRole('button', { name: /fixture-reload/ }));
    await waitFor(() => expect(bridge.commandExecute).toHaveBeenCalledTimes(1));
    expect(bridge.commandExecute).toHaveBeenCalledWith('fixture-reload', '', '', 7, 3);
  });

  it('sends a daemon effect to the daemon with the catalog version it resolved against', async () => {
    bridge.commandCatalog.mockResolvedValue(catalog());
    bridge.commandExecute.mockResolvedValue({
      runId: 'run-1',
      commandId: 'fixture-reload',
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
      target: { value: '/fixture-reload' },
    });
    fireEvent.keyDown(screen.getByPlaceholderText('Type a command'), { key: 'Enter' });
    await waitFor(() => expect(bridge.commandExecute).toHaveBeenCalledTimes(1));
    expect(bridge.commandExecute).toHaveBeenCalledWith('fixture-reload', '', '', 7, 3);
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
      target: { value: '/fixture-agents show tester' },
    });
    fireEvent.keyDown(screen.getByPlaceholderText('Type a command'), { key: 'Enter' });
    await waitFor(() =>
      expect(
        screen.getByText('/fixture-agents is not served by the daemon yet'),
      ).toBeTruthy(),
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
