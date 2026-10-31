import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { GitPanel } from './GitPanel';
import type { GitBranchListView, GitStatusView } from '../bridge-types';
import type { DiffAnswerView } from '../commands/diffAnswer';

const status = (overrides: Partial<GitStatusView> = {}): GitStatusView => ({
  branch: 'main',
  isDirty: false,
  commitHash: 'abc123',
  stashCount: 0,
  consistency: 'immutable_generation',
  ...overrides,
});

/** The daemon's whole-tree answer: one unified diff, no `HEAD` side. */
const treeDiff = (overrides: Partial<DiffAnswerView> = {}): DiffAnswerView => ({
  title: 'Repository Diff',
  path: null,
  oldContent: '',
  newContent: 'diff --git a/src/lib.rs b/src/lib.rs\n-old\n+new\n',
  ...overrides,
});

/** The daemon's per-file answer: both sides of one file. */
const fileDiff = (overrides: Partial<DiffAnswerView> = {}): DiffAnswerView => ({
  title: 'Diff: src/lib.rs',
  path: 'src/lib.rs',
  oldContent: 'old\n',
  newContent: 'new\n',
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
  consistency: 'immutable_generation',
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

  it("draws the daemon's whole-tree diff answer", () => {
    render(<GitPanel status={null} diff={treeDiff()} branches={null} />);
    // The answer is the diff itself, not a summary of it: the daemon's
    // title and the unified diff it read are what the panel shows.
    expect(screen.getByTestId('git-diff-heading')).toHaveTextContent('Repository Diff');
    expect(screen.getByTestId('git-diff-body')).toHaveTextContent(
      'diff --git a/src/lib.rs b/src/lib.rs',
    );
    expect(screen.getByTestId('git-diff-body')).toHaveTextContent('-old');
    expect(screen.getByTestId('git-diff-body')).toHaveTextContent('+new');
  });

  it("draws both sides of the daemon's per-file diff answer", () => {
    render(<GitPanel status={null} diff={fileDiff()} branches={null} />);
    expect(screen.getByTestId('git-diff-heading')).toHaveTextContent('Diff: src/lib.rs');
    expect(screen.getByTestId('git-diff-path')).toHaveTextContent('src/lib.rs');
    expect(screen.getByTestId('git-diff-old')).toHaveTextContent('old');
    expect(screen.getByTestId('git-diff-new')).toHaveTextContent('new');
  });

  it('says the whole tree has no changes when the daemon answered with no diff', () => {
    render(<GitPanel status={null} diff={treeDiff({ newContent: '' })} branches={null} />);
    expect(screen.getByTestId('git-diff-empty')).toHaveTextContent('No changes.');
    expect(screen.queryByTestId('git-diff-body')).toBeNull();
  });

  it('says a file is new when the daemon answered with no HEAD side', () => {
    render(<GitPanel status={null} diff={fileDiff({ oldContent: '' })} branches={null} />);
    expect(screen.getByTestId('git-diff-old-empty')).toHaveTextContent('No file at HEAD.');
    expect(screen.getByTestId('git-diff-new')).toHaveTextContent('new');
  });

  it('shows the consistency class of each fact set it read', () => {
    // The class says what the observation is worth. A panel that drew the
    // numbers without it would be implying a guarantee the daemon never
    // gave, so every fact set names the class it was read under.
    render(
      <GitPanel
        status={status({ consistency: 'captured_not_current' })}
        diff={treeDiff()}
        branches={branches({ consistency: 'no_object_generation' })}
      />,
    );
    expect(screen.getByTestId('git-status-consistency')).toHaveTextContent(
      'captured, possibly stale',
    );
    expect(screen.getByTestId('git-branch-consistency')).toHaveTextContent(
      'no generation observed',
    );
    // The wire class travels with the facts, so the element names it too.
    expect(screen.getByTestId('git-status-consistency')).toHaveAttribute(
      'data-consistency',
      'captured_not_current',
    );
  });

  it('invents no consistency class for the diff answer', () => {
    // The daemon's diff answer carries no consistency class: it is the
    // daemon's answer to a command, not a git fact read. The panel must
    // not borrow the class of some other read and present it as this one.
    render(<GitPanel status={null} diff={treeDiff()} branches={null} />);
    expect(screen.queryByTestId('git-diff-consistency')).toBeNull();
  });

  it('says the facts are unknown until the host has read them', () => {
    render(<GitPanel status={null} diff={null} branches={null} />);
    // "not read yet" and "nothing to report" are different claims, and an
    // absent fact set must not read as an empty one.
    expect(screen.getByTestId('git-status-unknown')).toHaveTextContent('Git status not read yet.');
    expect(screen.getByTestId('git-diff-unknown')).toHaveTextContent('Diff not read yet.');
    expect(screen.getByTestId('git-branches-unknown')).toHaveTextContent('Branches not read yet.');
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
