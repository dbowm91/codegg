# CodeGG Desktop Developer Setup

This optional Tauri app has its own Node package lock and Rust workspace. The
root Cargo workspace stays on Rust 1.89; this app pins Rust 1.90 for Tauri 2.12.

Install Node 22+, Rust 1.90.0, and the native Tauri prerequisites for your OS
(GTK/WebKitGTK on Linux, Xcode command line tools on macOS, or WebView2 build
tools on Windows). Then:

```bash
npm ci
npm run typecheck
npm test
npm run bindings:check
npm run build
```

For local Tauri development, set `CODEGG_DAEMON_EXECUTABLE` to a built root
`codegg` executable and run `npm run tauri dev`. The host uses that explicit
binary path for daemon autostart. To package the existing executable as a
resource, build the root `codegg` binary and run `npm run stage:daemon` before
`npm run tauri build`. This does not create or package a separate `codeggd`.

## Dependency security notes

This app keeps its own `package-lock.json` and `src-tauri/Cargo.lock`, so
Dependabot watches them separately from the root workspace.

### npm `overrides` in `package.json`

Two transitive dev-dependency advisories are pinned rather than fixed upstream,
because WebdriverIO 9 still resolves the vulnerable versions:

- `serialize-javascript` -> `^7.1.2` (GHSA-5c6j-r48x-rmvq, GHSA-qj8w-gfj5-8c6v).
  Only `mocha` pulls it, at `^6.0.2`; v7 keeps the same CommonJS
  `module.exports = function serialize(obj, options)` signature, so the
  single-argument call in `mocha/lib/nodejs/buffered-worker-pool.js` is
  unaffected.
- `@puppeteer/browsers` -> `^3.2.4` (GHSA-7pqw-9j4j-h8q3, GHSA-jmr9-qjv8-65gv,
  both in `extract-zip`). `extract-zip` has no patched release — 2.0.1 is the
  final publish — so 3.x is used specifically because it drops `extract-zip` in
  favor of `modern-tar`/`tar-fs`. 3.2.4 exports all seven symbols `@wdio/utils`
  imports (`install`, `canDownload`, `resolveBuildId`, `detectBrowserPlatform`,
  `Browser`, `ChromeReleaseChannel`, `computeExecutablePath`).

Drop both pins once a WebdriverIO release carries the fixed versions natively.

### Dismissed: `glib` GHSA-wrw7-89jp-8q8g (Dependabot #34)

Dismissed as `not_used` — no fix is available, and the affected code is
unreachable from this app.

- The advisory is fixed in `glib` 0.20.0; the lock resolves `glib` 0.18.5.
- `glib` is purely transitive here: `tauri` 2.12 -> `wry` 0.57 -> `gtk ^0.18` ->
  `glib ^0.18`. `gtk` 0.19 is the release that requires `glib ^0.22`, and no
  `wry` release at or above 0.57 depends on `gtk` 0.19, so a patched `glib` is
  unreachable without replacing Tauri. (`tauri` 3.0.0-alpha still pins
  `gtk ^0.18`.)
- Linux-only; the macOS and Windows builds never link this stack.
- The vulnerable API is `glib::VariantStrIter`, a D-Bus variant string iterator.
  `src-tauri/` contains no `gio`, `glib`, or D-Bus usage, so it is never
  exercised.

Re-evaluate this dismissal when Tauri or wry move to `gtk` 0.19 or newer.
