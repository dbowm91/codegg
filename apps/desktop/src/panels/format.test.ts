import { describe, expect, it } from 'vitest';
import {
  formatDuration,
  formatTokens,
  isLiveJobState,
  isRunningSubagent,
  subagentLabel,
  subagentStatusGlyph,
  todoStatusGlyph,
} from './format';

// These assertions mirror the TUI's own task-surface rules. The rules
// live in Rust (`src/tui/app/state/tasks.rs`,
// `src/tui/app/state/subagents.rs`, `src/tui/components/sidebar.rs`),
// so they are asserted here against the same semantic state rather than
// imported.

describe('mirrored TUI task-surface rules', () => {
  it('treats a known terminal job state as finished', () => {
    expect(isLiveJobState('succeeded')).toBe(false);
    expect(isLiveJobState('failed')).toBe(false);
    expect(isLiveJobState('cancelled')).toBe(false);
    expect(isLiveJobState('timed_out')).toBe(false);
  });

  it('treats an empty or unrecognised job state as live', () => {
    // An unknown state must not read as "finished": hiding work that is
    // still running costs the user the fact they asked for.
    expect(isLiveJobState('')).toBe(true);
    expect(isLiveJobState('quantum_flux')).toBe(true);
  });

  it('separates running subagents from finished ones', () => {
    expect(isRunningSubagent('running')).toBe(true);
    expect(isRunningSubagent('in_progress')).toBe(true);
    expect(isRunningSubagent('queued')).toBe(true);
    expect(isRunningSubagent('completed')).toBe(false);
    expect(isRunningSubagent('failed')).toBe(false);
  });

  it('names a subagent by its display name and falls back to the agent type', () => {
    expect(subagentLabel('Sally', 'explore')).toBe('Sally');
    expect(subagentLabel(null, 'explore')).toBe('explore');
    expect(subagentLabel('   ', 'explore')).toBe('explore');
  });

  it('draws the TUI todo glyphs', () => {
    expect(todoStatusGlyph('completed')).toBe('[x]');
    expect(todoStatusGlyph('in_progress')).toBe('[>]');
    expect(todoStatusGlyph('pending')).toBe('[ ]');
  });

  it('draws the TUI subagent glyphs', () => {
    expect(subagentStatusGlyph('failed')).toBe('✗');
    expect(subagentStatusGlyph('completed')).toBe('✓');
    expect(subagentStatusGlyph('running')).toBe('●');
  });

  it('writes a duration the way the TUI writes it', () => {
    expect(formatDuration(120)).toBe('120ms');
    expect(formatDuration(1200)).toBe('1.2s');
    expect(formatDuration(null)).toBeNull();
  });

  it('distinguishes an unresolved token counter from a zero one', () => {
    expect(formatTokens(null)).toBe('—');
    expect(formatTokens(0)).toBe('0');
  });
});
