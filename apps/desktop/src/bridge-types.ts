// Generated bridge contract. Keep in sync with src-tauri/src/bridge.rs.
export type ConnectionState = 'starting' | 'disconnected' | 'connecting' | 'connected' | 'incompatible';
export interface ConnectionSnapshot {
  state: ConnectionState;
  daemonId: string | null;
  protocolVersion: number | null;
  uptimeSeconds: number | null;
  activeSessions: number | null;
  error: string | null;
  connectionGeneration: number;
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
export interface SubscriptionInfo {
  subscriptionId: string;
  connectionGeneration: number;
}
// M004 route/session controller (WP B). Canonical workspace roots never
// appear here; the host resolves them from authorized ProjectGet state.
export interface RouteTokenView {
  connectionGeneration: number;
  routeGeneration: number;
  projectId: string;
  workspaceId: string;
  sessionId: string | null;
}
export interface WorkspaceView {
  workspaceId: string;
  displayName: string;
}
export interface ProjectDetailView {
  projectId: string;
  displayName: string;
  workspaces: WorkspaceView[];
  sessionCount: number;
  routeToken: RouteTokenView;
}
export interface SessionSummaryView {
  sessionId: string;
  title: string;
  projectId: string;
  workspaceId: string | null;
}
export interface SessionListView {
  sessions: SessionSummaryView[];
  routeToken: RouteTokenView;
}
export interface SessionView {
  session: SessionSummaryView;
  routeToken: RouteTokenView;
}
export interface SubscriptionHandle {
  subscriptionId: string;
  connectionGeneration: number;
  unsubscribe: () => void;
}
