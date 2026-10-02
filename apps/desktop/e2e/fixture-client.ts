/**
 * fixture-client.ts — Node driver for the deterministic Rust E2E fixture.
 *
 * Spawns `desktop_e2e_fixture` (see `src-tauri/src/bin/desktop_e2e_fixture.rs`)
 * and speaks its newline-delimited JSON protocol over stdio. The fixture owns
 * a TUI-kind observer client against an isolated daemon for the whole
 * trajectory, so specs can assert real `connected_clients` counts, register
 * deterministic projects, and emit project-catalog invalidations on demand.
 *
 * Isolation is fail-closed on the fixture side: `CODEGG_E2E_HOME` must resolve
 * under the OS temp directory and `CODEGG_DAEMON_EXECUTABLE` must point at a
 * daemon binary built from the tested revision. This client never touches the
 * operator's real daemon home.
 */
import { spawn, type ChildProcess } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import * as readline from 'node:readline';

export interface ClientSnapshot {
  client_id: string;
  client_name: string;
}

export interface DaemonSnapshot {
  daemon_id: string;
  total_clients: number;
  desktop_clients: number;
  clients: ClientSnapshot[];
}

export interface StartResult {
  daemon_id: string;
  endpoint: string;
  started_pid: number | null;
  workspace_id: string;
  project_id: string;
  project_name: string;
}

const DEFAULT_COMMAND_TIMEOUT_MS = 30_000;
const START_COMMAND_TIMEOUT_MS = 120_000;

function fixtureBinary(): string {
  const fromEnv = process.env.CODEGG_E2E_FIXTURE_BINARY;
  if (fromEnv) return fromEnv;
  // <repo>/apps/desktop/e2e -> <repo>/apps/desktop/src-tauri/target/debug/…
  return path.resolve(
    import.meta.dirname,
    '..',
    'src-tauri',
    'target',
    'debug',
    process.platform === 'win32' ? 'desktop_e2e_fixture.exe' : 'desktop_e2e_fixture',
  );
}

export class FixtureClient {
  private proc: ChildProcess | null = null;
  private reader: readline.Interface | null = null;
  private nextId = 1;
  private pending = new Map<
    number,
    { resolve: (value: Record<string, unknown>) => void; reject: (error: Error) => void }
  >();
  private stderrTail: string[] = [];

  readonly home: string;

  private constructor(home: string) {
    this.home = home;
  }

  static async launch(): Promise<FixtureClient> {
    const daemonExecutable = process.env.CODEGG_DAEMON_EXECUTABLE;
    if (!daemonExecutable) {
      throw new Error('CODEGG_DAEMON_EXECUTABLE is required for the E2E fixture');
    }
    const home = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'codegg-e2e-'));
    const client = new FixtureClient(home);
    const binary = fixtureBinary();
    if (!fs.existsSync(binary)) {
      throw new Error(
        `E2E fixture binary is missing: ${binary} (build it with: cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml --bin desktop_e2e_fixture)`,
      );
    }
    const proc = spawn(binary, [], {
      env: { ...process.env, CODEGG_E2E_HOME: home, CODEGG_DAEMON_EXECUTABLE: daemonExecutable },
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    client.proc = proc;
    proc.on('error', (error) => client.failAll(error));
    proc.stderr?.on('data', (chunk: Buffer) => {
      const text = chunk.toString();
      client.stderrTail.push(text);
      if (client.stderrTail.length > 20) client.stderrTail.shift();
      process.stderr.write(`[e2e-fixture] ${text}`);
    });
    proc.on('exit', (code) => {
      if (client.pending.size > 0) {
        client.failAll(new Error(`fixture exited with code ${code}`));
      }
    });
    client.reader = readline.createInterface({ input: proc.stdout! });
    client.reader.on('line', (line) => client.onLine(line));
    return client;
  }

  private failAll(error: Error): void {
    for (const { reject } of this.pending.values()) reject(error);
    this.pending.clear();
  }

  private onLine(line: string): void {
    let response: Record<string, unknown>;
    try {
      response = JSON.parse(line) as Record<string, unknown>;
    } catch {
      return;
    }
    const id = response['id'];
    if (typeof id !== 'number') return;
    const waiter = this.pending.get(id);
    if (!waiter) return;
    this.pending.delete(id);
    waiter.resolve(response);
  }

  private send(
    cmd: string,
    args: Record<string, unknown> = {},
    timeoutMs = DEFAULT_COMMAND_TIMEOUT_MS,
  ): Promise<Record<string, unknown>> {
    const proc = this.proc;
    if (!proc?.stdin?.writable) {
      return Promise.reject(new Error('fixture is not running'));
    }
    const id = this.nextId++;
    return new Promise<Record<string, unknown>>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`fixture command ${cmd} timed out after ${timeoutMs}ms`));
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });
      proc.stdin!.write(`${JSON.stringify({ id, cmd, ...args })}\n`);
    }).then((response: Record<string, unknown>) => {
      if (response['ok'] !== true) {
        throw new Error(`fixture ${cmd} failed: ${String(response['error'] ?? 'unknown error')}`);
      }
      return response;
    });
  }

  /** Start the isolated daemon, attach the observer, register workspace + probe project. */
  async start(): Promise<StartResult> {
    const response = await this.send('start', {}, START_COMMAND_TIMEOUT_MS);
    return {
      daemon_id: String(response['daemon_id']),
      endpoint: String(response['endpoint']),
      started_pid:
        typeof response['started_pid'] === 'number'
          ? (response['started_pid'] as number)
          : null,
      workspace_id: String(response['workspace_id']),
      project_id: String(response['project_id']),
      project_name: String(response['project_name']),
    };
  }

  async snapshot(): Promise<DaemonSnapshot> {
    const response = await this.send('snapshot');
    return response['snapshot'] as unknown as DaemonSnapshot;
  }

  async registerProject(displayName: string): Promise<string> {
    const response = await this.send('register_project', { display_name: displayName });
    return String(response['project_id']);
  }

  async archiveProject(projectId: string): Promise<void> {
    await this.send('archive_project', { project_id: projectId });
  }

  async restoreProject(projectId: string): Promise<void> {
    await this.send('restore_project', { project_id: projectId });
  }

  async stopDaemon(): Promise<void> {
    await this.send('stop_daemon', {}, START_COMMAND_TIMEOUT_MS);
  }

  /** Attach a fresh observer after the daemon was restarted (e.g. by desktop autostart). */
  async reattach(): Promise<string> {
    const response = await this.send('reattach', {}, START_COMMAND_TIMEOUT_MS);
    return String(response['daemon_id']);
  }

  /** Remove the isolated home and stop the test daemon. Best effort after this, the handle is dead. */
  async shutdown(): Promise<void> {
    try {
      await this.send('shutdown');
    } catch {
      // The fixture may already be gone; home removal below still applies.
    } finally {
      this.reader?.close();
      this.proc?.kill();
      this.proc = null;
      await fs.promises.rm(this.home, { recursive: true, force: true });
    }
  }

  /** Isolated daemon home the desktop app under test must use. */
  daemonHome(): string {
    return path.join(this.home, 'daemon-home');
  }

  async waitForSnapshot(
    predicate: (snapshot: DaemonSnapshot) => boolean,
    options: { timeoutMs?: number; intervalMs?: number; describe: string } = {
      describe: 'snapshot condition',
    },
  ): Promise<DaemonSnapshot> {
    const timeoutMs = options.timeoutMs ?? 60_000;
    const intervalMs = options.intervalMs ?? 500;
    const deadline = Date.now() + timeoutMs;
    let last: DaemonSnapshot | null = null;
    for (;;) {
      last = await this.snapshot();
      if (predicate(last)) return last;
      if (Date.now() >= deadline) {
        throw new Error(
          `timed out waiting for ${options.describe}; last snapshot: ${JSON.stringify(last)}`,
        );
      }
      await new Promise((resolve) => setTimeout(resolve, intervalMs));
    }
  }
}
