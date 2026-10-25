import { describe, expect, it } from 'vitest';
import { answerFrontendAction, UNCARRIED_ACTIONS } from './frontendActions';
import type { FrontendActionContext } from './frontendActions';
import type { SessionPresentationView, SkillCatalogueView } from '../bridge-types';

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

  it('carries the actions the daemon publishes and leaves only the terminal uncarried', () => {
    expect(UNCARRIED_ACTIONS.show_tool_contracts).toBeUndefined();
    expect(UNCARRIED_ACTIONS.show_shell_runs).toBeUndefined();
    expect(UNCARRIED_ACTIONS.show_terminal).toBe('the desktop has no terminal surface to focus');
  });

  it('refuses an action the desktop does not carry, and says what it is missing', () => {
    const answer = answerFrontendAction('show_terminal', context());
    expect(answer.performed).toBe(false);
    expect(answer.reason).toBe('the desktop has no terminal surface to focus');
  });

  it('refuses an action the desktop does not know, without inventing a reason', () => {
    const answer = answerFrontendAction('open_context_dialog', context());
    expect(answer.performed).toBe(false);
    expect(answer.reason).toBeNull();
  });
});
