import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { ChatRegion } from './ChatRegion';
import type { SessionPresentationView } from '../bridge-types';

const projection = (overrides: Partial<SessionPresentationView> = {}): SessionPresentationView => ({
  sessionId: 's-1',
  projectId: 'p0',
  workspaceId: 'w0',
  state: 'attached',
  selectedModel: null,
  selectedAgent: null,
  turn: null,
  messages: [{ messageId: 'm-1', role: 'assistant', text: 'on it', truncated: false }],
  truncatedMessages: 0,
  transcript: [],
  truncatedTranscriptMessages: 0,
  tools: [],
  runs: [],
  jobs: [],
  subagents: [],
  agentTree: [],
  agentRuns: [],
  toolPrograms: [],
  worktrees: [],
  runGroups: [],
  convergences: [],
  shellRuns: [],
  toolContracts: [],
  pluginCatalogue: [],
  recentTurns: [],
  todos: null,
  pendingPermissions: [],
  pendingQuestions: [],
  controller: null,
  artifactHandles: [],
  cursor: { eventSeq: 42, driverCursorSeq: 42, subscriptionKnown: true },
  resyncReason: null,
  ...overrides,
});

const handlers = () => ({
  onReadArtifact: vi.fn(),
  onRespondPermission: vi.fn(),
  onRespondQuestion: vi.fn(),
  onQuestionDraftChange: vi.fn(),
  onStopProjection: vi.fn(),
});

describe('the desktop chat region', () => {
  it('draws the projection’s messages', () => {
    render(<ChatRegion projection={projection()} excerpt={null} excerptBusy={null} responding={[]} questionDrafts={{}} {...handlers()} />);
    expect(screen.getByTestId('message-item')).toHaveTextContent('on it');
  });

  it('counts the messages the daemon omitted', () => {
    render(<ChatRegion projection={projection({ truncatedMessages: 4 })} excerpt={null} excerptBusy={null} responding={[]} questionDrafts={{}} {...handlers()} />);
    expect(screen.getByTestId('truncated-messages')).toHaveTextContent('4 older message(s)');
  });

  it('lists the tools the turn ran', () => {
    render(
      <ChatRegion
        projection={projection({
          tools: [
            {
              toolId: 't-1',
              toolName: 'bash',
              status: 'ok',
              summary: 'cargo test',
              hasArtifact: false,
              arguments: '',
              output: '',
              startedAt: null,
              completedAt: null,
              durationMs: null,
            },
          ],
        })}
        excerpt={null}
        excerptBusy={null}
        responding={[]}
        questionDrafts={{}}
        {...handlers()}
      />,
    );
    expect(screen.getByTestId('tool-item')).toHaveTextContent('cargo test');
  });

  it('offers the pending permissions and questions to the user', () => {
    render(
      <ChatRegion
        projection={projection({
          pendingPermissions: [
            { permissionId: 'perm-1', tool: 'write', scopeSummary: null, status: 'pending' },
          ],
          pendingQuestions: [
            { questionId: 'q-1', header: 'Choice', prompt: 'Pick one', status: 'pending' },
          ],
        })}
        excerpt={null}
        excerptBusy={null}
        responding={[]}
        questionDrafts={{}}
        {...handlers()}
      />,
    );
    expect(screen.getByTestId('permission-item')).toHaveTextContent('write');
    expect(screen.getByTestId('question-item')).toHaveTextContent('Pick one');
  });

  it('answers a permission through the renderer, not from the panel', () => {
    const handlers = {
      onReadArtifact: vi.fn(),
      onRespondPermission: vi.fn(),
      onRespondQuestion: vi.fn(),
      onQuestionDraftChange: vi.fn(),
      onStopProjection: vi.fn(),
    };
    render(
      <ChatRegion
        projection={projection({
          pendingPermissions: [
            { permissionId: 'perm-1', tool: 'write', scopeSummary: null, status: 'pending' },
          ],
        })}
        excerpt={null}
        excerptBusy={null}
        responding={[]}
        questionDrafts={{}}
        {...handlers}
      />,
    );
    fireEvent.click(screen.getByTestId('permission-deny-perm-1'));
    expect(handlers.onRespondPermission).toHaveBeenCalledWith('perm-1', 'deny');
  });

  it('says no session is attached when the renderer holds no projection', () => {
    render(<ChatRegion projection={null} excerpt={null} excerptBusy={null} responding={[]} questionDrafts={{}} {...handlers()} />);
    expect(screen.getByTestId('chat-empty')).toHaveTextContent('No session attached.');
  });
});
