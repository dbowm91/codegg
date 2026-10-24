import type { TurnSummaryView } from '../bridge-types';
import { formatTokens } from './format';

// The turn's own summary: its state and its token counters.
//
// The counters the host carries are `input_tokens`, `output_tokens`,
// `cached_tokens`, `context_limit` and `reasoning_tokens`
// (`apps/desktop/src-tauri/src/bridge.rs`). `reasoning_tokens` is a
// **count** and nothing else: the reasoning text itself is `Internal`
// classification and never crosses the bridge, so this panel must never
// widen that. A null counter means the daemon did not resolve it, which
// is shown as an em dash rather than as zero — an unresolved counter is
// not the same claim as an empty one.

export interface TurnSummaryProps {
  turn: TurnSummaryView | null;
}

export function TurnSummary({ turn }: TurnSummaryProps) {
  if (!turn) {
    return (
      <section className="surface" data-testid="turn-summary-surface">
        <h3>Turn</h3>
        <p className="muted" data-testid="turn-summary-empty">
          No turn yet.
        </p>
      </section>
    );
  }

  return (
    <section className="surface" data-testid="turn-summary-surface">
      <h3>Turn</h3>
      <dl>
        <dt>Id</dt>
        <dd data-testid="turn-summary-id">{turn.turnId}</dd>
        <dt>Status</dt>
        <dd data-testid="turn-summary-status">{turn.status}</dd>
        {turn.stopReason && (
          <>
            <dt>Stop reason</dt>
            <dd data-testid="turn-summary-stop-reason">{turn.stopReason}</dd>
          </>
        )}
        {turn.error && (
          <>
            <dt>Error</dt>
            <dd data-testid="turn-summary-error">{turn.error}</dd>
          </>
        )}
        <dt>Input tokens</dt>
        <dd data-testid="turn-input-tokens">{formatTokens(turn.inputTokens)}</dd>
        <dt>Output tokens</dt>
        <dd data-testid="turn-output-tokens">{formatTokens(turn.outputTokens)}</dd>
        <dt>Cached tokens</dt>
        <dd data-testid="turn-cached-tokens">{formatTokens(turn.cachedTokens)}</dd>
        <dt>Reasoning tokens</dt>
        <dd data-testid="turn-reasoning-tokens">{formatTokens(turn.reasoningTokens)}</dd>
        <dt>Context limit</dt>
        <dd data-testid="turn-context-limit">{formatTokens(turn.contextLimit)}</dd>
      </dl>
    </section>
  );
}
