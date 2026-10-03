/**
 * fixture-client.ts — Node driver for the deterministic Rust E2E fixture.
 *
 * The fixture (`desktop_e2e_fixture serve`, owned per phase by
 * `e2e/run-e2e.sh`) holds a TUI-kind observer client against an isolated
 * daemon and serves newline-delimited JSON commands over a Unix socket. A
 * socket (rather than stdio) is required because the `@wdio/tauri-service`
 * spawns the desktop app once per WebdriverIO invocation in the launcher
 * process: spec code in workers cannot own the fixture's stdio, but any
 * process can dial the socket.
 *
 * The socket path comes from `CODEGG_E2E_SOCKET` (set by `run-e2e.sh`).
 * Isolation is fail-closed on the fixture side: `CODEGG_E2E_HOME` must be
 * temp-scoped and `CODEGG_DAEMON_EXECUTABLE` must point at a daemon binary
 * built from the tested revision. This client never touches the operator's
 * real daemon home.
 */
import * as fs from 'node:fs';
import * as net from 'node:net';
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
  project_id?: string;
  project_name?: string;
}

const DEFAULT_COMMAND_TIMEOUT_MS = 30_000;
const START_COMMAND_TIMEOUT_MS = 120_000;
const CONNECT_TIMEOUT_MS = 15_000;

export function fixtureSocketPath(): string {
  const fromEnv = process.env.CODEGG_E2E_SOCKET;
  if (fromEnv) return fromEnv;
  const home = process.env.CODEGG_E2E_HOME;
  if (!home) {
    throw new Error('CODEGG_E2E_SOCKET or CODEGG_E2E_HOME is required for the E2E fixture');
  }
  return path.join(home, 'fixture.sock');
}

export class FixtureClient {
  private socket: net.Socket | null = null;
  private reader: readline.Interface | null = null;
  private nextId = 1;
  private pending = new Map<
    number,
    { resolve: (value: Record<string, unknown>) => void; reject: (error: Error) => void }
  >();

  private constructor() {}

  /** Dial the phase fixture server (started by `e2e/run-e2e.sh`). */
  static async connect(): Promise<FixtureClient> {
    const socketPath = fixtureSocketPath();
    if (!fs.existsSync(socketPath)) {
      throw new Error(
        `E2E fixture socket is missing: ${socketPath} (start it with e2e/run-e2e.sh)`,
      );
    }
    const client = new FixtureClient();
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`fixture connect timed out after ${CONNECT_TIMEOUT_MS}ms`));
      }, CONNECT_TIMEOUT_MS);
      const socket = net.connect(socketPath, () => {
        clearTimeout(timer);
        resolve();
      });
      socket.once('error', (error) => {
        clearTimeout(timer);
        reject(error);
      });
      client.socket = socket;
    });
    client.socket!.on('error', (error) => client.failAll(error));
    client.socket!.on('close', () => {
      if (client.pending.size > 0) {
        client.failAll(new Error('fixture socket closed'));
      }
    });
    client.reader = readline.createInterface({ input: client.socket! });
    client.reader.on('line', (line) => client.onLine(line));
    return client;
  }

  disconnect(): void {
    this.reader?.close();
    this.reader = null;
    this.socket?.destroy();
    this.socket = null;
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

  /** Low-level command for phase orchestration (`run-e2e.sh` pre/post steps). */
  async command(cmd: string, args: Record<string, unknown> = {}): Promise<Record<string, unknown>> {
    return this.send(cmd, args, START_COMMAND_TIMEOUT_MS);
  }

  private send(
    cmd: string,
    args: Record<string, unknown> = {},
    timeoutMs = DEFAULT_COMMAND_TIMEOUT_MS,
  ): Promise<Record<string, unknown>> {
    const socket = this.socket;
    if (!socket || socket.destroyed) {
      return Promise.reject(new Error('fixture is not connected'));
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
      socket.write(`${JSON.stringify({ id, cmd, ...args })}\n`);
    }).then((response: Record<string, unknown>) => {
      if (response['ok'] !== true) {
        throw new Error(`fixture ${cmd} failed: ${String(response['error'] ?? 'unknown error')}`);
      }
      return response;
    });
  }

  /**
   * Ensure the isolated daemon is up with workspace + probe project.
   * Idempotent: reuses the held observer when it is still alive, so the
   * phase script's pre-start and the specs share one daemon/observer.
   */
  async start(): Promise<StartResult> {
    const response = await this.send('start', {}, START_COMMAND_TIMEOUT_MS);
    const result: StartResult = {
      daemon_id: String(response['daemon_id']),
      endpoint: String(response['endpoint']),
      started_pid:
        typeof response['started_pid'] === 'number'
          ? (response['started_pid'] as number)
          : null,
      workspace_id: String(response['workspace_id']),
    };
    if (typeof response['project_id'] === 'string') {
      result.project_id = response['project_id'] as string;
      result.project_name = String(response['project_name'] ?? '');
    }
    return result;
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

  /** Attach a fresh observer after the daemon was restarted (e.g. by desktop autostart). */
  async reattach(): Promise<string> {
    const response = await this.send('reattach', {}, START_COMMAND_TIMEOUT_MS);
    return String(response['daemon_id']);
  }

  async stopDaemon(): Promise<void> {
    await this.send('stop_daemon', {}, START_COMMAND_TIMEOUT_MS);
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
