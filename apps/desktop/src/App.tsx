import { useEffect, useRef, useState } from 'react';
import { bridge } from './bridge';
import type { ConnectionSnapshot, ProjectSummary } from './bridge-types';

const initial: ConnectionSnapshot = { state: 'starting', daemonId: null, protocolVersion: null, uptimeSeconds: null, activeSessions: null, error: null, connectionGeneration: 0 };

export function App() {
  const [connection, setConnection] = useState(initial);
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [busy, setBusy] = useState(false);
  const generation = useRef(0);
  const connectionRef = useRef(connection);
  connectionRef.current = connection;
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
    }).catch(() => undefined);
    return () => {
      cancelled = true;
      handle?.unsubscribe();
    };
  }, [connection.state, connection.connectionGeneration]);
  const reconnect = async () => { const current = ++generation.current; setBusy(true); try { const snapshot = await bridge.connect(); if (current === generation.current) { connectionRef.current = snapshot; setConnection(snapshot); if (snapshot.state === 'connected') await refreshProjects(current, snapshot.connectionGeneration); } } catch (error) { if (current === generation.current) { const failed = { ...initial, state: 'disconnected' as const, error: String(error) }; connectionRef.current = failed; setConnection(failed); } } finally { if (current === generation.current) setBusy(false); } };
  return <main>
    <header><div><p className="eyebrow">LOCAL DESKTOP</p><h1>CodeGG</h1></div><span className={`status ${connection.state}`} data-testid="connection-status">{connection.state}</span></header>
    <section className="panel"><h2>Daemon</h2><dl><dt>Identity</dt><dd data-testid="daemon-identity">{connection.daemonId ?? '—'}</dd><dt>Generation</dt><dd data-testid="connection-generation">{connection.connectionGeneration}</dd><dt>Protocol</dt><dd>{connection.protocolVersion ?? '—'}</dd><dt>Uptime</dt><dd>{connection.uptimeSeconds === null ? '—' : `${connection.uptimeSeconds}s`}</dd><dt>Active sessions</dt><dd>{connection.activeSessions ?? '—'}</dd></dl>
      {connection.error && <p role="alert">{connection.error}</p>}<button data-testid="reconnect-button" disabled={busy} onClick={() => void reconnect()}>{busy ? 'Connecting…' : 'Reconnect'}</button>
    </section>
    <section className="panel"><h2>Projects <small>{displayedProjects.length} / 50</small></h2>{displayedProjects.length ? <ul data-testid="project-list">{displayedProjects.map((project) => <li key={project.projectId} data-testid="project-item" data-project-id={project.projectId}><span>{project.displayName}</span><small>{project.lifecycle}</small></li>)}</ul> : <p className="muted">No projects registered yet.</p>}</section>
  </main>;
}
