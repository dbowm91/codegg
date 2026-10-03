/**
 * m003-lifecycle.e2e.ts — C003 built-app visible-window trajectory, part 1.
 *
 * Drives the real Tauri window (real WebView, real IPC `Channel`, real local
 * daemon connection) through the M003 lifecycle: existing-daemon reuse,
 * visible reconnect, project-catalog invalidation over the current
 * subscription, renderer reload, and native close — with the reconnect/reload
 * cycle repeated twice to catch accumulation.
 *
 * Phase discipline (see `e2e/run-e2e.sh`): the phase script starts the
 * socket fixture and the isolated daemon BEFORE WebdriverIO launches, so the
 * single app instance the embedded provider spawns inherits the isolated
 * home from the start. This file only dials the fixture, shares its daemon
 * via an idempotent `start`, and disconnects afterwards; teardown belongs to
 * the phase script.
 */
import { $, $$, expect } from '@wdio/globals';
import * as fs from 'node:fs';
import { testBrowser as browser } from '../test-browser.js';
import { FixtureClient, type DaemonSnapshot } from '../fixture-client.js';

async function statusText(): Promise<string> {
  return await $('[data-testid="connection-status"]').getText();
}

async function waitConnected(): Promise<void> {
  await browser.waitUntil(async () => (await statusText()) === 'connected', {
    timeout: 120_000,
    timeoutMsg: 'desktop app did not render connected state',
  });
}

async function connectionGeneration(): Promise<number> {
  return Number.parseInt(await $('[data-testid="connection-generation"]').getText(), 10);
}

async function subscriptionId(): Promise<string> {
  const text = await $('[data-testid="subscription-id"]').getText();
  return text === '—' ? '' : text;
}

// The host takes the old subscription at reconnect commit, and the renderer
// re-subscribes asynchronously. Registering the probe before the new owner
// is installed would broadcast into zero desktop subscribers and lose the
// invalidation forever, so every mutation waits for a fresh installed
// subscription first. A renderer that never subscribes fails here distinctly
// instead of timing out on the project list.
async function waitForSubscriptionId(previous: string | null): Promise<string> {
  await browser.waitUntil(
    async () => {
      const id = await subscriptionId();
      return id !== '' && id !== previous;
    },
    {
      timeout: 60_000,
      timeoutMsg: 'renderer subscription was never installed for the current generation',
    },
  );
  return subscriptionId();
}

async function projectTexts(): Promise<string[]> {
  const items = await $$('[data-testid="project-item"]');
  const texts: string[] = [];
  for (const item of items) {
    texts.push(await item.getText());
  }
  return texts;
}

// Read-only in-page diagnostics through the page's own bundled Tauri API:
// distinguishes "host never served it" from "renderer never refreshed".
async function dumpState(label: string, daemonProjects: string[]): Promise<void> {
  try {
    const state = await browser.execute(() => {
      const internals = (
        window as unknown as {
          __TAURI_INTERNALS__?: { invoke: (cmd: string, args?: unknown) => Promise<unknown> };
        }
      ).__TAURI_INTERNALS__;
      if (!internals) return Promise.resolve({ invoke: false });
      return internals
        .invoke('desktop_project_list')
        .then((projects) =>
          internals.invoke('desktop_connection_snapshot').then((snapshot) => ({
            invoke: true,
            hostProjects: (projects as unknown[]).length,
            daemonId: (snapshot as { daemonId?: unknown }).daemonId,
            generation: (snapshot as { connectionGeneration?: unknown }).connectionGeneration,
            status: document.querySelector('[data-testid="connection-status"]')?.textContent,
            subscription: document.querySelector('[data-testid="subscription-id"]')
              ?.textContent,
            rendered: document.querySelectorAll('[data-testid="project-item"]').length,
          })),
        )
        .catch((error: unknown) => ({ invoke: 'error', message: String(error) }));
    });
    console.log(`DIAG ${label}: host=${JSON.stringify(state)} daemon=${JSON.stringify(daemonProjects)}`);
  } catch (error) {
    console.log(`DIAG ${label}: execute failed: ${String(error)}`);
  }
}

async function waitForProject(displayName: string): Promise<void> {
  await browser.waitUntil(
    async () => (await projectTexts()).some((text) => text.includes(displayName)),
    {
      timeout: 60_000,
      timeoutMsg: `project ${displayName} never rendered in the real window`,
    },
  );
}

describe('M003 built-app lifecycle (existing daemon, reconnect, reload, close)', () => {
  let fixture: FixtureClient | null = null;
  let daemonId = '';
  let probeProject = '';
  let lastSubId: string | null = null;

  before(async () => {
    const stateFile = process.env.CODEGG_E2E_STATE_FILE;
    if (!stateFile || !fs.existsSync(stateFile)) {
      throw new Error('CODEGG_E2E_STATE_FILE must point at the phase start state (run via e2e/run-e2e.sh)');
    }
    const prestart = JSON.parse(fs.readFileSync(stateFile, 'utf8')) as {
      daemon_id: string;
      project_name: string;
    };
    fixture = await FixtureClient.connect();
    // Idempotent: shares the phase daemon/observer the runner pre-started.
    const started = await fixture.start();
    daemonId = started.daemon_id;
    if (daemonId !== prestart.daemon_id) {
      throw new Error('fixture daemon changed under the phase — aborting');
    }
    probeProject = prestart.project_name;
  });

  after(() => {
    fixture?.disconnect();
    fixture = null;
  });

  function active(): FixtureClient {
    if (!fixture) throw new Error('fixture is not running');
    return fixture;
  }

  function expectCounts(snapshot: DaemonSnapshot, total: number, desktop: number): void {
    expect(snapshot.total_clients).toBe(total);
    expect(snapshot.desktop_clients).toBe(desktop);
  }

  it('renders the real daemon identity and the deterministic project catalog', async () => {
    await waitConnected();
    expect(await $('[data-testid="daemon-identity"]').getText()).toBe(daemonId);
    lastSubId = await waitForSubscriptionId(null);
    await waitForProject(probeProject);
    // Exactly the TUI-kind fixture observer plus one desktop client.
    expectCounts(await active().snapshot(), 2, 1);
  });

  async function reconnectCycle(extraProject: string): Promise<number> {
    const before = await connectionGeneration();
    const beforeSub = lastSubId;
    await $('[data-testid="reconnect-button"]').click();
    await browser.waitUntil(async () => (await connectionGeneration()) > before, {
      timeout: 60_000,
      timeoutMsg: 'visible reconnect did not install a newer connection generation',
    });
    await waitConnected();
    // The new owner must be installed before mutating: a broadcast emitted
    // while no desktop subscriber exists is lost, not queued.
    lastSubId = await waitForSubscriptionId(beforeSub);
    // Reconnect converges to exactly one desktop client, not two.
    const snapshot = await active().waitForSnapshot(
      (s) => s.total_clients === 2 && s.desktop_clients === 1,
      { describe: 'one-desktop convergence after reconnect' },
    );
    expectCounts(snapshot, 2, 1);
    // Catalog mutation through the fixture reaches the current renderer with
    // no manual refresh, proving the real Tauri Channel delivery.
    await active().registerProject(extraProject);
    const daemonProjects = await active()
      .projectList()
      .then((projects) => projects.map((p) => p.display_name))
      .catch((error: unknown) => [`fixture-list-error: ${String(error)}`]);
    await dumpState(`after-register-${extraProject}`, daemonProjects);
    await waitForProject(extraProject);
    return await connectionGeneration();
  }

  it('visible reconnect keeps one desktop client and delivers Channel invalidations', async () => {
    await reconnectCycle('e2e-reconnect-probe-1');
  });

  it('renderer reload converges without accumulating clients or subscriptions', async () => {
    // Real WebView reload through WebDriver (not an in-app state reset).
    await browser.refresh();
    await waitConnected();
    expect(await $('[data-testid="daemon-identity"]').getText()).toBe(daemonId);
    lastSubId = await waitForSubscriptionId(lastSubId);
    const snapshot = await active().waitForSnapshot(
      (s) => s.total_clients === 2 && s.desktop_clients === 1,
      { describe: 'one-desktop convergence after reload' },
    );
    expectCounts(snapshot, 2, 1);
    // Only the current renderer subscription receives the update.
    await active().registerProject('e2e-reload-probe-1');
    await waitForProject('e2e-reload-probe-1');
  });

  it('repeats reconnect and reload without accumulation (second cycle)', async () => {
    await reconnectCycle('e2e-reconnect-probe-2');
    await browser.refresh();
    await waitConnected();
    lastSubId = await waitForSubscriptionId(lastSubId);
    expectCounts(
      await active().waitForSnapshot(
        (s) => s.total_clients === 2 && s.desktop_clients === 1,
        { describe: 'one-desktop convergence after second reload' },
      ),
      2,
      1,
    );
    await active().registerProject('e2e-reload-probe-2');
    await waitForProject('e2e-reload-probe-2');
  });

  it('native close removes the desktop client while daemon and observer survive', async () => {
    // Close through the WebDriver/native window path, reaching the Rust host
    // native-close teardown hook.
    await browser.closeWindow();
    const snapshot = await active().waitForSnapshot(
      (s) => s.total_clients === 1 && s.desktop_clients === 0,
      { describe: 'observer-only baseline after native close' },
    );
    expectCounts(snapshot, 1, 0);
    // The TUI-kind observer remains connected and the daemon responsive.
    const again = await active().snapshot();
    expect(again.daemon_id).toBe(daemonId);
    expectCounts(again, 1, 0);
  });
});
