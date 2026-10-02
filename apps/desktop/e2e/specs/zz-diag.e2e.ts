/**
 * zz-diag.e2e.ts — TEMPORARY diagnostic probe (not part of the M003
 * trajectory). Dumps page state so a blank-window failure can be classified
 * as missing markup vs missing script vs missing Tauri bridge. Removed once
 * the harness is green.
 */
import { browser as rawBrowser, expect } from '@wdio/globals';

interface DiagBrowser {
  getTitle(): Promise<string>;
  getUrl(): Promise<string>;
  execute<T>(fn: () => T): Promise<T>;
}

const browser: DiagBrowser = rawBrowser as unknown as DiagBrowser;

describe('temporary diagnostic probe', () => {
  it('dumps page state', async () => {
    const title = await browser.getTitle();
    const url = await browser.getUrl();
    const state = await browser.execute(() => ({
      readyState: document.readyState,
      rootHtml: (document.getElementById('root')?.innerHTML ?? '<no-root>').slice(0, 500),
      scripts: Array.from(document.scripts).map((s) => s.src || '(inline)'),
      tauriGlobal: typeof (window as unknown as Record<string, unknown>).__TAURI__,
      tauriInternals: typeof (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__,
      userAgent: navigator.userAgent,
    }));
    console.log(`DIAG title=${JSON.stringify(title)} url=${JSON.stringify(url)}`);
    console.log(`DIAG state=${JSON.stringify(state)}`);
    expect(title.length).toBeGreaterThanOrEqual(0);
  });
});
