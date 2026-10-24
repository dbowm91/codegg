import { useState } from 'react';
import type { ToolSummaryView } from '../bridge-types';
import { formatDuration } from './format';

// Tool detail: the arguments, the output and the duration of every tool
// the turn ran.
//
// The TUI shows a tool row that can be expanded to reveal its input and
// output (`src/tui/components/tool_output.rs`, and the click-to-expand
// contract in `tests/tui_render/tui_main_buffer.inc:142`). The desktop
// carries the same already-redacted/summarised strings across the
// bridge (`apps/desktop/src-tauri/src/present.rs`), so the same
// disclosure is available here — collapsed by default, because a
// transcript of long tool outputs is unreadable.

export interface ToolDetailProps {
  tools: ToolSummaryView[];
}

export function ToolDetail({ tools }: ToolDetailProps) {
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});

  if (tools.length === 0) {
    return (
      <section className="surface" data-testid="tool-detail-surface">
        <h3>Tool detail</h3>
        <p className="muted" data-testid="tool-detail-empty">
          No tools in this turn.
        </p>
      </section>
    );
  }

  return (
    <section className="surface" data-testid="tool-detail-surface">
      <h3>Tool detail</h3>
      <ul data-testid="tool-detail-list">
        {tools.map((tool) => {
          const open = expanded[tool.toolId] === true;
          const duration = formatDuration(tool.durationMs);
          return (
            <li key={tool.toolId} data-testid="tool-detail-item" data-tool-id={tool.toolId}>
              <div className="tool-detail-head">
                <strong data-testid="tool-detail-name">{tool.toolName}</strong>
                <span data-testid="tool-detail-status">{tool.status}</span>
                {duration && <span data-testid="tool-detail-duration">{duration}</span>}
              </div>
              <p data-testid="tool-detail-summary">{tool.summary}</p>
              {tool.arguments && (
                <pre data-testid="tool-detail-arguments">{tool.arguments}</pre>
              )}
              <button
                data-testid={`tool-detail-toggle-${tool.toolId}`}
                aria-expanded={open}
                onClick={() =>
                  setExpanded((previous) => ({ ...previous, [tool.toolId]: !previous[tool.toolId] }))
                }
              >
                {open ? 'Hide output' : 'Show output'}
              </button>
              {open && (
                <pre data-testid="tool-detail-output">{tool.output || '(no output)'}</pre>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
