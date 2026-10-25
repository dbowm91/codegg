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

export interface ComposerRegionProps {
  /** Whether a workspace is selected — the prompt can be sent. */
  enabled: boolean;
  /** The prompt draft. */
  draft: string;
  /** Whether the prompt is a plan. */
  planMode: boolean;
  /** Whether a prompt is in flight. */
  busy: boolean;
  /** Change the prompt draft. */
  onDraftChange: (value: string) => void;
  /** Turn plan mode on or off. */
  onPlanModeChange: (value: boolean) => void;
  /** Send the prompt. */
  onSubmit: () => void;
}

export function ComposerRegion({
  enabled,
  draft,
  planMode,
  busy,
  onDraftChange,
  onPlanModeChange,
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
