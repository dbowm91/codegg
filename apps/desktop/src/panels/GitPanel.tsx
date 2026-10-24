import type { GitBranchListView, GitDiffView, GitStatusView } from '../bridge-types';
import { formatConsistency } from './format';

// The project's git facts.
//
// The TUI draws these in its "Git" sidebar section: the branch, the
// working tree's state, and the sync counts
// (`src/tui/components/sidebar.rs:1221-1284`). The desktop reads the
// same facts from the daemon's read-only git layer — one fact set per
// read, because the daemon's read is scoped to a single operation
// (`crates/codegg-protocol/src/git.rs:28-38`).
//
// A `null` fact set means the host has not read it yet — not that the
// repository has nothing to report. The panel says so instead of
// drawing an empty list, because "nothing changed" and "not known yet"
// are different claims.

export interface GitPanelProps {
  /** The repository's status facts, or null when not read yet. */
  status: GitStatusView | null;
  /** The working tree's diff summary, or null when not read yet. */
  diff: GitDiffView | null;
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
      <h3>
        Git
        {diff && <small data-testid="git-count">{diff.filesChanged} changed file(s)</small>}
      </h3>
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
          <p className="fact-row" data-testid="git-diff-counts">
            {diff.filesChanged} changed file(s), +{diff.insertions} -{diff.deletions}
            <span data-testid="git-diff-consistency" data-consistency={diff.consistency}>
              {formatConsistency(diff.consistency)}
            </span>
          </p>
          {diff.files.length ? (
            <ul data-testid="git-file-list">
              {diff.files.map((file) => (
                <li
                  key={file.path}
                  data-testid="git-file-item"
                  data-change-kind={file.kind}
                >
                  <span data-testid="git-file-path">{file.path}</span>
                  <span data-testid="git-file-kind">{file.kind}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="muted" data-testid="git-diff-empty">
              No changed files.
            </p>
          )}
          {diff.truncated && (
            // The daemon capped the list: the panel says so rather than
            // implying the list is complete.
            <p className="muted" data-testid="git-diff-truncated">
              Showing {diff.files.length} of {diff.totalFiles} changed file(s).
            </p>
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
