import { useEffect, useRef, useState } from 'react';
import { bridge } from './bridge';
import type { ConnectionSnapshot, ProjectSummary } from './bridge-types';

const initial: ConnectionSnapshot = { state: 'starting', daemonId: null, protocolVersion: null, uptimeSeconds: null, activeSessions: null, error: null };

export function App() {
  const [connection, setConnection] = useState(initial);
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [busy, setBusy] = useState(false);
  const generation = useRef(0);
  const refreshProjects = async (forGeneration = generation.current) => {
    try {
      const result = await bridge.projects();
      if (forGeneration === generation.current) setProjects(result);
    } catch (error) {
      if (forGeneration === generation.current) setConnection((s) => ({ ...s, error: String(error) }));
    }
  };
  const displayedProjects = projects.slice(0, 50);
  useEffect(() => {
    let mounted = true;
    const current = ++generation.current;
    setConnection({ ...initial, state: 'connecting' });
    void bridge.connect().then((snapshot) => { if (mounted && current === generation.current) { setConnection(snapshot); if (snapshot.state === 'connected') void refreshProjects(current); } })
      .catch((error: unknown) => { if (mounted && current === generation.current) setConnection({ ...initial, state: 'disconnected', error: String(error) }); });
    return () => { mounted = false; };
  }, []);
  useEffect(() => {
    if (connection.state !== 'connected') return;
    return bridge.subscribe(() => { void refreshProjects(generation.current); });
  }, [connection.state]);
  const reconnect = async () => { const current = ++generation.current; setBusy(true); try { const snapshot = await bridge.connect(); if (current === generation.current) { setConnection(snapshot); if (snapshot.state === 'connected') await refreshProjects(current); } } catch (error) { if (current === generation.current) setConnection({ ...initial, state: 'disconnected', error: String(error) }); } finally { if (current === generation.current) setBusy(false); } };
  return <main>
    <header><div><p className="eyebrow">LOCAL DESKTOP</p><h1>CodeGG</h1></div><span className={`status ${connection.state}`}>{connection.state}</span></header>
    <section className="panel"><h2>Daemon</h2><dl><dt>Identity</dt><dd>{connection.daemonId ?? '—'}</dd><dt>Protocol</dt><dd>{connection.protocolVersion ?? '—'}</dd><dt>Uptime</dt><dd>{connection.uptimeSeconds === null ? '—' : `${connection.uptimeSeconds}s`}</dd><dt>Active sessions</dt><dd>{connection.activeSessions ?? '—'}</dd></dl>
      {connection.error && <p role="alert">{connection.error}</p>}<button disabled={busy} onClick={() => void reconnect()}>{busy ? 'Connecting…' : 'Reconnect'}</button>
    </section>
    <section className="panel"><h2>Projects <small>{displayedProjects.length} / 50</small></h2>{displayedProjects.length ? <ul>{displayedProjects.map((project) => <li key={project.projectId}><span>{project.displayName}</span><small>{project.lifecycle}</small></li>)}</ul> : <p className="muted">No projects registered yet.</p>}</section>
  </main>;
}
