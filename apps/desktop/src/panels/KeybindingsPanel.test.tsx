import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { App } from '../App';
import { bridge } from '../bridge';
import { KeybindingsPanel } from './KeybindingsPanel';
import {
  ALL_ACTION_DECLARATIONS,
  buildEffectiveMap,
  currentPlatform,
  modKeyName,
  neutralChord,
  scopeKey,
} from '../keymap';

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

/** The modifier this platform's `mod` token is. */
const MOD = modKeyName(currentPlatform()) === 'meta' ? { metaKey: true } : { ctrlKey: true };

const captureChord = (event: { key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }) =>
  neutralChord({
    key: event.key,
    metaKey: event.metaKey,
    ctrlKey: event.ctrlKey,
    altKey: event.altKey,
    shiftKey: event.shiftKey,
  });

const summaryFor = (overrides: Record<string, string[]> = {}) => {
  const map = buildEffectiveMap(ALL_ACTION_DECLARATIONS, overrides);
  const summary: Record<string, string[]> = {};
  for (const declaration of ALL_ACTION_DECLARATIONS) {
    summary[declaration.id] = (map.bindings.get(declaration.id) ?? []).map((binding) => binding.chord);
  }
  return summary;
};

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
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  window.localStorage.clear();
});

describe('the key bindings surface', () => {
  it('panel_is_reachable_by_mouse_and_by_chord', async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByText('Demo project')).toBeInTheDocument());

    // The mouse route.
    await act(async () => {
      fireEvent.click(screen.getByTestId('keybinds-open-button'));
    });
    expect(screen.getByTestId('keybindings-panel')).toBeInTheDocument();
    await act(async () => {
      fireEvent.click(screen.getAllByTestId('overlay-close-button')[0]);
    });
    expect(screen.queryByTestId('keybindings-panel')).not.toBeInTheDocument();

    // The chord route.
    await act(async () => {
      fireEvent.keyDown(document.body, { key: 'k', ...MOD });
    });
    expect(screen.getByTestId('keybindings-panel')).toBeInTheDocument();
  });

  it('panel_is_fully_operable_with_the_keyboard', async () => {
    const onRemap = vi.fn(() => null);
    const onReset = vi.fn();
    const onImport = vi.fn(() => null);
    render(
      <KeybindingsPanel
        declarations={ALL_ACTION_DECLARATIONS}
        bindings={summaryFor()}
        captureChord={captureChord}
        onRemap={onRemap}
        onReset={onReset}
        exportText="{}"
        onImport={onImport}
        onClose={() => {}}
      />,
    );

    // The list is navigable: the selection moves with the arrow keys.
    const first = screen.getByTestId('keybinds-action-prompt.submit');
    expect(first).toHaveAttribute('aria-selected', 'true');
    await act(async () => {
      fireEvent.keyDown(first, { key: 'ArrowDown' });
    });
    expect(screen.getByTestId('keybinds-action-prompt.newline')).toHaveAttribute('aria-selected', 'true');

    // Enter starts a capture, and the captured key is committed.
    await act(async () => {
      fireEvent.keyDown(screen.getByTestId('keybinds-action-prompt.newline'), { key: 'Enter' });
    });
    const capture = await screen.findByTestId('keybinds-capture');
    await act(async () => {
      fireEvent.keyDown(capture, { key: 'g', ...MOD });
    });
    expect(onRemap).toHaveBeenCalledWith('prompt.newline', 'mod+g');

    // The reset, export and import actions have their own chords.
    await act(async () => {
      fireEvent.keyDown(screen.getByTestId('keybindings-panel'), {
        key: 'r',
        ...MOD,
        altKey: true,
        shiftKey: true,
      });
    });
    expect(onReset).toHaveBeenCalledTimes(1);
    await act(async () => {
      fireEvent.keyDown(screen.getByTestId('keybindings-panel'), {
        key: 'e',
        ...MOD,
        altKey: true,
        shiftKey: true,
      });
    });
    expect(screen.getByTestId('keybinds-export')).toHaveValue('{}');
    await act(async () => {
      fireEvent.keyDown(screen.getByTestId('keybindings-panel'), {
        key: 'i',
        ...MOD,
        altKey: true,
        shiftKey: true,
      });
    });
    expect(onImport).toHaveBeenCalled();
  });

  it('remap_flow_updates_the_binding_and_shows_the_conflict', async () => {
    let overrides: Record<string, string[]> = {};
    const onRemap = vi.fn((actionId: string, chord: string) => {
      const map = buildEffectiveMap(ALL_ACTION_DECLARATIONS, overrides);
      const holder = map.byScope.get(scopeKey({ kind: 'global' }))?.get(
        chord === 'mod+j' ? 'ctrl+j' : 'ctrl+p',
      );
      if (holder && holder !== actionId) return 'mod+p is already bound to Open the project switcher';
      overrides = { ...overrides, [actionId]: [chord] };
      return null;
    });
    const Panel = () => (
      <KeybindingsPanel
        declarations={ALL_ACTION_DECLARATIONS}
        bindings={summaryFor(overrides)}
        captureChord={captureChord}
        onRemap={onRemap}
        onReset={() => {}}
        exportText="{}"
        onImport={() => null}
        onClose={() => {}}
      />
    );
    const { rerender } = render(<Panel />);

    // A successful remap is reflected in the list.
    await act(async () => {
      fireEvent.keyDown(screen.getByTestId('keybinds-action-tasks.toggle'), { key: 'Enter' });
    });
    const capture = await screen.findByTestId('keybinds-capture');
    await act(async () => {
      fireEvent.keyDown(capture, { key: 'j', ...MOD });
    });
    expect(onRemap).toHaveBeenCalledWith('tasks.toggle', 'mod+j');
    rerender(<Panel />);
    expect(screen.getByTestId('keybinds-action-tasks.toggle')).toHaveTextContent('mod+j');

    // A colliding remap shows the conflict and leaves the binding alone.
    await act(async () => {
      fireEvent.keyDown(screen.getByTestId('keybinds-action-help.open'), { key: 'Enter' });
    });
    const second = await screen.findByTestId('keybinds-capture');
    await act(async () => {
      fireEvent.keyDown(second, { key: 'p', ...MOD });
    });
    expect(screen.getByTestId('keybinds-conflict')).toHaveTextContent('already bound to');
    rerender(<Panel />);
    expect(screen.getByTestId('keybinds-action-help.open')).toHaveTextContent('mod+/');
  });

  it('action_list_is_searchable', async () => {
    render(
      <KeybindingsPanel
        declarations={ALL_ACTION_DECLARATIONS}
        bindings={summaryFor()}
        captureChord={captureChord}
        onRemap={() => null}
        onReset={() => {}}
        exportText="{}"
        onImport={() => null}
        onClose={() => {}}
      />,
    );
    expect(screen.getByTestId('keybinds-action-prompt.submit')).toBeInTheDocument();
    await act(async () => {
      fireEvent.change(screen.getByTestId('keybinds-search'), { target: { value: 'tasks' } });
    });
    expect(screen.queryByTestId('keybinds-action-prompt.submit')).not.toBeInTheDocument();
    expect(screen.getByTestId('keybinds-action-tasks.toggle')).toBeInTheDocument();
  });
});
