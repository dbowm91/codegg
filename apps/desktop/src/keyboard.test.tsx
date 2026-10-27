import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { App } from './App';
import { bridge } from './bridge';
import {
  ALL_ACTION_DECLARATIONS,
  buildEffectiveMap,
  currentPlatform,
  modKeyName,
  resolveChordToken,
  scopeKey,
} from './keymap';
import { focusableControls, regionContainer } from './focus';

/*
 * The integration tests for the desktop's input model (M007-C, §10.3).
 *
 * They live in their own file: `App.test.tsx` is M007-B's compatibility
 * contract and stays byte-identical.
 */

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
    documentDiagnosticsSubscribe: vi.fn(() => Promise.resolve('diagnostics-test')),
    documentDiagnosticsUnsubscribe: vi.fn(),
    subscribeProjection: vi.fn(),
    subscribe: vi.fn(),
    disconnect: vi.fn(),
    unsubscribe: vi.fn(),
  },
}));

/** The modifier this platform's `mod` token is. */
const MOD = modKeyName(currentPlatform()) === 'meta' ? { metaKey: true } : { ctrlKey: true };

const token = (routeGeneration: number, sessionId: string | null = null) => ({
  connectionGeneration: 1,
  routeGeneration,
  projectId: 'p0',
  workspaceId: sessionId ? 'w0' : '',
  sessionId,
});

const connected = (generation: number) => ({
  state: 'connected' as const,
  daemonId: 'daemon-test',
  protocolVersion: 1,
  uptimeSeconds: 7,
  activeSessions: 2,
  error: null,
  connectionGeneration: generation,
});

/** A projection view, so the conversation region has something to render. */
const presentation = (sessionId: string) => ({
  sessionId,
  projectId: 'p0',
  workspaceId: 'w0',
  state: 'attached' as const,
  selectedModel: null,
  selectedAgent: null,
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
  worktrees: [],
  runGroups: [],
  convergences: [],
  shellRuns: [],
  toolContracts: [],
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
 * Press a key and report what the dispatcher did with it.
 *
 * The event is dispatched on the document body, so it reaches the one
 * keydown listener at the renderer's root.
 */
async function pressKey(
  key: string,
  init: KeyboardEventInit = {},
  target: EventTarget = document.body,
): Promise<KeyboardEvent> {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  await act(async () => {
    target.dispatchEvent(event);
  });
  return event;
}

/** Render the shell and route it to a workspace, so every region is live. */
async function renderRouted() {
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
}

beforeEach(() => {
  window.localStorage.clear();
  vi.mocked(bridge.snapshot).mockResolvedValue(connected(1));
  vi.mocked(bridge.connect).mockResolvedValue(connected(1));
  vi.mocked(bridge.projects).mockResolvedValue([
    { projectId: 'p0', displayName: 'Demo project', lifecycle: 'active' },
  ]);
  vi.mocked(bridge.projectDetail).mockImplementation(async () => ({
    projectId: 'p0',
    displayName: 'Demo project',
    workspaces: [{ workspaceId: 'w0', displayName: 'Workspace' }],
    sessionCount: 1,
    routeToken: token(1),
  }));
  vi.mocked(bridge.workspaceSelect).mockImplementation(async (_workspaceId: string, routeGeneration: number) => ({
    ...token(routeGeneration + 1),
    workspaceId: 'w0',
  }));
  vi.mocked(bridge.sessionList).mockImplementation(async (routeGeneration: number) => ({
    sessions: [{ sessionId: 's-1', title: 'First', projectId: 'p0', workspaceId: 'w0' }],
    routeToken: { ...token(routeGeneration, null), workspaceId: 'w0' },
  }));
  vi.mocked(bridge.sessionOpen).mockImplementation(async (sessionId: string, routeGeneration: number) => ({
    session: { sessionId, title: 'First', projectId: 'p0', workspaceId: 'w0' },
    routeToken: token(routeGeneration, sessionId),
  }));
  vi.mocked(bridge.promptSubmit).mockImplementation(
    async (_text: string, _planMode: boolean, routeGeneration: number) => ({
      intentId: 'intent-1',
      routeToken: token(routeGeneration + 1, 's-1'),
    }),
  );
  vi.mocked(bridge.disconnect).mockResolvedValue(undefined);
  vi.mocked(bridge.unsubscribe).mockResolvedValue(undefined);
  vi.mocked(bridge.projectionStart).mockImplementation(async (sessionId: string) => presentation(sessionId));
  vi.mocked(bridge.projectionCurrent).mockImplementation(async () => presentation('s-1'));
  vi.mocked(bridge.todoRefresh).mockImplementation(async () => presentation('s-1'));
  // The project's facts are not part of the input model; the reads fail
  // loudly rather than answering with an empty view.
  vi.mocked(bridge.gitRead).mockRejectedValue(new Error('git_unavailable'));
  vi.mocked(bridge.skillCatalogue).mockRejectedValue(new Error('skills_unavailable'));
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

describe('the desktop input model', () => {
  // The timeout is explicit because this test is the only one that walks every
  // declared action and clicks every affordance it finds. Each click is an
  // `await act(...)` flush, so the cost is proportional to the catalog, not to
  // one action — and vitest's 5s default is too tight for it on a loaded
  // machine, where it timed out at 5140ms. A per-test budget keeps the
  // default tight for every other test instead of loosening it globally.
  it('every_declared_action_is_reachable_by_chord_and_by_mouse', async () => {
    await renderRouted();
    // A draft, so the composer's own affordance is live.
    await act(async () => {
      fireEvent.change(screen.getByTestId('prompt-input'), { target: { value: 'a draft' } });
    });
    // An overlay is open, so the overlay family's affordance has a member.
    await pressKey('k', MOD);
    expect(screen.getByTestId('keybindings-panel')).toBeInTheDocument();

    // The editor is a docked panel, so its affordances are live only while
    // it is docked. Dock it, the way the overlay is opened above.
    await act(async () => {
      fireEvent.click(screen.getByTestId('panel-tab-editor'));
    });
    expect(screen.getByTestId('editor-viewport')).toBeInTheDocument();

    const map = buildEffectiveMap(ALL_ACTION_DECLARATIONS, {});
    for (const declaration of ALL_ACTION_DECLARATIONS) {
      // The chord route: the action holds at least one chord.
      const bindings = map.bindings.get(declaration.id) ?? [];
      expect(bindings.length, declaration.id).toBeGreaterThan(0);

      // The mouse route: the declared affordance exists, is visible and is
      // activatable.
      const elements = declaration.affordance.testIdPrefixes.flatMap((prefix) =>
        Array.from(document.querySelectorAll<HTMLElement>(`[data-testid^="${prefix}"]`)),
      );
      expect(elements.length, declaration.id).toBeGreaterThan(0);
      for (const element of elements) {
        expect(element, declaration.id).toBeVisible();
        if (declaration.affordance.class === 'editing') {
          expect(['INPUT', 'TEXTAREA'], declaration.id).toContain(element.tagName);
          expect(element, declaration.id).not.toBeDisabled();
        } else if (declaration.affordance.class === 'surface') {
          // A custom editing surface: it owns a caret and is focused by a
          // click, so it is focusable — and it holds no text of its own, so
          // it is not a native text field.
          expect(element.getAttribute('tabindex'), declaration.id).not.toBeNull();
          expect(element, declaration.id).not.toBeDisabled();
        } else if (declaration.affordance.class === 'button') {
          // An activatable control: a button, or a toggle the user clicks.
          expect(['BUTTON', 'INPUT'], declaration.id).toContain(element.tagName);
          expect(element, declaration.id).not.toBeDisabled();
        } else {
          // A region is activatable by a click on the region itself.
          await act(async () => {
            fireEvent.click(element);
          });
          expect(element.getAttribute('data-focus-region'), declaration.id).toBeTruthy();
        }
      }
    }
  }, 20_000);

  it('every_chord_reaches_exactly_one_action', () => {
    const map = buildEffectiveMap(ALL_ACTION_DECLARATIONS, {});
    const claims = new Map<string, string[]>();
    for (const declaration of ALL_ACTION_DECLARATIONS) {
      for (const binding of map.bindings.get(declaration.id) ?? []) {
        const resolved = resolveChordToken(binding.chord, currentPlatform());
        const key = `${scopeKey(binding.scope)} ${resolved}`;
        claims.set(key, [...(claims.get(key) ?? []), declaration.id]);
      }
    }
    expect(claims.size).toBeGreaterThan(0);
    for (const [key, actions] of claims) {
      expect(new Set(actions).size, key).toBe(1);
    }
  });

  it('mod_digit_focuses_the_nth_region', async () => {
    await renderRouted();
    await pressKey('2', MOD);
    const chat = regionContainer(document, 'chat');
    expect(chat?.getAttribute('data-focus-region')).toBe('chat');
    expect(chat?.contains(document.activeElement)).toBe(true);
  });

  it('tab_moves_within_the_focused_region_and_wraps', async () => {
    await renderRouted();
    await pressKey('1', MOD);
    const catalog = regionContainer(document, 'catalog');
    expect(catalog).not.toBeNull();
    const controls = focusableControls(catalog as HTMLElement);
    expect(controls.length).toBeGreaterThan(1);
    expect(document.activeElement).toBe(controls[0]);

    await pressKey('Tab');
    expect(document.activeElement).toBe(controls[1]);

    // The end of the region wraps to its beginning.
    for (let index = 1; index < controls.length; index += 1) {
      await pressKey('Tab');
    }
    expect(document.activeElement).toBe(controls[0]);

    // And backwards it wraps to the end.
    await pressKey('Tab', { shiftKey: true });
    expect(document.activeElement).toBe(controls[controls.length - 1]);
  });

  it('escape_pops_exactly_one_overlay_and_restores_focus', async () => {
    await renderRouted();
    const helpButton = screen.getByTestId('help-button');
    helpButton.focus();
    await act(async () => {
      fireEvent.click(helpButton);
    });
    const helpClose = screen
      .getByTestId('overlay-help')
      .querySelector('[data-testid="overlay-close-button"]');
    expect(helpClose).not.toBeNull();
    // The overlay took the focus.
    expect(document.activeElement).toBe(helpClose);

    // A second overlay on top of it.
    await pressKey('k', MOD);
    expect(screen.getByTestId('overlay-keybinds')).toBeInTheDocument();

    await pressKey('Escape');
    // Exactly one layer popped.
    expect(screen.queryByTestId('overlay-keybinds')).not.toBeInTheDocument();
    expect(screen.getByTestId('overlay-help')).toBeInTheDocument();
    // The focus returned to the element that had it before.
    expect(document.activeElement).toBe(helpClose);
  });

  it('escape_on_the_base_view_does_nothing', async () => {
    await renderRouted();
    await act(async () => {
      fireEvent.change(screen.getByTestId('prompt-input'), { target: { value: 'draft text' } });
    });
    const event = await pressKey('Escape');
    expect(event.defaultPrevented).toBe(false);
    expect(screen.queryByTestId('overlay-help')).not.toBeInTheDocument();
    expect(screen.queryByTestId('overlay-keybinds')).not.toBeInTheDocument();
    // The composer keeps its draft.
    expect(screen.getByTestId('prompt-input')).toHaveValue('draft text');
  });

  it('focus_is_always_visible', async () => {
    await renderRouted();
    await pressKey('1', MOD);
    const catalog = regionContainer(document, 'catalog');
    expect(catalog?.getAttribute('data-focus-region')).toBe('catalog');
    // The focused control carries the focus ring, not only `:focus-visible`.
    expect(document.activeElement?.classList.contains('focus-ring')).toBe(true);
  });

  it('focus_is_never_left_on_document_body', async () => {
    await renderRouted();
    await pressKey('3', MOD);
    expect(document.activeElement).not.toBe(document.body);

    // The side panel unmounts with its slot.
    await act(async () => {
      fireEvent.click(screen.getByTestId('sidepanel-toggle-button'));
    });
    expect(document.querySelector('[data-testid="side-panel-frame"]')).toBeNull();
    // The focus fell back to the declared fallback, not to the body.
    expect(document.activeElement).not.toBe(document.body);
    const composer = regionContainer(document, 'composer');
    expect(composer?.contains(document.activeElement)).toBe(true);
  });

  it('the_renderer_registers_no_os_global_shortcut', () => {
    // The token is assembled at runtime so this file does not carry it.
    const token = ['global', 'shortcut'].join('-');
    const sources = import.meta.glob('./**/*.{ts,tsx}', {
      query: '?raw',
      import: 'default',
      eager: true,
    }) as Record<string, string>;
    expect(Object.keys(sources).length).toBeGreaterThan(0);
    for (const [path, source] of Object.entries(sources)) {
      expect(source.includes(token), path).toBe(false);
    }
  });

  it('no_chord_shadows_an_os_accelerator', async () => {
    await renderRouted();
    for (const key of ['q', 'w', 'c', 'x', 'v', 'a']) {
      const event = await pressKey(key, MOD);
      expect(event.defaultPrevented, key).toBe(false);
    }
  });

  it('a_captured_accelerator_is_prevented', async () => {
    await renderRouted();
    const event = await pressKey('p', MOD);
    // `mod+p` is the WebView's print accelerator, so the chord captures it.
    expect(event.defaultPrevented).toBe(true);
  });

  it('typing_in_the_composer_is_unaffected', async () => {
    await renderRouted();
    const input = screen.getByTestId('prompt-input');
    await act(async () => {
      fireEvent.change(input, { target: { value: 'hello' } });
    });

    // An ordinary character key is not swallowed.
    const typed = await pressKey('a', {}, input);
    expect(typed.defaultPrevented).toBe(false);
    expect(screen.getByTestId('prompt-input')).toHaveValue('hello');

    // `enter` in the composer submits.
    await pressKey('Enter', {}, input);
    await waitFor(() => expect(bridge.promptSubmit).toHaveBeenCalledTimes(1));
    expect(bridge.promptSubmit).toHaveBeenCalledWith('hello', false, expect.any(Number));

    // `shift+enter` inserts a newline instead.
    await act(async () => {
      fireEvent.change(screen.getByTestId('prompt-input'), { target: { value: 'hello' } });
    });
    await pressKey('Enter', { shiftKey: true }, input);
    expect(screen.getByTestId('prompt-input')).toHaveValue('hello\n');
  });

  it('a_remap_committed_while_an_overlay_is_open_does_not_steal_focus', async () => {
    await renderRouted();
    await pressKey('k', MOD);
    const action = await screen.findByTestId('keybinds-action-tasks.toggle');
    await act(async () => {
      fireEvent.keyDown(action, { key: 'Enter' });
    });
    const capture = await screen.findByTestId('keybinds-capture');
    expect(document.activeElement).toBe(capture);

    // The remap is committed while the overlay is open.
    await act(async () => {
      fireEvent.keyDown(capture, { key: 'g', ...MOD });
    });
    expect(screen.queryByTestId('keybinds-capture')).not.toBeInTheDocument();
    // The focus stayed inside the overlay; it did not jump to the shell
    // behind it and it did not fall to the document body.
    expect(document.activeElement).not.toBe(document.body);
    expect(screen.getByTestId('overlay-keybinds')).toContainElement(
      document.activeElement as HTMLElement,
    );
  });

  it('a_remapped_binding_survives_a_renderer_reload', async () => {    await renderRouted();
    // Remap the tasks toggle through the bindings surface.
    await pressKey('k', MOD);
    const action = await screen.findByTestId('keybinds-action-tasks.toggle');
    await act(async () => {
      fireEvent.keyDown(action, { key: 'Enter' });
    });
    const capture = await screen.findByTestId('keybinds-capture');
    await act(async () => {
      fireEvent.keyDown(capture, { key: 'g', ...MOD });
    });
    await waitFor(() =>
      expect(screen.getByTestId('keybinds-action-tasks.toggle')).toHaveTextContent('mod+g'),
    );

    // A reload: the document is in the renderer's own storage.
    cleanup();
    render(<App />);
    await waitFor(() => expect(screen.getByText('Demo project')).toBeInTheDocument());
    await pressKey('k', MOD);
    expect(await screen.findByTestId('keybinds-action-tasks.toggle')).toHaveTextContent('mod+g');
  });
});
