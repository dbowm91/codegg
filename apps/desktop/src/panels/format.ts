// Display helpers shared by the desktop presenter surfaces.
//
// The rules here mirror the TUI's own task-surface rules so the desktop
// and the terminal never disagree about what a state means:
//
// - the live/terminal state rule
//   (`src/tui/app/state/tasks.rs:247-259`): an empty state is live, a
//   known terminal state is not, and an unrecognised non-empty state is
//   live — the asymmetry is deliberate, because hiding work that is
//   still running costs the user the fact they asked for;
// - the running-subagent rule
//   (`src/tui/app/state/subagents.rs:124-134`);
// - the subagent label rule: the curated display name when there is
//   one, the agent type otherwise
//   (`src/tui/app/state/subagents.rs:81-86`);
// - the todo status glyph
//   (`src/tui/components/sidebar.rs:1372-1379`);
// - the subagent status glyph
//   (`src/tui/components/sidebar.rs:1944-1953`);
// - the tool duration format
//   (`src/tui/components/tool_output.rs:41-47`).

/// The tokens that mean "finished", matched as substrings of a
/// lower-cased state.
const TERMINAL_STATE_TOKENS = [
  'succeed',
  'success',
  'complete',
  'fail',
  'error',
  'cancel',
  'abort',
  'expired',
  'timed_out',
  'timeout',
];

/// The tokens that mean "a subagent is still working".
const RUNNING_STATUS_TOKENS = [
  'running',
  'in_progress',
  'in-progress',
  'active',
  'pending',
  'queued',
  'started',
];

/// Whether a job state still means "something is being waited on".
///
/// Mirrors `is_live_state` (`src/tui/app/state/tasks.rs:247-259`).
export function isLiveJobState(state: string): boolean {
  const normalized = state.trim().toLowerCase();
  if (!normalized) return true;
  return !TERMINAL_STATE_TOKENS.some((token) => normalized.includes(token));
}

/// Whether a subagent is still working.
///
/// Mirrors `SubagentRow::is_running`
/// (`src/tui/app/state/subagents.rs:124-134`). A finished subagent is
/// not work in flight: the task surface is a status view, not a history
/// log.
export function isRunningSubagent(status: string): boolean {
  const normalized = status.trim().toLowerCase();
  if (!normalized) return true;
  return RUNNING_STATUS_TOKENS.some((token) => normalized.includes(token));
}

/// What a subagent is called: the curated display name when the
/// projection carried one, the agent type otherwise.
///
/// Mirrors `SubagentRow::label` (`src/tui/app/state/subagents.rs:81-86`).
export function subagentLabel(displayName: string | null, agent: string): string {
  const curated = displayName?.trim();
  return curated ? curated : agent;
}

/// The glyph a todo entry carries.
///
/// Mirrors the sidebar's todo row (`src/tui/components/sidebar.rs:1372-1379`).
export function todoStatusGlyph(status: string): string {
  switch (status.trim().toLowerCase()) {
    case 'completed':
      return '[x]';
    case 'in_progress':
      return '[>]';
    default:
      return '[ ]';
  }
}

/// The glyph a subagent's status carries.
///
/// Mirrors `subagent_glyph` (`src/tui/components/sidebar.rs:1944-1953`).
export function subagentStatusGlyph(status: string): string {
  const normalized = status.trim().toLowerCase();
  if (normalized.includes('fail') || normalized.includes('error')) return '✗';
  if (normalized.includes('complet') || normalized.includes('succeed')) return '✓';
  return '●';
}

/// How a tool's duration is written.
///
/// Mirrors `format_duration` (`src/tui/components/tool_output.rs:41-47`).
export function formatDuration(durationMs: number | null): string | null {
  if (durationMs === null) return null;
  if (durationMs < 1000) return `${durationMs}ms`;
  return `${(durationMs / 1000).toFixed(1)}s`;
}

/// How a token counter is written. `null` means the daemon did not
/// resolve the value, which is shown as an em dash rather than as zero:
/// an unresolved counter is not the same claim as an empty one.
export function formatTokens(value: number | null): string {
  return value === null ? '—' : String(value);
}
