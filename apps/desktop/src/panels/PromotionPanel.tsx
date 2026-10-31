import type { SkillPromotionView, SkillTargetScope } from '../bridge-types';

// The daemon's skill-promotion workflow.
//
// M007-I (plan 022, work package F): the daemon owns the durable promotion
// record, so the desktop asks and renders. The panel names the step of the
// workflow the human wants and shows what the daemon answered; it holds no
// promotion state of its own and it names no path.
//
// The human names two identifiers — the habit a request is derived from and
// the proposal a step acts on — because the daemon resolves them against its
// own store. Everything else (the record's namespace, the session, the
// publication's target root) is the daemon's to resolve.

export interface PromotionPanelProps {
  /** The daemon's last answer, or null when there is none yet. */
  answer: SkillPromotionView | null;
  /** The habit a `begin` request is derived from. */
  habitId: string;
  /** Change the habit a `begin` request is derived from. */
  onHabitIdChange: (value: string) => void;
  /** The proposal a step acts on. */
  proposalId: string;
  /** Change the proposal a step acts on. */
  onProposalIdChange: (value: string) => void;
  /** The scope a publication targets. */
  targetScope: SkillTargetScope;
  /** Change the scope a publication targets. */
  onTargetScopeChange: (value: SkillTargetScope) => void;
  /** Whether a request is in flight. */
  busy?: boolean;
  /** The last request's failure, if it failed. */
  error?: string | null;
  /** Ask the daemon to stage a request from the named habit. */
  onBegin?: () => void;
  /** Ask the daemon for the workspace's proposals. */
  onList?: () => void;
  /** Ask the daemon for the named proposal in full. */
  onPreview?: () => void;
  /** Ask the daemon to publish the named proposal. */
  onPublish?: () => void;
  /** Ask the daemon to reject the named proposal. */
  onReject?: () => void;
}

export function PromotionPanel({
  answer,
  habitId,
  onHabitIdChange,
  proposalId,
  onProposalIdChange,
  targetScope,
  onTargetScopeChange,
  busy = false,
  error = null,
  onBegin,
  onList,
  onPreview,
  onPublish,
  onReject,
}: PromotionPanelProps) {
  return (
    <section className="surface" data-testid="promotion-surface">
      <h3>Skill promotion</h3>
      {error && (
        // A failed request is reported here rather than as a route error: it
        // is a promotion-surface condition, and the last answer the daemon
        // gave is still the truth.
        <p role="status" data-testid="promotion-error">
          Promotion request failed: {error}
        </p>
      )}
      <label>
        Habit id
        <input
          data-testid="promotion-habit-input"
          value={habitId}
          onChange={(event) => onHabitIdChange(event.target.value)}
        />
      </label>
      <button
        data-testid="promotion-begin-button"
        disabled={busy || !onBegin || !habitId.trim()}
        onClick={onBegin}
      >
        Begin request
      </button>
      <label>
        Proposal id
        <input
          data-testid="promotion-proposal-input"
          value={proposalId}
          onChange={(event) => onProposalIdChange(event.target.value)}
        />
      </label>
      <label>
        Publication scope
        <select
          data-testid="promotion-scope-select"
          value={targetScope}
          onChange={(event) => onTargetScopeChange(event.target.value as SkillTargetScope)}
        >
          <option value="project">Project</option>
          <option value="global">Global</option>
        </select>
      </label>
      <div className="panel-actions">
        <button data-testid="promotion-list-button" disabled={busy || !onList} onClick={onList}>
          List proposals
        </button>
        <button
          data-testid="promotion-preview-button"
          disabled={busy || !onPreview || !proposalId.trim()}
          onClick={onPreview}
        >
          Preview proposal
        </button>
        <button
          data-testid="promotion-publish-button"
          disabled={busy || !onPublish || !proposalId.trim()}
          onClick={onPublish}
        >
          Publish proposal
        </button>
        <button
          data-testid="promotion-reject-button"
          disabled={busy || !onReject || !proposalId.trim()}
          onClick={onReject}
        >
          Reject proposal
        </button>
      </div>
      {answer ? (
        <div data-testid="promotion-answer">
          <p className="fact-row" data-testid="promotion-answer-action">
            <span>action: </span>
            <strong data-testid="promotion-action">{answer.action}</strong>
            <span data-testid="promotion-changed">
              {answer.changed ? 'the record changed' : 'the record did not change'}
            </span>
          </p>
          {answer.requestId && (
            <p className="fact-row" data-testid="promotion-request-id">
              request: {answer.requestId}
            </p>
          )}
          {answer.draftPrompt !== null && (
            <>
              <h4>Draft prompt</h4>
              <pre data-testid="promotion-draft-prompt">{answer.draftPrompt}</pre>
            </>
          )}
          {answer.proposals !== null && (
            <>
              <h4>Proposals</h4>
              {answer.proposals.length ? (
                <ul data-testid="promotion-proposal-list">
                  {answer.proposals.map((proposal) => (
                    <li
                      key={proposal.proposalId}
                      data-testid="promotion-proposal-item"
                      data-proposal-id={proposal.proposalId}
                    >
                      <span data-testid="promotion-proposal-name">{proposal.name}</span>
                      <span data-testid="promotion-proposal-status">{proposal.status}</span>
                      <span data-testid="promotion-proposal-revision">
                        revision {proposal.revision}
                      </span>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="muted" data-testid="promotion-proposals-empty">
                  No proposals.
                </p>
              )}
            </>
          )}
          {answer.detail && (
            <>
              <h4>Proposal</h4>
              <p className="fact-row" data-testid="promotion-detail-heading">
                <strong data-testid="promotion-detail-name">{answer.detail.name}</strong>
                <span data-testid="promotion-detail-status">{answer.detail.status}</span>
                <span data-testid="promotion-detail-scope">{answer.detail.targetScope}</span>
                <span data-testid="promotion-detail-revision">
                  revision {answer.detail.revision}
                </span>
              </p>
              {answer.detail.diagnostics.length ? (
                <ul data-testid="promotion-diagnostics">
                  {answer.detail.diagnostics.map((diagnostic, index) => (
                    <li
                      key={`${diagnostic.severity}-${index}`}
                      data-testid="promotion-diagnostic"
                      data-severity={diagnostic.severity}
                    >
                      <span data-testid="promotion-diagnostic-severity">
                        {diagnostic.severity}
                      </span>
                      <span data-testid="promotion-diagnostic-reason">{diagnostic.reason}</span>
                      {diagnostic.location && (
                        <span data-testid="promotion-diagnostic-location">
                          {diagnostic.location}
                        </span>
                      )}
                    </li>
                  ))}
                </ul>
              ) : null}
              {/* The body is the model's own draft, and the human reads it
                  before approving it. */}
              <pre data-testid="promotion-detail-body">{answer.detail.skillMarkdown}</pre>
            </>
          )}
          {answer.publishedRelativePath !== null && (
            <p className="fact-row" data-testid="promotion-published-path">
              published: {answer.publishedRelativePath}
            </p>
          )}
        </div>
      ) : (
        <p className="muted" data-testid="promotion-no-answer">
          No promotion answer yet.
        </p>
      )}
    </section>
  );
}
