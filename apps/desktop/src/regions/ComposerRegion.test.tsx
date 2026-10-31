import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { ComposerRegion } from './ComposerRegion';

describe('the desktop composer region', () => {
  it('sends the draft it was given', () => {
    const onSubmit = vi.fn();
    render(
      <ComposerRegion
        enabled
        draft="hello desktop"
        planMode={false}
        busy={false}
        shellRuns={[]}
        askRunId={null}
        onAskRunChange={vi.fn()}
        onDraftChange={vi.fn()}
        onPlanModeChange={vi.fn()}
        onSubmit={onSubmit}
      />,
    );
    fireEvent.click(screen.getByTestId('prompt-submit-button'));
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it('will not send an empty draft', () => {
    render(
      <ComposerRegion
        enabled
        draft="   "
        planMode={false}
        busy={false}
        shellRuns={[]}
        askRunId={null}
        onAskRunChange={vi.fn()}
        onDraftChange={vi.fn()}
        onPlanModeChange={vi.fn()}
        onSubmit={vi.fn()}
      />,
    );
    expect(screen.getByTestId('prompt-submit-button')).toBeDisabled();
  });

  it('reports a change to the draft and to plan mode', () => {
    const onDraftChange = vi.fn();
    const onPlanModeChange = vi.fn();
    render(
      <ComposerRegion
        enabled
        draft=""
        planMode={false}
        busy={false}
        shellRuns={[]}
        askRunId={null}
        onAskRunChange={vi.fn()}
        onDraftChange={onDraftChange}
        onPlanModeChange={onPlanModeChange}
        onSubmit={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByTestId('prompt-input'), { target: { value: 'hi' } });
    fireEvent.click(screen.getByTestId('plan-mode-checkbox'));
    expect(onDraftChange).toHaveBeenCalledWith('hi');
    expect(onPlanModeChange).toHaveBeenCalledWith(true);
  });

  it('draws no prompt when there is no workspace to route one to', () => {
    render(
      <ComposerRegion
        enabled={false}
        draft=""
        planMode={false}
        busy={false}
        shellRuns={[]}
        askRunId={null}
        onAskRunChange={vi.fn()}
        onDraftChange={vi.fn()}
        onPlanModeChange={vi.fn()}
        onSubmit={vi.fn()}
      />,
    );
    expect(screen.queryByTestId('prompt-input')).not.toBeInTheDocument();
    // The region itself stays in the frame.
    expect(screen.getByTestId('composer-region')).toBeInTheDocument();
  });

  // M007-I (plan 022, work package F): the ask names one of the daemon's
  // published runs. The strip offers exactly those runs and nothing else.
  it('offers the shell runs the daemon published and reports the chosen one', () => {
    const onAskRunChange = vi.fn();
    render(
      <ComposerRegion
        enabled
        draft="why did it fail?"
        planMode={false}
        busy={false}
        shellRuns={[
          { runId: 'run-1', command: 'cargo build', cwd: '/repo', origin: 'bang', status: 'exited' },
        ]}
        askRunId={null}
        onAskRunChange={onAskRunChange}
        onDraftChange={vi.fn()}
        onPlanModeChange={vi.fn()}
        onSubmit={vi.fn()}
      />,
    );
    const select = screen.getByTestId('composer-ask-run');
    expect(screen.getByRole('option', { name: /cargo build/ })).toBeInTheDocument();
    fireEvent.change(select, { target: { value: 'run-1' } });
    expect(onAskRunChange).toHaveBeenCalledWith('run-1');
  });

  it('offers no ask when the daemon published no shell run', () => {
    render(
      <ComposerRegion
        enabled
        draft=""
        planMode={false}
        busy={false}
        shellRuns={[]}
        askRunId={null}
        onAskRunChange={vi.fn()}
        onDraftChange={vi.fn()}
        onPlanModeChange={vi.fn()}
        onSubmit={vi.fn()}
      />,
    );
    expect(screen.queryByTestId('composer-ask')).not.toBeInTheDocument();
  });
});
