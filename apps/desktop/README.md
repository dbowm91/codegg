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
