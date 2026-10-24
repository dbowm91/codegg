import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { TaskPanel } from './TaskPanel';
import type { TodoListView } from '../bridge-types';

const list = (items: TodoListView['items']): TodoListView => ({
  sessionId: 's-1',
  items,
});

describe('the desktop task surface', () => {
  it('draws the TUI glyph and the content of every entry', () => {
    render(
      <TaskPanel
        todos={list([
          { id: 't1', content: 'Map the auth flow', status: 'completed', priority: 'high' },
          { id: 't2', content: 'Write the presenter', status: 'in_progress', priority: 'medium' },
        ])}
      />,
    );
    const items = screen.getAllByTestId('task-item');
    expect(items).toHaveLength(2);
    expect(screen.getAllByTestId('task-glyph').map((node) => node.textContent)).toEqual([
      '[x]',
      '[>]',
    ]);
    expect(screen.getAllByTestId('task-content').map((node) => node.textContent)).toEqual([
      'Map the auth flow',
      'Write the presenter',
    ]);
  });

  it('says the list is unknown until the host has read one', () => {
    render(<TaskPanel todos={null} />);
    // "No tasks" and "not read yet" are different claims, and an absent
    // list must not read as an empty one.
    expect(screen.getByTestId('task-unknown')).toHaveTextContent('Task list not read yet.');
    expect(screen.queryByTestId('task-empty')).not.toBeInTheDocument();
  });

  it('says the session has no tasks when the host read an empty list', () => {
    render(<TaskPanel todos={list([])} />);
    expect(screen.getByTestId('task-empty')).toHaveTextContent('No tasks.');
    expect(screen.queryByTestId('task-unknown')).not.toBeInTheDocument();
  });

  it('lists the live jobs and leaves the finished ones out', () => {
    render(
      <TaskPanel
        todos={list([])}
        jobs={[
          { jobId: 'job-1', kind: 'test', state: 'running', summary: 'cargo test' },
          { jobId: 'job-2', kind: 'shell', state: 'succeeded', summary: 'ls' },
        ]}
      />,
    );
    const live = screen.getByTestId('task-job-list');
    expect(live).toHaveTextContent('cargo test');
    // A finished job is not work in flight.
    expect(live).not.toHaveTextContent('ls');
  });

  it('reports a failed read without blanking the list it already has', () => {
    render(
      <TaskPanel
        todos={list([{ id: 't1', content: 'Ship it', status: 'pending', priority: 'low' }])}
        error="stale route"
      />,
    );
    expect(screen.getByTestId('task-error')).toHaveTextContent('Task read failed: stale route');
    expect(screen.getByTestId('task-item')).toHaveTextContent('Ship it');
  });

  it('asks the host to read the list again', () => {    const onRefresh = vi.fn();
    render(<TaskPanel todos={list([])} onRefresh={onRefresh} />);
    fireEvent.click(screen.getByTestId('task-refresh-button'));
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });
});
