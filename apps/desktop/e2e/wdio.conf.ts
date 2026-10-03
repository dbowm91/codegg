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
 * owned by the Rust fixture helper (`e2e/fixture-client.ts`). The embedded
 * provider spawns the app once per WebdriverIO invocation in the launcher
 * process, so `e2e/run-e2e.sh` runs one invocation per phase (lifecycle,
 * then autostart), each with its own fixture server, isolated home, and app
 * environment inherited from the phase script. An e2e-feature app binary
 * additionally refuses to connect when `CODEGG_DAEMON_HOME` is unset or
 * outside the OS temp directory.
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

// One app instance is spawned per WebdriverIO invocation, and the embedded
// server listens on a fixed port: phases run as separate invocations, so
// each phase takes its own port (a lingering app from an earlier phase can
// never capture or confuse a later one).
const embeddedPort = Number(process.env.CODEGG_E2E_WDIO_PORT ?? 4445);

// `WebdriverIO.Config` is the same intersection, spelled structurally: the
// global-namespace member does not evaluate under this repo's TypeScript
// toolchain (see test-browser.ts), while the constituent interfaces do.
export const config: Options.Testrunner & Capabilities.WithRequestedTestrunnerCapabilities = {
  runner: 'local',
  // Full trajectory in file order. `run-e2e.sh` selects one phase file per
  // invocation (`--spec`) because each phase needs its own app environment;
  // a bare `npm run test:e2e` runs both against the ambient environment
  // (fail-closed: the e2e app refuses to connect outside temp-scoped homes).
  // Absolute paths: WDIO resolves spec patterns relative to this config
  // file's directory, so config-relative literals would silently match
  // nothing (0 workers) when invoked from the app root.
  specs: [
    path.join(e2eDir, 'specs', 'm003-lifecycle.e2e.ts'),
    path.join(e2eDir, 'specs', 'm003-autostart.e2e.ts'),
    path.join(e2eDir, 'specs', 'm004-session.e2e.ts'),
  ],
  maxInstances: 1,
  capabilities: [
    {
      browserName: 'tauri',
      'tauri:options': {
        application: appBinary,
      },
      // Pin the session to the real window. E2E builds keep a hidden blank
      // `e2e-anchor` window (see `run()` in src-tauri/src/lib.rs) so the
      // native-close probe's `main` destroy does not take the automation
      // session down with it; without the pin the embedded server would bind
      // whichever window label it lists first.
      'wdio:tauriServiceOptions': {
        windowLabel: 'main',
      },
    } as TauriCapabilities,
  ],
  services: [
    [
      'tauri',
      {
        appBinaryPath: appBinary,
        driverProvider: 'embedded',
        embeddedPort,
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
