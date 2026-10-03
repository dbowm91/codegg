import { useEffect, useRef, useState } from 'react';
import type { ArtifactExcerptView, ControllerSummaryView } from './bridge-types';
import { bridge } from './bridge';
import type {
  ConnectionSnapshot,
  ProjectDetailView,
  ProjectSummary,
  RouteTokenView,
  SessionPresentationView,
  SessionSummaryView,
} from './bridge-types';

const initial: ConnectionSnapshot = { state: 'starting', daemonId: null, protocolVersion: null, uptimeSeconds: null, activeSessions: null, error: null, connectionGeneration: 0 };

export function App() {
  const [connection, setConnection] = useState(initial);
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [busy, setBusy] = useState(false);
  // Renderer-visible subscription identity for the active connection
  // generation. Display-only (mirrors the installed bridge handle); the E2E
  // trajectory waits on it to prove the re-subscribe landed before mutating
  // the catalog, instead of racing the async subscribe round-trip.
  const [subscriptionId, setSubscriptionId] = useState<string | null>(null);
  const generation = useRef(0);
  const connectionRef = useRef(connection);
  connectionRef.current = connection;

  // M004 route state: explicit project/workspace/session selection.
  // Route tokens are echoed back on mutating calls; the Rust host drops
  // any completion for a superseded generation. Renderer state here is
  // presentation-only and never projection authority.
  const [detail, setDetail] = useState<ProjectDetailView | null>(null);
  const [sessions, setSessions] = useState<SessionSummaryView[]>([]);
  const [projection, setProjection] = useState<SessionPresentationView | null>(null);
  const [routeError, setRouteError] = useState<string | null>(null);
  const [routeBusy, setRouteBusy] = useState(false);
  const [newTitle, setNewTitle] = useState('');
  const [controller, setController] = useState<ControllerSummaryView | null>(null);
  const [responding, setResponding] = useState<string[]>([]);
  const [questionDrafts, setQuestionDrafts] = useState<Record<string, string>>({});
  const [excerpt, setExcerpt] = useState<ArtifactExcerptView | null>(null);
  const [excerptBusy, setExcerptBusy] = useState<string | null>(null);
  const [promptDraft, setPromptDraft] = useState('');
  const [planMode, setPlanMode] = useState(false);
  const [promptBusy, setPromptBusy] = useState(false);
  const routeRequest = useRef(0);

  const currentToken: RouteTokenView | null = detail?.routeToken ?? null;

  const refreshProjects = async (forGeneration = generation.current, forConnection = connectionRef.current.connectionGeneration) => {
    try {
      const result = await bridge.projects();
      if (forGeneration === generation.current && forConnection === connectionRef.current.connectionGeneration) setProjects(result);
    } catch (error) {
      if (forGeneration === generation.current && forConnection === connectionRef.current.connectionGeneration) setConnection((s) => ({ ...s, error: String(error) }));
    }
  };
  const displayedProjects = projects.slice(0, 50);
  useEffect(() => {
    let mounted = true;
    const current = ++generation.current;
    const connecting = { ...initial, state: 'connecting' as const };
    connectionRef.current = connecting;
    setConnection(connecting);
    void bridge.connect().then((snapshot) => { if (mounted && current === generation.current) { connectionRef.current = snapshot; setConnection(snapshot); if (snapshot.state === 'connected') void refreshProjects(current, snapshot.connectionGeneration); } })
      .catch((error: unknown) => { if (mounted && current === generation.current) { const failed = { ...initial, state: 'disconnected' as const, error: String(error) }; connectionRef.current = failed; setConnection(failed); } });
    return () => { mounted = false; };
  }, []);
  useEffect(() => {
    if (connection.state !== 'connected') return;
    const expectedGeneration = connection.connectionGeneration;
    let cancelled = false;
    let handle: { unsubscribe: () => void } | null = null;
    void bridge.subscribe(() => {
      if (cancelled) return;
      if (connectionRef.current.connectionGeneration !== expectedGeneration) return;
      void refreshProjects(generation.current, expectedGeneration);
    }).then((subscription) => {
      if (cancelled) {
        subscription.unsubscribe();
        return;
      }
      // A reconnect may have installed a newer generation while subscribe
      // was in flight; immediately release this stale subscription.
      if (connectionRef.current.connectionGeneration !== subscription.connectionGeneration) {
        subscription.unsubscribe();
        return;
      }
      handle = subscription;
      setSubscriptionId(subscription.subscriptionId);
    }).catch(() => undefined);
    return () => {
      cancelled = true;
      handle?.unsubscribe();
      setSubscriptionId(null);
    };
  }, [connection.state, connection.connectionGeneration]);

  // A new connection invalidates all route/projection state: the Rust
  // host stopped the projection owner (cursor retained), so the
  // renderer clears its route and requires explicit re-selection before
  // any session attach can resume.
  useEffect(() => {
    if (connection.state !== 'connected') return;
    routeRequest.current += 1;
    setDetail(null);
    setSessions([]);
    setProjection(null);
    setRouteError(null);
    setNewTitle('');
  }, [connection.state, connection.connectionGeneration]);

  // Projection push subscription for the attached session. Latest-only:
  // each pushed view atomically replaces the previous one. The host
  // watcher exits on generation drift, so pushes are always current;
  // the renderer additionally fences on connection and session.
  useEffect(() => {
    if (!currentToken?.sessionId || connection.state !== 'connected') return;
    const expectedConnection = connection.connectionGeneration;
    const expectedRoute = currentToken.routeGeneration;
    const expectedSession = currentToken.sessionId;
    let cancelled = false;
    let handle: { unsubscribe: () => void } | null = null;
    void bridge
      .subscribeProjection((view) => {
        if (cancelled) return;
        if (connectionRef.current.connectionGeneration !== expectedConnection) return;
        if (view.sessionId !== expectedSession) return;
        setProjection(view);
      })
      .then((subscription) => {
        if (cancelled) {
          subscription.unsubscribe();
          return;
        }
        if (
          connectionRef.current.connectionGeneration !== expectedConnection ||
          currentToken?.routeGeneration !== expectedRoute
        ) {
          subscription.unsubscribe();
          return;
        }
        handle = subscription;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      handle?.unsubscribe();
    };
    // Re-subscribe when the attached session or its generations change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connection.state, connection.connectionGeneration, currentToken?.sessionId, currentToken?.routeGeneration]);

  const selectProject = async (projectId: string) => {
    const request = ++routeRequest.current;
    const forConnection = connectionRef.current.connectionGeneration;
    setRouteBusy(true);
    setRouteError(null);
    try {
      const result = await bridge.projectDetail(projectId);
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setDetail(result);
      setSessions([]);
      setProjection(null);
    } catch (error) {
      if (request !== routeRequest.current) return;
      setRouteError(String(error));
    } finally {
      if (request === routeRequest.current) setRouteBusy(false);
    }
  };

  const selectWorkspace = async (workspaceId: string) => {
    if (!currentToken) return;
    const request = ++routeRequest.current;
    const forConnection = connectionRef.current.connectionGeneration;
    setRouteBusy(true);
    setRouteError(null);
    try {
      const token = await bridge.workspaceSelect(workspaceId, currentToken.routeGeneration);
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setDetail((previous) => (previous ? { ...previous, routeToken: token } : previous));
      const listed = await bridge.sessionList(token.routeGeneration);
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setSessions(listed.sessions);
      setDetail((previous) => (previous ? { ...previous, routeToken: listed.routeToken } : previous));
      setProjection(null);
    } catch (error) {
      if (request !== routeRequest.current) return;
      setRouteError(String(error));
    } finally {
      if (request === routeRequest.current) setRouteBusy(false);
    }
  };

  const openSession = async (sessionId: string) => {
    if (!currentToken) return;
    const request = ++routeRequest.current;
    const forConnection = connectionRef.current.connectionGeneration;
    setRouteBusy(true);
    setRouteError(null);
    try {
      const opened = await bridge.sessionOpen(sessionId, currentToken.routeGeneration);
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setDetail((previous) => (previous ? { ...previous, routeToken: opened.routeToken } : previous));
      const view = await bridge.projectionStart(sessionId, opened.routeToken.routeGeneration);
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setProjection(view);
      await refreshControl(opened.routeToken.routeGeneration);
    } catch (error) {
      if (request !== routeRequest.current) return;
      setRouteError(String(error));
    } finally {
      if (request === routeRequest.current) setRouteBusy(false);
    }
  };

  const createSession = async () => {
    if (!currentToken) return;
    const request = ++routeRequest.current;
    const forConnection = connectionRef.current.connectionGeneration;
    setRouteBusy(true);
    setRouteError(null);
    try {
      const created = await bridge.sessionCreate(newTitle || null, currentToken.routeGeneration);
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setDetail((previous) => (previous ? { ...previous, routeToken: created.routeToken } : previous));
      setSessions((previous) => [created.session, ...previous].slice(0, 50));
      const view = await bridge.projectionStart(created.session.sessionId, created.routeToken.routeGeneration);
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setProjection(view);
      setNewTitle('');
      await refreshControl(created.routeToken.routeGeneration);
    } catch (error) {
      if (request !== routeRequest.current) return;
      setRouteError(String(error));
    } finally {
      if (request === routeRequest.current) setRouteBusy(false);
    }
  };

  const submitPrompt = async () => {
    if (!currentToken || promptBusy) return;
    const text = promptDraft;
    if (!text.trim()) {
      setRouteError('Prompt must not be empty');
      return;
    }
    const request = ++routeRequest.current;
    const forConnection = connectionRef.current.connectionGeneration;
    setPromptBusy(true);
    setRouteError(null);
    try {
      const accepted = await bridge.promptSubmit(text, planMode, currentToken.routeGeneration);
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setDetail((previous) => (previous ? { ...previous, routeToken: accepted.routeToken } : previous));
      setPromptDraft('');
      // Converge immediately; live projection events keep following.
      const sessionId = accepted.routeToken.sessionId;
      if (sessionId && (!projection || projection.sessionId !== sessionId)) {
        const view = await bridge.projectionStart(sessionId, accepted.routeToken.routeGeneration);
        if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
        setProjection(view);
      } else {
        const view = await bridge.projectionCurrent();
        if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
        setProjection(view);
      }
      await refreshControl(accepted.routeToken.routeGeneration);
    } catch (error) {
      if (request !== routeRequest.current) return;
      setRouteError(String(error));
    } finally {
      if (request === routeRequest.current) setPromptBusy(false);
    }
  };

  const refreshControl = async (routeGeneration: number) => {
    try {
      setController(await bridge.controlRefresh(routeGeneration));
    } catch (error) {
      setRouteError(String(error));
    }
  };

  const respondPermission = async (permissionId: string, choice: string) => {
    if (!currentToken || responding.includes(permissionId)) return;
    const request = ++routeRequest.current;
    const forConnection = connectionRef.current.connectionGeneration;
    setResponding((previous) => [...previous, permissionId]);
    setRouteError(null);
    try {
      const summary = await bridge.permissionRespond(permissionId, choice, currentToken.routeGeneration);
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setController(summary);
      const view = await bridge.projectionCurrent();
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setProjection(view);
    } catch (error) {
      if (request !== routeRequest.current) return;
      setRouteError(String(error));
    } finally {
      if (request === routeRequest.current) setResponding((previous) => previous.filter((id) => id !== permissionId));
    }
  };

  const respondQuestion = async (questionId: string) => {
    if (!currentToken || responding.includes(questionId)) return;
    const raw = questionDrafts[questionId] ?? '';
    let answers: unknown;
    try {
      answers = raw.trim() ? JSON.parse(raw) : [];
    } catch {
      answers = raw;
    }
    const request = ++routeRequest.current;
    const forConnection = connectionRef.current.connectionGeneration;
    setResponding((previous) => [...previous, questionId]);
    setRouteError(null);
    try {
      const summary = await bridge.questionRespond(questionId, answers, currentToken.routeGeneration);
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setController(summary);
      const view = await bridge.projectionCurrent();
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setProjection(view);
    } catch (error) {
      if (request !== routeRequest.current) return;
      setRouteError(String(error));
    } finally {
      if (request === routeRequest.current) setResponding((previous) => previous.filter((id) => id !== questionId));
    }
  };

  const readArtifact = async (handle: string) => {
    if (!currentToken || excerptBusy) return;
    const request = ++routeRequest.current;
    const forConnection = connectionRef.current.connectionGeneration;
    setExcerptBusy(handle);
    setRouteError(null);
    try {
      const view = await bridge.artifactRead(handle, 0, null, currentToken.routeGeneration);
      if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
      setExcerpt(view);
    } catch (error) {
      if (request !== routeRequest.current) return;
      setRouteError(String(error));
    } finally {
      if (request === routeRequest.current) setExcerptBusy(null);
    }
  };

  const stopProjection = async () => {
    try {
      const token = await bridge.projectionStop();
      setDetail((previous) => (previous ? { ...previous, routeToken: token } : previous));
      setProjection(null);
    } catch (error) {
      setRouteError(String(error));
    }
  };

  const reconnect = async () => { const current = ++generation.current; setBusy(true); try { const snapshot = await bridge.connect(); if (current === generation.current) { connectionRef.current = snapshot; setConnection(snapshot); if (snapshot.state === 'connected') await refreshProjects(current, snapshot.connectionGeneration); } } catch (error) { if (current === generation.current) { const failed = { ...initial, state: 'disconnected' as const, error: String(error) }; connectionRef.current = failed; setConnection(failed); } } finally { if (current === generation.current) setBusy(false); } };
  return <main>
    <header><div><p className="eyebrow">LOCAL DESKTOP</p><h1>CodeGG</h1></div><span className={`status ${connection.state}`} data-testid="connection-status">{connection.state}</span></header>
    <section className="panel"><h2>Daemon</h2><dl><dt>Identity</dt><dd data-testid="daemon-identity">{connection.daemonId ?? '—'}</dd><dt>Generation</dt><dd data-testid="connection-generation">{connection.connectionGeneration}</dd><dt>Subscription</dt><dd data-testid="subscription-id">{subscriptionId ?? '—'}</dd><dt>Protocol</dt><dd>{connection.protocolVersion ?? '—'}</dd><dt>Uptime</dt><dd>{connection.uptimeSeconds === null ? '—' : `${connection.uptimeSeconds}s`}</dd><dt>Active sessions</dt><dd>{connection.activeSessions ?? '—'}</dd></dl>
      {connection.error && <p role="alert">{connection.error}</p>}<button data-testid="reconnect-button" disabled={busy} onClick={() => void reconnect()}>{busy ? 'Connecting…' : 'Reconnect'}</button>
    </section>
    <section className="panel"><h2>Projects <small>{displayedProjects.length} / 50</small></h2>{displayedProjects.length ? <ul data-testid="project-list">{displayedProjects.map((project) => <li key={project.projectId} data-testid="project-item" data-project-id={project.projectId}><button data-testid={`project-select-${project.projectId}`} onClick={() => void selectProject(project.projectId)}>{project.displayName}</button><small>{project.lifecycle}</small></li>)}</ul> : <p className="muted">No projects registered yet.</p>}</section>
    {detail && <section className="panel"><h2>Route</h2><dl><dt>Project</dt><dd data-testid="route-project">{detail.displayName}</dd><dt>Workspace</dt><dd data-testid="route-workspace">{detail.routeToken.workspaceId || '—'}</dd><dt>Session</dt><dd data-testid="route-session">{detail.routeToken.sessionId ?? '—'}</dd><dt>Route generation</dt><dd data-testid="route-generation">{detail.routeToken.routeGeneration}</dd></dl>
      <h3>Workspaces</h3><ul data-testid="workspace-list">{detail.workspaces.map((workspace) => <li key={workspace.workspaceId}><button data-testid={`workspace-select-${workspace.workspaceId}`} onClick={() => void selectWorkspace(workspace.workspaceId)}>{workspace.displayName}</button></li>)}</ul>
      <h3>Sessions</h3>{sessions.length ? <ul data-testid="session-list">{sessions.map((session) => <li key={session.sessionId} data-testid="session-item" data-session-id={session.sessionId}><button data-testid={`session-open-${session.sessionId}`} onClick={() => void openSession(session.sessionId)}>{session.title || session.sessionId}</button></li>)}</ul> : <p className="muted">No sessions listed yet.</p>}
      <div><input data-testid="session-title-input" value={newTitle} maxLength={120} placeholder="New session title" onChange={(event) => setNewTitle(event.target.value)} /><button data-testid="session-create-button" disabled={routeBusy || !detail.routeToken.workspaceId} onClick={() => void createSession()}>Create session</button></div>
      {detail.routeToken.workspaceId && <div><h3>Prompt</h3><textarea data-testid="prompt-input" value={promptDraft} placeholder="Ask the session" onChange={(event) => setPromptDraft(event.target.value)} /><label><input type="checkbox" data-testid="plan-mode-checkbox" checked={planMode} onChange={(event) => setPlanMode(event.target.checked)} /> Plan mode</label><button data-testid="prompt-submit-button" disabled={promptBusy || !promptDraft.trim()} onClick={() => void submitPrompt()}>Submit prompt</button></div>}
    </section>}
    {routeError && <p role="alert" data-testid="route-error">{routeError}</p>}
    {projection && <section className="panel"><h2>Session</h2><dl><dt>Projection</dt><dd data-testid="projection-state">{projection.state}</dd><dt>Turn</dt><dd data-testid="turn-status">{projection.turn ? `${projection.turn.turnId} / ${projection.turn.status}` : 'idle'}</dd>{projection.truncatedMessages > 0 && <><dt>Omitted</dt><dd data-testid="truncated-messages">{projection.truncatedMessages} older message(s)</dd></>}<dt>Cursor</dt><dd data-testid="projection-cursor">seq {projection.cursor.eventSeq}</dd></dl>
      {projection.state === 'resyncing' && <p data-testid="resync-banner" role="status">Resuming projection — live updates paused.</p>}
      <h3>Messages</h3>{projection.messages.length ? <ul data-testid="message-list">{projection.messages.map((message) => <li key={message.messageId} data-testid="message-item"><strong>{message.role}</strong><p>{message.text}{message.truncated && ' …'}</p></li>)}</ul> : <p className="muted">No visible messages yet.</p>}
      {!!projection.artifactHandles.length && <><h3>Artifacts</h3><ul data-testid="artifact-list">{projection.artifactHandles.map((artifact) => <li key={artifact.handle} data-testid="artifact-item">{artifact.handle} ({artifact.byteLength} bytes)<button data-testid={`artifact-read-${artifact.handle}`} disabled={excerptBusy === artifact.handle} onClick={() => void readArtifact(artifact.handle)}>Read excerpt</button></li>)}</ul></>}
      {!!projection.tools.length && <><h3>Tools</h3><ul data-testid="tool-list">{projection.tools.map((tool) => <li key={tool.toolId} data-testid="tool-item">{tool.toolName} — {tool.status} — {tool.summary}{tool.hasArtifact && ' (artifact)'}</li>)}</ul></>}
      <dl><dt>Controller</dt><dd data-testid="controller-state">{controller ? `${controller.controllerPrincipal} @ rev ${controller.revision}` : 'no exclusive controller'}</dd></dl><button data-testid="control-refresh-button" onClick={() => currentToken && void refreshControl(currentToken.routeGeneration)}>Refresh control</button>
      {!!projection.pendingPermissions.length && <><h3>Permissions</h3><ul data-testid="permission-list">{projection.pendingPermissions.map((permission) => <li key={permission.permissionId} data-testid="permission-item">{permission.tool} — {permission.status}<button data-testid={`permission-allow-${permission.permissionId}`} disabled={responding.includes(permission.permissionId)} onClick={() => void respondPermission(permission.permissionId, 'allow')}>Allow</button><button data-testid={`permission-deny-${permission.permissionId}`} disabled={responding.includes(permission.permissionId)} onClick={() => void respondPermission(permission.permissionId, 'deny')}>Deny</button></li>)}</ul></>}
      {!!projection.pendingQuestions.length && <><h3>Questions</h3><ul data-testid="question-list">{projection.pendingQuestions.map((question) => <li key={question.questionId} data-testid="question-item">{question.header ?? 'Question'} — {question.prompt} ({question.status})<textarea data-testid={`question-answer-${question.questionId}`} value={questionDrafts[question.questionId] ?? ''} placeholder='JSON answers, e.g. ["a"]' onChange={(event) => setQuestionDrafts((previous) => ({ ...previous, [question.questionId]: event.target.value }))} /><button data-testid={`question-answer-button-${question.questionId}`} disabled={responding.includes(question.questionId)} onClick={() => void respondQuestion(question.questionId)}>Answer</button></li>)}</ul></>}
      {excerpt && <div><h3>Excerpt</h3><p data-testid="artifact-excerpt">{excerpt.content}</p><p data-testid="artifact-excerpt-meta">{excerpt.handle} bytes {excerpt.start}-{excerpt.end}{excerpt.truncated ? ' (truncated)' : ''}{excerpt.redacted ? ' (redacted)' : ''}</p></div>}
      <button data-testid="projection-stop-button" onClick={() => void stopProjection()}>Detach projection</button>
    </section>}
  </main>;
}
