/**
 * wdio.conf.ts — C003 built-app WebDriver qualification for the desktop M003 trajectory.
 *
 * Stack (per plan §4): WebdriverIO + `@wdio/tauri-service` with the default
 * `embedded` provider, so macOS, Linux, and Windows all run without a
 * platform-native external WebDriver. Ordinary DOM interaction only — the
 * `tauri-plugin-wdio` backend-execution plugin is intentionally not used.
 *
 * The app binary under test must be built with the `desktop-e2e` Cargo feature
 * (see `e2e/build-e2e-app.sh`), which is the only configuration that registers
 * the embedded WebDriver server plugin. Production builds never contain it.
 *
 * Each spec file launches the real Tauri window against an isolated daemon
 * owned by the Rust fixture helper (`e2e/fixture-client.ts`). Specs set
 * `CODEGG_DAEMON_HOME`/`CODEGG_DAEMON_EXECUTABLE` in `before` hooks and then
 * take a fresh session, so the app can only ever see the isolated home: an
 * e2e-feature app binary additionally refuses to connect when
 * `CODEGG_DAEMON_HOME` is unset or outside the OS temp directory.
 */
import * as path from 'node:path';
import type { Capabilities, Options } from '@wdio/types';
import type { TauriCapabilities } from '@wdio/tauri-service';

const e2eDir = import.meta.dirname;

function appBinaryPath(): string {
  const fromEnv = process.env.CODEGG_DESKTOP_APP_BINARY;
  if (fromEnv) return fromEnv;
  return path.join(
    e2eDir,
    '..',
    'src-tauri',
    'target',
    'debug',
    process.platform === 'win32' ? 'codegg-desktop.exe' : 'codegg-desktop',
  );
}

const appBinary = appBinaryPath();

// `WebdriverIO.Config` is the same intersection, spelled structurally: the
// global-namespace member does not evaluate under this repo's TypeScript
// toolchain (see test-browser.ts), while the constituent interfaces do.
export const config: Options.Testrunner & Capabilities.WithRequestedTestrunnerCapabilities = {
  runner: 'local',
  // Explicit order: the reconnect/reload/close trajectory first, explicit
  // autostart second. Each file owns its fixture and isolated daemon home.
  // Absolute paths: WDIO resolves spec patterns relative to this config
  // file's directory, so config-relative literals would silently match
  // nothing (0 workers) when invoked from the app root.
  // TEMPORARY: diagnostic probe runs first until the harness is green.
  specs: [
    path.join(e2eDir, 'specs', 'zz-diag.e2e.ts'),
    path.join(e2eDir, 'specs', 'm003-lifecycle.e2e.ts'),
    path.join(e2eDir, 'specs', 'm003-autostart.e2e.ts'),
  ],
  maxInstances: 1,
  capabilities: [
    {
      browserName: 'tauri',
      'tauri:options': {
        application: appBinary,
      },
    } as TauriCapabilities,
  ],
  services: [
    [
      'tauri',
      {
        appBinaryPath: appBinary,
        driverProvider: 'embedded',
      },
    ],
  ],
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: {
    ui: 'bdd',
    timeout: 240_000,
  },
  logLevel: 'info',
  bail: 0,
  waitforTimeout: 60_000,
  connectionRetryTimeout: 120_000,
  // Machine assertions are the closure authority; logs/screenshots below are
  // supplementary artifacts uploaded by the desktop-e2e workflow.
  outputDir: path.join(e2eDir, 'logs'),
  // TypeScript configs and specs are compiled on the fly by the CLI's bundled
  // tsx loader; no ts-node autoCompile options are needed (or exist) in v9.
};
