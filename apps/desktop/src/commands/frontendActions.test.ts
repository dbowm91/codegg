import { describe, expect, it, vi } from 'vitest';
import { answerFrontendAction, UNCARRIED_ACTIONS } from './frontendActions';
import type { FrontendActionContext } from './frontendActions';
import type { SessionPresentationView, SkillCatalogueView } from '../bridge-types';
import type { TerminalProcessView } from '../terminal/contract';

const presentation = (
  overrides: Partial<SessionPresentationView> = {},
): SessionPresentationView =>
  ({
    sessionId: 's-1',
    projectId: 'p0',
    workspaceId: 'w0',
    state: 'attached',
    selectedModel: null,
    selectedAgent: null,
    turn: null,
    messages: [],
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
    pluginCatalogue: [],
    recentTurns: [],
    todos: null,
    pendingPermissions: [],
    pendingQuestions: [],
    controller: null,
    artifactHandles: [],
    cursor: { eventSeq: 1, driverCursorSeq: 1, subscriptionKnown: true },
    resyncReason: null,
    ...overrides,
  }) as SessionPresentationView;

const catalogue = (overrides: Partial<SkillCatalogueView> = {}): SkillCatalogueView => ({
  projectId: 'p0',
  skills: [],
  total: 0,
  truncated: false,
  ...overrides,
});

const context = (overrides: Partial<FrontendActionContext> = {}): FrontendActionContext => ({
  presentation: null,
  catalogue: null,
  catalogueError: null,
  argument: null,
  terminals: [],
  focusTerminal: () => true,
  ...overrides,
});

const terminalProcess = (overrides: Partial<TerminalProcessView> = {}): TerminalProcessView => ({
  handle: 'h-1',
  workspaceId: 'w-0',
  command: 'bash',
  state: 'running',
  cols: 80,
  rows: 24,
  nextSeq: 0,
  retainedBytes: 0,
  totalBytes: 0,
  truncated: false,
  exitCode: null,
  exitSignal: null,
  ...overrides,
});

describe('the desktop frontend actions', () => {
  it('reports the skills the daemon published for the project', () => {
    const answer = answerFrontendAction(
      'show_skills',
      context({
        catalogue: catalogue({
          skills: [
            {
              name: 'testing-ci',
              description: 'Select focused verification.',
              source: 'project',
              contentDigest: 'sha256:abc',
              resourceCount: 0,
            },
            {
              name: 'global-one',
              description: 'A global skill.',
              source: 'global',
              contentDigest: 'sha256:def',
              resourceCount: 1,
            },
          ],
          total: 2,
        }),
      }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe('skills: 2 discovered (global 1, project 1)');
  });

  it('says the catalogue is truncated when the daemon bounded it', () => {
    const answer = answerFrontendAction(
      'show_skills',
      context({
        catalogue: catalogue({
          skills: [
            {
              name: 'testing-ci',
              description: 'Select focused verification.',
              source: 'project',
              contentDigest: 'sha256:abc',
              resourceCount: 0,
            },
          ],
          total: 9,
          truncated: true,
        }),
      }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe('skills: 1 of 9 discovered (project 1)');
  });

  it('names the failed catalogue read rather than reporting no skills', () => {
    const answer = answerFrontendAction(
      'show_skills',
      context({ catalogueError: 'the daemon refused the read' }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe('skills: unavailable (the daemon refused the read)');
  });

  it('reports the session status, the selected model and the turn token counters', () => {
    const answer = answerFrontendAction(
      'show_status',
      context({
        presentation: presentation({
          state: 'attached',
          selectedModel: 'gpt-5',
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
        }),
      }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe('status: attached | model: gpt-5 | tokens: 10↑ 20↓');
  });

  it('says the session has no selected model when the daemon published none', () => {
    const answer = answerFrontendAction(
      'show_status',
      context({ presentation: presentation({ selectedModel: null }) }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toContain('model: none selected');
  });

  it('reports the managed worktrees the projection carries', () => {
    const answer = answerFrontendAction(
      'show_worktrees',
      context({
        presentation: presentation({
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
            {
              worktreeId: 'wt-2',
              ownerRunId: 'run-2',
              branch: 'feature/x',
              state: 'leased',
              health: 'ok',
              dirty: true,
              conflicted: false,
              retainedForAttention: false,
              updatedAt: 2,
            },
          ],
        }),
      }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe('worktrees: 2 managed (main: ready, feature/x: leased, dirty)');
  });

  it('says there are no worktrees when the projection carries none', () => {
    const answer = answerFrontendAction('show_worktrees', context({ presentation: presentation() }));
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe('worktrees: none');
  });

  it('reports the tool contracts the daemon published', () => {
    const answer = answerFrontendAction(
      'show_tool_contracts',
      context({
        presentation: presentation({
          toolContracts: [
            {
              name: 'read',
              callerPolicy: 'direct_only',
              effectClass: 'read_only',
              idempotency: 'idempotent',
            },
            {
              name: 'bash',
              callerPolicy: 'direct_or_programmatic',
              effectClass: 'process_exec',
              idempotency: 'non_idempotent',
            },
          ],
        }),
      }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe(
      'tool contracts: 2 published (' +
        'read caller=direct_only effect=read_only idempotent=idempotent, ' +
        'bash caller=direct_or_programmatic effect=process_exec idempotent=non_idempotent)',
    );
  });

  it('says there are no tool contracts when the daemon published none', () => {
    const answer = answerFrontendAction(
      'show_tool_contracts',
      context({ presentation: presentation() }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe('tool contracts: none published');
  });

  it('reports the shell runs the daemon published, newest first', () => {
    const answer = answerFrontendAction(
      'show_shell_runs',
      context({
        presentation: presentation({
          shellRuns: [
            {
              runId: 'run-1',
              command: 'cargo test',
              cwd: '/repo',
              origin: 'bang',
              status: 'exited',
            },
            {
              runId: 'run-2',
              command: 'cargo fmt',
              cwd: '/repo',
              origin: 'bang_bang',
              status: 'running',
            },
          ],
        }),
      }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe(
      'shell runs: 2 published (cargo fmt (running) in /repo, cargo test (exited) in /repo)',
    );
  });

  it('says there are no shell runs when the daemon published none', () => {
    const answer = answerFrontendAction(
      'show_shell_runs',
      context({ presentation: presentation() }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe('shell runs: none published');
  });

  it('reports the plugin catalogue the daemon published', () => {
    const answer = answerFrontendAction(
      'show_plugins',
      context({
        presentation: presentation({
          pluginCatalogue: [
            {
              pluginId: 'plugin:alpha',
              name: 'alpha',
              version: '1.2.3',
              enabled: true,
              trust: 'builtin',
              runtimeKind: 'builtin',
              commandCount: 2,
              hookCount: 1,
              diagnosticCount: 0,
              hasError: false,
            },
            {
              pluginId: 'plugin:beta',
              name: 'beta',
              version: '0.1.0',
              enabled: false,
              trust: 'builtin',
              runtimeKind: 'builtin',
              commandCount: 0,
              hookCount: 0,
              diagnosticCount: 0,
              hasError: false,
            },
          ],
        }),
      }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe(
      'plugins: 2 published (plugin:alpha 1.2.3 enabled, plugin:beta 0.1.0 disabled)',
    );
  });

  it('says there are no plugins when the daemon published none', () => {
    const answer = answerFrontendAction(
      'show_plugins',
      context({ presentation: presentation() }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe('plugins: none published');
  });

  it('reports the plugin the info action names', () => {
    const answer = answerFrontendAction(
      'show_plugin_info',
      context({
        argument: 'plugin:alpha',
        presentation: presentation({
          pluginCatalogue: [
            {
              pluginId: 'plugin:alpha',
              name: 'alpha',
              version: '1.2.3',
              enabled: true,
              trust: 'builtin',
              runtimeKind: 'builtin',
              commandCount: 2,
              hookCount: 1,
              diagnosticCount: 0,
              hasError: false,
            },
          ],
        }),
      }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe(
      'plugin:alpha 1.2.3 enabled runtime=builtin trust=builtin commands=2 hooks=1 diagnostics=0',
    );
  });

  it('refuses the plugin info action for a plugin the daemon did not publish', () => {
    const answer = answerFrontendAction(
      'show_plugin_info',
      context({ argument: 'plugin:absent', presentation: presentation() }),
    );
    expect(answer.performed).toBe(false);
    expect(answer.reason).toBe('the daemon published no plugin named plugin:absent');
  });

  it('reports the published plugin diagnostics', () => {
    const answer = answerFrontendAction(
      'show_plugin_doctor',
      context({
        presentation: presentation({
          pluginCatalogue: [
            {
              pluginId: 'plugin:alpha',
              name: 'alpha',
              version: '1.2.3',
              enabled: true,
              trust: 'builtin',
              runtimeKind: 'builtin',
              commandCount: 2,
              hookCount: 1,
              diagnosticCount: 3,
              hasError: true,
            },
          ],
        }),
      }),
    );
    expect(answer.performed).toBe(true);
    expect(answer.message).toBe(
      'plugin diagnostics: 1 published ' +
        '(plugin:alpha diagnostics=3 error-level diagnostic present)',
    );
  });

  it('carries the plugin actions the daemon publishes', () => {
    expect(UNCARRIED_ACTIONS.show_plugins).toBeUndefined();
    expect(UNCARRIED_ACTIONS.show_plugin_info).toBeUndefined();
    expect(UNCARRIED_ACTIONS.show_plugin_doctor).toBeUndefined();
  });

  it('carries every action the daemon publishes, the terminal included', () => {
    expect(UNCARRIED_ACTIONS.show_tool_contracts).toBeUndefined();
    expect(UNCARRIED_ACTIONS.show_shell_runs).toBeUndefined();
    // The terminal is carried now: the desktop focuses the handle the
    // action names, so there is nothing left uncarried.
    expect(UNCARRIED_ACTIONS.show_terminal).toBeUndefined();
    expect(Object.keys(UNCARRIED_ACTIONS)).toHaveLength(0);
  });

  it('focuses the handle the terminal action names', () => {
    const focusTerminal = vi.fn(() => true);
    const answer = answerFrontendAction(
      'show_terminal',
      context({ argument: 'h-1', terminals: [terminalProcess()], focusTerminal }),
    );
    expect(answer.performed).toBe(true);
    expect(focusTerminal).toHaveBeenCalledWith('h-1');
  });

  it('refuses a terminal handle the daemon did not publish, and names it', () => {
    const focusTerminal = vi.fn(() => true);
    const answer = answerFrontendAction(
      'show_terminal',
      context({ argument: 'h-missing', terminals: [terminalProcess()], focusTerminal }),
    );
    expect(answer.performed).toBe(false);
    expect(answer.reason).toContain('h-missing');
    expect(focusTerminal).not.toHaveBeenCalled();
  });

  it('refuses the terminal action when it names no handle', () => {
    const answer = answerFrontendAction('show_terminal', context());
    expect(answer.performed).toBe(false);
    expect(answer.reason).toBe('the terminal action needs a handle');
  });

  it('refuses an action the desktop does not know, without inventing a reason', () => {
    const answer = answerFrontendAction('open_context_dialog', context());
    expect(answer.performed).toBe(false);
    expect(answer.reason).toBeNull();
  });
});
