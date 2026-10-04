/**
 * m004-session.e2e.ts — M004 built-app session/control-plane trajectory.
 *
 * Drives the real Tauri window (real WebView, real IPC, real local daemon
 * connection) through the M004 vertical slice WITHOUT any external model
 * provider: project/workspace route selection, session creation with
 * explicit identity, canonical projection attach, controller refresh,
 * prompt submission through the daemon-resolved path (which fails closed
 * with `model_unselected` in the provider-less fixture daemon — the typed
 * failure itself is the assertion), reconnect convergence, renderer-reload
 * convergence, and native close with daemon/observer survival.
 *
 * A live turn (assistant text, tool activity, permission round-trip) is
 * deliberately out of scope here: no offline provider exists in-repo, so
 * that leg waits on the deterministic provider-fixture follow-up. Every
 * other M004 seam runs against the real daemon in this phase.
 *
 * Phase discipline (see `e2e/run-e2e.sh`): same as the M003 specs — the
 * phase script owns the socket fixture and the isolated daemon, this file
 * only dials the fixture and shares its daemon via idempotent `start`.
 */
import { $, $$, expect } from '@wdio/globals';
import * as fs from 'node:fs';
import { testBrowser as browser } from '../test-browser.js';
import { FixtureClient } from '../fixture-client.js';

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

async function clickProjectByName(displayName: string): Promise<void> {
  const items = await $$('[data-testid="project-item"]');
  for (const item of items) {
    if ((await item.getText()).includes(displayName)) {
      await item.$('button').click();
      return;
    }
  }
  throw new Error(`no project row matched ${displayName}`);
}

async function waitForWorkspaceSelected(): Promise<void> {
  // selectWorkspace is async: the create button stays disabled through
  // routeBusy and until the workspace lands in the route token. Clicking
  // earlier would no-op on the disabled button.
  await browser.waitUntil(
    async () => await $('[data-testid="session-create-button"]').isEnabled(),
    { timeout: 60_000, timeoutMsg: 'workspace selection never enabled session create' },
  );
}
async function clickFirstWorkspace(): Promise<void> {
  // Workspace ids are fixture-assigned: click the first workspace row.
  const buttons = await $$('[data-testid="workspace-list"] button');
  let first = null;
  for (const button of buttons) {
    first = button;
    break;
  }
  if (!first) throw new Error('no workspace buttons rendered');
  await first.click();
}

describe('M004 built-app session vertical slice (route, projection, control, prompt)', () => {
  let fixture: FixtureClient | null = null;
  let daemonId = '';
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
    fixture = await FixtureClient.connect();
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

  it('renders the real daemon identity and the deterministic probe project', async () => {
    await waitConnected();
    expect(await $('[data-testid="daemon-identity"]').getText()).toBe(daemonId);
    await browser.waitUntil(
      async () => {
        const items = await $$('[data-testid="project-item"]');
        for (const item of items) {
          if ((await item.getText()).includes(probeProject)) return true;
        }
        return false;
      },
      { timeout: 60_000, timeoutMsg: 'probe project never rendered' },
    );
  });

  it('routes project to workspace to a created session with an attached projection', async () => {
    await clickProjectByName(probeProject);
    await $('[data-testid="workspace-list"]').waitForExist({ timeout: 60_000 });
    await clickFirstWorkspace();
    await waitForWorkspaceSelected();
    await $('[data-testid="session-title-input"]').waitForExist({ timeout: 60_000 });
    await $('[data-testid="session-title-input"]').setValue('e2e-m004');
    await $('[data-testid="session-create-button"]').click();
    await browser.waitUntil(
      async () => (await $('[data-testid="projection-state"]').getText()) === 'attached',
      { timeout: 120_000, timeoutMsg: 'projection never attached after session create' },
    );
    expect(await $('[data-testid="turn-status"]').getText()).toBe('idle');
    expect(await $('[data-testid="controller-state"]').getText()).toContain(
      'no exclusive controller',
    );
  });

  it('refreshes control state and fails prompt submit closed without a provider', async () => {
    await $('[data-testid="control-refresh-button"]').click();
    await browser.waitUntil(
      async () => (await $('[data-testid="controller-state"]').getText()).length > 0,
      { timeout: 60_000, timeoutMsg: 'controller state never rendered' },
    );
    // The fixture daemon has no model selection: the daemon-resolved
    // submit must fail closed with the typed code, and the draft stays
    // editable (no durable user message fabricated).
    await $('[data-testid="prompt-input"]').setValue('hello e2e');
    await $('[data-testid="prompt-submit-button"]').click();
    await browser.waitUntil(
      async () => (await $('[data-testid="route-error"]').getText()).includes('model_unselected'),
      {
        timeout: 120_000,
        timeoutMsg: 'prompt submit did not fail closed with model_unselected',
      },
    );
    expect(await $('[data-testid="prompt-input"]').getValue()).toBe('hello e2e');
  });

  it('runs a deterministic live turn: assistant text, denied write, completion', async () => {
    // Arm the session with the mock model through the ordinary daemon
    // selection APIs (fixture-driven, not a desktop bridge command: the
    // renderer never carries provider authority).
    const sessionId = await $('[data-testid="route-session"]').getText();
    const selected = await active().selectMockModel(sessionId);
    expect(selected.model_id).toBe('gpt-4o');
    await $('[data-testid="prompt-input"]').setValue('e2e deterministic turn');
    await $('[data-testid="prompt-submit-button"]').click();
    // The mock's first response raises PermissionPending for its
    // out-of-workspace `write`. Resolve the permission before checking the
    // rendered transcript because the assistant tool-call message may not be
    // committed to the session projection until the tool result is recorded.
    await $('[data-testid="permission-list"]').waitForExist({ timeout: 120_000 });
    let denied = false;
    for (const button of await $$('[data-testid^="permission-deny-"]')) {
      await button.click();
      denied = true;
      break;
    }
    expect(denied).toBe(true);
    // Denial feeds back as a tool result; the mock's second response
    // finishes the turn. Both converge through canonical projection.
    await browser.waitUntil(
      async () => (await $('[data-testid="turn-status"]').getText()).toLowerCase().includes('completed'),
      { timeout: 180_000, timeoutMsg: 'deterministic turn never completed after denial' },
    );
    await browser.waitUntil(
      async () => {
        try {
          const messages = await $('[data-testid="message-list"]');
          if (!(await messages.isExisting())) return false;
          const text = await messages.getText();
          return (
            text.includes('Examining your request.') &&
            text.includes('E2E deterministic turn complete.')
          );
        } catch {
          return false;
        }
      },
      { timeout: 60_000, timeoutMsg: 'completed assistant transcript never rendered' },
    );
  });

  it('visible reconnect converges the session projection on a new generation', async () => {
    const before = await connectionGeneration();
    await $('[data-testid="reconnect-button"]').click();
    await browser.waitUntil(async () => (await connectionGeneration()) > before, {
      timeout: 60_000,
      timeoutMsg: 'visible reconnect did not install a newer connection generation',
    });
    await waitConnected();
    // Re-drive the route on the new generation: the daemon session and
    // projection resume converge without duplicating sessions.
    await clickProjectByName(probeProject);
    await $('[data-testid="workspace-list"]').waitForExist({ timeout: 60_000 });
    await clickFirstWorkspace();
    await $('[data-testid="session-list"]').waitForExist({ timeout: 60_000 });
    let sessionCount = 0;
    for (const _item of await $$('[data-testid="session-item"]')) sessionCount += 1;
    expect(sessionCount).toBeGreaterThanOrEqual(1);
  });

  it('renderer reload re-drives the route without duplicating sessions', async () => {
    await browser.refresh();
    await waitConnected();
    expect(await $('[data-testid="daemon-identity"]').getText()).toBe(daemonId);
    await clickProjectByName(probeProject);
    await $('[data-testid="workspace-list"]').waitForExist({ timeout: 60_000 });
    await clickFirstWorkspace();
    await $('[data-testid="session-list"]').waitForExist({ timeout: 60_000 });
    let reloadedCount = 0;
    for (const _item of await $$('[data-testid="session-item"]')) reloadedCount += 1;
    // Exactly the one session this phase created: reload re-drives, never
    // duplicates.
    expect(reloadedCount).toBe(1);
  });

  function active(): FixtureClient {
    if (!fixture) throw new Error('fixture is not running');
    return fixture;
  }

  it('native close removes the desktop client while daemon and observer survive', async () => {
    await browser.closeWindow();
    const snapshot = await active().waitForSnapshot(
      (s) => s.total_clients === 1 && s.desktop_clients === 0,
      { describe: 'observer-only baseline after native close' },
    );
    expect(snapshot.total_clients).toBe(1);
    expect(snapshot.desktop_clients).toBe(0);
    expect(snapshot.daemon_id).toBe(daemonId);
  });
});
