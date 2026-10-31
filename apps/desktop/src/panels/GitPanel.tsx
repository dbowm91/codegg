import type { GitBranchListView, GitStatusView } from '../bridge-types';
import type { DiffAnswerView } from '../commands/diffAnswer';
import { formatConsistency } from './format';

// The project's git facts, and the daemon's diff answer.
//
// The TUI draws these in its "Git" sidebar section: the branch, the
// working tree's state, and the sync counts
// (`src/tui/components/sidebar.rs:1221-1284`). The desktop reads the
// same facts from the daemon's read-only git layer — one fact set per
// read, because the daemon's read is scoped to a single operation
// (`crates/codegg-protocol/src/git.rs:28-38`).
//
// The diff is not a fact set: it is the daemon's own answer to the
// `diff` command (ADR-0058 D1), and it carries the bytes of both
// sides. The desktop renders that answer rather than a summary of it,
// because a summary of a diff is not a diff.
//
// A `null` fact set — or a `null` diff — means the host has not read it
// yet — not that the repository has nothing to report. The panel says so
// instead of drawing an empty list, because "nothing changed" and "not
// known yet" are different claims.

export interface GitPanelProps {
  /** The repository's status facts, or null when not read yet. */
  status: GitStatusView | null;
  /** The daemon's diff answer, or null when not read yet. */
  diff: DiffAnswerView | null;
  /** The local branches, or null when not read yet. */
  branches: GitBranchListView | null;
  /** Whether a read is in flight. */
  busy?: boolean;
  /** The last read's failure, if it failed. */
  error?: string | null;
  /** Re-read the facts from the daemon. */
  onRefresh?: () => void;
}

export function GitPanel({
  status,
  diff,
  branches,
  busy = false,
  error = null,
  onRefresh,
}: GitPanelProps) {
  return (
    <section className="surface" data-testid="git-surface">
      <h3>Git</h3>
      {onRefresh && (
        <button data-testid="git-refresh-button" disabled={busy} onClick={onRefresh}>
          {busy ? 'Refreshing…' : 'Refresh git'}
        </button>
      )}
      {error && (
        // A failed read is reported here rather than as a route error: it
        // is a git-surface condition, and the facts the host last gave
        // us are still the truth.
        <p role="status" data-testid="git-error">
          Git read failed: {error}
        </p>
      )}
      {status ? (
        <p className="fact-row" data-testid="git-status">
          <span>branch: </span>
          <strong data-testid="git-branch">{status.branch}</strong>
          <span data-testid="git-state">{status.isDirty ? 'dirty' : 'clean'}</span>
          {status.commitHash && (
            <span data-testid="git-commit">commit {status.commitHash}</span>
          )}
          {status.stashCount > 0 && (
            <span data-testid="git-stash">{status.stashCount} stash(es)</span>
          )}
          {/* The class of the read travels with the facts: the panel says
              what the observation is worth rather than implying a
              guarantee the daemon never gave. */}
          <span data-testid="git-status-consistency" data-consistency={status.consistency}>
            {formatConsistency(status.consistency)}
          </span>
        </p>
      ) : (
        <p className="muted" data-testid="git-status-unknown">
          Git status not read yet.
        </p>
      )}
      <h4>Changes</h4>
      {diff ? (
        <>
          <p className="fact-row" data-testid="git-diff-title">
            <strong data-testid="git-diff-heading">{diff.title}</strong>
          </p>
          {diff.path === null ? (
            // The whole-tree branch: the daemon answered with one unified
            // diff and there is no separate `HEAD` side to draw.
            diff.newContent ? (
              <pre data-testid="git-diff-body">{diff.newContent}</pre>
            ) : (
              <p className="muted" data-testid="git-diff-empty">
                No changes.
              </p>
            )
          ) : (
            <>
              <p className="fact-row" data-testid="git-diff-path">
                {diff.path}
              </p>
              <h5>At HEAD</h5>
              {diff.oldContent ? (
                <pre data-testid="git-diff-old">{diff.oldContent}</pre>
              ) : (
                <p className="muted" data-testid="git-diff-old-empty">
                  No file at HEAD.
                </p>
              )}
              <h5>Working tree</h5>
              <pre data-testid="git-diff-new">{diff.newContent}</pre>
            </>
          )}
        </>
      ) : (
        <p className="muted" data-testid="git-diff-unknown">
          Diff not read yet.
        </p>
      )}
      <h4>Branches</h4>
      {branches ? (
        <>
          <ul data-testid="git-branch-list">
            {branches.branches.map((branch) => (
              <li
                key={branch.name}
                data-testid="git-branch-item"
                data-branch-name={branch.name}
              >
                <strong data-testid="git-branch-name">{branch.name}</strong>
                {branch.isCurrent && <span data-testid="git-branch-current">current</span>}
                {branch.isDetached && <span data-testid="git-branch-detached">detached</span>}
                {branch.upstream && (
                  <span data-testid="git-branch-upstream">{branch.upstream}</span>
                )}
                {(branch.ahead !== null || branch.behind !== null) && (
                  <span data-testid="git-branch-sync">
                    ↑{branch.ahead ?? 0} ↓{branch.behind ?? 0}
                  </span>
                )}
              </li>
            ))}
          </ul>
          {branches.truncated && (
            <p className="muted" data-testid="git-branch-truncated">
              Showing {branches.branches.length} of {branches.total} branch(es).
            </p>
          )}
          <p className="fact-row" data-testid="git-branches-consistency-row">
            <span data-testid="git-branch-consistency" data-consistency={branches.consistency}>
              {formatConsistency(branches.consistency)}
            </span>
          </p>
        </>
      ) : (
        <p className="muted" data-testid="git-branches-unknown">
          Branches not read yet.
        </p>
      )}
    </section>
  );
}
