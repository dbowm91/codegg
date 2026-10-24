import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { GitPanel } from './GitPanel';
import type { GitBranchListView, GitDiffView, GitStatusView } from '../bridge-types';

const status = (overrides: Partial<GitStatusView> = {}): GitStatusView => ({
  branch: 'main',
  isDirty: false,
  commitHash: 'abc123',
  stashCount: 0,
  ...overrides,
});

const diff = (overrides: Partial<GitDiffView> = {}): GitDiffView => ({
  filesChanged: 1,
  insertions: 4,
  deletions: 2,
  files: [{ path: 'src/lib.rs', kind: 'modified' }],
  totalFiles: 1,
  truncated: false,
  ...overrides,
});

const branches = (overrides: Partial<GitBranchListView> = {}): GitBranchListView => ({
  branches: [
    {
      name: 'main',
      isCurrent: true,
      isDetached: false,
      upstream: 'origin/main',
      ahead: 2,
      behind: 1,
      head: 'abc123',
    },
  ],
  total: 1,
  truncated: false,
  ...overrides,
});

describe('the desktop git surface', () => {
  it('draws the branch and the working tree state the TUI draws', () => {
    render(<GitPanel status={status({ isDirty: true })} diff={null} branches={null} />);
    // The TUI's git section names the branch and says whether the tree
    // is dirty (`src/tui/components/sidebar.rs:1224-1249`).
    expect(screen.getByTestId('git-branch')).toHaveTextContent('main');
    expect(screen.getByTestId('git-state')).toHaveTextContent('dirty');
  });

  it('says the tree is clean when it is clean', () => {
    render(<GitPanel status={status()} diff={null} branches={null} />);
    expect(screen.getByTestId('git-state')).toHaveTextContent('clean');
  });

  it('lists every changed file with its change kind', () => {
    render(
      <GitPanel
        status={null}
        diff={diff({
          filesChanged: 2,
          files: [
            { path: 'src/lib.rs', kind: 'modified' },
            { path: 'src/new.rs', kind: 'added' },
          ],
          totalFiles: 2,
        })}
        branches={null}
      />,
    );
    const files = screen.getAllByTestId('git-file-item');
    expect(files).toHaveLength(2);
    expect(screen.getAllByTestId('git-file-path').map((node) => node.textContent)).toEqual([
      'src/lib.rs',
      'src/new.rs',
    ]);
    expect(screen.getAllByTestId('git-file-kind').map((node) => node.textContent)).toEqual([
      'modified',
      'added',
    ]);
  });

  it('says the facts are unknown until the host has read them', () => {
    render(<GitPanel status={null} diff={null} branches={null} />);
    // "not read yet" and "nothing to report" are different claims, and an
    // absent fact set must not read as an empty one.
    expect(screen.getByTestId('git-status-unknown')).toHaveTextContent('Git status not read yet.');
    expect(screen.getByTestId('git-diff-unknown')).toHaveTextContent('Diff not read yet.');
    expect(screen.getByTestId('git-branches-unknown')).toHaveTextContent('Branches not read yet.');
  });

  it('says the daemon capped the changed file list', () => {
    render(
      <GitPanel
        status={null}
        diff={diff({ filesChanged: 900, totalFiles: 900, truncated: true })}
        branches={null}
      />,
    );
    // The daemon's bound is part of the answer: the panel must not imply
    // the list is complete.
    expect(screen.getByTestId('git-diff-truncated')).toHaveTextContent('Showing 1 of 900');
  });

  it('marks the current branch and its sync counts', () => {
    render(<GitPanel status={null} diff={null} branches={branches()} />);
    const items = screen.getAllByTestId('git-branch-item');
    expect(items).toHaveLength(1);
    expect(screen.getByTestId('git-branch-name')).toHaveTextContent('main');
    expect(screen.getByTestId('git-branch-current')).toHaveTextContent('current');
    expect(screen.getByTestId('git-branch-sync')).toHaveTextContent('↑2 ↓1');
  });

  it('reports a failed read without blanking the facts it already has', () => {
    render(
      <GitPanel
        status={status({ branch: 'main' })}
        diff={null}
        branches={null}
        error="stale route"
      />,
    );
    expect(screen.getByTestId('git-error')).toHaveTextContent('Git read failed: stale route');
    expect(screen.getByTestId('git-branch')).toHaveTextContent('main');
  });

  it('asks the host to read the facts again', () => {
    const onRefresh = vi.fn();
    render(<GitPanel status={null} diff={null} branches={null} onRefresh={onRefresh} />);
    fireEvent.click(screen.getByTestId('git-refresh-button'));
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });
});
