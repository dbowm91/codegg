/**
 * m003-autostart.e2e.ts — C003 built-app visible-window trajectory, part 2.
 *
 * Explicit-path autostart: the isolated daemon is stopped, a fresh built
 * desktop is launched with explicit `CODEGG_DAEMON_EXECUTABLE`, a new daemon
 * starts, the window renders connected state, and closing the desktop leaves
 * the autostarted daemon running and responsive.
 *
 * Owns its own fixture and isolated home; independent of the lifecycle file
 * apart from execution order (lifecycle first, autostart second).
 */
import { $, $$, expect } from '@wdio/globals';
import { testBrowser as browser } from '../test-browser.js';
import { FixtureClient } from '../fixture-client.js';

const ambientDaemonExecutable = process.env.CODEGG_DAEMON_EXECUTABLE;

async function waitConnected(): Promise<void> {
  await browser.waitUntil(
    async () => (await $('[data-testid="connection-status"]').getText()) === 'connected',
    {
      timeout: 120_000,
      timeoutMsg: 'autostarted desktop app did not render connected state',
    },
  );
}

describe('M003 built-app explicit autostart and daemon survival', () => {
  let fixture: FixtureClient | null = null;
  let firstDaemonId = '';
  let probeProject = '';

  before(async () => {
    if (!ambientDaemonExecutable) {
      throw new Error('CODEGG_DAEMON_EXECUTABLE must point at a daemon binary built from the tested revision');
    }
    fixture = await FixtureClient.launch();
    const started = await fixture.start();
    firstDaemonId = started.daemon_id;
    probeProject = started.project_name;
    // Stop the isolated daemon; the desktop launched below must autostart a
    // new one from the explicit executable (identity-checked kill).
    await fixture.stopDaemon();
    process.env.CODEGG_DAEMON_HOME = fixture.daemonHome();
    process.env.CODEGG_DAEMON_EXECUTABLE = ambientDaemonExecutable;
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

  it('autostarts a new isolated daemon and renders connected state', async () => {
    await waitConnected();
    const daemonIdentity = await $('[data-testid="daemon-identity"]').getText();
    expect(daemonIdentity.length).toBeGreaterThan(0);
    expect(daemonIdentity).not.toBe(firstDaemonId);
    // The daemon-side project catalog survived the restart (same home).
    const items = await $$('[data-testid="project-item"]');
    const texts: string[] = [];
    for (const item of items) {
      texts.push(await item.getText());
    }
    expect(texts.some((text) => text.includes(probeProject))).toBe(true);
    // Reattach the observer to the autostarted daemon and prove convergence.
    const liveId = await active().reattach();
    expect(liveId).toBe(daemonIdentity);
    const snapshot = await active().waitForSnapshot(
      (s) => s.total_clients === 2 && s.desktop_clients === 1,
      { describe: 'one-desktop convergence after autostart' },
    );
    expect(snapshot.total_clients).toBe(2);
    expect(snapshot.desktop_clients).toBe(1);
  });

  it('desktop exit leaves the autostarted daemon running and responsive', async () => {
    await browser.closeWindow();
    const snapshot = await active().waitForSnapshot(
      (s) => s.total_clients === 1 && s.desktop_clients === 0,
      { describe: 'observer-only baseline after desktop exit' },
    );
    expect(snapshot.total_clients).toBe(1);
    expect(snapshot.desktop_clients).toBe(0);
    const again = await active().snapshot();
    expect(again.total_clients).toBe(1);
  });
});
