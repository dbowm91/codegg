// M007-017: the desktop's half of a declared frontend action.
//
// The daemon owns the catalog: which commands exist, their spellings, their
// descriptions, their kinds, and — since M007-017 — which declared
// `FrontendAction` a frontend-effect form resolves to. The catalog publishes
// that action's stable wire name on the form, so the renderer dispatches by
// the daemon's own answer and never guesses an action from a spelling.
//
// This module owns the other half: given an action's wire name, what the
// desktop does. It is keyed by the wire name — never by a command spelling
// and never by a command id — so the renderer holds no second catalog, and a
// command the daemon renames or re-ids keeps working.
//
// Every action here changes only what the frontend is showing. None of them
// reads or writes durable state: they report what the daemon already
// published to the renderer.
//
// The wire names are the daemon's (`FrontendAction::as_str()` in
// `crates/codegg-core/src/frontend_action.rs`). The desktop answers to the
// names the daemon declares and does not extend the set.
//
// M007-018: the daemon publishes the session's selected model, its shell
// command runs and its tool contracts, so `show_tool_contracts` and
// `show_shell_runs` are performed from the projection the renderer already
// holds, and `show_status` names the model. The dispatch stays
// synchronous: an action reports what the renderer was given, and never
// reads the daemon again.

import type { SessionPresentationView, SkillCatalogueView } from '../bridge-types';
import type { TerminalProcessView } from '../terminal/contract';

/** What the desktop shows for one declared frontend action. */
export interface FrontendActionAnswer {
  /** Whether the desktop performed the action. */
  performed: boolean;
  /** The message the renderer shows when the desktop performed the action. */
  message: string;
  /**
   * Why the desktop does not carry the action, when it knows the action and
   * cannot perform it. It is null for an action the desktop performed, and
   * for an action the desktop does not know at all.
   */
  reason: string | null;
}

/** What the desktop already holds, and can report without asking again. */
export interface FrontendActionContext {
  /** The route-selected session's projection, or null when none is attached. */
  presentation: SessionPresentationView | null;
  /** The skill catalogue the daemon published for the selected project. */
  catalogue: SkillCatalogueView | null;
  /** The catalogue read's failure, when the read failed. */
  catalogueError: string | null;
  /**
   * The action's typed argument, or null when the form carries none.
   *
   * An action that names something — a handle, a path — receives it here, so
   * the desktop answers the action the user asked for rather than a
   * generalisation of it.
   */
  argument: string | null;
  /** The interactive processes the daemon published for the workspace. */
  terminals: TerminalProcessView[];
  /**
   * Focus a named terminal handle in the desktop's terminal panel.
   *
   * It answers whether the focus moved: an action that cannot focus what it
   * was asked to focus says so instead of claiming it did.
   */
  focusTerminal: (handle: string) => boolean;
}

/**
 * The declared frontend actions the desktop does not carry, and why.
 *
 * The reasons are the desktop's own, and they say what the desktop is
 * missing — never what the command means. A reason names the gap in the
 * data the daemon publishes to the renderer, so the message does not read as
 * a temporary refusal.
 *
 * The desktop carries every declared action: the daemon publishes the
 * session's selected model, its shell command runs and its tool contracts,
 * and the desktop renders what it published. The terminal is carried too:
 * the daemon publishes the workspace's interactive processes, and the
 * desktop focuses the handle the action names.
 */
export const UNCARRIED_ACTIONS: Readonly<Record<string, string | undefined>> = {};

/**
 * The desktop's report of the skills the daemon discovered for the project.
 *
 * The catalogue is the daemon's own discovery, published to the renderer
 * for the selected project. It is the desktop's counterpart to the TUI's
 * held asset snapshot: a read of what the project has, not a second copy of
 * a store the renderer keeps.
 */
function skillsReport(context: FrontendActionContext): string {
  const catalogue = context.catalogue;
  if (context.catalogueError) return `skills: unavailable (${context.catalogueError})`;
  if (!catalogue) return 'skills: no catalogue read for this project yet';
  const shown = catalogue.skills.length;
  const scope = catalogue.truncated ? `${shown} of ${catalogue.total}` : `${shown}`;
  const bySource = new Map<string, number>();
  for (const skill of catalogue.skills) {
    bySource.set(skill.source, (bySource.get(skill.source) ?? 0) + 1);
  }
  const sources = [...bySource.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([source, count]) => `${source} ${count}`);
  const tail = sources.length > 0 ? ` (${sources.join(', ')})` : '';
  return `skills: ${scope} discovered${tail}`;
}

/**
 * The desktop's report of the session's status.
 *
 * The token counters are the current turn's — the projection carries the
 * turn's counters, not a session-cumulative pair. The model is the
 * daemon's own selection, as the session summary published it; a
 * selection the daemon has not published is reported as such rather than
 * as a model name.
 */
function statusReport(context: FrontendActionContext): string {
  const presentation = context.presentation;
  if (!presentation) return 'status: no session attached';
  const input = presentation.turn?.inputTokens ?? 0;
  const output = presentation.turn?.outputTokens ?? 0;
  const model = presentation.selectedModel ?? 'none selected';
  return `status: ${presentation.state} | model: ${model} | tokens: ${input}↑ ${output}↓`;
}

/** The desktop's report of the managed worktrees the projection carries. */
function worktreesReport(context: FrontendActionContext): string {
  const worktrees = context.presentation?.worktrees ?? [];
  if (worktrees.length === 0) return 'worktrees: none';
  const limit = 8;
  const shown = worktrees.slice(0, limit).map((worktree) => {
    const name = worktree.branch ?? worktree.worktreeId;
    return worktree.dirty ? `${name}: ${worktree.state}, dirty` : `${name}: ${worktree.state}`;
  });
  const rest = worktrees.length - shown.length;
  const tail = rest > 0 ? `, +${rest} more` : '';
  return `worktrees: ${worktrees.length} managed (${shown.join(', ')}${tail})`;
}

/**
 * The desktop's report of the tool contracts the daemon published.
 *
 * The four fields are the ones the TUI's `/tool-contracts` renders — the
 * tool's name, its caller policy, its effect class and its idempotency
 * class — so the two frontends say the same thing about the same tool.
 * The contracts keep the order the daemon published them in.
 */
function toolContractsReport(context: FrontendActionContext): string {
  const contracts = context.presentation?.toolContracts ?? [];
  if (contracts.length === 0) return 'tool contracts: none published';
  const limit = 8;
  const shown = contracts.slice(0, limit).map((contract) => {
    return (
      `${contract.name} caller=${contract.callerPolicy} ` +
      `effect=${contract.effectClass} idempotent=${contract.idempotency}`
    );
  });
  const rest = contracts.length - shown.length;
  const tail = rest > 0 ? `, +${rest} more` : '';
  return `tool contracts: ${contracts.length} published (${shown.join(', ')}${tail})`;
}

/**
 * The desktop's report of the shell command runs the daemon published.
 *
 * The runs are shown newest first: the daemon appends a run as it
 * happens, so the newest run is last in the published list. The run's
 * command, its status and its working directory are what the report
 * names.
 */
function shellRunsReport(context: FrontendActionContext): string {
  const runs = context.presentation?.shellRuns ?? [];
  if (runs.length === 0) return 'shell runs: none published';
  const limit = 8;
  const newestFirst = [...runs].reverse();
  const shown = newestFirst.slice(0, limit).map((run) => {
    return `${run.command} (${run.status}) in ${run.cwd}`;
  });
  const rest = runs.length - shown.length;
  const tail = rest > 0 ? `, +${rest} more` : '';
  return `shell runs: ${runs.length} published (${shown.join(', ')}${tail})`;
}

/**
 * The desktop's answer to the terminal action.
 *
 * The action names a handle, and the desktop focuses **that** handle: it
 * never focuses an arbitrary terminal. A handle the daemon did not publish
 * is refused with the reason, so the message says what is missing rather
 * than showing the wrong terminal.
 */
function terminalReport(context: FrontendActionContext): FrontendActionAnswer {
  const handle = (context.argument ?? '').trim();
  if (!handle) {
    return { performed: false, message: '', reason: 'the terminal action needs a handle' };
  }
  const known = context.terminals.some((process) => process.handle === handle);
  if (!known) {
    return {
      performed: false,
      message: '',
      reason: `the daemon published no terminal with the handle ${handle}`,
    };
  }
  if (!context.focusTerminal(handle)) {
    return { performed: false, message: '', reason: 'the desktop could not focus that terminal' };
  }
  return { performed: true, message: `terminal ${handle} is focused`, reason: null };
}

/**
 * Answer one declared frontend action.
 *
 * The action's wire name is the daemon's, so an action the desktop does not
 * know is reported honestly rather than guessed at, and an action the
 * desktop knows it cannot perform says why.
 */
export function answerFrontendAction(
  action: string,
  context: FrontendActionContext,
): FrontendActionAnswer {
  switch (action) {
    case 'show_skills':
      return { performed: true, message: skillsReport(context), reason: null };
    case 'show_status':
      return { performed: true, message: statusReport(context), reason: null };
    case 'show_worktrees':
      return { performed: true, message: worktreesReport(context), reason: null };
    case 'show_tool_contracts':
      return { performed: true, message: toolContractsReport(context), reason: null };
    case 'show_shell_runs':
      return { performed: true, message: shellRunsReport(context), reason: null };
    case 'show_terminal':
      return terminalReport(context);
    default:
      return {
        performed: false,
        message: '',
        reason: UNCARRIED_ACTIONS[action] ?? null,
      };
  }
}
