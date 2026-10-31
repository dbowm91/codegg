// The composer region: the prompt.
//
// It is the one region the user writes into, so it sits at the bottom of
// the frame, directly above the status row — the same order the TUI uses
// (transcript, then the prompt box, then the footer).
//
// The prompt is only meaningful once a workspace is selected: the daemon
// routes a prompt to a workspace, and a route without one has nowhere to
// send it. The region stays in the frame either way, so the frame's
// regions do not come and go with the route.
//
// M007-I (plan 022, work package F): the region also carries the **ask**.
// The human names one of the daemon's published shell runs, and the submit
// becomes the daemon's atomic ask — it stages the evidence and admits the
// turn together. The region names the run and never the bytes: the run's
// output is the daemon's own, and it is the daemon that reads it.

import type { ShellRunView } from '../bridge-types';

export interface ComposerRegionProps {
  /** Whether a workspace is selected — the prompt can be sent. */
  enabled: boolean;
  /** The prompt draft. */
  draft: string;
  /** Whether the prompt is a plan. */
  planMode: boolean;
  /** Whether a prompt is in flight. */
  busy: boolean;
  /** The shell runs the daemon published for the session. */
  shellRuns: ShellRunView[];
  /** The run an ask names, or null for an ordinary prompt. */
  askRunId: string | null;
  /** Change the prompt draft. */
  onDraftChange: (value: string) => void;
  /** Turn plan mode on or off. */
  onPlanModeChange: (value: boolean) => void;
  /** Name the shell run an ask is about, or clear the ask. */
  onAskRunChange: (runId: string | null) => void;
  /** Send the prompt. */
  onSubmit: () => void;
}

export function ComposerRegion({
  enabled,
  draft,
  planMode,
  busy,
  shellRuns,
  askRunId,
  onDraftChange,
  onPlanModeChange,
  onAskRunChange,
  onSubmit,
}: ComposerRegionProps) {
  return (
    <section
      className="composer-region"
      data-testid="composer-region"
      data-enabled={enabled ? 'true' : 'false'}
    >
      {enabled && (
        <div>
          {shellRuns.length > 0 && (
            <label className="composer-ask" data-testid="composer-ask">
              <span>Ask about</span>
              <select
                data-testid="composer-ask-run"
                value={askRunId ?? ''}
                onChange={(event) => onAskRunChange(event.target.value || null)}
              >
                <option value="">no shell run</option>
                {shellRuns.map((run) => (
                  <option key={run.runId} value={run.runId}>
                    {run.command} ({run.status})
                  </option>
                ))}
              </select>
            </label>
          )}
          <textarea
            data-testid="prompt-input"
            value={draft}
            placeholder="Ask the session"
            onChange={(event) => onDraftChange(event.target.value)}
          />
          <label>
            <input
              type="checkbox"
              data-testid="plan-mode-checkbox"
              checked={planMode}
              onChange={(event) => onPlanModeChange(event.target.checked)}
            />{' '}
            Plan mode
          </label>
          <button
            data-testid="prompt-submit-button"
            disabled={busy || !draft.trim()}
            onClick={onSubmit}
          >
            Submit prompt
          </button>
        </div>
      )}
    </section>
  );
}
