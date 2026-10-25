import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { CSSProperties } from 'react';
import type { ArtifactExcerptView, ControllerSummaryView } from './bridge-types';
import { bridge } from './bridge';
import { GitPanel } from './panels/GitPanel';
import { SkillsPanel } from './panels/SkillsPanel';
import {
  dock,
  initialPanelHostState,
  resize,
  resolveColumnWidth,
  setPanelScroll,
  toggleSlot,
} from './panels/host';
import type { PanelHostState, PanelSize } from './panels/host';
import type { PanelId, PanelPlacement } from './panels/registry';
import { ChatRegion, ComposerRegion, PanelControls, SidePanelFrame, StatusRow } from './regions';
import type {
  ConnectionSnapshot,
  GitBranchListView,
  GitDiffView,
  GitReadKind,
  GitStatusView,
  ProjectDetailView,
  ProjectSummary,
  RouteTokenView,
  SessionPresentationView,
  SessionSummaryView,
  SkillCatalogueView,
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
  const todoRequest = useRef(0);
  const [todoBusy, setTodoBusy] = useState(false);
  const [todoError, setTodoError] = useState<string | null>(null);
  // Phase 3 project facts: the route-selected project's git facts and
  // skill catalogue. Both are project-scoped reads, not projection
  // data, so the renderer asks the host for them and keeps what the
  // host last gave it.
  const gitRequest = useRef(0);
  const [gitFacts, setGitFacts] = useState<{
    status: GitStatusView | null;
    diff: GitDiffView | null;
    branches: GitBranchListView | null;
  }>({ status: null, diff: null, branches: null });
  const [gitBusy, setGitBusy] = useState(false);
  const [gitError, setGitError] = useState<string | null>(null);
  const skillsRequest = useRef(0);
  const [skills, setSkills] = useState<SkillCatalogueView | null>(null);
  const [skillsBusy, setSkillsBusy] = useState(false);
  const [skillsError, setSkillsError] = useState<string | null>(null);

  // The side-panel host: one slot per edge, and the panel state that
  // travels with the panel id. The placement is renderer-local — there is
  // no durable panel-layout store, because a persisted layout would be a
  // frontend-owned durable store this milestone has no storage or
  // configuration change for.
  const [host, setHost] = useState<PanelHostState>(initialPanelHostState);
  // The frame the panels live in. Its size is what the resize seam clamps
  // against, and it is measured rather than assumed.
  const frameRef = useRef<HTMLDivElement>(null);
  const [frameSize, setFrameSize] = useState({ width: 0, height: 0 });

  const currentToken: RouteTokenView | null = detail?.routeToken ?? null;
  // The route's own state, readable from the connection effect without
  // making that effect depend on `detail` (which would re-run it on
  // every route change).
  const detailRef = useRef<ProjectDetailView | null>(null);
  detailRef.current = detail;

  // A projection push carries no todo list — todos live behind their
  // own daemon read — so the last list the host gave us is kept rather
  // than blanked by a push. The list belongs to one session, so it is
  // dropped when the session changes.
  const applyProjection = useCallback((view: SessionPresentationView) => {
    setProjection((previous) =>
      view.todos
        ? view
        : {
            ...view,
            todos:
              previous && previous.sessionId === view.sessionId ? previous.todos : null,
          },
    );
  }, []);

  // Task state: the host reads the route-selected session's todo list
  // from the daemon and carries it into the presented view. The
  // renderer asks for it when the attached session changes and when the
  // turn's state changes (a todo list is written during a turn and
  // settles when it finishes). The host fences each read on the route
  // generation, so a stale read is discarded by the host rather than
  // merged here.
  const refreshTodos = useCallback(
    async (routeGeneration: number, sessionId: string) => {
      const request = ++todoRequest.current;
      setTodoBusy(true);
      setTodoError(null);
      try {
        const view = await bridge.todoRefresh(routeGeneration);
        if (request !== todoRequest.current) return;
        if (view.sessionId !== sessionId) return;
        // Only the list is taken from the reply: the rest of it is the
        // host's snapshot at read time, and the renderer's own snapshot
        // may already be newer.
        if (view.todos) {
          setProjection((previous) => (previous ? { ...previous, todos: view.todos } : view));
        }
      } catch (error) {
        if (request !== todoRequest.current) return;
        // A failed read is not an empty list: the task panel keeps the
        // last list it was given and says the read failed.
        setTodoError(String(error));
      } finally {
        if (request === todoRequest.current) setTodoBusy(false);
      }
    },
    [],
  );

  // Phase 3 project facts. The host reads the route-selected project's
  // git facts and skill catalogue from the daemon; the renderer only
  // names the fact set it wants and renders what comes back. The host
  // fences each read on the route generation, and the renderer fences
  // on its own request counter, so a read that answers for a route
  // that has moved on is never merged in.
  const refreshGit = useCallback(async (routeGeneration: number) => {
    const request = ++gitRequest.current;
    setGitBusy(true);
    setGitError(null);
    const read = async (operation: GitReadKind) => {
      try {
        const view = await bridge.gitRead(operation, routeGeneration);
        return { ok: true as const, operation, view };
      } catch (error) {
        return { ok: false as const, operation, error: String(error) };
      }
    };
    const results = await Promise.all([
      read('status'),
      read('diff'),
      read('branches'),
    ]);
    if (request !== gitRequest.current) return;
    const failures: string[] = [];
    const facts = { status: null, diff: null, branches: null } as {
      status: GitStatusView | null;
      diff: GitDiffView | null;
      branches: GitBranchListView | null;
    };
    for (const result of results) {
      if (!result.ok) {
        // The three reads can fail the same way; report each distinct
        // failure once.
        if (!failures.includes(result.error)) failures.push(result.error);
        continue;
      }
      const view = result.view;
      if (view.kind === 'status') facts.status = view.status;
      if (view.kind === 'diff') facts.diff = view.diff;
      if (view.kind === 'branches') facts.branches = view.branches;
    }
    // A failed read is not an empty answer: the panel keeps the facts it
    // already has and says which read failed.
    setGitFacts((previous) => ({
      status: facts.status ?? previous.status,
      diff: facts.diff ?? previous.diff,
      branches: facts.branches ?? previous.branches,
    }));
    if (failures.length) setGitError(failures.join('; '));
    setGitBusy(false);
  }, []);

  const refreshSkills = useCallback(async (routeGeneration: number) => {
    const request = ++skillsRequest.current;
    setSkillsBusy(true);
    setSkillsError(null);
    try {
      const catalogue = await bridge.skillCatalogue(routeGeneration);
      if (request !== skillsRequest.current) return;
      setSkills(catalogue);
    } catch (error) {
      if (request !== skillsRequest.current) return;
      // A failed read is not an empty catalogue: the panel keeps the one
      // it already has and says the read failed.
      setSkillsError(String(error));
    } finally {
      if (request === skillsRequest.current) setSkillsBusy(false);
    }
  }, []);

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
    // Only an *established* route is invalidated. The first connect
    // establishes the route rather than replacing one, and bumping the
    // request counter there dropped a selection the user made after the
    // connection landed — the selection's own connection-generation
    // check is what fences work across a real reconnect.
    if (detailRef.current) routeRequest.current += 1;
    setDetail(null);
    setSessions([]);
    setProjection(null);
    setRouteError(null);
    setNewTitle('');
  }, [connection.state, connection.connectionGeneration]);

  // The project facts belong to one project: a different project starts
  // from "not read yet" rather than from the previous project's facts.
  // The read effect below resets them when the project changes.
  const factsProject = useRef<string | undefined>(undefined);

  // Read the route-selected project's git facts and skill catalogue.
  // They are project-scoped reads, not projection data, so the host
  // reads them for the route's project and fences each read on the
  // route generation. A route change re-reads them; a read that answers
  // for a superseded route is discarded by the host.
  useEffect(() => {
    const projectId = detail?.projectId;
    const routeGeneration = detail?.routeToken.routeGeneration;
    if (factsProject.current !== projectId) {
      factsProject.current = projectId;
      // The in-flight reads are for the previous project: drop them and
      // stop claiming to be busy.
      gitRequest.current += 1;
      skillsRequest.current += 1;
      setGitFacts({ status: null, diff: null, branches: null });
      setGitError(null);
      setGitBusy(false);
      setSkills(null);
      setSkillsError(null);
      setSkillsBusy(false);
    }
    if (!detail || routeGeneration === undefined || connection.state !== 'connected') return;
    void refreshGit(routeGeneration);
    void refreshSkills(routeGeneration);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connection.state, connection.connectionGeneration, detail?.projectId, detail?.routeToken.routeGeneration]);

  // Projection push subscription for the attached session. Latest-only:
  // each pushed view atomically replaces the previous one. The host
  // watcher exits on generation drift, so pushes are always current;
  // the renderer additionally fences on connection and session.
  //
  // Ordering: subscribe only once a projection view exists for the
  // session. `projectionStart` installs the host owner before it
  // resolves its first view, so gating on `projection` removes the
  // create-vs-subscribe race where the effect won and the host
  // rejected with "no session projection attached". Failures retry
  // bounded with backoff and then surface loudly; a silent swallow
  // here leaves the renderer permanently deaf (M004 live-turn).
  useEffect(() => {
    if (!currentToken?.sessionId || connection.state !== 'connected') return;
    if (!projection || projection.sessionId !== currentToken.sessionId) return;
    const expectedConnection = connection.connectionGeneration;
    const expectedRoute = currentToken.routeGeneration;
    const expectedSession = currentToken.sessionId;
    let cancelled = false;
    let handle: { unsubscribe: () => void } | null = null;
    const current = () =>
      !cancelled &&
      connectionRef.current.connectionGeneration === expectedConnection &&
      currentToken?.routeGeneration === expectedRoute &&
      currentToken?.sessionId === expectedSession;
    const attempt = async (left: number): Promise<void> => {
      if (!current()) return;
      try {
        const subscription = await bridge.subscribeProjection((view) => {
          if (cancelled) return;
          if (connectionRef.current.connectionGeneration !== expectedConnection) return;
          if (view.sessionId !== expectedSession) return;
          applyProjection(view);
        });
        if (!current()) {
          subscription.unsubscribe();
          return;
        }
        handle = subscription;
      } catch (error) {
        if (!current()) return;
        if (left <= 1) {
          setRouteError(`Projection subscription failed: ${String(error)}`);
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, 400));
        await attempt(left - 1);
      }
    };
    void attempt(5);
    return () => {
      cancelled = true;
      handle?.unsubscribe();
    };
    // Re-subscribe when the attached session, its projection, or its
    // generations change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connection.state, connection.connectionGeneration, currentToken?.sessionId, currentToken?.routeGeneration, projection?.sessionId]);

  // Read the attached session's todo list. The list is not in the
  // projection, so it is read on its own: once when the session is
  // attached, and again whenever the turn's identity or state changes
  // (a todo list is written during a turn and settles when the turn
  // finishes). The host fences each read on the route generation.
  useEffect(() => {
    const sessionId = currentToken?.sessionId;
    if (!sessionId || connection.state !== 'connected') return;
    if (!projection || projection.sessionId !== sessionId) return;
    const routeGeneration = currentToken?.routeGeneration;
    if (routeGeneration === undefined) return;
    void refreshTodos(routeGeneration, sessionId);
    // Re-read when the attached session, its route generation, or the
    // turn's identity/state changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connection.state, connection.connectionGeneration, currentToken?.sessionId, currentToken?.routeGeneration, projection?.sessionId, projection?.turn?.turnId, projection?.turn?.status]);

  // The frame's own size. The panels are sized against it rather than
  // against the window, and a frame too narrow to host a column beside the
  // main region's minimum collapses that column instead of squeezing the
  // transcript out of existence (`src/tui/layout.rs:292-298`).
  useLayoutEffect(() => {
    const element = frameRef.current;
    if (!element) return;
    const measure = () => {
      const rect = element.getBoundingClientRect();
      setFrameSize((previous) =>
        previous.width === rect.width && previous.height === rect.height
          ? previous
          : { width: rect.width, height: rect.height },
      );
    };
    measure();
    window.addEventListener('resize', measure);
    return () => window.removeEventListener('resize', measure);
  }, []);

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
      applyProjection(view);
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
      applyProjection(view);
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
        applyProjection(view);
      } else {
        const view = await bridge.projectionCurrent();
        if (request !== routeRequest.current || forConnection !== connectionRef.current.connectionGeneration) return;
        applyProjection(view);
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
      applyProjection(view);
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
      applyProjection(view);
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

  // The host's operations. Every one of them is renderer-local: the
  // placement and the panel sizes live in the renderer, and nothing here
  // reaches the daemon.
  const dockPanel = useCallback((panelId: PanelId, placement: PanelPlacement) => {
    setHost((previous) => dock(previous, panelId, placement));
  }, []);
  const resizePanel = useCallback((placement: PanelPlacement, size: PanelSize) => {
    setHost((previous) => resize(previous, placement, size));
  }, []);
  const scrollPanel = useCallback((panelId: PanelId, scrollTop: number) => {
    setHost((previous) => setPanelScroll(previous, panelId, scrollTop));
  }, []);
  const togglePanelSlot = useCallback((placement: PanelPlacement) => {
    setHost((previous) => toggleSlot(previous, placement));
  }, []);

  // The frame's grid geometry. The columns are the panels' own widths and
  // the main region takes the rest; the bottom row is the panel's share of
  // the frame height.
  const occupied = (placement: PanelPlacement) => {
    const slot = host.slots[placement];
    return slot.visible && slot.panelId !== null;
  };
  const clampColumn = (desired: number, otherWidth: number) =>
    frameSize.width > 0 ? resolveColumnWidth(desired, frameSize.width, otherWidth) : desired;
  const leftAlone = occupied('left') ? clampColumn(host.layout.leftWidth, 0) : 0;
  const rightAlone = occupied('right') ? clampColumn(host.layout.rightWidth, 0) : 0;
  const leftWidth = occupied('left') ? clampColumn(host.layout.leftWidth, rightAlone) : 0;
  const rightWidth = occupied('right') ? clampColumn(host.layout.rightWidth, leftAlone) : 0;
  const bottomRatio = occupied('bottom') ? host.layout.bottomRatio : 0;
  const frameStyle = {
    '--panel-left-width': `${leftWidth}px`,
    '--panel-right-width': `${rightWidth}px`,
    '--panel-bottom-height': `${bottomRatio * 100}%`,
  } as CSSProperties;

  const reconnect = async () => { const current = ++generation.current; setBusy(true); try { const snapshot = await bridge.connect(); if (current === generation.current) { connectionRef.current = snapshot; setConnection(snapshot); if (snapshot.state === 'connected') await refreshProjects(current, snapshot.connectionGeneration); } } catch (error) { if (current === generation.current) { const failed = { ...initial, state: 'disconnected' as const, error: String(error) }; connectionRef.current = failed; setConnection(failed); } } finally { if (current === generation.current) setBusy(false); } };
  return (
    <main className="app-shell">
      <header className="app-header">
        <div>
          <p className="eyebrow">LOCAL DESKTOP</p>
          <h1>CodeGG</h1>
        </div>
        <span className={`status ${connection.state}`} data-testid="connection-status">
          {connection.state}
        </span>
      </header>

      <div className="app-catalog">
        <section className="panel">
          <h2>Daemon</h2>
          <dl>
            <dt>Identity</dt>
            <dd data-testid="daemon-identity">{connection.daemonId ?? '—'}</dd>
            <dt>Generation</dt>
            <dd data-testid="connection-generation">{connection.connectionGeneration}</dd>
            <dt>Subscription</dt>
            <dd data-testid="subscription-id">{subscriptionId ?? '—'}</dd>
            <dt>Protocol</dt>
            <dd>{connection.protocolVersion ?? '—'}</dd>
            <dt>Uptime</dt>
            <dd>{connection.uptimeSeconds === null ? '—' : `${connection.uptimeSeconds}s`}</dd>
            <dt>Active sessions</dt>
            <dd>{connection.activeSessions ?? '—'}</dd>
          </dl>
          {connection.error && <p role="alert">{connection.error}</p>}
          <button data-testid="reconnect-button" disabled={busy} onClick={() => void reconnect()}>
            {busy ? 'Connecting…' : 'Reconnect'}
          </button>
        </section>
        <section className="panel">
          <h2>
            Projects <small>{displayedProjects.length} / 50</small>
          </h2>
          {displayedProjects.length ? (
            <ul data-testid="project-list">
              {displayedProjects.map((project) => (
                <li
                  key={project.projectId}
                  data-testid="project-item"
                  data-project-id={project.projectId}
                >
                  <button
                    data-testid={`project-select-${project.projectId}`}
                    onClick={() => void selectProject(project.projectId)}
                  >
                    {project.displayName}
                  </button>
                  <small>{project.lifecycle}</small>
                </li>
              ))}
            </ul>
          ) : (
            <p className="muted">No projects registered yet.</p>
          )}
        </section>
        {detail && (
          <section className="panel">
            <h2>Route</h2>
            <dl>
              <dt>Project</dt>
              <dd data-testid="route-project">{detail.displayName}</dd>
              <dt>Workspace</dt>
              <dd data-testid="route-workspace">{detail.routeToken.workspaceId || '—'}</dd>
              <dt>Session</dt>
              <dd data-testid="route-session">{detail.routeToken.sessionId ?? '—'}</dd>
              <dt>Route generation</dt>
              <dd data-testid="route-generation">{detail.routeToken.routeGeneration}</dd>
            </dl>
            <h3>Workspaces</h3>
            <ul data-testid="workspace-list">
              {detail.workspaces.map((workspace) => (
                <li key={workspace.workspaceId}>
                  <button
                    data-testid={`workspace-select-${workspace.workspaceId}`}
                    onClick={() => void selectWorkspace(workspace.workspaceId)}
                  >
                    {workspace.displayName}
                  </button>
                </li>
              ))}
            </ul>
            <h3>Sessions</h3>
            {sessions.length ? (
              <ul data-testid="session-list">
                {sessions.map((session) => (
                  <li
                    key={session.sessionId}
                    data-testid="session-item"
                    data-session-id={session.sessionId}
                  >
                    <button
                      data-testid={`session-open-${session.sessionId}`}
                      onClick={() => void openSession(session.sessionId)}
                    >
                      {session.title || session.sessionId}
                    </button>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="muted">No sessions listed yet.</p>
            )}
            <div>
              <input
                data-testid="session-title-input"
                value={newTitle}
                maxLength={120}
                placeholder="New session title"
                onChange={(event) => setNewTitle(event.target.value)}
              />
              <button
                data-testid="session-create-button"
                disabled={routeBusy || !detail.routeToken.workspaceId}
                onClick={() => void createSession()}
              >
                Create session
              </button>
            </div>
          </section>
        )}
        {routeError && (
          <p role="alert" data-testid="route-error">
            {routeError}
          </p>
        )}
        <PanelControls host={host} onDock={dockPanel} onToggle={togglePanelSlot} />
        {detail && (
          <>
            <GitPanel
              status={gitFacts.status}
              diff={gitFacts.diff}
              branches={gitFacts.branches}
              busy={gitBusy}
              error={gitError}
              onRefresh={currentToken ? () => void refreshGit(currentToken.routeGeneration) : undefined}
            />
            <SkillsPanel
              catalogue={skills}
              busy={skillsBusy}
              error={skillsError}
              onRefresh={currentToken ? () => void refreshSkills(currentToken.routeGeneration) : undefined}
            />
          </>
        )}
      </div>

      <div className="app-frame" ref={frameRef} style={frameStyle}>
        <ChatRegion
          projection={projection}
          excerpt={excerpt}
          excerptBusy={excerptBusy}
          responding={responding}
          questionDrafts={questionDrafts}
          onReadArtifact={(handle) => void readArtifact(handle)}
          onRespondPermission={(permissionId, choice) => void respondPermission(permissionId, choice)}
          onRespondQuestion={(questionId) => void respondQuestion(questionId)}
          onQuestionDraftChange={(questionId, value) =>
            setQuestionDrafts((previous) => ({ ...previous, [questionId]: value }))
          }
          onStopProjection={() => void stopProjection()}
        />
        <SidePanelFrame
          host={host}
          panelProps={{
            projection,
            controller,
            todoBusy,
            todoError,
            onRefreshTodos:
              currentToken && projection
                ? () => void refreshTodos(currentToken.routeGeneration, projection.sessionId)
                : undefined,
          }}
          frameRef={frameRef}
          onResize={resizePanel}
          onPanelScroll={scrollPanel}
        />
        <ComposerRegion
          enabled={Boolean(detail?.routeToken.workspaceId)}
          draft={promptDraft}
          planMode={planMode}
          busy={promptBusy}
          onDraftChange={setPromptDraft}
          onPlanModeChange={setPlanMode}
          onSubmit={() => void submitPrompt()}
        />
        <StatusRow
          projection={projection}
          controller={controller}
          onRefreshControl={() => {
            if (currentToken) void refreshControl(currentToken.routeGeneration);
          }}
        />
      </div>
    </main>
  );
}
