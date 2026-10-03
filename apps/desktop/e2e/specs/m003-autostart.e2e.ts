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

async function projectTexts(): Promise<string[]> {
  const items = await $$('[data-testid="project-item"]');
  const texts: string[] = [];
  for (const item of items) {
    texts.push(await item.getText());
  }
  return texts;
}

// Read-only in-page diagnostics through the page's own bundled Tauri API,
// mirroring the lifecycle spec: separates "host never served it" from
// "renderer never refreshed".
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

// Strict host≡daemon convergence (see the lifecycle spec): presence plus
// rendered-count equality plus row uniqueness. A re-`start` against the
// warm phase home once stacked a same-named probe here and presence-only
// checks let the duplicate through.
async function waitForCatalog(daemonNames: () => Promise<string[]>, ...expected: string[]): Promise<void> {
  await browser.waitUntil(
    async () => {
      const texts = await projectTexts();
      if (texts.length !== (await daemonNames()).length) return false;
      if (new Set(texts).size !== texts.length) return false;
      return expected.every((name) => texts.some((text) => text.includes(name)));
    },
    {
      timeout: 60_000,
      timeoutMsg: `host render diverged from daemon catalog (expected ${expected.join(', ')})`,
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

  async function daemonProjectNames(): Promise<string[]> {
    return active()
      .projectList()
      .then((projects) => projects.map((p) => p.display_name))
      .catch((error: unknown) => [`fixture-list-error: ${String(error)}`]);
  }

  it('autostarts a new isolated daemon and renders connected state', async () => {
    await waitConnected();
    const daemonIdentity = await $('[data-testid="daemon-identity"]').getText();
    expect(daemonIdentity.length).toBeGreaterThan(0);
    expect(daemonIdentity).not.toBe(firstDaemonId);
    // The mount subscription must be installed (proves the app bound the
    // autostarted connection, not just a snapshot).
    await browser.waitUntil(
      async () => {
        const text = await $('[data-testid="subscription-id"]').getText();
        return text !== '' && text !== '—';
      },
      {
        timeout: 60_000,
        timeoutMsg: 'renderer subscription was never installed after autostart',
      },
    );
    // The daemon-side project catalog survived the restart (same home):
    // the pre-start probe renders exactly once — no lost rows, no duplicates.
    await dumpState('after-autostart', await daemonProjectNames());
    await waitForCatalog(daemonProjectNames, probeProject);
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
