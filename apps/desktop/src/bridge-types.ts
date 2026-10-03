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
// M004 prompt submission (WP D). Resolves Ok only once the daemon accepts
// the turn; failures surface as invoke errors and leave the draft intact.
export interface PromptSubmitView {
  intentId: string;
  routeToken: RouteTokenView;
}
// M004 projection presentation (WP C). Bounded, renderer-safe views
// derived from the canonical snapshot by the Rust host. No raw tool
// arguments/output, no filesystem paths, no cursor authority.
export interface MessageView {
  messageId: string;
  role: string;
  text: string;
  truncated: boolean;
}
export interface TurnSummaryView {
  turnId: string;
  status: string;
  updatedAt: number;
  stopReason: string | null;
  error: string | null;
  messageCount: number;
  toolCount: number;
  pendingPermissions: number;
  pendingQuestions: number;
  inputTokens: number | null;
  outputTokens: number | null;
}
export interface ToolSummaryView {
  toolId: string;
  toolName: string;
  status: string;
  summary: string;
  hasArtifact: boolean;
}
export interface RunSummaryView {
  runId: string;
  kind: string;
  status: string;
  summary: string;
}
export interface JobSummaryView {
  jobId: string;
  kind: string;
  state: string;
  summary: string;
}
export interface SubagentSummaryView {
  taskId: number;
  agent: string;
  description: string;
  status: string;
  resultSummary: string | null;
}
export interface PendingPermissionView {
  permissionId: string;
  tool: string;
  scopeSummary: string | null;
  status: string;
}
export interface PendingQuestionView {
  questionId: string;
  header: string | null;
  prompt: string;
  status: string;
}
export interface ArtifactHandleView {
  handle: string;
  byteLength: number;
}
export interface ControllerSummaryView {
  turnId: string;
  controllerPrincipal: string;
  revision: number;
}
export interface CursorDiagnosticView {
  eventSeq: number;
  driverCursorSeq: number | null;
  subscriptionKnown: boolean;
}
export interface SessionPresentationView {
  sessionId: string;
  projectId: string;
  workspaceId: string;
  state: string;
  turn: TurnSummaryView | null;
  messages: MessageView[];
  truncatedMessages: number;
  tools: ToolSummaryView[];
  runs: RunSummaryView[];
  jobs: JobSummaryView[];
  subagents: SubagentSummaryView[];
  recentTurns: TurnSummaryView[];
  pendingPermissions: PendingPermissionView[];
  pendingQuestions: PendingQuestionView[];
  controller: ControllerSummaryView | null;
  artifactHandles: ArtifactHandleView[];
  cursor: CursorDiagnosticView;
  resyncReason: string | null;
}
export interface SubscriptionHandle {
  subscriptionId: string;
  connectionGeneration: number;
  unsubscribe: () => void;
}
