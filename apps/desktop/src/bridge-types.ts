// Generated bridge contract. Keep in sync with src-tauri/src/bridge.rs.
export type ConnectionState = 'starting' | 'disconnected' | 'connecting' | 'connected' | 'incompatible';
export interface ConnectionSnapshot {
  state: ConnectionState;
  daemonId: string | null;
  protocolVersion: number | null;
  uptimeSeconds: number | null;
  activeSessions: number | null;
  error: string | null;
}
export interface ProjectSummary {
  projectId: string;
  displayName: string;
  lifecycle: string;
}
export interface DesktopEvent {
  version: 1;
  eventSeq: number;
  kind: 'project_catalog_changed';
}
