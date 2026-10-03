/**
 * m003-autostart.e2e.ts — C003 built-app visible-window trajectory, part 2.
 *
 * Explicit-path autostart: the phase script pre-starts the isolated daemon
 * and then stops it (warm home, dead daemon), so the fresh built desktop
 * launched for this phase must autostart a new daemon from the explicit
 * `CODEGG_DAEMON_EXECUTABLE`. The window renders connected state, and closing
 * the desktop leaves the autostarted daemon running and responsive.
 *
 * Same phase discipline as the lifecycle file: the runner owns the fixture
 * server, the isolated home, and teardown; this file only dials the fixture.
 */
import { $, $$, expect } from '@wdio/globals';
import * as fs from 'node:fs';
import { testBrowser as browser } from '../test-browser.js';
import { FixtureClient } from '../fixture-client.js';

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
    const stateFile = process.env.CODEGG_E2E_STATE_FILE;
    if (!stateFile || !fs.existsSync(stateFile)) {
      throw new Error('CODEGG_E2E_STATE_FILE must point at the phase start state (run via e2e/run-e2e.sh)');
    }
    const prestart = JSON.parse(fs.readFileSync(stateFile, 'utf8')) as {
      daemon_id: string;
      project_name: string;
    };
    firstDaemonId = prestart.daemon_id;
    probeProject = prestart.project_name;
    fixture = await FixtureClient.connect();
    // The phase daemon was stopped after pre-start, so this reattaches the
    // fixture observer to whatever daemon is alive (the desktop autostart
    // below wins the race or already won it — either way one daemon serves
    // this home).
    await fixture.start();
  });

  after(() => {
    fixture?.disconnect();
    fixture = null;
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
