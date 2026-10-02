/**
 * test-browser.ts — Minimal structural typing for the WebDriver browser object.
 *
 * The trajectory uses exactly four browser-level commands (`waitUntil`,
 * `reloadSession`, `refresh`, `closeWindow`). They are standard, stable
 * WebDriver/WebdriverIO APIs, but the full `WebdriverIO.Browser` interface is
 * assembled from deep mapped command types that the repo's TypeScript
 * toolchain does not evaluate, so this harness types the four commands
 * structurally instead of importing the global interface.
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
  reloadSession(): Promise<void>;
  refresh(): Promise<void>;
  closeWindow(): Promise<void>;
}

export const testBrowser: TestBrowser = wdioBrowser as unknown as TestBrowser;
