import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { RunHistory } from './RunHistory';

describe('the desktop run history', () => {
  it("shows the daemon's own status token and summary for every run", () => {
    render(
      <RunHistory
        state="attached"
        runs={[{ runId: 'run-1', kind: 'test', status: 'running', summary: 'cargo test' }]}
      />,
    );
    expect(screen.getByTestId('run-history-kind')).toHaveTextContent('test');
    expect(screen.getByTestId('run-history-status')).toHaveTextContent('running');
    expect(screen.getByTestId('run-history-summary')).toHaveTextContent('cargo test');
  });

  it('says the snapshot is not installed yet instead of drawing an empty list', () => {
    render(<RunHistory state="connecting" runs={[]} />);
    expect(screen.getByTestId('run-history-no-snapshot')).toHaveTextContent(
      'No snapshot installed yet.',
    );
    expect(screen.queryByTestId('run-history-empty')).not.toBeInTheDocument();
  });

  it('says the session has no runs once a snapshot is installed', () => {
    render(<RunHistory state="attached" runs={[]} />);
    expect(screen.getByTestId('run-history-empty')).toHaveTextContent('No runs.');
    expect(screen.queryByTestId('run-history-no-snapshot')).not.toBeInTheDocument();
  });

  it('says no session is attached when the renderer holds no projection', () => {
    render(<RunHistory state={null} runs={[]} />);
    expect(screen.getByTestId('run-history-no-session')).toHaveTextContent('No session attached.');
  });
});
