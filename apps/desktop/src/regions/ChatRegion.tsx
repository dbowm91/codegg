import type { ArtifactExcerptView, SessionPresentationView } from '../bridge-types';
import { ToolDetail } from '../panels/ToolDetail';
import { TranscriptPanel } from '../panels/TranscriptPanel';
import { TurnSummary } from '../panels/TurnSummary';

// The chat region: the conversation.
//
// This is the surface the TUI calls the main buffer. It carries the
// projection's messages, the count of the ones the daemon omitted, the
// tools it ran, the artifacts it produced, and the two things the user
// owes the daemon an answer on — the pending permissions and the pending
// questions.
//
// The region is the frame's own scroll container: it scrolls on its own,
// apart from the side panel, so a long conversation never drags the panel
// with it.

export interface ChatRegionProps {
  /** The session's presentation view, or null when nothing is attached. */
  projection: SessionPresentationView | null;
  /** The artifact excerpt the user last read. */
  excerpt: ArtifactExcerptView | null;
  /** The handles whose excerpt is being read. */
  excerptBusy: string | null;
  /** The pending items the renderer is answering. */
  responding: string[];
  /** The answer draft of each pending question. */
  questionDrafts: Record<string, string>;
  /** Read an artifact's excerpt. */
  onReadArtifact: (handle: string) => void;
  /** Answer a pending permission. */
  onRespondPermission: (permissionId: string, choice: 'allow' | 'deny') => void;
  /** Answer a pending question. */
  onRespondQuestion: (questionId: string) => void;
  /** Change a pending question's answer draft. */
  onQuestionDraftChange: (questionId: string, value: string) => void;
  /** Detach the projection. */
  onStopProjection: () => void;
}

export function ChatRegion({
  projection,
  excerpt,
  excerptBusy,
  responding,
  questionDrafts,
  onReadArtifact,
  onRespondPermission,
  onRespondQuestion,
  onQuestionDraftChange,
  onStopProjection,
}: ChatRegionProps) {
  if (!projection) {
    return (
      <section className="chat-region" data-testid="chat-region">
        <p className="muted" data-testid="chat-empty">
          No session attached.
        </p>
      </section>
    );
  }

  return (
    <section className="chat-region" data-testid="chat-region">
      {projection.truncatedMessages > 0 && (
        <dl className="chat-facts">
          <dt>Omitted</dt>
          <dd data-testid="truncated-messages">{projection.truncatedMessages} older message(s)</dd>
        </dl>
      )}
      <h3>Messages</h3>
      {projection.messages.length ? (
        <ul data-testid="message-list">
          {projection.messages.map((message) => (
            <li key={message.messageId} data-testid="message-item">
              <strong>{message.role}</strong>
              <p>
                {message.text}
                {message.truncated && ' …'}
              </p>
            </li>
          ))}
        </ul>
      ) : (
        <p className="muted">No visible messages yet.</p>
      )}
      {!!projection.artifactHandles.length && (
        <>
          <h3>Artifacts</h3>
          <ul data-testid="artifact-list">
            {projection.artifactHandles.map((artifact) => (
              <li key={artifact.handle} data-testid="artifact-item">
                {artifact.handle} ({artifact.byteLength} bytes)
                <button
                  data-testid={`artifact-read-${artifact.handle}`}
                  disabled={excerptBusy === artifact.handle}
                  onClick={() => onReadArtifact(artifact.handle)}
                >
                  Read excerpt
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
      {!!projection.tools.length && (
        <>
          <h3>Tools</h3>
          <ul data-testid="tool-list">
            {projection.tools.map((tool) => (
              <li key={tool.toolId} data-testid="tool-item">
                {tool.toolName} — {tool.status} — {tool.summary}
                {tool.hasArtifact && ' (artifact)'}
              </li>
            ))}
          </ul>
        </>
      )}
      {!!projection.pendingPermissions.length && (
        <>
          <h3>Permissions</h3>
          <ul data-testid="permission-list">
            {projection.pendingPermissions.map((permission) => (
              <li key={permission.permissionId} data-testid="permission-item">
                {permission.tool} — {permission.status}
                <button
                  data-testid={`permission-allow-${permission.permissionId}`}
                  disabled={responding.includes(permission.permissionId)}
                  onClick={() => onRespondPermission(permission.permissionId, 'allow')}
                >
                  Allow
                </button>
                <button
                  data-testid={`permission-deny-${permission.permissionId}`}
                  disabled={responding.includes(permission.permissionId)}
                  onClick={() => onRespondPermission(permission.permissionId, 'deny')}
                >
                  Deny
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
      {!!projection.pendingQuestions.length && (
        <>
          <h3>Questions</h3>
          <ul data-testid="question-list">
            {projection.pendingQuestions.map((question) => (
              <li key={question.questionId} data-testid="question-item">
                {question.header ?? 'Question'} — {question.prompt} ({question.status})
                <textarea
                  data-testid={`question-answer-${question.questionId}`}
                  value={questionDrafts[question.questionId] ?? ''}
                  placeholder='JSON answers, e.g. ["a"]'
                  onChange={(event) => onQuestionDraftChange(question.questionId, event.target.value)}
                />
                <button
                  data-testid={`question-answer-button-${question.questionId}`}
                  disabled={responding.includes(question.questionId)}
                  onClick={() => onRespondQuestion(question.questionId)}
                >
                  Answer
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
      {excerpt && (
        <div>
          <h3>Excerpt</h3>
          <p data-testid="artifact-excerpt">{excerpt.content}</p>
          <p data-testid="artifact-excerpt-meta">
            {excerpt.handle} bytes {excerpt.start}-{excerpt.end}
            {excerpt.truncated ? ' (truncated)' : ''}
            {excerpt.redacted ? ' (redacted)' : ''}
          </p>
        </div>
      )}
      <button data-testid="projection-stop-button" onClick={onStopProjection}>
        Detach projection
      </button>
      <TurnSummary turn={projection.turn} />
      <TranscriptPanel
        messages={projection.transcript}
        truncatedMessages={projection.truncatedTranscriptMessages}
      />
      <ToolDetail tools={projection.tools} />
    </section>
  );
}
