import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { ToolDetail } from './ToolDetail';
import type { ToolSummaryView } from '../bridge-types';

const tool = (overrides: Partial<ToolSummaryView> = {}): ToolSummaryView => ({
  toolId: 'tool-1',
  toolName: 'bash',
  status: 'completed',
  summary: 'ran the suite',
  hasArtifact: false,
  arguments: 'cargo test',
  output: '42 passed; 0 failed',
  startedAt: 10,
  completedAt: 1120,
  durationMs: 1110,
  ...overrides,
});

describe('the desktop tool-detail surface', () => {
  it('shows the arguments and keeps the output collapsed until it is asked for', () => {
    render(<ToolDetail tools={[tool()]} />);
    expect(screen.getByTestId('tool-detail-arguments')).toHaveTextContent('cargo test');
    expect(screen.queryByTestId('tool-detail-output')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('tool-detail-toggle-tool-1'));
    expect(screen.getByTestId('tool-detail-output')).toHaveTextContent('42 passed; 0 failed');
  });

  it('writes the duration the way the TUI writes it', () => {
    render(<ToolDetail tools={[tool({ durationMs: 1110 })]} />);
    expect(screen.getByTestId('tool-detail-duration')).toHaveTextContent('1.1s');
  });

  it('says so when the turn ran no tools', () => {
    render(<ToolDetail tools={[]} />);
    expect(screen.getByTestId('tool-detail-empty')).toHaveTextContent('No tools in this turn.');
  });
});
