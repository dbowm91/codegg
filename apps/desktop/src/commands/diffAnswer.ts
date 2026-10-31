// The daemon's diff answer.
//
// M007-I (plan 022, work package F): the working tree's diff is the
// daemon's to read. The `diff` command is a daemon effect, so the daemon
// resolves the checkout from its own durable binding, reads the bytes and
// answers with them; the desktop renders the answer and never reads the
// working tree itself.
//
// The answer arrives as the execution's inner response — the daemon's own
// `CoreResponse`, serialized by the daemon. The parser below reads that
// tagged shape and returns `null` for anything that is not a diff answer,
// so an execution that produced something else is never drawn as a diff.

/**
 * The daemon's identity for the diff command.
 *
 * This is the command's **id**, not a spelling: the daemon publishes it in
 * its catalog, and the renderer resolves the command there before it
 * dispatches anything.
 */
export const DIFF_COMMAND_ID = 'diff';

/** The daemon's diff answer. */
export interface DiffAnswerView {
  /** The title the daemon gave the diff. */
  title: string;
  /**
   * The repository-relative path the diff covered, or `null` for the
   * whole-tree branch.
   */
  path: string | null;
  /**
   * The bytes as they stood at `HEAD`. The whole-tree branch has no
   * `HEAD` side, so it is empty there.
   */
  oldContent: string;
  /**
   * The bytes as they stand in the working tree — or, for the whole-tree
   * branch, the unified diff itself.
   */
  newContent: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Read the daemon's diff answer out of a command execution's response.
 *
 * Returns `null` when the response is not a diff answer.
 */
export function readDiffAnswer(response: unknown): DiffAnswerView | null {
  if (!isRecord(response) || response.type !== 'git_diff') return null;
  const result = response.result;
  if (!isRecord(result)) return null;
  const title = result.title;
  const oldContent = result.old_content;
  const newContent = result.new_content;
  if (typeof title !== 'string') return null;
  if (typeof oldContent !== 'string' || typeof newContent !== 'string') return null;
  const path = typeof result.path === 'string' ? result.path : null;
  return { title, path, oldContent, newContent };
}
