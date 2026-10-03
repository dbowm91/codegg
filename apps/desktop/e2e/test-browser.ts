/**
 * test-browser.ts — Minimal structural typing for the WebDriver browser object.
 *
 * The trajectory uses exactly three browser-level commands (`waitUntil`,
 * `refresh`, `closeWindow`). They are standard, stable WebDriver/WebdriverIO
 * APIs, but the full `WebdriverIO.Browser` interface is assembled from deep
 * mapped command types that the repo's TypeScript toolchain does not
 * evaluate, so this harness types the three commands structurally instead
 * of importing the global interface.
 *
 * Runtime behavior is unchanged: these are the real WebdriverIO browser
 * commands, and the signatures below match the documented API. Element-level
 * `$`/`$$`/`expect` keep their first-party types.
 */
import { browser as wdioBrowser } from '@wdio/globals';

export interface TestBrowser {
  waitUntil(
    condition: () => Promise<boolean>,
    options?: { timeout?: number; timeoutMsg?: string; interval?: number },
  ): Promise<void>;
  refresh(): Promise<void>;
  closeWindow(): Promise<void>;
  // Raw script execution (proven working). Used only for read-only
  // in-page diagnostics (document state, host snapshot/project counts via
  // the page's own bundled Tauri API) — never to drive the app.
  execute<T>(script: string | ((...args: unknown[]) => T), ...args: unknown[]): Promise<T>;
}

export const testBrowser: TestBrowser = wdioBrowser as unknown as TestBrowser;
