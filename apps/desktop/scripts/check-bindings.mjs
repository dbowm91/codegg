import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// The bridge contract uses a deliberately small, reviewed DTO surface. This
// guard makes the Rust/TypeScript field mapping explicit without deriving TS
// from protocol-wide durable DTOs.
const rust = readFileSync(resolve('src-tauri/src/bridge.rs'), 'utf8');
const ts = readFileSync(resolve('src/bridge-types.ts'), 'utf8');
const expected = {
  ConnectionSnapshot: ['state', 'daemon_id', 'protocol_version', 'uptime_seconds', 'active_sessions', 'error'],
  ProjectSummary: ['project_id', 'display_name', 'lifecycle'],
  DesktopEvent: ['version', 'event_seq', 'kind'],
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
