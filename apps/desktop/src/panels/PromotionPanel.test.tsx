import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { PromotionPanel } from './PromotionPanel';
import type { SkillPromotionView } from '../bridge-types';

const answer = (overrides: Partial<SkillPromotionView> = {}): SkillPromotionView => ({
  action: 'list',
  changed: false,
  requestId: null,
  draftPrompt: null,
  proposals: null,
  detail: null,
  publishedRelativePath: null,
  ...overrides,
});

describe('the desktop skill-promotion surface', () => {
  it('says no answer has arrived yet', () => {
    render(
      <PromotionPanel
        answer={null}
        habitId=""
        onHabitIdChange={() => {}}
        proposalId=""
        onProposalIdChange={() => {}}
        targetScope="project"
        onTargetScopeChange={() => {}}
      />,
    );
    // "not asked yet" and "nothing to promote" are different claims.
    expect(screen.getByTestId('promotion-no-answer')).toHaveTextContent(
      'No promotion answer yet.',
    );
  });

  it('asks the daemon to begin a request from the named habit', () => {
    const onBegin = vi.fn();
    render(
      <PromotionPanel
        answer={null}
        habitId="habit-1"
        onHabitIdChange={() => {}}
        proposalId=""
        onProposalIdChange={() => {}}
        targetScope="project"
        onTargetScopeChange={() => {}}
        onBegin={onBegin}
      />,
    );
    fireEvent.click(screen.getByTestId('promotion-begin-button'));
    expect(onBegin).toHaveBeenCalledTimes(1);
  });

  it('will not begin a request without a habit', () => {
    // The daemon resolves the habit from its own store, so an unnamed habit
    // is a request it would refuse. The button says so instead of sending it.
    render(
      <PromotionPanel
        answer={null}
        habitId="  "
        onHabitIdChange={() => {}}
        proposalId=""
        onProposalIdChange={() => {}}
        targetScope="project"
        onTargetScopeChange={() => {}}
        onBegin={vi.fn()}
      />,
    );
    expect(screen.getByTestId('promotion-begin-button')).toBeDisabled();
  });

  it('will not act on a proposal the human has not named', () => {
    render(
      <PromotionPanel
        answer={null}
        habitId=""
        onHabitIdChange={() => {}}
        proposalId=""
        onProposalIdChange={() => {}}
        targetScope="project"
        onTargetScopeChange={() => {}}
        onPreview={vi.fn()}
        onPublish={vi.fn()}
        onReject={vi.fn()}
      />,
    );
    expect(screen.getByTestId('promotion-preview-button')).toBeDisabled();
    expect(screen.getByTestId('promotion-publish-button')).toBeDisabled();
    expect(screen.getByTestId('promotion-reject-button')).toBeDisabled();
  });

  it('draws the proposals the daemon listed', () => {
    render(
      <PromotionPanel
        answer={answer({
          action: 'list',
          proposals: [
            {
              proposalId: 'proposal-1',
              name: 'testing-ci',
              description: 'Select focused verification.',
              status: 'awaiting_review',
              revision: 2,
            },
          ],
        })}
        habitId=""
        onHabitIdChange={() => {}}
        proposalId=""
        onProposalIdChange={() => {}}
        targetScope="project"
        onTargetScopeChange={() => {}}
      />,
    );
    const items = screen.getAllByTestId('promotion-proposal-item');
    expect(items).toHaveLength(1);
    expect(screen.getByTestId('promotion-proposal-name')).toHaveTextContent('testing-ci');
    expect(screen.getByTestId('promotion-proposal-status')).toHaveTextContent('awaiting_review');
    expect(screen.getByTestId('promotion-proposal-revision')).toHaveTextContent('revision 2');
  });

  it('draws the proposal the daemon previewed, with its body', () => {
    render(
      <PromotionPanel
        answer={answer({
          action: 'preview',
          detail: {
            proposalId: 'proposal-1',
            name: 'testing-ci',
            status: 'awaiting_review',
            revision: 2,
            targetScope: 'project',
            skillMarkdown: '# testing-ci\n\nSelect focused verification.\n',
            diagnostics: [
              { severity: 'warning', reason: 'the name is long', location: 'SKILL.md:1' },
            ],
            publishedRelativePath: null,
          },
        })}
        habitId=""
        onHabitIdChange={() => {}}
        proposalId=""
        onProposalIdChange={() => {}}
        targetScope="project"
        onTargetScopeChange={() => {}}
      />,
    );
    // The human reads the body before approving it, so the preview shows it.
    expect(screen.getByTestId('promotion-detail-body')).toHaveTextContent('Select focused verification.');
    expect(screen.getByTestId('promotion-detail-scope')).toHaveTextContent('project');
    const diagnostics = screen.getAllByTestId('promotion-diagnostic');
    expect(diagnostics).toHaveLength(1);
    expect(screen.getByTestId('promotion-diagnostic-severity')).toHaveTextContent('warning');
    expect(screen.getByTestId('promotion-diagnostic-location')).toHaveTextContent('SKILL.md:1');
  });

  it('draws the draft prompt the daemon staged', () => {
    render(
      <PromotionPanel
        answer={answer({
          action: 'begin',
          changed: true,
          requestId: 'request-1',
          draftPrompt: 'Draft a skill for the habit.',
        })}
        habitId=""
        onHabitIdChange={() => {}}
        proposalId=""
        onProposalIdChange={() => {}}
        targetScope="project"
        onTargetScopeChange={() => {}}
      />,
    );
    expect(screen.getByTestId('promotion-action')).toHaveTextContent('begin');
    expect(screen.getByTestId('promotion-changed')).toHaveTextContent('the record changed');
    expect(screen.getByTestId('promotion-request-id')).toHaveTextContent('request-1');
    expect(screen.getByTestId('promotion-draft-prompt')).toHaveTextContent(
      'Draft a skill for the habit.',
    );
  });

  it('draws where the daemon published the proposal', () => {
    render(
      <PromotionPanel
        answer={answer({
          action: 'publish',
          changed: true,
          publishedRelativePath: '.opencode/skills/testing-ci/SKILL.md',
        })}
        habitId=""
        onHabitIdChange={() => {}}
        proposalId=""
        onProposalIdChange={() => {}}
        targetScope="project"
        onTargetScopeChange={() => {}}
      />,
    );
    expect(screen.getByTestId('promotion-published-path')).toHaveTextContent(
      '.opencode/skills/testing-ci/SKILL.md',
    );
  });

  it('reports a failed request without blanking the answer it already has', () => {
    render(
      <PromotionPanel
        answer={answer({ action: 'list' })}
        habitId=""
        onHabitIdChange={() => {}}
        proposalId=""
        onProposalIdChange={() => {}}
        targetScope="project"
        onTargetScopeChange={() => {}}
        error="skill_promotion_no_session: the promotion request names no session"
      />,
    );
    expect(screen.getByTestId('promotion-error')).toHaveTextContent(
      'Promotion request failed: skill_promotion_no_session',
    );
    expect(screen.getByTestId('promotion-action')).toHaveTextContent('list');
  });
});
