/**
 * m003-lifecycle.e2e.ts — C003 built-app visible-window trajectory, part 1.
 *
 * Drives the real Tauri window (real WebView, real IPC `Channel`, real local
 * daemon connection) through the M003 lifecycle: existing-daemon reuse,
 * visible reconnect, project-catalog invalidation over the current
 * subscription, renderer reload, and native close — with the reconnect/reload
 * cycle repeated twice to catch accumulation.
 *
 * Daemon side is owned by the Rust fixture helper: a TUI-kind observer holds
 * the isolated daemon for the whole file, so every assertion below is backed
 * by real `SnapshotDaemon.connected_clients` counts, not just pixels.
 *
 * Session/env discipline: this file starts its own fixture, points the worker
 * at the isolated home, and takes a fresh session before asserting, so the
 * app under test can only ever see the isolated home. The `after` hook shuts
 * the fixture down and clears the worker env so later files start clean.
 */
import { $, $$, expect } from '@wdio/globals';
import { testBrowser as browser } from '../test-browser.js';
import { FixtureClient, type DaemonSnapshot } from '../fixture-client.js';

const ambientDaemonExecutable = process.env.CODEGG_DAEMON_EXECUTABLE;

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

async function projectTexts(): Promise<string[]> {
  const items = await $$('[data-testid="project-item"]');
  const texts: string[] = [];
  for (const item of items) {
    texts.push(await item.getText());
  }
  return texts;
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

  before(async () => {
    if (!ambientDaemonExecutable) {
      throw new Error('CODEGG_DAEMON_EXECUTABLE must point at a daemon binary built from the tested revision');
    }
    fixture = await FixtureClient.launch();
    const started = await fixture.start();
    daemonId = started.daemon_id;
    probeProject = started.project_name;
    process.env.CODEGG_DAEMON_HOME = fixture.daemonHome();
    process.env.CODEGG_DAEMON_EXECUTABLE = ambientDaemonExecutable;
    // Fresh session however this file was entered: if a stale ambient launch
    // exists it is fail-closed (disconnected, no daemon touched); otherwise
    // the first command below creates the session with the isolated env.
    try {
      await browser.reloadSession();
    } catch {
      // No session yet — the first browser command below creates it.
    }
  });

  after(async () => {
    try {
      await fixture?.shutdown();
    } finally {
      fixture = null;
      delete process.env.CODEGG_DAEMON_HOME;
      delete process.env.CODEGG_DAEMON_EXECUTABLE;
    }
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
    await waitForProject(probeProject);
    // Exactly the TUI-kind fixture observer plus one desktop client.
    expectCounts(await active().snapshot(), 2, 1);
  });

  async function reconnectCycle(extraProject: string): Promise<number> {
    const before = await connectionGeneration();
    await $('[data-testid="reconnect-button"]').click();
    await browser.waitUntil(async () => (await connectionGeneration()) > before, {
      timeout: 60_000,
      timeoutMsg: 'visible reconnect did not install a newer connection generation',
    });
    await waitConnected();
    // Reconnect converges to exactly one desktop client, not two.
    const snapshot = await active().waitForSnapshot(
      (s) => s.total_clients === 2 && s.desktop_clients === 1,
      { describe: 'one-desktop convergence after reconnect' },
    );
    expectCounts(snapshot, 2, 1);
    // Catalog mutation through the fixture reaches the current renderer with
    // no manual refresh, proving the real Tauri Channel delivery.
    await active().registerProject(extraProject);
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
