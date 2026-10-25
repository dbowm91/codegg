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
        onDraftChange={vi.fn()}
        onPlanModeChange={vi.fn()}
        onSubmit={vi.fn()}
      />,
    );
    expect(screen.queryByTestId('prompt-input')).not.toBeInTheDocument();
    // The region itself stays in the frame.
    expect(screen.getByTestId('composer-region')).toBeInTheDocument();
  });
});
