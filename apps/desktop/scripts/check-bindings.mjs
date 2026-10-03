import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// The bridge contract uses a deliberately small, reviewed DTO surface. This
// guard makes the Rust/TypeScript field mapping explicit without deriving TS
// from protocol-wide durable DTOs.
const rust = readFileSync(resolve('src-tauri/src/bridge.rs'), 'utf8');
const ts = readFileSync(resolve('src/bridge-types.ts'), 'utf8');
const expected = {
  ConnectionSnapshot: ['state', 'daemon_id', 'protocol_version', 'uptime_seconds', 'active_sessions', 'error', 'connection_generation'],
  ProjectSummary: ['project_id', 'display_name', 'lifecycle'],
  DesktopEvent: ['version', 'event_seq', 'kind'],
  SubscriptionInfo: ['subscription_id', 'connection_generation'],
  RouteTokenView: ['connection_generation', 'route_generation', 'project_id', 'workspace_id', 'session_id'],
  WorkspaceView: ['workspace_id', 'display_name'],
  ProjectDetailView: ['project_id', 'display_name', 'workspaces', 'session_count', 'route_token'],
  SessionSummaryView: ['session_id', 'title', 'project_id', 'workspace_id'],
  SessionListView: ['sessions', 'route_token'],
  SessionView: ['session', 'route_token'],
  MessageView: ['message_id', 'role', 'text', 'truncated'],
  TurnSummaryView: ['turn_id', 'status', 'updated_at', 'stop_reason', 'error', 'message_count', 'tool_count', 'pending_permissions', 'pending_questions', 'input_tokens', 'output_tokens'],
  ToolSummaryView: ['tool_id', 'tool_name', 'status', 'summary', 'has_artifact'],
  RunSummaryView: ['run_id', 'kind', 'status', 'summary'],
  JobSummaryView: ['job_id', 'kind', 'state', 'summary'],
  SubagentSummaryView: ['task_id', 'agent', 'description', 'status', 'result_summary'],
  PendingPermissionView: ['permission_id', 'tool', 'scope_summary', 'status'],
  PendingQuestionView: ['question_id', 'header', 'prompt', 'status'],
  ArtifactHandleView: ['handle', 'byte_length'],
  ControllerSummaryView: ['turn_id', 'controller_principal', 'revision'],
  CursorDiagnosticView: ['event_seq', 'driver_cursor_seq', 'subscription_known'],
  SessionPresentationView: ['session_id', 'project_id', 'workspace_id', 'state', 'turn', 'messages', 'truncated_messages', 'tools', 'runs', 'jobs', 'subagents', 'recent_turns', 'pending_permissions', 'pending_questions', 'controller', 'artifact_handles', 'cursor', 'resync_reason'],
};
for (const [name, fields] of Object.entries(expected)) {
  const rustBlock = rust.match(new RegExp(`pub struct ${name} \\{([\\s\\S]*?)\\n\\}`))?.[1];
  const tsBlock = ts.match(new RegExp(`export interface ${name} \\{([\\s\\S]*?)\\n\\}`))?.[1];
  if (!rustBlock || !tsBlock) throw new Error(`missing bridge type ${name}`);
  for (const field of fields) {
    const camel = field.replace(/_([a-z])/g, (_, char) => char.toUpperCase());
    if (!rustBlock.includes(` ${field}:`) || !tsBlock.includes(`${camel}:`)) throw new Error(`bridge field drift: ${name}.${field}`);
  }
}
console.log('desktop bridge DTO fields match');
