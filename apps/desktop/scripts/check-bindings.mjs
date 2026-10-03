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
